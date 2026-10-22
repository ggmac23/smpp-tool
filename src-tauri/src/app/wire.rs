//! The shapes that cross to the renderer.
//!
//! The renderer never sees a socket, a `Vec<u8>` or a PDU model: bytes travel
//! as lowercase hex strings and every key is camelCase, because qooxdoo
//! widgets read these names directly. A rename here is a silent break there,
//! so the spelling is pinned by a test.

use indexmap::IndexMap;

use crate::app::burst::BurstRow;
use crate::app::dlr::CorrelationConfig;
use crate::app::loadtest::LoadMode;
use crate::app::recipients::RecipientSource;
use crate::app::session::{BindMode, BindState};
use crate::app::trace::{Direction, TraceEntry};
use crate::smpp::constants::command_status_name;
use crate::smpp::decode::{DestEntry, FieldValue};
use crate::smpp::encode::PduInput;
use crate::smpp::segment::BodyPlacement;
use crate::smpp::summary::DescribedField;
use crate::smpp::text::{Gsm7Packing, SegmentationMode};
use crate::smpp::tlv::Tlv;

/// A PDU as the renderer sees it: already decoded, summarised and hexed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WirePdu {
    /// Row ordinal in the trace.
    pub seq: u64,
    pub at: u64,
    pub direction: String,
    pub command_name: String,
    pub command_status: u32,
    pub status_name: String,
    pub sequence_number: u32,
    pub summary: String,
    /// Lowercase hex of the exact bytes on the wire.
    pub hex: String,
    pub decode_error: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireTlv {
    pub tag: u16,
    #[serde(default)]
    pub hex: String,
}

/// One entry of submit_multi's dest_address list.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireDest {
    pub flag: u8,
    #[serde(default)]
    pub ton: Option<u8>,
    #[serde(default)]
    pub npi: Option<u8>,
    #[serde(default)]
    pub addr: Option<String>,
    #[serde(default)]
    pub dl_name: Option<String>,
}

/// A PDU the renderer wants sent. Octet fields arrive as hex.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendPduRequest {
    pub command_id: u32,
    #[serde(default)]
    pub command_status: Option<u32>,
    #[serde(default)]
    pub fields: IndexMap<String, serde_json::Value>,
    #[serde(default)]
    pub octet_fields: IndexMap<String, String>,
    #[serde(default)]
    pub tlvs: Vec<WireTlv>,
    /// submit_multi's repeating list. The count field is derived here.
    #[serde(default)]
    pub dest_list: Vec<WireDest>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireTlsOptions {
    pub enabled: bool,
    #[serde(default)]
    pub reject_unauthorized: bool,
    #[serde(default)]
    pub servername: Option<String>,
    /// Paths, not contents: read at connect time, so a renewed certificate
    /// takes effect without re-saving anything.
    #[serde(default)]
    pub ca_path: Option<String>,
    #[serde(default)]
    pub client_cert_path: Option<String>,
    #[serde(default)]
    pub client_key_path: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectRequest {
    pub host: String,
    pub port: u16,
    pub tls: WireTlsOptions,
    #[serde(default)]
    pub correlation: CorrelationConfig,
    pub window_size: usize,
    pub response_timeout_ms: u64,
    pub connect_timeout_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindRequest {
    pub system_id: String,
    pub password: String,
    #[serde(default)]
    pub system_type: String,
    pub interface_version: u8,
    pub mode: BindMode,
    #[serde(default)]
    pub addr_ton: u8,
    #[serde(default)]
    pub addr_npi: u8,
    #[serde(default)]
    pub address_range: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasureRequest {
    pub text: String,
    pub data_coding: u8,
    pub mode: SegmentationMode,
    #[serde(default)]
    pub packing: Gsm7Packing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Csv,
    Json,
    Text,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Json => "json",
            ExportFormat::Text => "txt",
        }
    }
}

/// What the renderer gets back for a hand-written PDU: what it decodes to,
/// and whether it can be sent as it stands.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PduPreview {
    /// Whether the bytes decode as a PDU.
    pub ok: bool,
    /// Why they do not, when they do not.
    pub error: Option<String>,
    pub summary: String,
    pub fields: Vec<DescribedField>,
    pub byte_count: usize,
    /// Deliberately independent of `ok`: a PDU that will not decode is often
    /// exactly what a tester wants to send, so the preview informs and never
    /// gates.
    pub sendable: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    pub state: BindState,
    pub mode: Option<BindMode>,
    pub connected: bool,
    pub dropped: u64,
    /// Bytes the framer holds, and the length the pending PDU claims. Without
    /// this a stalled `command_length` is invisible.
    pub pending_buffered: usize,
    pub pending_declared: Option<u32>,
    pub inflight: usize,
}

