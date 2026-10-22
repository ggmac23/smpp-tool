//! SMPP numeric constants.
//!
//! Every value here comes from the SMPP 3.4 and 5.0 specifications. Nothing is
//! invented: a value the protocol does not define has no entry, because a
//! fabricated command id produces a PDU no SMSC can parse.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Requests that have a matching `_resp`.
const REQUESTS: &[(&str, u32)] = &[
    ("bind_receiver", 0x0000_0001),
    ("bind_transmitter", 0x0000_0002),
    ("query_sm", 0x0000_0003),
    ("submit_sm", 0x0000_0004),
    ("deliver_sm", 0x0000_0005),
    ("unbind", 0x0000_0006),
    ("replace_sm", 0x0000_0007),
    ("cancel_sm", 0x0000_0008),
    ("bind_transceiver", 0x0000_0009),
    ("enquire_link", 0x0000_0015),
    ("submit_multi", 0x0000_0021),
    ("data_sm", 0x0000_0103),
    ("broadcast_sm", 0x0000_0111),
    ("query_broadcast_sm", 0x0000_0112),
    ("cancel_broadcast_sm", 0x0000_0113),
];

/// Commands the protocol defines with no response of their own.
const ONE_WAY: &[(&str, u32)] = &[
    ("outbind", 0x0000_000b),
    ("alert_notification", 0x0000_0102),
];

const GENERIC_NACK: (&str, u32) = ("generic_nack", 0x8000_0000);

pub const RESPONSE_BIT: u32 = 0x8000_0000;
pub const PDU_HEADER_LENGTH: usize = 16;

fn by_name() -> &'static HashMap<String, u32> {
    static MAP: OnceLock<HashMap<String, u32>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = HashMap::new();
        for (name, id) in REQUESTS {
            map.insert((*name).to_string(), *id);
            map.insert(format!("{name}_resp"), RESPONSE_BIT | *id);
        }
        for (name, id) in ONE_WAY {
            map.insert((*name).to_string(), *id);
        }
        map.insert(GENERIC_NACK.0.to_string(), GENERIC_NACK.1);
        map
    })
}

fn by_id() -> &'static HashMap<u32, String> {
    static MAP: OnceLock<HashMap<u32, String>> = OnceLock::new();
    MAP.get_or_init(|| by_name().iter().map(|(n, i)| (*i, n.clone())).collect())
}

/// The numeric id for a command name, or `None` when the protocol defines no
/// such command. Note what is absent: SMPP has no `broadcast_multi`.
pub fn command_id(name: &str) -> Option<u32> {
    by_name().get(name).copied()
}

/// The command's name, or `unknown(0x…)` for an id the protocol does not define.
pub fn command_name(id: u32) -> String {
    by_id()
        .get(&id)
        .cloned()
        .unwrap_or_else(|| format!("unknown({})", hex(id)))
}

pub fn is_response(id: u32) -> bool {
    id & RESPONSE_BIT != 0
}

pub fn response_id_for(request_id: u32) -> u32 {
    RESPONSE_BIT | request_id
}

fn hex(n: u32) -> String {
    format!("0x{n:x}")
}

pub const ESME_ROK: u32 = 0x0000_0000;

const STATUSES: &[(&str, u32)] = &[
    ("ESME_ROK", 0x0000_0000),
    ("ESME_RINVMSGLEN", 0x0000_0001),
    ("ESME_RINVCMDLEN", 0x0000_0002),
    ("ESME_RINVCMDID", 0x0000_0003),
    ("ESME_RINVBNDSTS", 0x0000_0004),
    ("ESME_RALYBND", 0x0000_0005),
    ("ESME_RINVPRTFLG", 0x0000_0006),
    ("ESME_RINVREGDLVFLG", 0x0000_0007),
    ("ESME_RSYSERR", 0x0000_0008),
    ("ESME_RINVSRCADR", 0x0000_000a),
    ("ESME_RINVDSTADR", 0x0000_000b),
    ("ESME_RINVMSGID", 0x0000_000c),
    ("ESME_RBINDFAIL", 0x0000_000d),
    ("ESME_RINVPASWD", 0x0000_000e),
    ("ESME_RINVSYSID", 0x0000_000f),
    ("ESME_RCANCELFAIL", 0x0000_0011),
    ("ESME_RREPLACEFAIL", 0x0000_0013),
    ("ESME_RMSGQFUL", 0x0000_0014),
    ("ESME_RINVSERTYP", 0x0000_0015),
    ("ESME_RINVNUMDESTS", 0x0000_0033),
    ("ESME_RINVDLNAME", 0x0000_0034),
    ("ESME_RINVDESTFLAG", 0x0000_0040),
    ("ESME_RINVSUBREP", 0x0000_0042),
    ("ESME_RINVESMCLASS", 0x0000_0043),
    ("ESME_RCNTSUBDL", 0x0000_0044),
    ("ESME_RSUBMITFAIL", 0x0000_0045),
    ("ESME_RINVSRCTON", 0x0000_0048),
    ("ESME_RINVSRCNPI", 0x0000_0049),
    ("ESME_RINVDSTTON", 0x0000_0050),
    ("ESME_RINVDSTNPI", 0x0000_0051),
    ("ESME_RINVSYSTYP", 0x0000_0053),
    ("ESME_RINVREPFLAG", 0x0000_0054),
    ("ESME_RINVNUMMSGS", 0x0000_0055),
    ("ESME_RTHROTTLED", 0x0000_0058),
    ("ESME_RINVSCHED", 0x0000_0061),
    ("ESME_RINVEXPIRY", 0x0000_0062),
    ("ESME_RINVDFTMSGID", 0x0000_0063),
    ("ESME_RX_T_APPN", 0x0000_0064),
    ("ESME_RX_P_APPN", 0x0000_0065),
    ("ESME_RX_R_APPN", 0x0000_0066),
    ("ESME_RQUERYFAIL", 0x0000_0067),
    ("ESME_RINVOPTPARSTREAM", 0x0000_00c0),
    ("ESME_ROPTPARNOTALLWD", 0x0000_00c1),
    ("ESME_RINVPARLEN", 0x0000_00c2),
    ("ESME_RMISSINGOPTPARAM", 0x0000_00c3),
    ("ESME_RINVOPTPARAMVAL", 0x0000_00c4),
    ("ESME_RDELIVERYFAILURE", 0x0000_00fe),
    ("ESME_RUNKNOWNERR", 0x0000_00ff),
];

