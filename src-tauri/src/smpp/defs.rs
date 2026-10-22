//! Mandatory-parameter tables, in wire order, for every command the client
//! speaks. Sourced from SMPP 3.4 and the 5.0 broadcast additions.
//!
//! `submit_multi` and `submit_multi_resp` carry repeating structures, so the
//! field types go beyond the scalars: `DestList` and `UnsuccessList` are
//! decoded and encoded as lists.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    CString,
    U8,
    U16,
    U32,
    Octets,
    DestList,
    UnsuccessList,
}

#[derive(Debug, Clone, Copy)]
pub struct FieldDef {
    pub name: &'static str,
    pub ty: FieldType,
    /// Spec maximum, including the NUL for a C-string. Advisory: never enforced.
    pub max_len: Option<usize>,
    /// For `Octets`, `DestList` and `UnsuccessList`: the field holding the count.
    pub len_from: Option<&'static str>,
}

const fn c(name: &'static str, max_len: usize) -> FieldDef {
    FieldDef {
        name,
        ty: FieldType::CString,
        max_len: Some(max_len),
        len_from: None,
    }
}

const fn b(name: &'static str) -> FieldDef {
    FieldDef {
        name,
        ty: FieldType::U8,
        max_len: None,
        len_from: None,
    }
}

const fn octets(name: &'static str, len_from: &'static str, max_len: usize) -> FieldDef {
    FieldDef {
        name,
        ty: FieldType::Octets,
        max_len: Some(max_len),
        len_from: Some(len_from),
    }
}

/// service_type plus the source and destination addresses: the shared head.
const ADDRESSED: [FieldDef; 7] = [
    c("service_type", 6),
    b("source_addr_ton"),
    b("source_addr_npi"),
    c("source_addr", 21),
    b("dest_addr_ton"),
    b("dest_addr_npi"),
    c("destination_addr", 21),
];

const SUBMIT_TAIL: [FieldDef; 11] = [
    b("esm_class"),
    b("protocol_id"),
    b("priority_flag"),
    c("schedule_delivery_time", 17),
    c("validity_period", 17),
    b("registered_delivery"),
    b("replace_if_present_flag"),
    b("data_coding"),
    b("sm_default_msg_id"),
    b("sm_length"),
    octets("short_message", "sm_length", 254),
];

const BIND: [FieldDef; 7] = [
    c("system_id", 16),
    c("password", 9),
    c("system_type", 13),
    b("interface_version"),
    b("addr_ton"),
    b("addr_npi"),
    c("address_range", 41),
];

const BIND_RESP: [FieldDef; 1] = [c("system_id", 16)];
const MESSAGE_ID: [FieldDef; 1] = [c("message_id", 65)];
const NONE: [FieldDef; 0] = [];

const SUBMIT_SM: [FieldDef; 18] = concat_addressed_tail();
const DELIVER_SM: [FieldDef; 18] = concat_addressed_tail();

const fn concat_addressed_tail() -> [FieldDef; 18] {
    let mut out = [ADDRESSED[0]; 18];
    let mut i = 0;
    while i < 7 {
        out[i] = ADDRESSED[i];
        i += 1;
    }
    let mut j = 0;
    while j < 11 {
        out[7 + j] = SUBMIT_TAIL[j];
        j += 1;
    }
    out
}

const DATA_SM: [FieldDef; 10] = [
    ADDRESSED[0],
    ADDRESSED[1],
    ADDRESSED[2],
    ADDRESSED[3],
    ADDRESSED[4],
    ADDRESSED[5],
    ADDRESSED[6],
    b("esm_class"),
    b("registered_delivery"),
    b("data_coding"),
];

const QUERY_SM: [FieldDef; 4] = [
    c("message_id", 65),
    b("source_addr_ton"),
    b("source_addr_npi"),
    c("source_addr", 21),
];

const QUERY_SM_RESP: [FieldDef; 4] = [
    c("message_id", 65),
    c("final_date", 17),
    b("message_state"),
    b("error_code"),
];

const CANCEL_SM: [FieldDef; 8] = [
    c("service_type", 6),
    c("message_id", 65),
    ADDRESSED[1],
    ADDRESSED[2],
    ADDRESSED[3],
    ADDRESSED[4],
    ADDRESSED[5],
    ADDRESSED[6],
];

const REPLACE_SM: [FieldDef; 10] = [
    c("message_id", 65),
    b("source_addr_ton"),
    b("source_addr_npi"),
    c("source_addr", 21),
    c("schedule_delivery_time", 17),
    c("validity_period", 17),
    b("registered_delivery"),
    b("sm_default_msg_id"),
    b("sm_length"),
    octets("short_message", "sm_length", 254),
];

