//! Encoding a PDU from a field model.
//!
//! No validation, no clamping, no truncation: an over-long string, an
//! out-of-range enum value and a length field that contradicts its payload all
//! go on the wire exactly as given. The only field this computes is
//! `command_length`, and a length or count field the caller supplied always
//! wins over the derived one.

use indexmap::IndexMap;

use crate::smpp::constants::{command_name, PDU_HEADER_LENGTH};
use crate::smpp::decode::{DestEntry, FieldValue, UnsuccessEntry};
use crate::smpp::defs::{fields_for, FieldType};
use crate::smpp::tlv::{encode_tlvs, Tlv};

#[derive(Debug, Clone, Default)]
pub struct PduInput {
    pub command_id: u32,
    pub command_status: u32,
    pub sequence_number: u32,
    pub fields: IndexMap<String, FieldValue>,
    pub tlvs: Vec<Tlv>,
}

impl PduInput {
    pub fn new(command_id: u32, sequence_number: u32) -> Self {
        Self {
            command_id,
            command_status: 0,
            sequence_number,
            fields: IndexMap::new(),
            tlvs: Vec::new(),
        }
    }

    pub fn with_status(mut self, status: u32) -> Self {
        self.command_status = status;
        self
    }

    pub fn with_num(mut self, name: &str, value: u32) -> Self {
        self.fields.insert(name.to_string(), FieldValue::Num(value));
        self
    }

    pub fn with_str(mut self, name: &str, value: &str) -> Self {
        self.fields
            .insert(name.to_string(), FieldValue::Str(value.to_string()));
        self
    }

    pub fn with_bytes(mut self, name: &str, value: Vec<u8>) -> Self {
        self.fields
            .insert(name.to_string(), FieldValue::Bytes(value));
        self
    }

    /// The repeating list submit_multi_resp carries. Its count field is
    /// derived at encode time, like every other.
    pub fn with_unsuccess_list(mut self, name: &str, value: Vec<UnsuccessEntry>) -> Self {
        self.fields
            .insert(name.to_string(), FieldValue::UnsuccessList(value));
        self
    }

    pub fn with_dest_list(mut self, name: &str, value: Vec<DestEntry>) -> Self {
        self.fields
            .insert(name.to_string(), FieldValue::DestList(value));
        self
    }

    pub fn with_tlv(mut self, tlv: Tlv) -> Self {
        self.tlvs.push(tlv);
        self
    }

    pub fn with_tlvs(mut self, tlvs: Vec<Tlv>) -> Self {
        self.tlvs.extend(tlvs);
        self
    }
}

fn cstring(value: Option<&FieldValue>) -> Vec<u8> {
    let mut out: Vec<u8> = match value {
        Some(FieldValue::Str(s)) => s.chars().map(|c| c as u8).collect(),
        Some(FieldValue::Num(n)) => n.to_string().into_bytes(),
        Some(FieldValue::Bytes(b)) => b.clone(),
        _ => Vec::new(),
    };
    out.push(0);
    out
}

fn uint(value: Option<&FieldValue>, bytes: usize) -> Vec<u8> {
    let n = match value {
        Some(FieldValue::Num(n)) => *n,
        Some(FieldValue::Str(s)) => s.parse::<u32>().unwrap_or(0),
        _ => 0,
    };
    match bytes {
        1 => vec![(n & 0xff) as u8],
        2 => ((n & 0xffff) as u16).to_be_bytes().to_vec(),
        _ => n.to_be_bytes().to_vec(),
    }
}

fn octets(value: Option<&FieldValue>) -> Vec<u8> {
    match value {
        Some(FieldValue::Bytes(b)) => b.clone(),
        Some(FieldValue::Str(s)) => s.chars().map(|c| c as u8).collect(),
        _ => Vec::new(),
    }
}

