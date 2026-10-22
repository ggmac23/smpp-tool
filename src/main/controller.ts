import { Transport } from "./transport";
import { Session, BindMode, BindState } from "./session";
import { TraceStore, TraceEntry, TraceFileSink } from "./trace";
import { toCsv, traceToText } from "./export";
import { BurstRunner, BurstRow, BurstStats, BurstDoneReason } from "./burst";
import { ProfileStore, Profile } from "./profiles";
import { CorrelationConfig, DEFAULT_CORRELATION } from "./dlr";
import { IpcHandle } from "./ipc";
import { decodePdu, Pdu, PduDecodeError, FieldValue } from "../smpp/decode";
import { PduInput, encodeRaw } from "../smpp/encode";
import { summarise, describe, DescribedField } from "../smpp/summary";
import { measure } from "../smpp/text";
import { commandName, statusName } from "../smpp/constants";
import type {
  Commands,
  ConnectRequest,
  BindRequest,
  SendPduRequest,
  StartBurstRequest,
  MeasureRequest,
  WirePdu,
  ExportFormat,
} from "../shared/api";

export interface Dialogs {
  openFile(kind: "csv" | "ca" | "cert" | "key"): Promise<string | null>;
  saveFile(suggestedName: string): Promise<string | null>;
}

export interface ControllerDeps {
  profiles: ProfileStore;
  dialogs: Dialogs;
  readFile(path: string): Buffer;
  writeFile(path: string, data: string): void;
  push: IpcHandle;
  traceCap?: number;
}

const TRACE_CSV_COLUMNS = [
  "seq",
  "at",
  "direction",
  "commandName",
  "statusName",
  "sequenceNumber",
  "summary",
  "hex",
];

const BURST_CSV_COLUMNS = [
  "id",
  "recipient",
  "body",
  "pdus",
  "messageId",
  "commandStatus",
  "statusName",
  "submitLatencyMs",
  "dlrState",
  "dlrLatencyMs",
];

function hexToBuffer(hex: string): Buffer {
  return Buffer.from(hex.replace(/[^0-9a-fA-F]/g, ""), "hex");
}

/** Renderer-safe request to a PDU model: hex strings become Buffers here. */
export function toPduInput(req: SendPduRequest): Omit<PduInput, "sequenceNumber"> {
  const fields: Record<string, FieldValue> = { ...(req.fields ?? {}) };
  for (const [name, hex] of Object.entries(req.octetFields ?? {})) {
    fields[name] = hexToBuffer(hex);
  }
  if (req.destList) fields.dest_address = req.destList;
  return {
    commandId: req.commandId,
    commandStatus: req.commandStatus,
    fields,
    tlvs: (req.tlvs ?? []).map((t) => ({ tag: t.tag, value: hexToBuffer(t.hex) })),
  };
}

export function toWirePdu(e: TraceEntry): WirePdu {
  return {
    seq: e.seq,
    at: e.at,
    direction: e.direction,
    commandName: e.commandName,
    commandStatus: e.commandStatus,
    statusName: statusName(e.commandStatus),
    sequenceNumber: e.sequenceNumber,
    summary: e.summary,
    hex: e.raw.toString("hex"),
    decodeError: e.decodeError,
  };
}

/**
 * Everything the renderer can ask for, in one object.
 *
 * It owns the transport, the session, the trace and the burst runner, and it
 * is the only place they are assembled. The renderer never sees a socket, a
 * Buffer or a PDU model — it sends intents and gets plain data back.
 */
export class Controller implements Commands {
  private transport?: Transport;
  private session?: Session;
  private burst?: BurstRunner;
  private sink?: TraceFileSink;
  private correlation: CorrelationConfig = DEFAULT_CORRELATION;
  private nextRow = 1;

  private readonly trace_ = new TraceStore();

  constructor(private readonly deps: ControllerDeps) {
    if (deps.traceCap !== undefined) this.trace_.setCap(deps.traceCap);
  }

  /* ---------------------------------------------------------------- */
  /* connection                                                       */

