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
    ("ESME_RSERTYPUNAUTH", 0x0000_0100),
    ("ESME_RPROHIBITED", 0x0000_0101),
    ("ESME_RSERTYPUNAVAIL", 0x0000_0102),
    ("ESME_RSERTYPDENIED", 0x0000_0103),
    ("ESME_RINVDCS", 0x0000_0104),
    ("ESME_RINVSRCADDRSUBUNIT", 0x0000_0105),
    ("ESME_RINVDSTADDRSUBUNIT", 0x0000_0106),
    ("ESME_RINVBCASTFREQINT", 0x0000_0107),
    ("ESME_RINVBCASTALIAS_NAME", 0x0000_0108),
    ("ESME_RINVBCASTAREAFMT", 0x0000_0109),
    ("ESME_RINVNUMBCAST_AREAS", 0x0000_010a),
    ("ESME_RINVBCASTCNTTYPE", 0x0000_010b),
    ("ESME_RINVBCASTMSGCLASS", 0x0000_010c),
    ("ESME_RBCASTFAIL", 0x0000_010d),
    ("ESME_RBCASTQUERYFAIL", 0x0000_010e),
    ("ESME_RBCASTCANCELFAIL", 0x0000_010f),
    ("ESME_RINVBCAST_REP", 0x0000_0110),
    ("ESME_RINVBCASTSRVGRP", 0x0000_0111),
    ("ESME_RINVBCASTCHANIND", 0x0000_0112),
];

pub fn command_status_name(status: u32) -> String {
    STATUSES
        .iter()
        .find(|(_, v)| *v == status)
        .map(|(n, _)| (*n).to_string())
        .unwrap_or_else(|| format!("unknown({})", hex(status)))
}

/// The status value for a name, for tests and for composing responses.
/// SMPP 5.0 renamed the five "optional parameter" errors to "TLV" errors.
/// Both spellings name the same value; `command_status_name` keeps the 3.4
/// one, which is what aggregator documentation quotes.
const STATUS_ALIASES: &[(&str, u32)] = &[
    ("ESME_RINVTLVSTREAM", 0x0000_00c0),
    ("ESME_RTLVNOTALLWD", 0x0000_00c1),
    ("ESME_RINVTLVLEN", 0x0000_00c2),
    ("ESME_RMISSINGTLV", 0x0000_00c3),
    ("ESME_RINVTLVVAL", 0x0000_00c4),
];

