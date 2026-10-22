//! One-line and expanded renderings of a PDU, for the trace.

use crate::smpp::constants::{
    command_name, command_status_name, tlv_name, ESME_ROK, TLV_MESSAGE_PAYLOAD,
};
use crate::smpp::decode::{FieldValue, Pdu};
use crate::smpp::defs::fields_for;
use crate::smpp::receipt::parse_receipt;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DescribedField {
    pub name: String,
    pub value: String,
}

fn render(value: &FieldValue) -> String {
    match value {
        FieldValue::Num(n) => n.to_string(),
        FieldValue::Str(s) => s.clone(),
        FieldValue::Bytes(b) => hex(b),
        FieldValue::DestList(l) => format!("{l:?}"),
        FieldValue::UnsuccessList(l) => format!("{l:?}"),
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The one-line decoded form shown in the trace table: the command, who it is
/// between, how big the body is, the message id, the receipt state, and the
/// status when it is not OK.
pub fn summarise(pdu: &Pdu) -> String {
    let name = command_name(pdu.command_id);
    let mut parts = vec![name];

    let src = pdu.str_field("source_addr");
    let dst = pdu.str_field("destination_addr");
    match (&src, &dst) {
        (Some(s), Some(d)) if !s.is_empty() || !d.is_empty() => parts.push(format!(
            "{} -> {}",
            if s.is_empty() { "-" } else { s },
            if d.is_empty() { "-" } else { d }
        )),
        _ => {
            if let Some(system_id) = pdu.str_field("system_id").filter(|s| !s.is_empty()) {
                parts.push(system_id);
            }
        }
    }

    if let Some(id) = pdu.str_field("message_id").filter(|s| !s.is_empty()) {
        parts.push(format!("id={id}"));
    }

    let body_length = pdu
        .bytes_field("short_message")
        .map(|b| b.len())
        .or_else(|| {
            pdu.tlvs
                .iter()
                .find(|t| t.tag == TLV_MESSAGE_PAYLOAD)
                .map(|t| t.value.len())
        })
        .unwrap_or(0);
    if body_length > 0 {
        parts.push(format!("{body_length} bytes"));
    }

    if let Some(receipt) = parse_receipt(pdu) {
        if let Some(id) = receipt.message_id {
            parts.push(format!("receipt id={id}"));
        }
        if let Some(state) = receipt.state {
            parts.push(state);
        }
    }

    if pdu.command_status != ESME_ROK {
        parts.push(command_status_name(pdu.command_status));
    }

    if !pdu.tlvs.is_empty() {
        parts.push(format!("{} TLV", pdu.tlvs.len()));
    }

    parts.join(" ")
}

/// Every field of the PDU, header first, for the expanded view.
pub fn describe(pdu: &Pdu) -> Vec<DescribedField> {
    let row = |name: &str, value: String| DescribedField {
        name: name.to_string(),
        value,
    };

    let mut rows = vec![
        row("command_length", pdu.raw.len().to_string()),
        row(
            "command_id",
            format!("{} (0x{:x})", command_name(pdu.command_id), pdu.command_id),
        ),
        row(
            "command_status",
            format!(
                "{} ({})",
                command_status_name(pdu.command_status),
                pdu.command_status
            ),
        ),
        row("sequence_number", pdu.sequence_number.to_string()),
    ];

    match fields_for(&command_name(pdu.command_id)) {
        Some(defs) => {
            for def in defs {
                if let Some(v) = pdu.fields.get(def.name) {
                    rows.push(row(def.name, render(v)));
                }
            }
        }
        None => {
            if let Some(body) = pdu.fields.get("body") {
                rows.push(row("body", render(body)));
            }
        }
    }

    for tlv in &pdu.tlvs {
        rows.push(row(&tlv_name(tlv.tag), hex(&tlv.value)));
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smpp::constants::{command_id, command_status};
    use crate::smpp::decode::decode_pdu;
    use crate::smpp::encode::{encode_pdu, PduInput};
    use crate::smpp::tlv::Tlv;

    fn pdu_of(input: PduInput) -> Pdu {
        decode_pdu(&encode_pdu(&input)).unwrap()
    }

    fn submit() -> Pdu {
        pdu_of(
            PduInput::new(command_id("submit_sm").unwrap(), 42)
                .with_str("source_addr", "393331234567")
                .with_str("destination_addr", "393337654321")
                .with_bytes("short_message", b"hello".to_vec()),
        )
    }

    #[test]
    fn names_the_command_and_the_two_addresses_for_submit_sm() {
        let s = summarise(&submit());
        assert!(s.contains("submit_sm"), "{s}");
        assert!(s.contains("393331234567"), "{s}");
        assert!(s.contains("393337654321"), "{s}");
    }

    #[test]
    fn reports_the_body_length_for_submit_sm() {
        assert!(summarise(&submit()).contains("5 bytes"));
    }

    #[test]
    fn names_submit_sm_resp_with_its_message_id() {
        let p = pdu_of(PduInput::new(command_id("submit_sm_resp").unwrap(), 42).with_str("message_id", "A7F2"));
        assert!(summarise(&p).contains("A7F2"));
    }

    #[test]
    fn names_the_status_when_it_is_not_ok() {
        let p = pdu_of(
            PduInput::new(command_id("submit_sm_resp").unwrap(), 1)
                .with_status(command_status("ESME_RTHROTTLED").unwrap()),
        );
        assert!(summarise(&p).contains("ESME_RTHROTTLED"));
    }

    #[test]
    fn does_not_clutter_a_successful_response_with_esme_rok() {
        let p = pdu_of(PduInput::new(command_id("enquire_link_resp").unwrap(), 1));
        assert!(!summarise(&p).contains("ESME_ROK"));
    }

    #[test]
    fn reports_the_receipt_state_for_a_deliver_sm_that_is_one() {
        let p = pdu_of(
            PduInput::new(command_id("deliver_sm").unwrap(), 7)
                .with_num("esm_class", 4)
                .with_bytes("short_message", b"id:A7F2 stat:DELIVRD err:000".to_vec()),
        );
        let s = summarise(&p);
        assert!(s.contains("DELIVRD"), "{s}");
        assert!(s.contains("A7F2"), "{s}");
    }

    #[test]
    fn names_an_unknown_command_by_its_id() {
        let p = pdu_of(PduInput::new(0x4242, 1));
        assert!(summarise(&p).contains("0x4242"));
    }

    #[test]
    fn names_the_bind_system_id() {
        let p = pdu_of(PduInput::new(command_id("bind_transceiver").unwrap(), 1).with_str("system_id", "esme"));
        assert!(summarise(&p).contains("esme"));
    }

    #[test]
    fn mentions_the_tlv_count_when_a_pdu_carries_tlvs() {
        let p = pdu_of(
            PduInput::new(command_id("submit_sm").unwrap(), 1)
                .with_tlv(Tlv::new(0x0424, b"x".to_vec()))
                .with_tlv(Tlv::new(0x020c, vec![1, 2])),
        );
        assert!(summarise(&p).contains("2 TLV"));
    }

    #[test]
    fn lists_every_mandatory_field_as_a_name_value_pair() {
        let rows = describe(&submit());
        let find = |name: &str| rows.iter().find(|r| r.name == name).map(|r| r.value.clone());
        assert_eq!(find("source_addr").as_deref(), Some("393331234567"));
        assert_eq!(find("destination_addr").as_deref(), Some("393337654321"));
    }

    #[test]
    fn includes_the_header_fields() {
        let rows = describe(&submit());
        let find = |name: &str| rows.iter().find(|r| r.name == name).map(|r| r.value.clone()).unwrap();
        assert!(find("command_id").contains("submit_sm"));
        assert_eq!(find("sequence_number"), "42");
        assert!(find("command_status").contains("ESME_ROK"));
    }

    #[test]
    fn renders_an_octet_field_as_hex() {
        let rows = describe(&submit());
        let sm = rows.iter().find(|r| r.name == "short_message").unwrap();
        assert_eq!(sm.value, "68656c6c6f");
    }

    #[test]
    fn names_each_tlv_by_its_tag_name() {
        let p = pdu_of(PduInput::new(command_id("submit_sm").unwrap(), 1).with_tlv(Tlv::new(0x0424, b"hi".to_vec())));
        assert!(describe(&p).iter().any(|r| r.name == "message_payload"));
    }

    #[test]
    fn names_an_unknown_tlv_by_its_hex_tag() {
        let p = pdu_of(PduInput::new(command_id("submit_sm").unwrap(), 1).with_tlv(Tlv::new(0x9999, vec![1])));
        assert!(describe(&p).iter().any(|r| r.name == "unknown(0x9999)"));
    }
}
