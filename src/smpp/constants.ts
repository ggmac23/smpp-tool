/**
 * SMPP numeric constants.
 *
 * Every value here comes from the SMPP 3.4 and 5.0 specifications. Nothing is
 * invented: a value the protocol does not define has no entry, because sending
 * a fabricated command id would produce a PDU no SMSC can parse.
 */

function hex(n: number): string {
  return "0x" + (n >>> 0).toString(16);
}

/** Requests that have a matching `_resp`. */
const REQUESTS = {
  bind_receiver: 0x00000001,
  bind_transmitter: 0x00000002,
  query_sm: 0x00000003,
  submit_sm: 0x00000004,
  deliver_sm: 0x00000005,
  unbind: 0x00000006,
  replace_sm: 0x00000007,
  cancel_sm: 0x00000008,
  bind_transceiver: 0x00000009,
  enquire_link: 0x00000015,
  submit_multi: 0x00000021,
  data_sm: 0x00000103,
  broadcast_sm: 0x00000111,
  query_broadcast_sm: 0x00000112,
  cancel_broadcast_sm: 0x00000113,
} as const;

/** Requests the protocol defines with no response. */
const ONE_WAY = {
  outbind: 0x0000000b,
  alert_notification: 0x00000102,
} as const;

const RESPONSE_BIT = 0x80000000;

type RequestName = keyof typeof REQUESTS;
type RespName = `${RequestName}_resp`;

export type CommandName = RequestName | RespName | keyof typeof ONE_WAY | "generic_nack";

const ids: Record<string, number> = { ...REQUESTS, ...ONE_WAY, generic_nack: 0x80000000 };
for (const [name, id] of Object.entries(REQUESTS)) {
  ids[`${name}_resp`] = (RESPONSE_BIT | id) >>> 0;
}

export const CommandId = ids as Record<CommandName, number> & typeof REQUESTS;

const NAMES_BY_ID = new Map<number, string>();
for (const [name, id] of Object.entries(CommandId)) NAMES_BY_ID.set(id, name);

/** The command's name, or `unknown(0x…)` for an id the protocol does not define. */
export function commandName(id: number): string {
  return NAMES_BY_ID.get(id >>> 0) ?? `unknown(${hex(id)})`;
}

export function isResponse(id: number): boolean {
  return ((id >>> 0) & RESPONSE_BIT) !== 0;
}

/** The response id for a request id. */
export function responseIdFor(requestId: number): number {
  return (RESPONSE_BIT | requestId) >>> 0;
}

export const CommandStatus = {
  ESME_ROK: 0x00000000,
  ESME_RINVMSGLEN: 0x00000001,
  ESME_RINVCMDLEN: 0x00000002,
  ESME_RINVCMDID: 0x00000003,
  ESME_RINVBNDSTS: 0x00000004,
  ESME_RALYBND: 0x00000005,
  ESME_RINVPRTFLG: 0x00000006,
  ESME_RINVREGDLVFLG: 0x00000007,
  ESME_RSYSERR: 0x00000008,
  ESME_RINVSRCADR: 0x0000000a,
  ESME_RINVDSTADR: 0x0000000b,
  ESME_RINVMSGID: 0x0000000c,
  ESME_RBINDFAIL: 0x0000000d,
  ESME_RINVPASWD: 0x0000000e,
  ESME_RINVSYSID: 0x0000000f,
  ESME_RCANCELFAIL: 0x00000011,
  ESME_RREPLACEFAIL: 0x00000013,
  ESME_RMSGQFUL: 0x00000014,
  ESME_RINVSERTYP: 0x00000015,
  ESME_RINVNUMDESTS: 0x00000033,
  ESME_RINVDLNAME: 0x00000034,
  ESME_RINVDESTFLAG: 0x00000040,
  ESME_RINVSUBREP: 0x00000042,
  ESME_RINVESMCLASS: 0x00000043,
  ESME_RCNTSUBDL: 0x00000044,
  ESME_RSUBMITFAIL: 0x00000045,
  ESME_RINVSRCTON: 0x00000048,
  ESME_RINVSRCNPI: 0x00000049,
  ESME_RINVDSTTON: 0x00000050,
  ESME_RINVDSTNPI: 0x00000051,
  ESME_RINVSYSTYP: 0x00000053,
  ESME_RINVREPFLAG: 0x00000054,
  ESME_RINVNUMMSGS: 0x00000055,
  ESME_RTHROTTLED: 0x00000058,
  ESME_RINVSCHED: 0x00000061,
  ESME_RINVEXPIRY: 0x00000062,
  ESME_RINVDFTMSGID: 0x00000063,
  ESME_RX_T_APPN: 0x00000064,
  ESME_RX_P_APPN: 0x00000065,
  ESME_RX_R_APPN: 0x00000066,
  ESME_RQUERYFAIL: 0x00000067,
  ESME_RINVOPTPARSTREAM: 0x000000c0,
  ESME_ROPTPARNOTALLWD: 0x000000c1,
  ESME_RINVPARLEN: 0x000000c2,
  ESME_RMISSINGOPTPARAM: 0x000000c3,
  ESME_RINVOPTPARAMVAL: 0x000000c4,
  ESME_RDELIVERYFAILURE: 0x000000fe,
  ESME_RUNKNOWNERR: 0x000000ff,
} as const;