/// A burst as the renderer configures it. The body is composed here, from
/// `text` and the segmentation options, not in the renderer.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartBurstRequest {
    pub recipients: RecipientSource,
    pub rate_per_sec: u32,
    pub template: SendPduRequest,
    #[serde(default)]
    pub text: String,
    pub data_coding: u8,
    #[serde(default)]
    pub segmentation: SegmentationMode,
    #[serde(default)]
    pub placement: BodyPlacement,
    #[serde(default)]
    pub packing: Gsm7Packing,
    #[serde(default)]
    pub backoff_on_throttle: bool,
    /// How long to keep waiting for delivery receipts after the last submit.
    #[serde(default)]
    pub await_dlr_ms: u64,
}

/// A load test as the renderer configures it. It carries its own connection
/// and bind details: the load test opens its own sockets rather than borrowing
/// the session the user is inspecting.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartLoadRequest {
    pub connect: ConnectRequest,
    pub bind: BindRequest,
    pub connections: usize,
    pub messages: usize,
    pub mode: LoadMode,
    pub template: SendPduRequest,
    #[serde(default)]
    pub destination: String,
    #[serde(default)]
    pub text: String,
    pub data_coding: u8,
    #[serde(default)]
    pub segmentation: SegmentationMode,
    #[serde(default)]
    pub placement: BodyPlacement,
    #[serde(default)]
    pub packing: Gsm7Packing,
}

/// A burst row as the renderer sees it: the row itself, plus the name of the
/// status it carries. The renderer must not hold a status table of its own
/// just to label a row.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireBurstRow {
    #[serde(flatten)]
    pub row: BurstRow,
    pub status_name: String,
}

pub fn to_wire_burst_row(row: &BurstRow) -> WireBurstRow {
    WireBurstRow {
        row: row.clone(),
        // Empty, not "ESME_ROK": a row with no verdict yet has not been
        // accepted, and saying so would be a lie the user acts on.
        status_name: row
            .command_status
            .map(command_status_name)
            .unwrap_or_default(),
    }
}

/// Strips everything that is not a hex digit, so a user may type `0x`,
/// spaces, colons or newlines and still mean the same bytes. The `0x` goes
/// first: `0` is itself a hex digit, and filtering before stripping would
/// shift every byte by half.
pub fn clean_hex(s: &str) -> String {
    let t = s.trim();
    let body = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .unwrap_or(t);
    body.chars().filter(|c| c.is_ascii_hexdigit()).collect()
}

/// Hex to bytes, dropping a trailing half-byte rather than inventing a zero
/// for it: a user mid-keystroke must not send a byte they never typed.
pub fn hex_to_bytes(s: &str) -> Vec<u8> {
    let clean = clean_hex(s);
    let (pairs, _trailing_nibble) = clean.as_bytes().as_chunks::<2>();
    pairs
        .iter()
        .map(|pair| {
            let hi = (pair[0] as char).to_digit(16).unwrap_or(0) as u8;
            let lo = (pair[1] as char).to_digit(16).unwrap_or(0) as u8;
            (hi << 4) | lo
        })
        .collect()
}

