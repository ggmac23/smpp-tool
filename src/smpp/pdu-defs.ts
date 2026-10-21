/**
 * Mandatory-parameter tables, in wire order, for every command the client
 * speaks. Sourced from SMPP 3.4 and the 5.0 broadcast additions.
 *
 * `submit_multi` and `submit_multi_resp` carry repeating structures, so the
 * field types go beyond the scalar four: `dest_list` and `unsuccess_list`
 * are decoded and encoded as arrays.
 */

export type FieldType =
  | "cstring"
  | "u8"
  | "u16"
  | "u32"
  | "octets"
  | "dest_list"
  | "unsuccess_list";

export interface FieldDef {
  name: string;
  type: FieldType;
  /** Spec maximum, including the NUL for a cstring. Advisory: never enforced. */
  maxLen?: number;
  /** For `octets`, `dest_list` and `unsuccess_list`: the field holding the count. */
  lenFrom?: string;
}

const c = (name: string, maxLen?: number): FieldDef => ({ name, type: "cstring", maxLen });
const b = (name: string): FieldDef => ({ name, type: "u8" });

/** service_type + source address + destination address, the shared head. */
const ADDRESSED: FieldDef[] = [
  c("service_type", 6),
  b("source_addr_ton"),
  b("source_addr_npi"),
  c("source_addr", 21),
  b("dest_addr_ton"),
  b("dest_addr_npi"),
  c("destination_addr", 21),
];

const SUBMIT_TAIL: FieldDef[] = [
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
  { name: "short_message", type: "octets", lenFrom: "sm_length", maxLen: 254 },
];

const BIND: FieldDef[] = [
  c("system_id", 16),
  c("password", 9),
  c("system_type", 13),
  b("interface_version"),
  b("addr_ton"),
  b("addr_npi"),
  c("address_range", 41),
];

const BIND_RESP: FieldDef[] = [c("system_id", 16)];
const MESSAGE_ID: FieldDef[] = [c("message_id", 65)];
const NONE: FieldDef[] = [];

export const PDU_FIELDS: Record<string, FieldDef[]> = {
  bind_transmitter: BIND,
  bind_transmitter_resp: BIND_RESP,
  bind_receiver: BIND,
  bind_receiver_resp: BIND_RESP,
  bind_transceiver: BIND,
  bind_transceiver_resp: BIND_RESP,

  outbind: [c("system_id", 16), c("password", 9)],

  unbind: NONE,
  unbind_resp: NONE,
  enquire_link: NONE,
  enquire_link_resp: NONE,
  generic_nack: NONE,

  submit_sm: [...ADDRESSED, ...SUBMIT_TAIL],
  submit_sm_resp: MESSAGE_ID,

  deliver_sm: [...ADDRESSED, ...SUBMIT_TAIL],
  // The spec defines message_id here and says it is unused; it is still on the wire.
  deliver_sm_resp: [c("message_id", 1)],

  data_sm: [
    ...ADDRESSED,
    b("esm_class"),
    b("registered_delivery"),
    b("data_coding"),
  ],
  data_sm_resp: MESSAGE_ID,

  query_sm: [c("message_id", 65), b("source_addr_ton"), b("source_addr_npi"), c("source_addr", 21)],
  query_sm_resp: [
    c("message_id", 65),
    c("final_date", 17),
    b("message_state"),
    b("error_code"),
  ],

  cancel_sm: [c("service_type", 6), c("message_id", 65), ...ADDRESSED.slice(1)],
  cancel_sm_resp: NONE,

  replace_sm: [
    c("message_id", 65),
    b("source_addr_ton"),
    b("source_addr_npi"),
    c("source_addr", 21),
    c("schedule_delivery_time", 17),
    c("validity_period", 17),
    b("registered_delivery"),
    b("sm_default_msg_id"),
    b("sm_length"),
    { name: "short_message", type: "octets", lenFrom: "sm_length", maxLen: 254 },
  ],
  replace_sm_resp: NONE,

  submit_multi: [
    c("service_type", 6),
    b("source_addr_ton"),
    b("source_addr_npi"),
    c("source_addr", 21),
    b("number_of_dests"),
    { name: "dest_address", type: "dest_list", lenFrom: "number_of_dests" },
    ...SUBMIT_TAIL,
  ],
  submit_multi_resp: [
    c("message_id", 65),
    b("no_unsuccess"),
    { name: "unsuccess_sme", type: "unsuccess_list", lenFrom: "no_unsuccess" },
  ],

  broadcast_sm: [
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
  ],
  broadcast_sm_resp: MESSAGE_ID,

  query_broadcast_sm: [
    c("message_id", 65),
    b("source_addr_ton"),
    b("source_addr_npi"),
    c("source_addr", 21),
  ],
  query_broadcast_sm_resp: MESSAGE_ID,

  cancel_broadcast_sm: [
    c("service_type", 6),
    c("message_id", 65),
    b("source_addr_ton"),
    b("source_addr_npi"),
    c("source_addr", 21),
  ],
  cancel_broadcast_sm_resp: NONE,

  alert_notification: [
    b("source_addr_ton"),
    b("source_addr_npi"),
    c("source_addr", 65),
    b("esme_addr_ton"),
    b("esme_addr_npi"),
    c("esme_addr", 65),
  ],
};