fn dest_list(value: Option<&FieldValue>) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(FieldValue::DestList(entries)) = value {
        for e in entries {
            if e.flag == 2 {
                out.push(2);
                out.extend(cstring(Some(&FieldValue::Str(
                    e.dl_name.clone().unwrap_or_default(),
                ))));
            } else {
                out.push(if e.flag == 0 { 1 } else { e.flag });
                out.push(e.ton.unwrap_or(0));
                out.push(e.npi.unwrap_or(0));
                out.extend(cstring(Some(&FieldValue::Str(
                    e.addr.clone().unwrap_or_default(),
                ))));
            }
        }
    }
    out
}

fn unsuccess_list(value: Option<&FieldValue>) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(FieldValue::UnsuccessList(entries)) = value {
        for e in entries {
            out.push(e.ton);
            out.push(e.npi);
            out.extend(cstring(Some(&FieldValue::Str(e.addr.clone()))));
            out.extend(e.error_status.to_be_bytes());
        }
    }
    out
}

fn derived_count(value: Option<&FieldValue>) -> u32 {
    match value {
        Some(FieldValue::Bytes(b)) => b.len() as u32,
        Some(FieldValue::Str(s)) => s.chars().count() as u32,
        Some(FieldValue::DestList(l)) => l.len() as u32,
        Some(FieldValue::UnsuccessList(l)) => l.len() as u32,
        _ => 0,
    }
}

pub fn encode_pdu(input: &PduInput) -> Vec<u8> {
    let name = command_name(input.command_id);

    let body = match fields_for(&name) {
        // An id the protocol does not define: whatever the caller called the body.
        None => octets(input.fields.get("body")),
        Some(defs) => {
            let mut resolved = input.fields.clone();

            // Derive a count or length field only when the caller omitted it.
            for def in defs {
                let Some(from) = def.len_from else { continue };
                if resolved.contains_key(from) {
                    continue;
                }
                let n = derived_count(resolved.get(def.name));
                resolved.insert(from.to_string(), FieldValue::Num(n));
            }

            let mut body = Vec::new();
            for def in defs {
                let v = resolved.get(def.name);
                body.extend(match def.ty {
                    FieldType::CString => cstring(v),
                    FieldType::U8 => uint(v, 1),
                    FieldType::U16 => uint(v, 2),
                    FieldType::U32 => uint(v, 4),
                    FieldType::Octets => octets(v),
                    FieldType::DestList => dest_list(v),
                    FieldType::UnsuccessList => unsuccess_list(v),
                });
            }
            body
        }
    };

    let tlvs = if input.tlvs.is_empty() {
        Vec::new()
    } else {
        encode_tlvs(&input.tlvs)
    };

    let total = PDU_HEADER_LENGTH + body.len() + tlvs.len();
    let mut out = Vec::with_capacity(total);
    out.extend((total as u32).to_be_bytes());
    out.extend(input.command_id.to_be_bytes());
    out.extend(input.command_status.to_be_bytes());
    out.extend(input.sequence_number.to_be_bytes());
    out.extend(body);
    out.extend(tlvs);
    out
}

