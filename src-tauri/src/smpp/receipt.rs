//! Reading a delivery receipt out of an inbound PDU.
//!
//! A PDU counts as a receipt when esm_class says so, or when it carries a
//! receipted_message_id TLV, or when its body reads as receipt text. Real
//! SMSCs do all three, and some send receipt text with esm_class 0 — this is
//! an interop-testing tool, so it recognises what arrives rather than what the
//! specification wishes had arrived.

use crate::smpp::constants::{
    command_id, TLV_MESSAGE_PAYLOAD, TLV_MESSAGE_STATE, TLV_RECEIPTED_MESSAGE_ID,
};
use crate::smpp::decode::Pdu;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReceiptSource {
    Tlv,
    Text,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Receipt {
    pub message_id: Option<String>,
    /// Where the message id came from: the TLV, or the receipt text.
    pub source: ReceiptSource,
    pub state: Option<String>,
    pub error_code: Option<String>,
    pub submit_date: Option<String>,
    pub done_date: Option<String>,
}

/// esm_class message-type bits: 0x04 delivery receipt, 0x20 intermediate notification.
const ESM_MESSAGE_TYPE_MASK: u32 = 0x3c;
const ESM_DELIVERY_RECEIPT: u32 = 0x04;
const ESM_INTERMEDIATE_NOTIFICATION: u32 = 0x20;

fn message_state_name(value: u8) -> String {
    match value {
        0 => "SCHEDULED".into(),
        1 => "ENROUTE".into(),
        2 => "DELIVERED".into(),
        3 => "EXPIRED".into(),
        4 => "DELETED".into(),
        5 => "UNDELIVERABLE".into(),
        6 => "ACCEPTED".into(),
        7 => "UNKNOWN".into(),
        8 => "REJECTED".into(),
        9 => "SKIPPED".into(),
        other => format!("state({other})"),
    }
}

fn body_text(pdu: &Pdu) -> String {
    if let Some(sm) = pdu.bytes_field("short_message") {
        if !sm.is_empty() {
            return sm.iter().map(|b| *b as char).collect();
        }
    }
    pdu.tlvs
        .iter()
        .find(|t| t.tag == TLV_MESSAGE_PAYLOAD)
        .map(|t| t.value.iter().map(|b| *b as char).collect())
        .unwrap_or_default()
}

/// `key:value` out of receipt text, case-insensitively, tolerating unknown keys.
fn field(text: &str, key: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let needle = format!("{}:", key.to_ascii_lowercase());
    let mut from = 0usize;
    while let Some(found) = lower[from..].find(&needle) {
        let at = from + found;
        let preceded_by_space = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        if preceded_by_space {
            let start = at + needle.len();
            let rest = &text[start..];
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            return Some(rest[..end].to_string());
        }
        from = at + needle.len();
    }
    None
}

/// `submit date:` and `done date:` carry a space inside the key.
fn date_field(text: &str, which: &str) -> Option<String> {
    field(text, &format!("{which} date")).or_else(|| {
        // Tolerate the space: look for "<which> date:" directly.
        let lower = text.to_ascii_lowercase();
        let needle = format!("{which} date:");
        lower.find(&needle).map(|at| {
            let rest = &text[at + needle.len()..];
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            rest[..end].to_string()
        })
    })
}

fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    bytes[..end].iter().map(|b| *b as char).collect()
}

