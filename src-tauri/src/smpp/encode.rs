//! Encoding a PDU from a field model.
//!
//! No validation, no clamping, no truncation: an over-long string, an
//! out-of-range enum value and a length field that contradicts its payload all
//! go on the wire exactly as given. The only field this computes is
//! `command_length`, and a length or count field the caller supplied always
//! wins over the derived one.

use indexmap::IndexMap;

use crate::smpp::constants::{command_name, PDU_HEADER_LENGTH};
use crate::smpp::decode::{DestEntry, FieldValue};
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