/// The hex composer's path onto the wire: the bytes go out exactly as
/// composed. Nothing is validated, nothing is rewritten — not even
/// `command_length`.
pub fn encode_raw(bytes: &[u8]) -> Vec<u8> {
    bytes.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smpp::constants::command_id;
    use crate::smpp::decode::{decode_pdu, DestEntry};
    use crate::smpp::tlv::Tlv;

    /// The bytes a command family puts on the wire, written out by hand from
    /// the SMPP specification.
    ///
    /// Every other encode test here round-trips through `decode_pdu`, and the
    /// encoder and the decoder read the same table in `defs.rs` — so a wrong
    /// field order, a swapped width or a misspelled name round-trips
    /// perfectly and every one of those tests passes while every bind against
    /// every real SMSC fails. These assertions are the only thing standing
    /// between that mistake and the wire.
    mod on_the_wire {
        use super::*;

        /// `command_length`, `command_id`, `command_status`, `sequence_number`.
        fn header(id: u32, status: u32, sequence: u32, body_len: usize) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&((16 + body_len) as u32).to_be_bytes());
            out.extend_from_slice(&id.to_be_bytes());
            out.extend_from_slice(&status.to_be_bytes());
            out.extend_from_slice(&sequence.to_be_bytes());
            out
        }

        fn expect(input: &PduInput, body: &[u8]) {
            let mut wanted = header(
                input.command_id,
                input.command_status,
                input.sequence_number,
                body.len(),
            );
            wanted.extend_from_slice(body);
            let got = encode_pdu(input);
            assert_eq!(
                got,
                wanted,
                "\n   got {}\nwanted {}",
                crate::smpp::summary::hex(&got),
                crate::smpp::summary::hex(&wanted)
            );
        }

        #[test]
        fn bind_transceiver() {
            // 4.1.5: system_id, password, system_type, interface_version,
            // addr_ton, addr_npi, address_range.
            let input = PduInput::new(id("bind_transceiver"), 1)
                .with_str("system_id", "esme")
                .with_str("password", "pw")
                .with_str("system_type", "SMPP")
                .with_num("interface_version", 0x34)
                .with_num("addr_ton", 1)
                .with_num("addr_npi", 1)
                .with_str("address_range", "^44");

            expect(
                &input,
                &[
                    b'e', b's', b'm', b'e', 0x00, // system_id
                    b'p', b'w', 0x00, // password
                    b'S', b'M', b'P', b'P', 0x00, // system_type
                    0x34, // interface_version
                    0x01, // addr_ton
                    0x01, // addr_npi
                    b'^', b'4', b'4', 0x00, // address_range
                ],
            );
        }

        #[test]
        fn bind_transceiver_resp() {
            // 4.1.6: system_id only.
            let input = PduInput::new(id("bind_transceiver_resp"), 1).with_str("system_id", "SMSC");
            expect(&input, &[b'S', b'M', b'S', b'C', 0x00]);
        }

        #[test]
        fn data_sm() {
            // 4.2.2: the addressed head, then esm_class, registered_delivery,
            // data_coding — and no sm_length or short_message at all.
            let input = PduInput::new(id("data_sm"), 2)
                .with_str("service_type", "CMT")
                .with_num("source_addr_ton", 5)
                .with_num("source_addr_npi", 0)
                .with_str("source_addr", "INFO")
                .with_num("dest_addr_ton", 1)
                .with_num("dest_addr_npi", 1)
                .with_str("destination_addr", "4477")
                .with_num("esm_class", 0x40)
                .with_num("registered_delivery", 1)
                .with_num("data_coding", 8);

            expect(
                &input,
                &[
                    b'C', b'M', b'T', 0x00, // service_type
                    0x05, 0x00, // source ton/npi
                    b'I', b'N', b'F', b'O', 0x00, // source_addr
                    0x01, 0x01, // dest ton/npi
                    b'4', b'4', b'7', b'7', 0x00, // destination_addr
                    0x40, // esm_class
                    0x01, // registered_delivery
                    0x08, // data_coding
                ],
            );
        }

        #[test]
        fn replace_sm() {
            // 4.10.1: message_id, the source address, the two times,
            // registered_delivery, sm_default_msg_id, sm_length, body.
            let input = PduInput::new(id("replace_sm"), 3)
                .with_str("message_id", "AB12")
                .with_num("source_addr_ton", 1)
                .with_num("source_addr_npi", 1)
                .with_str("source_addr", "4477")
                .with_str("schedule_delivery_time", "")
                .with_str("validity_period", "")
                .with_num("registered_delivery", 1)
                .with_num("sm_default_msg_id", 0)
                .with_bytes("short_message", b"hi".to_vec());

            expect(
                &input,
                &[
                    b'A', b'B', b'1', b'2', 0x00, // message_id
                    0x01, 0x01, // source ton/npi
                    b'4', b'4', b'7', b'7', 0x00, // source_addr
                    0x00, // schedule_delivery_time
                    0x00, // validity_period
                    0x01, // registered_delivery
                    0x00, // sm_default_msg_id
                    0x02, // sm_length, derived from the body
                    b'h', b'i',
                ],
            );
        }

        #[test]
        fn query_sm_resp() {
            // 4.8.2: message_id, final_date, message_state, error_code.
            let input = PduInput::new(id("query_sm_resp"), 4)
                .with_str("message_id", "AB12")
                .with_str("final_date", "261022120000000R")
                .with_num("message_state", 2)
                .with_num("error_code", 0);

            let mut body = vec![b'A', b'B', b'1', b'2', 0x00];
            body.extend_from_slice(b"261022120000000R");
            body.push(0x00);
            body.push(0x02);
            body.push(0x00);
            expect(&input, &body);
        }

        #[test]
        fn submit_multi_resp_with_one_failure() {
            // 4.5.2: message_id, no_unsuccess, then one entry per failure of
            // ton, npi, address, and a four-octet error status.
            let input = PduInput::new(id("submit_multi_resp"), 5)
                .with_str("message_id", "AB12")
                .with_unsuccess_list(
                    "unsuccess_sme",
                    vec![crate::smpp::decode::UnsuccessEntry {
                        ton: 1,
                        npi: 1,
                        addr: "4477".into(),
                        error_status: 0x0000_000b,
                    }],
                );

            expect(
                &input,
                &[
                    b'A', b'B', b'1', b'2', 0x00, // message_id
                    0x01, // no_unsuccess, derived from the list
                    0x01, 0x01, // ton/npi
                    b'4', b'4', b'7', b'7', 0x00, // destination_addr
                    0x00, 0x00, 0x00, 0x0b, // error_status
                ],
            );
        }

        #[test]
        fn broadcast_sm() {
            // 4.4.1: service_type, the source address, message_id,
            // priority_flag, the two times, replace_if_present, data_coding,
            // sm_default_msg_id. The body travels in a TLV, not here.
            let input = PduInput::new(id("broadcast_sm"), 6)
                .with_str("service_type", "")
                .with_num("source_addr_ton", 1)
                .with_num("source_addr_npi", 1)
                .with_str("source_addr", "4477")
                .with_str("message_id", "")
                .with_num("priority_flag", 0)
                .with_str("schedule_delivery_time", "")
                .with_str("validity_period", "")
                .with_num("replace_if_present_flag", 0)
                .with_num("data_coding", 0)
                .with_num("sm_default_msg_id", 0);

            expect(
                &input,
                &[
                    0x00, // service_type
                    0x01, 0x01, // source ton/npi
                    b'4', b'4', b'7', b'7', 0x00, // source_addr
                    0x00, // message_id
                    0x00, // priority_flag
                    0x00, // schedule_delivery_time
                    0x00, // validity_period
                    0x00, // replace_if_present_flag
                    0x00, // data_coding
                    0x00, // sm_default_msg_id
                ],
            );
        }

        #[test]
        fn alert_notification() {
            // 4.11.1: the source address, then the ESME address. No status
            // field of its own and no response.
            let input = PduInput::new(id("alert_notification"), 7)
                .with_num("source_addr_ton", 1)
                .with_num("source_addr_npi", 1)
                .with_str("source_addr", "4477")
                .with_num("esme_addr_ton", 0)
                .with_num("esme_addr_npi", 0)
                .with_str("esme_addr", "ESME");

            expect(
                &input,
                &[
                    0x01, 0x01, // source ton/npi
                    b'4', b'4', b'7', b'7', 0x00, // source_addr
                    0x00, 0x00, // esme ton/npi
                    b'E', b'S', b'M', b'E', 0x00, // esme_addr
                ],
            );
        }

        #[test]
        fn submit_multi_puts_the_count_before_the_addresses() {
            // 4.5.1: number_of_dests, then one entry each, before the tail
            // submit_sm shares.
            let input = PduInput::new(id("submit_multi"), 8)
                .with_str("service_type", "")
                .with_num("source_addr_ton", 1)
                .with_num("source_addr_npi", 1)
                .with_str("source_addr", "44")
                .with_dest_list(
                    "dest_address",
                    vec![
                        DestEntry {
                            flag: 1,
                            ton: Some(1),
                            npi: Some(1),
                            addr: Some("4477".into()),
                            dl_name: None,
                        },
                        DestEntry {
                            flag: 2,
                            ton: None,
                            npi: None,
                            addr: None,
                            dl_name: Some("list".into()),
                        },
                    ],
                );

            let got = encode_pdu(&input);
            // Up to and including the two destination entries; the tail is
            // zeros and is covered by the round-trip tests.
            // service_type, ton/npi, source_addr, the count, then the two
            // entries: 21 octets before the tail submit_sm shares.
            let head = &got[16..16 + 21];
            assert_eq!(
                head,
                &[
                    0x00, // service_type
                    0x01, 0x01, // source ton/npi
                    b'4', b'4', 0x00, // source_addr
                    0x02, // number_of_dests, derived from the list
                    0x01, // dest_flag: an address
                    0x01, 0x01, // ton/npi
                    b'4', b'4', b'7', b'7', 0x00, // destination_addr
                    0x02, // dest_flag: a distribution list
                    b'l', b'i', b's', b't', 0x00,
                ],
                "\n   got {}",
                crate::smpp::summary::hex(&got)
            );
        }

        #[test]
        fn enquire_link_is_a_header_and_nothing_else() {
            let input = PduInput::new(id("enquire_link"), 9);
            expect(&input, &[]);
        }
    }

    fn id(name: &str) -> u32 {
        command_id(name).unwrap()
    }

    fn submit() -> PduInput {
        PduInput::new(id("submit_sm"), 1)
            .with_str("service_type", "")
            .with_num("source_addr_ton", 1)
            .with_num("source_addr_npi", 1)
            .with_str("source_addr", "393331234567")
            .with_num("dest_addr_ton", 1)
            .with_num("dest_addr_npi", 1)
            .with_str("destination_addr", "393337654321")
            .with_num("esm_class", 0)
            .with_num("registered_delivery", 1)
            .with_num("data_coding", 0)
            .with_bytes("short_message", b"hello".to_vec())
    }

    #[test]
    fn round_trips_submit_sm_through_encode_and_decode() {
        let p = decode_pdu(&encode_pdu(&submit())).unwrap();
        assert_eq!(p.command_id, id("submit_sm"));
        assert_eq!(p.sequence_number, 1);
        assert_eq!(p.str_field("source_addr").unwrap(), "393331234567");
        assert_eq!(p.str_field("destination_addr").unwrap(), "393337654321");
        assert_eq!(p.u32_field("registered_delivery"), Some(1));
        assert_eq!(p.bytes_field("short_message").unwrap(), b"hello".to_vec());
    }

    #[test]
    fn writes_command_length_to_match_the_produced_buffer() {
        let buf = encode_pdu(&submit());
        assert_eq!(
            u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize,
            buf.len()
        );
    }

    #[test]
    fn writes_an_over_long_service_type_verbatim() {
        let buf = encode_pdu(&submit().with_str("service_type", "ABCDEFGHIJKL"));
        assert_eq!(
            decode_pdu(&buf).unwrap().str_field("service_type").unwrap(),
            "ABCDEFGHIJKL"
        );
    }

    #[test]
    fn writes_a_data_coding_outside_the_spec_range_verbatim() {
        let buf = encode_pdu(&submit().with_num("data_coding", 0xff));
        assert_eq!(
            decode_pdu(&buf).unwrap().u32_field("data_coding"),
            Some(0xff)
        );
    }

    #[test]
    fn derives_sm_length_when_the_caller_omits_it() {
        let p = decode_pdu(&encode_pdu(&submit())).unwrap();
        assert_eq!(p.u32_field("sm_length"), Some(5));
    }

    #[test]
    fn keeps_a_caller_supplied_sm_length_that_disagrees_with_short_message() {
        let buf = encode_pdu(
            &submit()
                .with_num("sm_length", 200)
                .with_bytes("short_message", b"hi".to_vec()),
        );
        // The declared length goes on the wire as given; the bytes are the two supplied.
        let at = buf.len() - 3;
        assert_eq!(buf[at], 200);
        assert_eq!(&buf[at + 1..], b"hi");
        // And such a PDU is exactly as broken as the caller asked for.
        assert!(decode_pdu(&buf).is_err());
    }

    #[test]
    fn writes_a_zero_value_for_every_omitted_field() {
        let p = decode_pdu(&encode_pdu(&PduInput::new(id("submit_sm"), 9))).unwrap();
        assert_eq!(p.str_field("source_addr").unwrap(), "");
        assert_eq!(p.u32_field("esm_class"), Some(0));
        assert_eq!(p.u32_field("sm_length"), Some(0));
        assert_eq!(p.bytes_field("short_message").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn appends_tlvs_after_the_mandatory_fields() {
        let tlv = Tlv::new(0x0424, b"payload".to_vec());
        let p = decode_pdu(&encode_pdu(&submit().with_tlv(tlv.clone()))).unwrap();
        assert_eq!(p.tlvs, vec![tlv]);
        assert_eq!(p.bytes_field("short_message").unwrap(), b"hello".to_vec());
    }

    #[test]
    fn encodes_bind_transceiver() {
        let p = decode_pdu(&encode_pdu(
            &PduInput::new(id("bind_transceiver"), 1)
                .with_str("system_id", "esme")
                .with_str("password", "secret")
                .with_str("system_type", "SMPP")
                .with_num("interface_version", 0x34),
        ))
        .unwrap();
        assert_eq!(p.str_field("system_id").unwrap(), "esme");
        assert_eq!(p.u32_field("interface_version"), Some(0x34));
    }

    #[test]
    fn encodes_submit_multi_with_a_destination_list() {
        let dests = vec![
            DestEntry {
                flag: 1,
                ton: Some(1),
                npi: Some(1),
                addr: Some("393331111111".into()),
                dl_name: None,
            },
            DestEntry {
                flag: 2,
                dl_name: Some("my-list".into()),
                ..Default::default()
            },
        ];
        let p = decode_pdu(&encode_pdu(
            &PduInput::new(id("submit_multi"), 1)
                .with_str("source_addr", "111")
                .with_dest_list("dest_address", dests.clone())
                .with_bytes("short_message", b"hi".to_vec()),
        ))
        .unwrap();
        assert_eq!(p.u32_field("number_of_dests"), Some(2));
        match p.fields.get("dest_address") {
            Some(crate::smpp::decode::FieldValue::DestList(list)) => assert_eq!(*list, dests),
            other => panic!("expected a destination list, got {other:?}"),
        }
    }

    #[test]
    fn encodes_an_unknown_command_id_from_a_raw_body() {
        let p = decode_pdu(&encode_pdu(
            &PduInput::new(0x4242, 1).with_bytes("body", vec![1, 2, 3, 4]),
        ))
        .unwrap();
        assert_eq!(p.bytes_field("body").unwrap(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn encode_raw_returns_the_bytes_untouched() {
        let mut bytes = vec![0u8; 20];
        bytes[..4].copy_from_slice(&0xdead_beefu32.to_be_bytes());
        assert_eq!(encode_raw(&bytes), bytes);
        assert_eq!(
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            0xdead_beef
        );
    }

    #[test]
    fn encode_raw_does_not_rewrite_a_truncated_pdu() {
        let bytes = vec![0x00, 0x00, 0x00, 0x10, 0x00];
        assert_eq!(encode_raw(&bytes), bytes);
    }
}
