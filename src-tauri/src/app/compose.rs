//! The submit form, composed into the PDUs it means.
//!
//! This moved out of the renderer: a qooxdoo widget now sends the values a
//! person typed and gets back the PDUs, the warnings and the counter. It is
//! pure, so the composition is testable without a socket.
//!
//! Validation only ever warns. An over-long field, a short address, a
//! `data_coding` the spec does not list — all of it goes on the wire exactly
//! as typed. That is the point of the tool.

use crate::app::wire::{clean_hex, hex_to_bytes, PduPreview, SendPduRequest, WireTlv};
use crate::smpp::constants::command_id;
use crate::smpp::decode::{decode_pdu, FieldValue};
use crate::smpp::segment::{segment, BodyPlacement, SegmentOptions};
use crate::smpp::summary::{describe, summarise};
use crate::smpp::text::{measure, Gsm7Packing, Measurement, SegmentationMode};

/// One row of the TLV editor, as typed.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TlvRow {
    /// Tag as hex, with or without a `0x` prefix.
    pub tag_hex: String,
    /// Value as hex, when the user typed hex.
    #[serde(default)]
    pub value_hex: Option<String>,
    /// Value as text, when the user typed text. Latin-1, as the spec's octet
    /// strings are.
    #[serde(default)]
    pub value_text: Option<String>,
}

/// The submit form, as the widgets hold it.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitFormValues {
    #[serde(default)]
    pub service_type: String,
    #[serde(default)]
    pub source_addr_ton: u8,
    #[serde(default)]
    pub source_addr_npi: u8,
    #[serde(default)]
    pub source_addr: String,
    #[serde(default)]
    pub dest_addr_ton: u8,
    #[serde(default)]
    pub dest_addr_npi: u8,
    #[serde(default)]
    pub destination_addr: String,
    #[serde(default)]
    pub esm_class: u8,
    #[serde(default)]
    pub protocol_id: u8,
    #[serde(default)]
    pub priority_flag: u8,
    #[serde(default)]
    pub registered_delivery: u8,
    /// Already resolved to the SMPP time format by the renderer, which owns
    /// the preset list.
    #[serde(default)]
    pub schedule_delivery_time: String,
    #[serde(default)]
    pub validity_period: String,
    #[serde(default)]
    pub data_coding: u8,
    #[serde(default)]
    pub replace_if_present: u8,
    #[serde(default)]
    pub sm_default_msg_id: u8,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub placement: BodyPlacement,
    #[serde(default)]
    pub segmentation: SegmentationMode,
    #[serde(default)]
    pub packing: Gsm7Packing,
    /// A hand-written UDH. Its presence turns automatic concatenation off.
    #[serde(default)]
    pub udh_hex: String,
    #[serde(default)]
    pub tlvs: Vec<TlvRow>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposedSubmit {
    /// One request per segment, ready for `send_pdu`.
    pub requests: Vec<SendPduRequest>,
    /// Things worth saying. Never a reason to withhold the PDU.
    pub warnings: Vec<String>,
    pub measurement: Measurement,
}

/// The reference tool wants at least this many digits in a destination.
const MIN_ADDRESS_LENGTH: usize = 8;
/// `sm_length` is one octet, so a single PDU cannot carry more than this.
pub const SHORT_MESSAGE_MAX: usize = 254;
/// The codings the spec lists. Anything else is warned about, never refused.
const SPEC_DATA_CODINGS: std::ops::RangeInclusive<u8> = 0..=8;

