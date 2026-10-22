//! Turning a message body into the segments that carry it.
//!
//! `None` returns exactly one segment however long the text is — the user
//! asked for one PDU and gets one PDU. A body that fits a single segment is
//! one segment even in a concatenating mode, so a plain 160-character message
//! is never fragmented. A caller-supplied UDH wins over automatic
//! concatenation: the user's bytes go on the wire and nothing is added.

use indexmap::IndexMap;

use crate::smpp::constants::{
    DataCoding, ESM_UDHI_BIT, TLV_MESSAGE_PAYLOAD, TLV_SAR_MSG_REF_NUM, TLV_SAR_SEGMENT_SEQNUM,
    TLV_SAR_TOTAL_SEGMENTS,
};
use crate::smpp::decode::FieldValue;
use crate::smpp::text::{
    encode_text, septets_for, split_by_code_units, split_septets, Gsm7Packing, SegmentationMode,
    GSM7_CONCAT, GSM7_SINGLE, OCTET_CONCAT, OCTET_SINGLE, UCS2_CONCAT_UNITS, UCS2_SINGLE_UNITS,
};
use crate::smpp::tlv::Tlv;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyPlacement {
    #[default]
    ShortMessage,
    MessagePayload,
}

#[derive(Debug, Clone)]
pub struct SegmentOptions {
    pub text: String,
    pub data_coding: u8,
    pub mode: SegmentationMode,
    pub placement: BodyPlacement,
    pub packing: Gsm7Packing,
    /// A UDH the user composed. Its presence turns concatenation off.
    pub user_udh: Option<Vec<u8>>,
    /// Concatenation reference. Chosen when omitted.
    pub ref_num: Option<u16>,
    /// The messaging mode the form selected; UDHI is OR-ed into it.
    pub esm_class: u8,
}

#[derive(Debug, Clone)]
pub struct Segment {
    /// The fields this segment owns; merge over the form's template.
    pub fields: IndexMap<String, FieldValue>,
    pub tlvs: Vec<Tlv>,
    /// Characters the chosen data_coding could not represent.
    pub unmappable: Vec<String>,
}

fn concat_udh(ref_num: u8, total: u8, seq: u8) -> Vec<u8> {
    // UDH length 5, IE 0x00 (concatenated short message, 8-bit reference), IE length 3.
    vec![0x05, 0x00, 0x03, ref_num, total, seq]
}

/// Split `text` into the per-segment chunks its coding calls for.
fn chunk(text: &str, data_coding: u8, packing: Gsm7Packing) -> Vec<String> {
    if data_coding == DataCoding::GSM_DEFAULT {
        let septets = septets_for(text);
        if septets.len() <= GSM7_SINGLE {
            return vec![text.to_string()];
        }
        // Split on septets so an escape pair is never cut in half, then map the
        // runs back to text.
        let runs = split_septets(&septets, GSM7_CONCAT);
        let chars: Vec<char> = text.chars().collect();
        let mut chunks = Vec::with_capacity(runs.len());
        let mut index = 0usize;
        for run in runs {
            let mut used = 0usize;
            let mut s = String::new();
            while index < chars.len() {
                let cost = septets_for(&chars[index].to_string()).len();
                if used + cost > run.len() {
                    break;
                }
                s.push(chars[index]);
                used += cost;
                index += 1;
            }
            chunks.push(s);
        }
        return chunks;
    }

    if data_coding == DataCoding::UCS2 {
        let units: usize = text.chars().map(|c| c.len_utf16()).sum();
        return if units <= UCS2_SINGLE_UNITS {
            vec![text.to_string()]
        } else {
            split_by_code_units(text, UCS2_CONCAT_UNITS)
        };
    }

    let bytes = encode_text(text, data_coding, packing).bytes.len();
    if bytes <= OCTET_SINGLE {
        return vec![text.to_string()];
    }
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(OCTET_CONCAT)
        .map(|c| c.iter().collect::<String>())
        .collect()
}

fn random_ref(max: u16) -> u16 {
    // A reference only has to differ between concurrent messages, so the clock
    // is enough and avoids a dependency for it.
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(1);
    1 + (nanos % max as u32) as u16
}