pub fn parse_receipt(pdu: &Pdu) -> Option<Receipt> {
    let deliver = command_id("deliver_sm")?;
    let data_sm = command_id("data_sm")?;
    if pdu.command_id != deliver && pdu.command_id != data_sm {
        return None;
    }

    let esm = pdu.u32_field("esm_class").unwrap_or(0) & ESM_MESSAGE_TYPE_MASK;
    let by_esm = esm == ESM_DELIVERY_RECEIPT || esm == ESM_INTERMEDIATE_NOTIFICATION;

    let tlv_id = pdu
        .tlvs
        .iter()
        .find(|t| t.tag == TLV_RECEIPTED_MESSAGE_ID)
        .map(|t| cstr(&t.value));

    let text = body_text(pdu);
    let text_id = field(&text, "id");
    let state = field(&text, "stat");

    let looks_like_receipt = text_id.is_some() && (state.is_some() || by_esm);
    if !by_esm && tlv_id.is_none() && !looks_like_receipt {
        return None;
    }

    let state_from_tlv = pdu
        .tlvs
        .iter()
        .find(|t| t.tag == TLV_MESSAGE_STATE)
        .and_then(|t| t.value.first().copied())
        .map(message_state_name);

    let source = if tlv_id.is_some() {
        ReceiptSource::Tlv
    } else {
        ReceiptSource::Text
    };

    Some(Receipt {
        message_id: tlv_id.or(text_id),
        source,
        state: state.or(state_from_tlv),
        error_code: field(&text, "err"),
        submit_date: date_field(&text, "submit"),
        done_date: date_field(&text, "done"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_message_states_at_both_ends_of_the_table() {
        // 0 and 9 are SMPP 5.0's; without them an SMSC's state reads as a
        // bare number.
        assert_eq!(message_state_name(0), "SCHEDULED");
        assert_eq!(message_state_name(9), "SKIPPED");
        assert_eq!(message_state_name(2), "DELIVERED");
        assert_eq!(message_state_name(200), "state(200)");
    }

    use crate::smpp::constants::command_id;
    use crate::smpp::decode::decode_pdu;
    use crate::smpp::encode::{encode_pdu, PduInput};
    use crate::smpp::tlv::Tlv;

    const RECEIPT: &str = "id:0A1B2C3D sub:001 dlvrd:001 submit date:2610211200 done date:2610211201 stat:DELIVRD err:000 Text:hello";

    fn deliver(text: &str, esm_class: u32, tlvs: Vec<Tlv>) -> crate::smpp::decode::Pdu {
        let input = PduInput::new(command_id("deliver_sm").unwrap(), 1)
            .with_str("source_addr", "393331234567")
            .with_str("destination_addr", "393337654321")
            .with_num("esm_class", esm_class)
            .with_bytes("short_message", text.as_bytes().to_vec())
            .with_tlvs(tlvs);
        decode_pdu(&encode_pdu(&input)).unwrap()
    }

    #[test]
    fn prefers_the_receipted_message_id_tlv_over_the_text() {
        let r = parse_receipt(&deliver(
            RECEIPT,
            0x04,
            vec![Tlv::new(0x001e, b"FROM-TLV\0".to_vec())],
        ))
        .unwrap();
        assert_eq!(r.message_id.as_deref(), Some("FROM-TLV"));
        assert_eq!(r.source, ReceiptSource::Tlv);
    }

    #[test]
    fn parses_id_stat_and_err_out_of_the_receipt_text() {
        let r = parse_receipt(&deliver(RECEIPT, 0x04, vec![])).unwrap();
        assert_eq!(r.message_id.as_deref(), Some("0A1B2C3D"));
        assert_eq!(r.state.as_deref(), Some("DELIVRD"));
        assert_eq!(r.error_code.as_deref(), Some("000"));
        assert_eq!(r.source, ReceiptSource::Text);
    }

    #[test]
    fn parses_the_submit_and_done_dates() {
        let r = parse_receipt(&deliver(RECEIPT, 0x04, vec![])).unwrap();
        assert_eq!(r.submit_date.as_deref(), Some("2610211200"));
        assert_eq!(r.done_date.as_deref(), Some("2610211201"));
    }

    #[test]
    fn returns_nothing_for_a_deliver_sm_that_is_not_a_receipt() {
        assert!(parse_receipt(&deliver("just an inbound message", 0, vec![])).is_none());
    }

    #[test]
    fn recognises_an_intermediate_notification() {
        let r = parse_receipt(&deliver(RECEIPT, 0x20, vec![])).unwrap();
        assert_eq!(r.state.as_deref(), Some("DELIVRD"));
    }

    #[test]
    fn tolerates_receipt_text_with_extra_unknown_keys() {
        let r = parse_receipt(&deliver(
            "id:XYZ vendor:acme ratio:7 stat:EXPIRED err:042 nonsense",
            0x04,
            vec![],
        ))
        .unwrap();
        assert_eq!(r.message_id.as_deref(), Some("XYZ"));
        assert_eq!(r.state.as_deref(), Some("EXPIRED"));
        assert_eq!(r.error_code.as_deref(), Some("042"));
    }

    #[test]
    fn is_case_insensitive_about_the_receipt_keys() {
        let r = parse_receipt(&deliver("ID:ABC STAT:UNDELIV ERR:001", 0x04, vec![])).unwrap();
        assert_eq!(r.message_id.as_deref(), Some("ABC"));
        assert_eq!(r.state.as_deref(), Some("UNDELIV"));
    }

    #[test]
    fn falls_back_to_the_message_state_tlv_when_the_text_has_no_stat() {
        let r = parse_receipt(&deliver("id:ABC", 0x04, vec![Tlv::new(0x0427, vec![2])])).unwrap();
        assert_eq!(r.state.as_deref(), Some("DELIVERED"));
    }

    #[test]
    fn reads_the_body_from_message_payload_when_short_message_is_empty() {
        let input = PduInput::new(command_id("deliver_sm").unwrap(), 1)
            .with_num("esm_class", 0x04)
            .with_tlv(Tlv::new(0x0424, RECEIPT.as_bytes().to_vec()));
        let pdu = decode_pdu(&encode_pdu(&input)).unwrap();
        assert_eq!(
            parse_receipt(&pdu).unwrap().message_id.as_deref(),
            Some("0A1B2C3D")
        );
    }

    #[test]
    fn returns_nothing_for_a_pdu_that_is_not_a_delivery() {
        let pdu = decode_pdu(&encode_pdu(&PduInput::new(
            command_id("submit_sm").unwrap(),
            1,
        )))
        .unwrap();
        assert!(parse_receipt(&pdu).is_none());
    }

    #[test]
    fn also_accepts_a_receipt_delivered_as_data_sm() {
        let input = PduInput::new(command_id("data_sm").unwrap(), 1)
            .with_num("esm_class", 0x04)
            .with_tlv(Tlv::new(0x0424, RECEIPT.as_bytes().to_vec()));
        let pdu = decode_pdu(&encode_pdu(&input)).unwrap();
        assert!(parse_receipt(&pdu).is_some());
    }
}