fn field_value(v: &serde_json::Value) -> FieldValue {
    match v {
        serde_json::Value::Number(n) => FieldValue::Num(n.as_u64().unwrap_or(0) as u32),
        serde_json::Value::String(s) => FieldValue::Str(s.clone()),
        serde_json::Value::Bool(b) => FieldValue::Num(u32::from(*b)),
        // Null and the rest: an empty string is what an untouched text field
        // means, and it encodes as the single NUL the spec asks for.
        _ => FieldValue::Str(String::new()),
    }
}

/// The renderer's request as a PDU model. The sequence number is the
/// session's to assign, so it stays 0 here.
pub fn to_pdu_input(req: &SendPduRequest) -> PduInput {
    let mut input = PduInput::new(req.command_id, 0);
    if let Some(status) = req.command_status {
        input.command_status = status;
    }

    for (name, value) in &req.fields {
        input.fields.insert(name.clone(), field_value(value));
    }
    for (name, hex) in &req.octet_fields {
        input
            .fields
            .insert(name.clone(), FieldValue::Bytes(hex_to_bytes(hex)));
    }
    if !req.dest_list.is_empty() {
        let entries: Vec<DestEntry> = req
            .dest_list
            .iter()
            .map(|d| DestEntry {
                flag: d.flag,
                ton: d.ton,
                npi: d.npi,
                addr: d.addr.clone(),
                dl_name: d.dl_name.clone(),
            })
            .collect();
        input
            .fields
            .insert("dest_address".into(), FieldValue::DestList(entries));
    }

    input.tlvs = req
        .tlvs
        .iter()
        .map(|t| Tlv::new(t.tag, hex_to_bytes(&t.hex)))
        .collect();
    input
}