const STATUS_BY_ID = new Map<number, string>(
  Object.entries(CommandStatus).map(([n, v]) => [v as number, n])
);

export function statusName(n: number): string {
  return STATUS_BY_ID.get(n >>> 0) ?? `unknown(${hex(n)})`;
}

export const InterfaceVersion = {
  DEFAULT: 0x00,
  V33: 0x33,
  V34: 0x34,
  V50: 0x50,
} as const;

export const Ton = {
  UNKNOWN: 0,
  INTERNATIONAL: 1,
  NATIONAL: 2,
  NETWORK_SPECIFIC: 3,
  SUBSCRIBER_NUMBER: 4,
  ALPHANUMERIC: 5,
  ABBREVIATED: 6,
} as const;

export const Npi = {
  UNKNOWN: 0,
  ISDN_E163_E164: 1,
  DATA_X121: 3,
  TELEX_F69: 4,
  LAND_MOBILE_E212: 6,
  NATIONAL: 8,
  PRIVATE: 9,
  ERMES: 10,
  INTERNET_IP: 14,
  WAP_CLIENT_ID: 18,
} as const;

/** data_coding 0–8, the set the reference tool exposes. */
export const DataCoding = {
  GSM_DEFAULT: 0,
  IA5_ASCII: 1,
  OCTET_UNSPECIFIED_2: 2,
  LATIN_1: 3,
  OCTET_UNSPECIFIED_4: 4,
  JIS: 5,
  CYRILLIC: 6,
  LATIN_HEBREW: 7,
  UCS2: 8,
} as const;

/** esm_class: messaging mode in bits 0–1, UDHI in bit 6. */
export const EsmClass = {
  DEFAULT: 0x00,
  DATAGRAM: 0x01,
  FORWARD: 0x02,
  STORE_AND_FORWARD: 0x03,
  DEFAULT_UDHI: 0x40,
  DATAGRAM_UDHI: 0x41,
  FORWARD_UDHI: 0x42,
  STORE_AND_FORWARD_UDHI: 0x43,
} as const;

export const ESM_UDHI_BIT = 0x40;

/** registered_delivery: receipt in bits 0–1, intermediate notification in bit 4. */
export const RegisteredDelivery = {
  NONE: 0,
  FAILURE_AND_SUCCESS: 1,
  FAILURE_ONLY: 2,
  SUCCESS_ONLY: 3,
  INTERMEDIATE_NONE: 16,
  INTERMEDIATE_FAILURE_AND_SUCCESS: 17,
  INTERMEDIATE_FAILURE_ONLY: 18,
  INTERMEDIATE_SUCCESS_ONLY: 19,
} as const;

export const PriorityFlag = {
  LEVEL_0: 0,
  LEVEL_1: 1,
  LEVEL_2: 2,
  LEVEL_3: 3,
  LEVEL_4: 4,
} as const;

export type TlvType = "octets" | "cstring" | "u8" | "u16" | "u32";

export const KnownTlv: Record<number, { name: string; type: TlvType }> = {
  0x001e: { name: "receipted_message_id", type: "cstring" },
  0x0204: { name: "user_message_reference", type: "u16" },
  0x020a: { name: "source_port", type: "u16" },
  0x020b: { name: "destination_port", type: "u16" },
  0x020c: { name: "sar_msg_ref_num", type: "u16" },
  0x020d: { name: "language_indicator", type: "u8" },
  0x020e: { name: "sar_total_segments", type: "u8" },
  0x020f: { name: "sar_segment_seqnum", type: "u8" },
  0x0019: { name: "esm_class_outbind", type: "u8" },
  0x0423: { name: "network_error_code", type: "octets" },
  0x0424: { name: "message_payload", type: "octets" },
  0x0425: { name: "delivery_failure_reason", type: "u8" },
  0x0426: { name: "more_messages_to_send", type: "u8" },
  0x0427: { name: "message_state", type: "u8" },
  0x0501: { name: "its_reply_type", type: "u8" },
} as const;