pub fn compose_submit(v: &SubmitFormValues) -> ComposedSubmit {
    let mut warnings = Vec::new();

    let dest_len = v.destination_addr.chars().count();
    if dest_len == 0 {
        warnings.push("destination_addr is empty. Sending it anyway.".to_string());
    } else if dest_len < MIN_ADDRESS_LENGTH {
        warnings.push(format!(
            "destination_addr \"{}\" is {dest_len} characters; the reference tool wants at \
             least {MIN_ADDRESS_LENGTH}. Sending it anyway.",
            v.destination_addr
        ));
    }
    if v.service_type.chars().count() > 5 {
        warnings.push(format!(
            "service_type is {} characters; the spec allows 5. Sending it anyway.",
            v.service_type.chars().count()
        ));
    }
    if !SPEC_DATA_CODINGS.contains(&v.data_coding) {
        warnings.push(format!(
            "data_coding {} is outside the 0-8 range the spec lists. Sending it anyway.",
            v.data_coding
        ));
    }

    // A hand-written UDH turns automatic concatenation off, so whatever the
    // segmentation selector says, one PDU goes out and the counter must agree.
    let udh = hex_to_bytes(&v.udh_hex);
    let effective_mode = if udh.is_empty() {
        v.segmentation
    } else {
        SegmentationMode::None
    };
    let measurement = measure(&v.text, v.data_coding, effective_mode, v.packing);

    if !measurement.unmappable.is_empty() {
        warnings.push(format!(
            "data_coding {} cannot represent {}; those characters go out as question marks.",
            v.data_coding,
            measurement.unmappable.join(" ")
        ));
    }
    let segments = segment(&SegmentOptions {
        text: v.text.clone(),
        data_coding: v.data_coding,
        mode: effective_mode,
        placement: v.placement,
        packing: v.packing,
        user_udh: if udh.is_empty() { None } else { Some(udh) },
        ref_num: None,
        esm_class: v.esm_class,
    });

    // The warning belongs to the body that was composed, not to the text that
    // was typed: a hand-written UDH adds octets, and a part of a segmented
    // message is not the whole message. Guarding on "one part" used to let a
    // multi-part overflow pass in silence.
    if v.placement == BodyPlacement::ShortMessage {
        for (index, seg) in segments.iter().enumerate() {
            let Some(FieldValue::Bytes(body)) = seg.fields.get("short_message") else {
                continue;
            };
            if body.len() > SHORT_MESSAGE_MAX {
                let which = if segments.len() == 1 {
                    "the body is".to_string()
                } else {
                    format!("part {} of {} is", index + 1, segments.len())
                };
                warnings.push(format!(
                    "{which} {} octets but sm_length is a single octet, so it will be written \
                     as {}. Segment the message or use message_payload. Sending it anyway.",
                    body.len(),
                    body.len() & 0xff
                ));
            }
        }
    }

    let manual_tlvs: Vec<WireTlv> = v
        .tlvs
        .iter()
        .filter(|row| !clean_hex(&row.tag_hex).is_empty())
        .map(|row| WireTlv {
            tag: u16::from_str_radix(&clean_hex(&row.tag_hex), 16).unwrap_or(0),
            hex: match &row.value_hex {
                Some(h) if !clean_hex(h).is_empty() => clean_hex(h),
                // Latin-1: one octet per character, which is what an SMPP
                // octet string is.
                _ => latin1_hex(row.value_text.as_deref().unwrap_or("")),
            },
        })
        .collect();

    let submit_sm = command_id("submit_sm").unwrap_or(4);

    let requests = segments
        .iter()
        .map(|seg| {
            let mut req = SendPduRequest {
                command_id: submit_sm,
                ..Default::default()
            };

            req.fields
                .insert("service_type".into(), v.service_type.clone().into());
            req.fields
                .insert("source_addr_ton".into(), v.source_addr_ton.into());
            req.fields
                .insert("source_addr_npi".into(), v.source_addr_npi.into());
            req.fields
                .insert("source_addr".into(), v.source_addr.clone().into());
            req.fields
                .insert("dest_addr_ton".into(), v.dest_addr_ton.into());
            req.fields
                .insert("dest_addr_npi".into(), v.dest_addr_npi.into());
            req.fields
                .insert("destination_addr".into(), v.destination_addr.clone().into());
            // A segment carries its own esm_class when it sets the UDHI bit.
            let esm = match seg.fields.get("esm_class") {
                Some(FieldValue::Num(n)) => *n as u8,
                _ => v.esm_class,
            };
            req.fields.insert("esm_class".into(), esm.into());
            req.fields
                .insert("protocol_id".into(), v.protocol_id.into());
            req.fields
                .insert("priority_flag".into(), v.priority_flag.into());
            req.fields.insert(
                "schedule_delivery_time".into(),
                v.schedule_delivery_time.clone().into(),
            );
            req.fields
                .insert("validity_period".into(), v.validity_period.clone().into());
            req.fields
                .insert("registered_delivery".into(), v.registered_delivery.into());
            req.fields.insert(
                "replace_if_present_flag".into(),
                v.replace_if_present.into(),
            );
            req.fields
                .insert("data_coding".into(), v.data_coding.into());
            req.fields
                .insert("sm_default_msg_id".into(), v.sm_default_msg_id.into());

            // The body crosses as bytes. Handing it over as text would have
            // the renderer's encoding re-applied on the way out.
            for (name, value) in &seg.fields {
                if let FieldValue::Bytes(bytes) = value {
                    req.octet_fields
                        .insert(name.clone(), crate::smpp::summary::hex(bytes));
                }
            }
            if let Some(FieldValue::Num(n)) = seg.fields.get("sm_length") {
                req.fields.insert("sm_length".into(), (*n).into());
            }

            req.tlvs = seg
                .tlvs
                .iter()
                .map(|t| WireTlv {
                    tag: t.tag,
                    hex: crate::smpp::summary::hex(&t.value),
                })
                .chain(manual_tlvs.iter().cloned())
                .collect();
            req
        })
        .collect();

    ComposedSubmit {
        requests,
        warnings,
        measurement,
    }
}