pub fn command_status_name(status: u32) -> String {
    STATUSES
        .iter()
        .find(|(_, v)| *v == status)
        .map(|(n, _)| (*n).to_string())
        .unwrap_or_else(|| format!("unknown({})", hex(status)))
}

/// The status value for a name, for tests and for composing responses.
pub fn command_status(name: &str) -> Option<u32> {
    STATUSES.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

pub struct InterfaceVersion;
impl InterfaceVersion {
    pub const DEFAULT: u8 = 0x00;
    pub const V33: u8 = 0x33;
    pub const V34: u8 = 0x34;
    pub const V50: u8 = 0x50;
}

pub struct Ton;
impl Ton {
    pub const UNKNOWN: u8 = 0;
    pub const INTERNATIONAL: u8 = 1;
    pub const NATIONAL: u8 = 2;
    pub const NETWORK_SPECIFIC: u8 = 3;
    pub const SUBSCRIBER_NUMBER: u8 = 4;
    pub const ALPHANUMERIC: u8 = 5;
    pub const ABBREVIATED: u8 = 6;
}

pub struct Npi;
impl Npi {
    pub const UNKNOWN: u8 = 0;
    pub const ISDN_E163_E164: u8 = 1;
    pub const DATA_X121: u8 = 3;
    pub const TELEX_F69: u8 = 4;
    pub const LAND_MOBILE_E212: u8 = 6;
    pub const NATIONAL: u8 = 8;
    pub const PRIVATE: u8 = 9;
    pub const ERMES: u8 = 10;
    pub const INTERNET_IP: u8 = 14;
    pub const WAP_CLIENT_ID: u8 = 18;
}

/// data_coding 0-8, the set the reference tool exposes.
pub struct DataCoding;
impl DataCoding {
    pub const GSM_DEFAULT: u8 = 0;
    pub const IA5_ASCII: u8 = 1;
    pub const OCTET_UNSPECIFIED_2: u8 = 2;
    pub const LATIN_1: u8 = 3;
    pub const OCTET_UNSPECIFIED_4: u8 = 4;
    pub const JIS: u8 = 5;
    pub const CYRILLIC: u8 = 6;
    pub const LATIN_HEBREW: u8 = 7;
    pub const UCS2: u8 = 8;
}

/// esm_class: messaging mode in bits 0-1, UDHI in bit 6.
pub struct EsmClass;
impl EsmClass {
    pub const DEFAULT: u8 = 0x00;
    pub const DATAGRAM: u8 = 0x01;
    pub const FORWARD: u8 = 0x02;
    pub const STORE_AND_FORWARD: u8 = 0x03;
    pub const DEFAULT_UDHI: u8 = 0x40;
    pub const DATAGRAM_UDHI: u8 = 0x41;
    pub const FORWARD_UDHI: u8 = 0x42;
    pub const STORE_AND_FORWARD_UDHI: u8 = 0x43;
}

pub const ESM_UDHI_BIT: u8 = 0x40;

/// registered_delivery: receipt in bits 0-1, intermediate notification in bit 4.
pub struct RegisteredDelivery;
impl RegisteredDelivery {
    pub const NONE: u8 = 0;
    pub const FAILURE_AND_SUCCESS: u8 = 1;
    pub const FAILURE_ONLY: u8 = 2;
    pub const SUCCESS_ONLY: u8 = 3;
    pub const INTERMEDIATE_NONE: u8 = 16;
    pub const INTERMEDIATE_FAILURE_AND_SUCCESS: u8 = 17;
    pub const INTERMEDIATE_FAILURE_ONLY: u8 = 18;
    pub const INTERMEDIATE_SUCCESS_ONLY: u8 = 19;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlvType {
    Octets,
    CString,
    U8,
    U16,
    U32,
}

pub const KNOWN_TLVS: &[(u16, &str, TlvType)] = &[
    (0x001e, "receipted_message_id", TlvType::CString),
    (0x0204, "user_message_reference", TlvType::U16),
    (0x020a, "source_port", TlvType::U16),
    (0x020b, "destination_port", TlvType::U16),
    (0x020c, "sar_msg_ref_num", TlvType::U16),
    (0x020d, "language_indicator", TlvType::U8),
    (0x020e, "sar_total_segments", TlvType::U8),
    (0x020f, "sar_segment_seqnum", TlvType::U8),
    (0x0423, "network_error_code", TlvType::Octets),
    (0x0424, "message_payload", TlvType::Octets),
    (0x0425, "delivery_failure_reason", TlvType::U8),
    (0x0426, "more_messages_to_send", TlvType::U8),
    (0x0427, "message_state", TlvType::U8),
    (0x0501, "its_reply_type", TlvType::U8),
];

pub const TLV_MESSAGE_PAYLOAD: u16 = 0x0424;
pub const TLV_RECEIPTED_MESSAGE_ID: u16 = 0x001e;
pub const TLV_MESSAGE_STATE: u16 = 0x0427;
pub const TLV_SAR_MSG_REF_NUM: u16 = 0x020c;
pub const TLV_SAR_TOTAL_SEGMENTS: u16 = 0x020e;
pub const TLV_SAR_SEGMENT_SEQNUM: u16 = 0x020f;

pub fn known_tlv(tag: u16) -> Option<(&'static str, TlvType)> {
    KNOWN_TLVS
        .iter()
        .find(|(t, _, _)| *t == tag)
        .map(|(_, n, ty)| (*n, *ty))
}

pub fn tlv_name(tag: u16) -> String {
    known_tlv(tag)
        .map(|(n, _)| n.to_string())
        .unwrap_or_else(|| format!("unknown(0x{tag:04x})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_responses_from_their_request_id() {
        assert_eq!(command_id("submit_sm"), Some(0x0000_0004));
        assert_eq!(command_id("submit_sm_resp"), Some(0x8000_0004));
        assert_eq!(command_name(0x8000_0004), "submit_sm_resp");
    }

    #[test]
    fn names_an_unrecognised_command_id() {
        assert_eq!(command_name(0x4242), "unknown(0x4242)");
    }

    #[test]
    fn names_every_command_in_the_spec_list() {
        for name in [
            "bind_transmitter",
            "bind_receiver",
            "bind_transceiver",
            "unbind",
            "submit_sm",
            "data_sm",
            "query_sm",
            "cancel_sm",
            "replace_sm",
            "submit_multi",
            "broadcast_sm",
            "query_broadcast_sm",
            "cancel_broadcast_sm",
            "enquire_link",
            "deliver_sm",
            "generic_nack",
        ] {
            let id = command_id(name).unwrap_or_else(|| panic!("no id for {name}"));
            assert_eq!(command_name(id), name, "round trip for {name}");
        }
    }

    #[test]
    fn there_is_no_broadcast_multi_because_smpp_defines_none() {
        assert_eq!(command_id("broadcast_multi"), None);
    }

    #[test]
    fn names_command_statuses() {
        assert_eq!(command_status_name(ESME_ROK), "ESME_ROK");
        assert_eq!(command_status_name(0x0000_0058), "ESME_RTHROTTLED");
        assert_eq!(command_status_name(0x1_2345), "unknown(0x12345)");
    }

    #[test]
    fn tells_a_response_from_a_request() {
        assert!(is_response(0x8000_0004));
        assert!(!is_response(0x0000_0004));
        assert_eq!(response_id_for(0x0000_0004), 0x8000_0004);
    }

    #[test]
    fn names_known_tlv_tags_and_hexes_the_rest() {
        assert_eq!(tlv_name(0x0424), "message_payload");
        assert_eq!(tlv_name(0x001e), "receipted_message_id");
        assert_eq!(tlv_name(0x9999), "unknown(0x9999)");
    }

    #[test]
    fn exposes_the_field_value_sets_the_form_offers() {
        assert_eq!(InterfaceVersion::V34, 0x34);
        assert_eq!(Ton::ALPHANUMERIC, 5);
        assert_eq!(Npi::ISDN_E163_E164, 1);
        assert_eq!(DataCoding::UCS2, 8);
        assert_eq!(EsmClass::STORE_AND_FORWARD_UDHI, 0x43);
        assert_eq!(ESM_UDHI_BIT, 0x40);
        assert_eq!(RegisteredDelivery::INTERMEDIATE_SUCCESS_ONLY, 19);
    }
}