const SUBMIT_MULTI: [FieldDef; 17] = [
    c("service_type", 6),
    b("source_addr_ton"),
    b("source_addr_npi"),
    c("source_addr", 21),
    b("number_of_dests"),
    FieldDef {
        name: "dest_address",
        ty: FieldType::DestList,
        max_len: None,
        len_from: Some("number_of_dests"),
    },
    SUBMIT_TAIL[0],
    SUBMIT_TAIL[1],
    SUBMIT_TAIL[2],
    SUBMIT_TAIL[3],
    SUBMIT_TAIL[4],
    SUBMIT_TAIL[5],
    SUBMIT_TAIL[6],
    SUBMIT_TAIL[7],
    SUBMIT_TAIL[8],
    SUBMIT_TAIL[9],
    SUBMIT_TAIL[10],
];

const SUBMIT_MULTI_RESP: [FieldDef; 3] = [
    c("message_id", 65),
    b("no_unsuccess"),
    FieldDef {
        name: "unsuccess_sme",
        ty: FieldType::UnsuccessList,
        max_len: None,
        len_from: Some("no_unsuccess"),
    },
];

const BROADCAST_SM: [FieldDef; 11] = [
    c("service_type", 6),
    b("source_addr_ton"),
    b("source_addr_npi"),
    c("source_addr", 21),
    c("message_id", 65),
    b("priority_flag"),
    c("schedule_delivery_time", 17),
    c("validity_period", 17),
    b("replace_if_present_flag"),
    b("data_coding"),
    b("sm_default_msg_id"),
];

const QUERY_BROADCAST_SM: [FieldDef; 4] = QUERY_SM;

const CANCEL_BROADCAST_SM: [FieldDef; 5] = [
    c("service_type", 6),
    c("message_id", 65),
    b("source_addr_ton"),
    b("source_addr_npi"),
    c("source_addr", 21),
];

const OUTBIND: [FieldDef; 2] = [c("system_id", 16), c("password", 9)];

const ALERT_NOTIFICATION: [FieldDef; 6] = [
    b("source_addr_ton"),
    b("source_addr_npi"),
    c("source_addr", 65),
    b("esme_addr_ton"),
    b("esme_addr_npi"),
    c("esme_addr", 65),
];

/// The mandatory parameters for a command, or `None` when the protocol defines
/// no such command — in which case the body is kept whole and no TLVs parsed.
pub fn fields_for(command: &str) -> Option<&'static [FieldDef]> {
    Some(match command {
        "bind_transmitter" | "bind_receiver" | "bind_transceiver" => &BIND,
        "bind_transmitter_resp" | "bind_receiver_resp" | "bind_transceiver_resp" => &BIND_RESP,
        "outbind" => &OUTBIND,
        "unbind"
        | "unbind_resp"
        | "enquire_link"
        | "enquire_link_resp"
        | "generic_nack"
        | "cancel_sm_resp"
        | "replace_sm_resp"
        | "cancel_broadcast_sm_resp" => &NONE,
        "submit_sm" => &SUBMIT_SM,
        "deliver_sm" => &DELIVER_SM,
        // The spec defines message_id here and says it is unused; it is still
        // on the wire, so it is still decoded.
        "deliver_sm_resp" => &MESSAGE_ID,
        "submit_sm_resp" | "data_sm_resp" | "broadcast_sm_resp" | "query_broadcast_sm_resp" => {
            &MESSAGE_ID
        }
        "data_sm" => &DATA_SM,
        "query_sm" => &QUERY_SM,
        "query_sm_resp" => &QUERY_SM_RESP,
        "cancel_sm" => &CANCEL_SM,
        "replace_sm" => &REPLACE_SM,
        "submit_multi" => &SUBMIT_MULTI,
        "submit_multi_resp" => &SUBMIT_MULTI_RESP,
        "broadcast_sm" => &BROADCAST_SM,
        "query_broadcast_sm" => &QUERY_BROADCAST_SM,
        "cancel_broadcast_sm" => &CANCEL_BROADCAST_SM,
        "alert_notification" => &ALERT_NOTIFICATION,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defines_a_field_table_for_every_command_the_client_sends() {
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
            assert!(fields_for(name).is_some(), "no field table for {name}");
        }
    }

    #[test]
    fn defines_a_field_table_for_every_response() {
        for name in [
            "submit_sm_resp",
            "bind_transceiver_resp",
            "query_sm_resp",
            "deliver_sm_resp",
        ] {
            assert!(fields_for(name).is_some(), "no field table for {name}");
        }
    }

    #[test]
    fn gives_short_message_its_length_from_sm_length() {
        let f = fields_for("submit_sm")
            .unwrap()
            .iter()
            .find(|f| f.name == "short_message")
            .unwrap();
        assert_eq!(f.ty, FieldType::Octets);
        assert_eq!(f.len_from, Some("sm_length"));
    }

    #[test]
    fn has_no_table_for_a_command_the_protocol_does_not_define() {
        assert!(fields_for("broadcast_multi").is_none());
        assert!(fields_for("unknown(0x4242)").is_none());
    }

    #[test]
    fn lists_submit_sm_fields_in_wire_order() {
        let names: Vec<_> = fields_for("submit_sm")
            .unwrap()
            .iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names[0], "service_type");
        assert_eq!(names[3], "source_addr");
        assert_eq!(names[6], "destination_addr");
        assert_eq!(names[names.len() - 1], "short_message");
    }
}