/// What a hand-written PDU decodes to, and whether there is anything to send.
pub fn preview_pdu(hex: &str) -> PduPreview {
    let bytes = hex_to_bytes(hex);
    let sendable = !bytes.is_empty();

    match decode_pdu(&bytes) {
        Ok(pdu) => PduPreview {
            ok: true,
            error: None,
            summary: summarise(&pdu),
            fields: describe(&pdu),
            byte_count: bytes.len(),
            sendable,
        },
        Err(e) => PduPreview {
            ok: false,
            error: Some(e.message),
            summary: String::new(),
            fields: Vec::new(),
            byte_count: bytes.len(),
            sendable,
        },
    }
}

fn latin1_hex(s: &str) -> String {
    s.chars()
        .map(|c| {
            let byte = if (c as u32) <= 0xff { c as u8 } else { b'?' };
            format!("{byte:02x}")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smpp::constants::command_id;
    use crate::smpp::text::{Gsm7Packing, SegmentationMode};

    fn form(text: &str) -> SubmitFormValues {
        SubmitFormValues {
            service_type: String::new(),
            source_addr: "InfoSMS".into(),
            destination_addr: "447700900123".into(),
            data_coding: 0,
            text: text.into(),
            segmentation: SegmentationMode::Udh,
            packing: Gsm7Packing::Unpacked,
            ..Default::default()
        }
    }

    #[test]
    fn composes_one_submit_sm_for_a_short_message() {
        let out = compose_submit(&form("hello"));
        assert_eq!(out.requests.len(), 1);
        assert_eq!(out.requests[0].command_id, command_id("submit_sm").unwrap());
        assert_eq!(out.warnings, Vec::<String>::new());
    }

    #[test]
    fn composes_one_pdu_per_segment_for_a_long_message() {
        let out = compose_submit(&form(&"a".repeat(200)));
        assert_eq!(
            out.requests.len(),
            2,
            "200 GSM characters is two concatenated parts"
        );
        assert_eq!(
            out.measurement.parts, 2,
            "the counter must agree with what was composed"
        );
    }

    #[test]
    fn carries_the_body_as_an_octet_field_not_as_text() {
        let out = compose_submit(&form("hello"));
        assert!(
            out.requests[0].octet_fields.contains_key("short_message"),
            "the body must cross as bytes: text would be re-encoded on the way"
        );
        assert!(!out.requests[0].fields.contains_key("short_message"));
    }

    #[test]
    fn warns_about_a_short_destination_without_withholding_the_pdus() {
        let mut v = form("hello");
        v.destination_addr = "12345".into();
        let out = compose_submit(&v);
        assert_eq!(out.requests.len(), 1, "a warning is not a refusal");
        assert!(
            out.warnings
                .iter()
                .any(|w| w.contains("destination_addr") && w.contains("anyway")),
            "expected a destination warning, got {:?}",
            out.warnings
        );
        assert_eq!(
            out.requests[0].fields.get("destination_addr"),
            Some(&serde_json::json!("12345")),
            "the short address must reach the wire exactly as typed"
        );
    }

    #[test]
    fn warns_about_an_empty_destination_and_still_composes() {
        let mut v = form("hello");
        v.destination_addr = String::new();
        let out = compose_submit(&v);
        assert_eq!(out.requests.len(), 1);
        assert!(
            out.warnings.iter().any(|w| w.contains("empty")),
            "{:?}",
            out.warnings
        );
    }

    #[test]
    fn warns_about_a_service_type_longer_than_the_spec_allows() {
        let mut v = form("hello");
        v.service_type = "TOOLONG".into();
        let out = compose_submit(&v);
        assert!(
            out.warnings.iter().any(|w| w.contains("service_type")),
            "{:?}",
            out.warnings
        );
        assert_eq!(
            out.requests[0].fields.get("service_type"),
            Some(&serde_json::json!("TOOLONG"))
        );
    }

    #[test]
    fn warns_about_a_data_coding_the_spec_does_not_list() {
        let mut v = form("hello");
        v.data_coding = 0xf6;
        let out = compose_submit(&v);
        assert!(
            out.warnings.iter().any(|w| w.contains("data_coding")),
            "{:?}",
            out.warnings
        );
        assert_eq!(
            out.requests[0].fields.get("data_coding"),
            Some(&serde_json::json!(0xf6))
        );
    }

    #[test]
    fn warns_about_characters_the_coding_cannot_represent() {
        let mut v = form("snow \u{2744} man");
        v.data_coding = 0;
        let out = compose_submit(&v);
        assert!(
            out.warnings.iter().any(|w| w.contains("question marks")),
            "{:?}",
            out.warnings
        );
    }

    #[test]
    fn warns_when_the_body_overflows_sm_length_and_composes_anyway() {
        let mut v = form(&"a".repeat(300));
        v.segmentation = SegmentationMode::None;
        let out = compose_submit(&v);
        assert_eq!(
            out.requests.len(),
            1,
            "one PDU was asked for, one is composed"
        );
        assert!(
            out.warnings.iter().any(|w| w.contains("sm_length")),
            "the overflow must be named: {:?}",
            out.warnings
        );
    }

    #[test]
    fn warns_about_the_body_that_was_actually_composed_not_the_text_that_was_typed() {
        // A hand-written UDH turns concatenation off, so one PDU carries the
        // UDH octets as well as the body. The warning used to name the text's
        // own length, so it told the user a number the wire never saw.
        let mut v = form(&"a".repeat(400));
        v.udh_hex = "050003010201".into();
        let out = compose_submit(&v);

        assert_eq!(out.requests.len(), 1);
        let body = out.requests[0].octet_fields.get("short_message").unwrap();
        let octets = body.len() / 2;
        assert_eq!(octets, 406, "6 UDH octets and 400 of text");
        assert!(
            out.warnings
                .iter()
                .any(|w| w.contains("406") && w.contains(&(406 & 0xff).to_string())),
            "the warning should name the body that was composed: {:?}",
            out.warnings
        );
    }

    #[test]
    fn warns_when_a_segment_overflows_even_though_there_are_several() {
        // The guard used to be "one part only", so a multi-part message whose
        // parts were each too long said nothing at all.
        let mut v = form(&"\u{3042}".repeat(200));
        v.data_coding = 5;
        v.segmentation = SegmentationMode::None;
        let out = compose_submit(&v);
        assert!(
            out.warnings.iter().any(|w| w.contains("sm_length")),
            "an overflowing body must be named whatever the part count: {:?}",
            out.warnings
        );
    }

    #[test]
    fn a_hand_written_udh_turns_concatenation_off() {
        let mut v = form(&"a".repeat(200));
        v.udh_hex = "050003010201".into();
        let out = compose_submit(&v);
        assert_eq!(
            out.requests.len(),
            1,
            "the user's own UDH owns the concatenation"
        );
        assert_eq!(out.measurement.parts, 1, "the counter must agree");
    }

    #[test]
    fn keeps_the_users_tlv_rows_beside_the_segmentation_tlvs() {
        let mut v = form(&"a".repeat(200));
        v.segmentation = SegmentationMode::Sar;
        v.tlvs = vec![TlvRow {
            tag_hex: "0424".into(),
            value_hex: Some("deadbeef".into()),
            value_text: None,
        }];
        let out = compose_submit(&v);
        assert_eq!(out.requests.len(), 2);
        for req in &out.requests {
            let tags: Vec<u16> = req.tlvs.iter().map(|t| t.tag).collect();
            assert!(
                tags.contains(&0x0424),
                "the user's TLV was dropped from a part: {tags:?}"
            );
            assert!(tags.len() > 1, "the sar_* TLVs were dropped: {tags:?}");
        }
    }

    #[test]
    fn reads_a_tlv_value_typed_as_text_as_latin_1() {
        let mut v = form("hello");
        v.tlvs = vec![TlvRow {
            tag_hex: "0x1400".into(),
            value_hex: None,
            value_text: Some("Ab".into()),
        }];
        let out = compose_submit(&v);
        let tlv = out.requests[0].tlvs.last().unwrap();
        assert_eq!(tlv.tag, 0x1400, "the 0x prefix is the user's to type");
        assert_eq!(tlv.hex, "4162");
    }

    #[test]
    fn skips_a_tlv_row_with_no_tag() {
        let mut v = form("hello");
        v.tlvs = vec![TlvRow {
            tag_hex: String::new(),
            value_hex: Some("ff".into()),
            value_text: None,
        }];
        let out = compose_submit(&v);
        assert!(
            out.requests[0].tlvs.is_empty(),
            "an empty row is a row the user has not filled in"
        );
    }

    #[test]
    fn previews_a_decodable_pdu_with_its_fields() {
        // An enquire_link: 16 bytes of header and nothing else.
        let preview = preview_pdu("00000010000000150000000000000001");
        assert!(preview.ok, "{:?}", preview.error);
        assert_eq!(preview.byte_count, 16);
        assert!(preview.sendable);
        assert!(
            preview.summary.contains("enquire_link"),
            "{}",
            preview.summary
        );
        assert!(!preview.fields.is_empty());
    }

    #[test]
    fn keeps_undecodable_bytes_sendable() {
        // Four bytes are not a PDU. Sending them is still the point.
        let preview = preview_pdu("deadbeef");
        assert!(!preview.ok);
        assert!(preview.error.is_some());
        assert!(
            preview.sendable,
            "refusing to send malformed bytes would defeat the tool"
        );
        assert_eq!(preview.byte_count, 4);
    }

    #[test]
    fn has_nothing_to_send_for_an_empty_box() {
        let preview = preview_pdu("");
        assert!(!preview.sendable);
        assert_eq!(preview.byte_count, 0);
    }

    #[test]
    fn previews_a_command_length_that_lies_without_refusing_it() {
        // 0xdeadbeef declared, 16 bytes present. The decoder must say so and
        // the bytes must still be sendable.
        let preview = preview_pdu("deadbeef 00000015 00000000 00000001");
        assert!(!preview.ok);
        assert!(preview.sendable);
    }
}