pub fn command_status(name: &str) -> Option<u32> {
    STATUSES
        .iter()
        .chain(STATUS_ALIASES.iter())
        .find(|(n, _)| *n == name)
        .map(|(_, v)| *v)
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

/// Every TLV SMPP 3.4 and 5.0 define, with the width the spec gives it. A tag
/// outside this table is still carried byte-for-byte; it just reads as
/// `unknown(0x…)` rather than by name.
pub const KNOWN_TLVS: &[(u16, &str, TlvType)] = &[
    (0x0005, "dest_addr_subunit", TlvType::U8),
    (0x0006, "dest_network_type", TlvType::U8),
    (0x0007, "dest_bearer_type", TlvType::U8),
    (0x0008, "dest_telematics_id", TlvType::U16),
    (0x000d, "source_addr_subunit", TlvType::U8),
    (0x000e, "source_network_type", TlvType::U8),
    (0x000f, "source_bearer_type", TlvType::U8),
    (0x0010, "source_telematics_id", TlvType::U16),
    (0x0017, "qos_time_to_live", TlvType::U32),
    (0x0019, "payload_type", TlvType::U8),
    (0x001d, "additional_status_info_text", TlvType::CString),
    (0x001e, "receipted_message_id", TlvType::CString),
    (0x0030, "ms_msg_wait_facilities", TlvType::U8),
    (0x0201, "privacy_indicator", TlvType::U8),
    (0x0202, "source_subaddress", TlvType::Octets),
    (0x0203, "dest_subaddress", TlvType::Octets),
    (0x0204, "user_message_reference", TlvType::U16),
    (0x0205, "user_response_code", TlvType::U8),
    (0x020a, "source_port", TlvType::U16),
    (0x020b, "destination_port", TlvType::U16),
    (0x020c, "sar_msg_ref_num", TlvType::U16),
    (0x020d, "language_indicator", TlvType::U8),
    (0x020e, "sar_total_segments", TlvType::U8),
    (0x020f, "sar_segment_seqnum", TlvType::U8),
    (0x0210, "sc_interface_version", TlvType::U8),
    (0x0302, "callback_num_pres_ind", TlvType::U8),
    (0x0303, "callback_num_atag", TlvType::Octets),
    (0x0304, "number_of_messages", TlvType::U8),
    (0x0381, "callback_num", TlvType::Octets),
    (0x0420, "dpf_result", TlvType::U8),
    (0x0421, "set_dpf", TlvType::U8),
    (0x0422, "ms_availability_status", TlvType::U8),
    (0x0423, "network_error_code", TlvType::Octets),
    (0x0424, "message_payload", TlvType::Octets),
    (0x0425, "delivery_failure_reason", TlvType::U8),
    (0x0426, "more_messages_to_send", TlvType::U8),
    (0x0427, "message_state", TlvType::U8),
    (0x0428, "congestion_state", TlvType::U8),
    (0x0501, "ussd_service_op", TlvType::U8),
    (0x0600, "broadcast_channel_indicator", TlvType::U8),
    (0x0601, "broadcast_content_type", TlvType::Octets),
    (0x0602, "broadcast_content_type_info", TlvType::Octets),
    (0x0603, "broadcast_message_class", TlvType::U8),
    (0x0604, "broadcast_rep_num", TlvType::U8),
    (0x0605, "broadcast_frequency_interval", TlvType::Octets),
    (0x0606, "broadcast_area_identifier", TlvType::Octets),
    (0x0607, "broadcast_error_status", TlvType::U32),
    (0x0608, "broadcast_area_success", TlvType::U8),
    (0x0609, "broadcast_end_time", TlvType::Octets),
    (0x060a, "broadcast_service_group", TlvType::Octets),
    (0x060b, "billing_identification", TlvType::Octets),
    (0x060d, "source_network_id", TlvType::CString),
    (0x060e, "dest_network_id", TlvType::CString),
    (0x060f, "source_node_id", TlvType::Octets),
    (0x0610, "dest_node_id", TlvType::Octets),
    (0x0611, "dest_addr_np_resolution", TlvType::U8),
    (0x0612, "dest_addr_np_information", TlvType::Octets),
    (0x0613, "dest_addr_np_country", TlvType::Octets),
    (0x1201, "display_time", TlvType::U8),
    (0x1203, "sms_signal", TlvType::U16),
    (0x1204, "ms_validity", TlvType::Octets),
    (0x130c, "alert_on_message_delivery", TlvType::U8),
    (0x1380, "its_reply_type", TlvType::U8),
    (0x1383, "its_session_info", TlvType::Octets),
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
    fn names_0x0501_as_ussd_service_op_not_its_reply_type() {
        // 0x0501 is ussd_service_op; its_reply_type is 0x1380. Labelling a
        // USSD TLV as an ITS one is a trace that lies about the wire.
        assert_eq!(tlv_name(0x0501), "ussd_service_op");
        assert_eq!(tlv_name(0x1380), "its_reply_type");
    }

    #[test]
    fn uses_the_3_4_name_for_the_destination_port_tlv() {
        // SMPP 3.4 section 5.3.2.21 calls it destination_port; 5.0 renamed it
        // dest_port. The display name follows 3.4, as with the TLV error
        // statuses, because that is the spelling aggregator documentation
        // quotes and what the trace export is grepped for.
        assert_eq!(tlv_name(0x020b), "destination_port");
    }

    #[test]
    fn accepts_the_smpp_5_spelling_of_the_tlv_errors() {
        // 3.4 calls them optional parameters, 5.0 calls them TLVs. The same
        // five values have two names, and someone reading a 5.0 document must
        // be able to type what they read.
        assert_eq!(
            command_status("ESME_RINVTLVSTREAM"),
            command_status("ESME_RINVOPTPARSTREAM")
        );
        assert_eq!(command_status("ESME_RINVTLVLEN"), Some(0x0000_00c2));
        assert_eq!(command_status("ESME_RMISSINGTLV"), Some(0x0000_00c3));
        // The display name stays the 3.4 one, which is what aggregators quote.
        assert_eq!(command_status_name(0x0000_00c2), "ESME_RINVPARLEN");
    }

    #[test]
    fn names_the_statuses_smpp_5_added() {
        assert_eq!(command_status_name(0x0000_0100), "ESME_RSERTYPUNAUTH");
        assert_eq!(command_status_name(0x0000_0104), "ESME_RINVDCS");
        assert_eq!(command_status_name(0x0000_0112), "ESME_RINVBCASTCHANIND");
    }

    #[test]
    fn names_the_tlvs_an_aggregator_actually_sends() {
        // Each of these used to read as unknown(0x....) in the trace.
        assert_eq!(tlv_name(0x0017), "qos_time_to_live");
        assert_eq!(tlv_name(0x001d), "additional_status_info_text");
        assert_eq!(tlv_name(0x0381), "callback_num");
        assert_eq!(tlv_name(0x0422), "ms_availability_status");
        assert_eq!(tlv_name(0x0428), "congestion_state");
        assert_eq!(tlv_name(0x060e), "dest_network_id");
        assert_eq!(tlv_name(0x1203), "sms_signal");
    }

    #[test]
    fn knows_every_tlv_the_spec_defines() {
        assert_eq!(
            KNOWN_TLVS.len(),
            64,
            "SMPP 3.4 and 5.0 define 64 TLVs between them"
        );
        let mut tags: Vec<u16> = KNOWN_TLVS.iter().map(|(t, _, _)| *t).collect();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), KNOWN_TLVS.len(), "a tag is listed twice");
    }

    #[test]
    fn reads_the_widths_the_spec_gives_the_new_tlvs() {
        assert_eq!(
            known_tlv(0x0017).unwrap().1,
            TlvType::U32,
            "qos_time_to_live is 4 octets"
        );
        assert_eq!(
            known_tlv(0x0607).unwrap().1,
            TlvType::U32,
            "broadcast_error_status is a status"
        );
        assert_eq!(
            known_tlv(0x1203).unwrap().1,
            TlvType::U16,
            "sms_signal is 2 octets"
        );
        assert_eq!(known_tlv(0x060d).unwrap().1, TlvType::CString);
        assert_eq!(known_tlv(0x0381).unwrap().1, TlvType::Octets);
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
