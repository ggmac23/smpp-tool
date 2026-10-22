/**
 * The protocol values the widgets need to build their option lists, and the
 * shapes that cross the bridge.
 *
 * These are declarations, not logic: numbers from the SMPP specification and
 * the field names the Rust backend serialises. Nothing here encodes, decodes
 * or segments anything — the renderer carries no codec.
 *
 * The backend never trusts these. It takes whatever number arrives and puts
 * it on the wire, so a wrong value here shows up in the trace immediately
 * rather than being silently corrected.
 */

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

/** esm_class: messaging mode in bits 0-1, UDHI in bit 6. */
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

export const CommandId = {
  query_sm: 0x00000003,
  submit_sm: 0x00000004,
  replace_sm: 0x00000007,
  cancel_sm: 0x00000008,
  enquire_link: 0x00000015,
  submit_multi: 0x00000021,
  data_sm: 0x00000103,
  broadcast_sm: 0x00000111,
  query_broadcast_sm: 0x00000112,
  cancel_broadcast_sm: 0x00000113,
} as const;

export const TLV_MESSAGE_PAYLOAD = 0x0424;

/** This build's ceilings. The backend enforces them; these are for the UI. */
export const MAX_BURST = 50000;
export const MAX_RATE = 500;

/** The four sample messages the reference tool offers. */
export const SAMPLES = {
  gsm1: "GSM 03.38 sample: price €10 {braces} [brackets] ^caret~ |pipe| @ £ $ ¥ § ¿ ¡ ÄÖÑÜäöñüà",
  gsm3:
    "This is a three-part GSM 03.38 message used to exercise concatenation. " +
    "It runs past one hundred and sixty septets so the client has to split it, " +
    "and it keeps going past three hundred and six so that a third segment is " +
    "needed as well. It stays inside the default alphabet throughout, apart " +
    "from the euro sign € which costs two septets and therefore proves that an " +
    "escape pair is never cut in half at a segment boundary. Keep typing.",
  ucs2_1: "UCS2 sample: Привет мир — שלום עולם — こんにちは — ciao 🙂",
  ucs2_3:
    "UCS2 three-part sample. Это сообщение намеренно длинное, чтобы клиент " +
    "разбил его на три части. これは三つに分割される長いメッセージです 🙂 " +
    "и ещё немного текста, чтобы третья часть точно понадобилась.",
} as const;

/* ------------------------------------------------------------------ */
/* the shapes that cross the bridge                                   */

export type BindMode = "transceiver" | "transmitter" | "receiver";
export type BindState = "unbound" | "binding" | "bound" | "unbinding" | "closed";
export type SegmentationMode = "none" | "udh" | "sar";
export type Gsm7Packing = "unpacked" | "packed";
export type BodyPlacement = "short_message" | "message_payload";
export type ExportFormat = "csv" | "json" | "text";

export interface CorrelationConfig {
  preferTlv: boolean;
  parseText: boolean;
  normaliseRadix: boolean;
  stripLeadingZeros: boolean;
}

/**
 * What the backend correlates receipts by, when nothing has been changed.
 * Every option on, because an aggregator that does something unusual with a
 * message id is exactly what this tool is pointed at.
 */
export const DEFAULT_CORRELATION: CorrelationConfig = {
  preferTlv: true,
  parseText: true,
  normaliseRadix: true,
  stripLeadingZeros: true,
};

export interface DescribedField {
  name: string;
  value: string;
}

/** A PDU as the renderer sees it: already decoded, summarised and hexed. */
export interface WirePdu {
  seq: number;
  at: number;
  direction: "in" | "out";
  commandName: string;
  commandStatus: number;
  statusName: string;
  sequenceNumber: number;
  summary: string;
  hex: string;
  decodeError?: string | null;
}

export interface WireTlv {
  tag: number;
  hex: string;
}

export interface WireDest {
  flag: number;
  ton?: number;
  npi?: number;
  addr?: string;
  dlName?: string;
}

/** A PDU the renderer wants sent. Octet fields travel as hex. */
export interface SendPduRequest {
  commandId: number;
  commandStatus?: number;
  fields?: Record<string, string | number>;
  octetFields?: Record<string, string>;
  tlvs?: WireTlv[];
  destList?: WireDest[];
}

export interface WireTlsOptions {
  enabled: boolean;
  rejectUnauthorized: boolean;
  servername?: string | null;
  caPath?: string | null;
  clientCertPath?: string | null;
  clientKeyPath?: string | null;
  /**
   * The passphrase for an encrypted client key. Carried so the backend can
   * say plainly that it cannot open one, rather than failing with a parse
   * error the user has to guess at.
   */
  clientKeyPassphrase?: string | null;
}

export interface ConnectRequest {
  host: string;
  port: number;
  tls: WireTlsOptions;
  correlation: CorrelationConfig;
  windowSize: number;
  responseTimeoutMs: number;
  connectTimeoutMs: number;
}

export interface BindRequest {
  systemId: string;
  password: string;
  systemType: string;
  interfaceVersion: number;
  mode: BindMode;
  addrTon: number;
  addrNpi: number;
  addressRange: string;
}

export interface Measurement {
  chars: number;
  bits: number;
  bytes: number;
  parts: number;
  unmappable: string[];
}

export interface BurstStats {
  messages: number;
  pdus: number;
  submitted: number;
  accepted: number;
  errorsByStatus: Record<string, number>;
  achievedRatePerSec: number;
  latencyP50: number;
  latencyP95: number;
  latencyP99: number;
  dlrReceived: number;
  dlrUnmatched: number;
}

/** A burst row, with the status name the backend spelt out. */
export interface BurstRow {
  id: string;
  recipient: string;
  body: string;
  pdus: number;
  messageId: string | null;
  commandStatus: number | null;
  statusName: string;
  submitLatencyMs: number | null;
  dlrState: string | null;
  dlrLatencyMs: number | null;
}
