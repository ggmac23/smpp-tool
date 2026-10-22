/**
 * The one contract both processes import.
 *
 * The renderer never builds or parses a PDU: it sends intents through
 * `Commands` and receives `Events`. Everything privileged — the socket, the
 * codec, the disk, the keychain — stays in the main process.
 */

import type { BindMode, BindState } from "../main/session";
import type { BurstRow, BurstStats, BurstDoneReason, BurstConfig } from "../main/burst";
import type { Profile } from "../main/profiles";
import type { Counter } from "../main/trace";
import type { CorrelationConfig } from "../main/dlr";
import type { SegmentationMode, Gsm7Packing, Measurement } from "../smpp/text";
import type { BodyPlacement } from "../smpp/segment";
import type { DescribedField } from "../smpp/summary";

/** A PDU as the renderer sees it: already decoded and summarised. */
export interface WirePdu {
  /** Row ordinal in the trace. */
  seq: number;
  at: number;
  direction: "in" | "out";
  commandName: string;
  commandStatus: number;
  statusName: string;
  sequenceNumber: number;
  summary: string;
  /** Lowercase hex of the exact bytes on the wire. */
  hex: string;
  fields?: DescribedField[];
  decodeError?: string;
}

export interface ConnectRequest {
  host: string;
  port: number;
  tls: {
    enabled: boolean;
    rejectUnauthorized: boolean;
    servername?: string;
    caPath?: string;
    clientCertPath?: string;
    clientKeyPath?: string;
    clientKeyPassphrase?: string;
  };
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
  addrTon?: number;
  addrNpi?: number;
  addressRange?: string;
}

/** A PDU the renderer wants sent, in renderer-safe form (hex for octet fields). */
export interface SendPduRequest {
  commandId: number;
  commandStatus?: number;
  /** Octet fields arrive as hex strings and are converted in the main process. */
  fields?: Record<string, string | number>;
  octetFields?: Record<string, string>;
  tlvs?: { tag: number; hex: string }[];
  /**
   * submit_multi's repeating dest_address list. `flag` 1 is an address,
   * `flag` 2 a distribution-list name. The count field is derived in main.
   */
  destList?: { flag: number; ton?: number; npi?: number; addr?: string; dlName?: string }[];
}

export interface MeasureRequest {
  text: string;
  dataCoding: number;
  mode: SegmentationMode;
  packing?: Gsm7Packing;
}

/** A burst as the renderer configures it; the body is composed in main. */
export interface StartBurstRequest extends Omit<BurstConfig, "template" | "recipients"> {
  recipients: BurstConfig["recipients"];
  template: SendPduRequest;
}

export type ExportFormat = "csv" | "json" | "text";

export interface Commands {
  connect(req: ConnectRequest): Promise<void>;
  disconnect(): Promise<void>;
  bind(req: BindRequest): Promise<{ systemId: string }>;
  unbind(): Promise<void>;
  sendPdu(req: SendPduRequest): Promise<{ sequenceNumber: number; commandStatus: number }>;
  sendRaw(hex: string): Promise<void>;
  startBurst(req: StartBurstRequest): Promise<{ reason: BurstDoneReason; stats: BurstStats }>;
  stopBurst(): Promise<void>;
  trace(from: number, limit: number): Promise<WirePdu[]>;
  pduFields(seq: number): Promise<DescribedField[]>;
  pduHex(seq: number): Promise<string>;
  clearTrace(): Promise<void>;
  setTraceCap(n: number): Promise<void>;
  exportTrace(format: ExportFormat): Promise<{ path: string } | null>;
  exportBurst(): Promise<{ path: string } | null>;
  burstRows(from: number, limit: number): Promise<BurstRow[]>;
  listProfiles(): Promise<Profile[]>;
  saveProfile(p: Profile, password?: string, keyPassphrase?: string): Promise<void>;
  removeProfile(id: string): Promise<void>;
  profileSecrets(id: string): Promise<{ password: string | null; keyPassphrase: string | null }>;
  measure(req: MeasureRequest): Promise<Measurement>;
  pickFile(kind: "csv" | "ca" | "cert" | "key"): Promise<{ path: string; text?: string } | null>;
  status(): Promise<{
    state: BindState;
    mode: BindMode | null;
    dropped: number;
    /** Bytes the framer holds, and the length the pending PDU claims. */
    pending: { buffered: number; declared: number | null };
  }>;
}

/**
 * The runtime list of command names. Kept in step with `Commands` by the two
 * compile-time checks below: adding a command without listing it, or listing a
 * name that is not a command, fails the type-check.
 */
export const COMMAND_NAMES = [
  "connect",
  "disconnect",
  "bind",
  "unbind",
  "sendPdu",
  "sendRaw",
  "startBurst",
  "stopBurst",
  "trace",
  "pduFields",
  "pduHex",
  "clearTrace",
  "setTraceCap",
  "exportTrace",
  "exportBurst",
  "burstRows",
  "listProfiles",
  "saveProfile",
  "removeProfile",
  "profileSecrets",
  "measure",
  "pickFile",
  "status",
] as const;

export type CommandName = (typeof COMMAND_NAMES)[number];

type Unlisted = Exclude<keyof Commands, CommandName>;
type NotACommand = Exclude<CommandName, keyof Commands>;
const _everyCommandIsListed: Unlisted extends never ? true : ["missing from COMMAND_NAMES", Unlisted] =
  true;
const _everyListedNameIsACommand: NotACommand extends never
  ? true
  : ["not a command", NotACommand] = true;
void _everyCommandIsListed;
void _everyListedNameIsACommand;

export interface Events {
  state: { state: BindState; mode: BindMode | null };
  /** Batched: one message carries many PDUs. */
  pdu: WirePdu[];
  counters: Record<string, Counter>;
  "burst-progress": { sent: number; total: number; stats: BurstStats };
  /** Batched. */
  "burst-row": BurstRow[];
  "burst-done": { reason: BurstDoneReason; stats: BurstStats };
  error: { message: string; where: string };
}

export type EventName = keyof Events;

export const EVENT_NAMES: EventName[] = [
  "state",
  "pdu",
  "counters",
  "burst-progress",
  "burst-row",
  "burst-done",
  "error",
];

/** What a command returns over IPC: never a throw, always a verdict. */
export type Result<T> = { _ok: true; value: T } | { _ok: false; error: string };

/** The surface the preload script exposes on `window.smpp`. */
export interface SmppBridge extends Commands {
  on<K extends EventName>(name: K, cb: (payload: Events[K]) => void): () => void;
}