  async connect(req: ConnectRequest): Promise<void> {
    if (this.transport) await this.disconnect();
    this.correlation = req.correlation;

    this.transport = new Transport({
      host: req.host,
      port: req.port,
      connectTimeoutMs: req.connectTimeoutMs,
      tls: {
        enabled: req.tls.enabled,
        rejectUnauthorized: req.tls.rejectUnauthorized,
        servername: req.tls.servername,
        ca: req.tls.caPath ? this.deps.readFile(req.tls.caPath) : undefined,
        clientCert: req.tls.clientCertPath ? this.deps.readFile(req.tls.clientCertPath) : undefined,
        clientKey: req.tls.clientKeyPath ? this.deps.readFile(req.tls.clientKeyPath) : undefined,
        clientKeyPassphrase: req.tls.clientKeyPassphrase,
      },
    });

    this.session = new Session(this.transport, {
      windowSize: req.windowSize,
      responseTimeoutMs: req.responseTimeoutMs,
    });
    this.wire(this.session);

    await this.transport.connect();
  }

  async disconnect(): Promise<void> {
    this.burst?.stop();
    this.transport?.destroy();
    this.transport = undefined;
    this.session = undefined;
    this.sink?.close();
    this.sink = undefined;
  }

  async bind(req: BindRequest): Promise<{ systemId: string }> {
    const resp = await this.need().bind({
      systemId: req.systemId,
      password: req.password,
      systemType: req.systemType,
      interfaceVersion: req.interfaceVersion,
      mode: req.mode,
      addrTon: req.addrTon,
      addrNpi: req.addrNpi,
      addressRange: req.addressRange,
    });
    return { systemId: String(resp.fields.system_id ?? "") };
  }

  async unbind(): Promise<void> {
    await this.need().unbind();
  }

  async status(): Promise<{
    state: BindState;
    mode: BindMode | null;
    dropped: number;
    pending: { buffered: number; declared: number | null };
  }> {
    return {
      state: this.session?.state ?? "unbound",
      mode: this.session?.mode ?? null,
      dropped: this.trace_.dropped,
      // Bytes held waiting for a PDU that declared more than it sent: without
      // this the stall is silent.
      pending: this.transport?.pending ?? { buffered: 0, declared: null },
    };
  }

  /* ---------------------------------------------------------------- */
  /* sending                                                          */

  async sendPdu(req: SendPduRequest): Promise<{ sequenceNumber: number; commandStatus: number }> {
    const resp = await this.need().send(toPduInput(req));
    return { sequenceNumber: resp.sequenceNumber, commandStatus: resp.commandStatus };
  }

  async sendRaw(hex: string): Promise<void> {
    this.need().sendRaw(encodeRaw(hexToBuffer(hex)));
  }

  async measure(req: MeasureRequest) {
    return measure(req.text, req.dataCoding, req.mode, req.packing);
  }

  /* ---------------------------------------------------------------- */
  /* burst                                                            */

  async startBurst(
    req: StartBurstRequest
  ): Promise<{ reason: BurstDoneReason; stats: BurstStats }> {
    const runner = new BurstRunner(this.need(), this.correlation);
    this.burst = runner;

    runner.on("row", (row: BurstRow) => this.deps.push.pushBurstRow(row));
    runner.on("progress", (p: { sent: number; total: number; stats: BurstStats }) =>
      this.deps.push.pushEvent("burst-progress", p)
    );

    let outcome: { reason: BurstDoneReason; stats: BurstStats } | undefined;
    runner.on("done", (e: { reason: BurstDoneReason; stats: BurstStats }) => (outcome = e));

    await runner.start({ ...req, template: toPduInput(req.template) });

    const done = outcome ?? { reason: "complete" as BurstDoneReason, stats: runner.stats };
    this.deps.push.pushEvent("burst-done", done);
    return done;
  }

  async stopBurst(): Promise<void> {
    this.burst?.stop();
  }

  async burstRows(from: number, limit: number): Promise<BurstRow[]> {
    return (this.burst?.rows ?? []).slice(from, from + limit);
  }

  /* ---------------------------------------------------------------- */
  /* trace                                                            */

  async trace(from: number, limit: number): Promise<WirePdu[]> {
    return this.trace_.entries(from, limit).map(toWirePdu);
  }

  async pduFields(seq: number): Promise<DescribedField[]> {
    const entry = this.entryAt(seq);
    if (!entry) return [];
    try {
      return describe(decodePdu(entry.raw));
    } catch (e) {
      return [{ name: "decode error", value: (e as Error).message }];
    }
  }

  async pduHex(seq: number): Promise<string> {
    return this.entryAt(seq)?.raw.toString("hex") ?? "";
  }

  async clearTrace(): Promise<void> {
    this.trace_.clear();
    this.deps.push.pushCounters(this.trace_.counters);
  }

  async setTraceCap(n: number): Promise<void> {
    this.trace_.setCap(n);
  }