pub fn to_wire_pdu(e: &TraceEntry) -> WirePdu {
    WirePdu {
        seq: e.seq,
        at: e.at,
        direction: match e.direction {
            Direction::In => "in".into(),
            Direction::Out => "out".into(),
        },
        command_name: e.command_name.clone(),
        command_status: e.command_status,
        status_name: command_status_name(e.command_status),
        sequence_number: e.sequence_number,
        summary: e.summary.clone(),
        hex: crate::smpp::summary::hex(&e.raw),
        decode_error: e.decode_error.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::trace::{Direction, TraceEntry};
    use crate::smpp::decode::FieldValue;

    fn request() -> SendPduRequest {
        SendPduRequest {
            command_id: 4,
            command_status: None,
            fields: Default::default(),
            octet_fields: Default::default(),
            tlvs: Vec::new(),
            dest_list: Vec::new(),
        }
    }

    #[test]
    fn turns_an_octet_field_from_hex_into_bytes() {
        let mut req = request();
        req.octet_fields
            .insert("short_message".into(), "48 65 6c 6c 6f".into());
        let input = to_pdu_input(&req);
        assert_eq!(
            input.fields.get("short_message"),
            Some(&FieldValue::Bytes(b"Hello".to_vec()))
        );
    }

    #[test]
    fn keeps_a_numeric_field_numeric_and_a_text_field_text() {
        let mut req = request();
        req.fields
            .insert("data_coding".into(), serde_json::json!(8));
        req.fields
            .insert("destination_addr".into(), serde_json::json!("447700900123"));
        let input = to_pdu_input(&req);
        assert_eq!(input.fields.get("data_coding"), Some(&FieldValue::Num(8)));
        assert_eq!(
            input.fields.get("destination_addr"),
            Some(&FieldValue::Str("447700900123".into()))
        );
    }

    #[test]
    fn turns_a_dest_list_into_the_dest_address_field() {
        let mut req = request();
        req.dest_list = vec![
            WireDest {
                flag: 1,
                ton: Some(1),
                npi: Some(1),
                addr: Some("447700900123".into()),
                dl_name: None,
            },
            WireDest {
                flag: 2,
                ton: None,
                npi: None,
                addr: None,
                dl_name: Some("friends".into()),
            },
        ];
        let input = to_pdu_input(&req);
        match input.fields.get("dest_address") {
            Some(FieldValue::DestList(entries)) => {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].addr.as_deref(), Some("447700900123"));
                assert_eq!(entries[1].dl_name.as_deref(), Some("friends"));
            }
            other => panic!("expected a dest list, got {other:?}"),
        }
    }

    #[test]
    fn ignores_the_separators_a_user_types_into_a_hex_field() {
        let mut req = request();
        req.tlvs = vec![WireTlv {
            tag: 0x0424,
            hex: "0x de:ad be-ef".into(),
        }];
        let input = to_pdu_input(&req);
        assert_eq!(input.tlvs[0].value, vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn drops_a_trailing_half_byte_rather_than_inventing_one() {
        // A user mid-keystroke has typed an odd number of nibbles. Guessing a
        // zero would send a byte they never typed.
        let mut req = request();
        req.octet_fields
            .insert("short_message".into(), "abc".into());
        let input = to_pdu_input(&req);
        assert_eq!(
            input.fields.get("short_message"),
            Some(&FieldValue::Bytes(vec![0xab]))
        );
    }

    #[test]
    fn renders_a_trace_entry_with_its_bytes_and_its_status_name() {
        let entry = TraceEntry {
            seq: 7,
            at: 1_700_000_000_000,
            direction: Direction::In,
            command_name: "submit_sm_resp".into(),
            command_status: 0x58,
            sequence_number: 3,
            summary: "throttled".into(),
            raw: vec![0x00, 0xff, 0x10],
            decode_error: None,
        };
        let wire = to_wire_pdu(&entry);
        assert_eq!(wire.hex, "00ff10");
        assert_eq!(wire.status_name, "ESME_RTHROTTLED");
        assert_eq!(wire.seq, 7);
        assert_eq!(wire.direction, "in");
    }

    #[test]
    fn spells_out_the_status_name_on_a_burst_row() {
        // The renderer must not carry a 67-entry status table just to label a
        // row. The backend owns the names.
        let row = crate::app::burst::BurstRow {
            id: "r0".into(),
            recipient: "393331234567".into(),
            body: "hi".into(),
            pdus: 1,
            message_id: Some("abc".into()),
            command_status: Some(0x58),
            submit_latency_ms: Some(12),
            ..Default::default()
        };
        let wire = to_wire_burst_row(&row);
        assert_eq!(wire.status_name, "ESME_RTHROTTLED");
        assert_eq!(wire.row.command_status, Some(0x58));

        let json = serde_json::to_value(&wire).unwrap();
        for key in [
            "id",
            "recipient",
            "pdus",
            "messageId",
            "commandStatus",
            "statusName",
        ] {
            assert!(
                json.get(key).is_some(),
                "the renderer's `{key}` is missing from {json}"
            );
        }
    }

    #[test]
    fn leaves_the_status_name_empty_while_a_row_has_no_verdict() {
        let row = crate::app::burst::BurstRow {
            id: "r0".into(),
            recipient: "393331234567".into(),
            body: "hi".into(),
            pdus: 1,
            ..Default::default()
        };
        assert_eq!(to_wire_burst_row(&row).status_name, "");
    }

    #[test]
    fn names_the_renderers_fields_in_camel_case() {
        // The qooxdoo widgets read these keys. A rename here is a silent
        // break there, so the spelling is pinned.
        let entry = TraceEntry {
            seq: 1,
            at: 2,
            direction: Direction::Out,
            command_name: "submit_sm".into(),
            command_status: 0,
            sequence_number: 9,
            summary: "s".into(),
            raw: vec![],
            decode_error: Some("short".into()),
        };
        let json = serde_json::to_value(to_wire_pdu(&entry)).unwrap();
        for key in [
            "seq",
            "at",
            "direction",
            "commandName",
            "commandStatus",
            "statusName",
            "sequenceNumber",
            "summary",
            "hex",
            "decodeError",
        ] {
            assert!(
                json.get(key).is_some(),
                "the renderer's `{key}` is missing from {json}"
            );
        }
    }
}