pub fn segment(opts: &SegmentOptions) -> Vec<Segment> {
    let build = |text: &str, udh: Option<&[u8]>, tlvs: Vec<Tlv>, set_udhi: bool| -> Segment {
        let encoded = encode_text(text, opts.data_coding, opts.packing);
        let body = match udh {
            Some(u) => {
                let mut v = u.to_vec();
                v.extend_from_slice(&encoded.bytes);
                v
            }
            None => encoded.bytes,
        };

        let mut fields: IndexMap<String, FieldValue> = IndexMap::new();
        fields.insert(
            "esm_class".to_string(),
            FieldValue::Num(if set_udhi {
                (opts.esm_class | ESM_UDHI_BIT) as u32
            } else {
                opts.esm_class as u32
            }),
        );

        let mut all = tlvs;
        match opts.placement {
            BodyPlacement::MessagePayload => all.push(Tlv::new(TLV_MESSAGE_PAYLOAD, body)),
            BodyPlacement::ShortMessage => {
                fields.insert("short_message".to_string(), FieldValue::Bytes(body));
            }
        }

        Segment {
            fields,
            tlvs: all,
            unmappable: encoded.unmappable,
        }
    };

    // The user's own UDH turns automatic concatenation off: one segment,
    // their bytes, nothing added.
    if let Some(udh) = opts.user_udh.as_ref().filter(|u| !u.is_empty()) {
        return vec![build(&opts.text, Some(udh), Vec::new(), true)];
    }

    if opts.mode == SegmentationMode::None {
        return vec![build(&opts.text, None, Vec::new(), false)];
    }

    let chunks = chunk(&opts.text, opts.data_coding, opts.packing);
    if chunks.len() == 1 {
        return vec![build(&chunks[0], None, Vec::new(), false)];
    }

    let total = chunks.len() as u8;

    if opts.mode == SegmentationMode::Sar {
        let reference = opts.ref_num.unwrap_or_else(|| random_ref(0xfffe));
        return chunks
            .iter()
            .enumerate()
            .map(|(i, c)| {
                build(
                    c,
                    None,
                    vec![
                        Tlv::new(TLV_SAR_MSG_REF_NUM, reference.to_be_bytes().to_vec()),
                        Tlv::new(TLV_SAR_TOTAL_SEGMENTS, vec![total]),
                        Tlv::new(TLV_SAR_SEGMENT_SEQNUM, vec![(i + 1) as u8]),
                    ],
                    false,
                )
            })
            .collect();
    }

    let reference = opts.ref_num.unwrap_or_else(|| random_ref(0xfe)) as u8;
    chunks
        .iter()
        .enumerate()
        .map(|(i, c)| {
            build(
                c,
                Some(&concat_udh(reference, total, (i + 1) as u8)),
                Vec::new(),
                true,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smpp::constants::{
        DataCoding, EsmClass, ESM_UDHI_BIT, TLV_MESSAGE_PAYLOAD, TLV_SAR_MSG_REF_NUM,
        TLV_SAR_SEGMENT_SEQNUM, TLV_SAR_TOTAL_SEGMENTS,
    };
    use crate::smpp::text::{decode_text, measure, Gsm7Packing, SegmentationMode};

    fn opts(text: &str) -> SegmentOptions {
        SegmentOptions {
            text: text.to_string(),
            data_coding: DataCoding::GSM_DEFAULT,
            mode: SegmentationMode::Udh,
            placement: BodyPlacement::ShortMessage,
            packing: Gsm7Packing::Unpacked,
            user_udh: None,
            ref_num: None,
            esm_class: 0,
        }
    }

    fn body(seg: &Segment) -> Vec<u8> {
        match seg.fields.get("short_message") {
            Some(crate::smpp::decode::FieldValue::Bytes(b)) => b.clone(),
            other => panic!("expected a short_message, got {other:?}"),
        }
    }

    fn esm(seg: &Segment) -> u8 {
        match seg.fields.get("esm_class") {
            Some(crate::smpp::decode::FieldValue::Num(n)) => *n as u8,
            other => panic!("expected an esm_class, got {other:?}"),
        }
    }

    fn tlv(seg: &Segment, tag: u16) -> Vec<u8> {
        seg.tlvs
            .iter()
            .find(|t| t.tag == tag)
            .expect("missing TLV")
            .value
            .clone()
    }

    #[test]
    fn returns_one_oversized_segment_in_none_mode() {
        let segs = segment(&SegmentOptions {
            mode: SegmentationMode::None,
            ..opts(&"a".repeat(400))
        });
        assert_eq!(segs.len(), 1);
        assert_eq!(body(&segs[0]).len(), 400);
    }

    #[test]
    fn splits_gsm7_into_153_septet_segments_in_udh_mode() {
        let segs = segment(&opts(&"a".repeat(400)));
        assert_eq!(segs.len(), 3);
        assert_eq!(body(&segs[0]).len(), 6 + 153);
        assert_eq!(body(&segs[2]).len(), 6 + (400 - 306));
    }

    #[test]
    fn splits_ucs2_into_67_character_segments_in_udh_mode() {
        let segs = segment(&SegmentOptions {
            data_coding: DataCoding::UCS2,
            ..opts(&"a".repeat(200))
        });
        assert_eq!(segs.len(), 3);
        assert_eq!(body(&segs[0]).len(), 6 + 67 * 2);
    }

    #[test]
    fn reassembles_to_the_original_text_across_segments() {
        let text = "a".repeat(400);
        let joined: String = segment(&opts(&text))
            .iter()
            .map(|s| {
                decode_text(
                    &body(s)[6..],
                    DataCoding::GSM_DEFAULT,
                    Gsm7Packing::Unpacked,
                )
            })
            .collect();
        assert_eq!(joined, text);
    }

    #[test]
    fn writes_a_concatenation_udh_with_a_shared_reference_and_rising_sequence() {
        let segs = segment(&SegmentOptions {
            ref_num: Some(0x2a),
            ..opts(&"a".repeat(400))
        });
        for (i, seg) in segs.iter().enumerate() {
            let udh = &body(seg)[..6];
            assert_eq!(udh[0], 0x05);
            assert_eq!(udh[1], 0x00);
            assert_eq!(udh[2], 0x03);
            assert_eq!(udh[3], 0x2a);
            assert_eq!(udh[4], 3);
            assert_eq!(udh[5], (i + 1) as u8);
        }
    }

    #[test]
    fn sets_the_udhi_bit_in_esm_class_in_udh_mode_only() {
        assert_eq!(
            esm(&segment(&opts(&"a".repeat(400)))[0]) & ESM_UDHI_BIT,
            ESM_UDHI_BIT
        );
        let sar = segment(&SegmentOptions {
            mode: SegmentationMode::Sar,
            ..opts(&"a".repeat(400))
        });
        assert_eq!(esm(&sar[0]) & ESM_UDHI_BIT, 0);
        let none = segment(&SegmentOptions {
            mode: SegmentationMode::None,
            ..opts("hi")
        });
        assert_eq!(esm(&none[0]) & ESM_UDHI_BIT, 0);
    }

    #[test]
    fn keeps_the_callers_messaging_mode_when_it_sets_udhi() {
        let segs = segment(&SegmentOptions {
            esm_class: EsmClass::STORE_AND_FORWARD,
            ..opts(&"a".repeat(400))
        });
        assert_eq!(esm(&segs[0]), EsmClass::STORE_AND_FORWARD | ESM_UDHI_BIT);
    }

    #[test]
    fn emits_sar_tlvs_with_a_matching_reference_in_sar_mode() {
        let segs = segment(&SegmentOptions {
            mode: SegmentationMode::Sar,
            ref_num: Some(0x1234),
            ..opts(&"a".repeat(400))
        });
        assert_eq!(segs.len(), 3);
        for seg in &segs {
            assert_eq!(tlv(seg, TLV_SAR_MSG_REF_NUM), vec![0x12, 0x34]);
            assert_eq!(tlv(seg, TLV_SAR_TOTAL_SEGMENTS), vec![3]);
        }
    }

    #[test]
    fn numbers_sar_segments_from_one() {
        let segs = segment(&SegmentOptions {
            mode: SegmentationMode::Sar,
            ..opts(&"a".repeat(400))
        });
        let seqs: Vec<u8> = segs
            .iter()
            .map(|s| tlv(s, TLV_SAR_SEGMENT_SEQNUM)[0])
            .collect();
        assert_eq!(seqs, vec![1, 2, 3]);
    }

    #[test]
    fn adds_no_udh_to_the_body_in_sar_mode() {
        let segs = segment(&SegmentOptions {
            mode: SegmentationMode::Sar,
            ..opts(&"a".repeat(400))
        });
        assert_eq!(body(&segs[0]).len(), 153);
    }

    #[test]
    fn a_user_supplied_udh_turns_automatic_concatenation_off() {
        let udh = vec![0x03, 0x20, 0x01, 0x05];
        let segs = segment(&SegmentOptions {
            user_udh: Some(udh.clone()),
            ..opts(&"a".repeat(400))
        });
        assert_eq!(segs.len(), 1);
        assert_eq!(&body(&segs[0])[..4], &udh[..]);
        // The whole body rides along: nothing was split behind the user's back.
        assert_eq!(body(&segs[0]).len(), udh.len() + 400);
    }

    #[test]
    fn still_sets_udhi_for_a_user_supplied_udh() {
        let segs = segment(&SegmentOptions {
            user_udh: Some(vec![0x00]),
            mode: SegmentationMode::None,
            ..opts("hi")
        });
        assert_eq!(esm(&segs[0]) & ESM_UDHI_BIT, ESM_UDHI_BIT);
    }

    #[test]
    fn puts_the_body_in_message_payload_when_the_placement_says_so() {
        let segs = segment(&SegmentOptions {
            placement: BodyPlacement::MessagePayload,
            mode: SegmentationMode::None,
            ..opts("hello")
        });
        assert_eq!(tlv(&segs[0], TLV_MESSAGE_PAYLOAD), b"hello".to_vec());
        assert!(segs[0].fields.get("short_message").is_none());
    }

    #[test]
    fn reports_unmappable_characters_on_each_segment() {
        let segs = segment(&SegmentOptions {
            mode: SegmentationMode::None,
            ..opts("hi 🙂")
        });
        assert_eq!(segs[0].unmappable, vec!["🙂".to_string()]);
    }

    #[test]
    fn segments_packed_gsm7_into_134_octet_bodies() {
        let segs = segment(&SegmentOptions {
            packing: Gsm7Packing::Packed,
            ..opts(&"a".repeat(400))
        });
        assert_eq!(body(&segs[0]).len(), 6 + 134);
    }

    #[test]
    fn sends_a_full_single_segment_as_one_pdu_not_two() {
        // 160 septets is the textbook single SMS. Splitting it would fragment
        // the most common message this tool will ever send.
        assert_eq!(segment(&opts(&"a".repeat(160))).len(), 1);
        assert_eq!(segment(&opts(&"a".repeat(161))).len(), 2);

        let ucs2 = |n: usize| SegmentOptions {
            data_coding: DataCoding::UCS2,
            ..opts(&"a".repeat(n))
        };
        assert_eq!(segment(&ucs2(70)).len(), 1);
        assert_eq!(segment(&ucs2(71)).len(), 2);

        let oct = |n: usize| SegmentOptions {
            data_coding: DataCoding::OCTET_UNSPECIFIED_4,
            ..opts(&"a".repeat(n))
        };
        assert_eq!(segment(&oct(140)).len(), 1);
        assert_eq!(segment(&oct(141)).len(), 2);
    }

    #[test]
    fn agrees_with_the_live_counter_about_how_many_parts_there_will_be() {
        let cases: &[(usize, u8)] = &[
            (153, DataCoding::GSM_DEFAULT),
            (157, DataCoding::GSM_DEFAULT),
            (160, DataCoding::GSM_DEFAULT),
            (161, DataCoding::GSM_DEFAULT),
            (320, DataCoding::GSM_DEFAULT),
            (67, DataCoding::UCS2),
            (69, DataCoding::UCS2),
            (70, DataCoding::UCS2),
            (71, DataCoding::UCS2),
            (134, DataCoding::OCTET_UNSPECIFIED_4),
            (138, DataCoding::OCTET_UNSPECIFIED_4),
            (140, DataCoding::OCTET_UNSPECIFIED_4),
            (141, DataCoding::OCTET_UNSPECIFIED_4),
        ];
        for (n, dc) in cases {
            let text = "a".repeat(*n);
            let counted = measure(&text, *dc, SegmentationMode::Udh, Gsm7Packing::Unpacked).parts;
            let actual = segment(&SegmentOptions {
                data_coding: *dc,
                ..opts(&text)
            })
            .len();
            assert_eq!(actual, counted, "{n} chars at data_coding {dc}");
        }
    }
}