  async exportTrace(format: ExportFormat): Promise<{ path: string } | null> {
    const path = await this.deps.dialogs.saveFile(`trace.${format === "text" ? "txt" : format}`);
    if (!path) return null;

    const entries = this.trace_.entries();
    const body =
      format === "text"
        ? traceToText(entries)
        : format === "json"
          ? JSON.stringify(entries.map(toWirePdu), null, 2)
          : toCsv(
              entries.map(toWirePdu) as unknown as Record<string, unknown>[],
              TRACE_CSV_COLUMNS
            );

    this.deps.writeFile(path, body);
    return { path };
  }

  async exportBurst(): Promise<{ path: string } | null> {
    const path = await this.deps.dialogs.saveFile("burst.csv");
    if (!path) return null;

    const rows = (this.burst?.rows ?? []).map((r) => ({
      ...r,
      statusName: r.commandStatus === null ? "" : statusName(r.commandStatus),
    }));
    this.deps.writeFile(
      path,
      toCsv(rows as unknown as Record<string, unknown>[], BURST_CSV_COLUMNS)
    );
    return { path };
  }

  /* ---------------------------------------------------------------- */
  /* profiles and files                                               */

  async listProfiles(): Promise<Profile[]> {
    return this.deps.profiles.list();
  }

  async saveProfile(p: Profile, password?: string, keyPassphrase?: string): Promise<void> {
    this.deps.profiles.save(p, password, keyPassphrase);
  }

  async removeProfile(id: string): Promise<void> {
    this.deps.profiles.remove(id);
  }

  async profileSecrets(
    id: string
  ): Promise<{ password: string | null; keyPassphrase: string | null }> {
    return {
      password: this.deps.profiles.password(id),
      keyPassphrase: this.deps.profiles.keyPassphrase(id),
    };
  }

  async pickFile(
    kind: "csv" | "ca" | "cert" | "key"
  ): Promise<{ path: string; text?: string } | null> {
    const path = await this.deps.dialogs.openFile(kind);
    if (!path) return null;
    // Only a CSV's contents come back. A key's bytes have no business in the
    // renderer, so the profile keeps the path and main reads it at connect time.
    return kind === "csv" ? { path, text: this.deps.readFile(path).toString("utf8") } : { path };
  }

  /* ---------------------------------------------------------------- */

  /** Start appending the trace to a file, for sessions longer than the cap. */
  openTraceFile(path: string): void {
    this.sink?.close();
    this.sink = new TraceFileSink(path);
  }

  private entryAt(seq: number): TraceEntry | undefined {
    const entry = this.trace_.entries(seq, 1)[0];
    return entry && entry.seq === seq ? entry : undefined;
  }

  private need(): Session {
    if (!this.session) throw new Error("not connected");
    return this.session;
  }

  private wire(session: Session): void {
    const record = (direction: "in" | "out", pdu: Pdu, at: number): void =>
      this.add({
        seq: this.nextRow++,
        at,
        direction,
        commandName: commandName(pdu.commandId),
        commandStatus: pdu.commandStatus,
        sequenceNumber: pdu.sequenceNumber,
        summary: summarise(pdu),
        raw: pdu.raw,
      });

    session.on("pdu-out", (e: { pdu: Pdu; at: number }) => record("out", e.pdu, e.at));
    session.on("pdu-in", (e: { pdu: Pdu; at: number }) => record("in", e.pdu, e.at));

    session.on("decode-error", (e: { error: PduDecodeError }) =>
      this.add({
        seq: this.nextRow++,
        at: Date.now(),
        direction: "in",
        commandName: "(undecodable)",
        commandStatus: 0,
        sequenceNumber: 0,
        summary: "",
        raw: e.error.raw ?? Buffer.alloc(0),
        decodeError: e.error.message,
      })
    );

    session.on("raw-out", (e: { bytes: Buffer; at: number }) =>
      this.add({
        seq: this.nextRow++,
        at: e.at,
        direction: "out",
        commandName: "(raw)",
        commandStatus: 0,
        sequenceNumber: 0,
        summary: `${e.bytes.length} bytes composed by hand`,
        raw: e.bytes,
      })
    );

    session.on("state", (payload: { state: BindState; mode: BindMode | null }) =>
      this.deps.push.pushState(payload)
    );
  }

  private add(entry: TraceEntry): void {
    this.trace_.add(entry);
    this.sink?.write(entry);
    this.deps.push.pushPdu(toWirePdu(entry));
    this.deps.push.pushCounters(this.trace_.counters);
  }
}
