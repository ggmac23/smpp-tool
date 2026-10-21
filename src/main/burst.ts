import { EventEmitter } from "node:events";
import { Session } from "./session";
import { DlrCorrelator, CorrelationConfig } from "./dlr";
import { RecipientSource, resolveRecipients, MAX_RATE } from "./recipients";
import { PduInput } from "../smpp/encode";
import { Pdu } from "../smpp/decode";
import { parseReceipt } from "../smpp/receipt";
import { segment, BodyPlacement } from "../smpp/segment";
import { SegmentationMode, Gsm7Packing } from "../smpp/text";
import { CommandStatus, statusName } from "../smpp/constants";

export interface BurstConfig {
  recipients: RecipientSource;
  ratePerSec: number;
  /** The submit_sm template: everything except the destination and the body. */
  template: Omit<PduInput, "sequenceNumber">;
  text: string;
  dataCoding: number;
  segmentation: SegmentationMode;
  placement: BodyPlacement;
  packing?: Gsm7Packing;
  /** On ESME_RTHROTTLED: halve the rate, instead of holding it and recording the error. */
  backoffOnThrottle: boolean;
  /** How long to keep waiting for delivery receipts once every submit is answered. */
  awaitDlrMs: number;
}

export interface BurstRow {
  id: string;
  recipient: string;
  body: string;
  /** PDUs this message took. A 3-part message is one row with pdus 3. */
  pdus: number;
  messageId: string | null;
  commandStatus: number | null;
  submitLatencyMs: number | null;
  dlrState: string | null;
  dlrLatencyMs: number | null;
}

export interface BurstStats {
  /** Messages the user asked for. */
  messages: number;
  /** PDUs those messages took. */
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

export type BurstDoneReason = "complete" | "stopped" | "connection-lost";

const TICK_MS = 20;
const PROGRESS_MS = 100;

function percentile(sorted: number[], p: number): number {
  if (sorted.length === 0) return 0;
  const i = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[i];
}

/**
 * Sends a burst of messages at a requested rate and records what came back.
 *
 * On ESME_RTHROTTLED the default is to record the error and hold the requested
 * rate — the point of the exercise is to find where the SMSC actually breaks.
 * If the bind drops the burst stops and says so; it never reconnects.
 *
 * `rows` is never capped. The trace may lose history under pressure; the
 * result table may not.
 */
export class BurstRunner extends EventEmitter {
  private readonly _rows: BurstRow[] = [];
  private readonly rowById = new Map<string, BurstRow>();
  private readonly latencies: number[] = [];
  private readonly errors: Record<string, number> = {};
  private correlator: DlrCorrelator;

  private running = false;
  private stopping = false;
  private lostConnection = false;
  private startedAt = 0;
  private finishedAt = 0;
  /** When the last submit settled. The rate is measured to here, not to the
   * end of the wait for delivery receipts, which would dilute it. */
  private sendingEndedAt = 0;
  private _submitted = 0;
  private _accepted = 0;
  private _pdus = 0;
  private _messages = 0;
  private _dlrReceived = 0;
  private _rate = 0;
  /** Parts of each row already answered, so a row is accepted only when all are. */
  private readonly partsDone = new Map<string, number>();

  constructor(
    private readonly session: Session,
    private readonly correlation: CorrelationConfig
  ) {
    super();
    this.correlator = new DlrCorrelator(correlation);
  }

  get rows(): BurstRow[] {
    return this._rows;
  }

  /** The rate currently in force: the requested one unless backoff kicked in. */
  get currentRatePerSec(): number {
    return this._rate;
  }

  get stats(): BurstStats {
    const sorted = [...this.latencies].sort((a, b) => a - b);
    const sendingEnded = this.sendingEndedAt || this.finishedAt || Date.now();
    const elapsed = (sendingEnded - this.startedAt) / 1000;
    return {
      messages: this._messages,
      pdus: this._pdus,
      submitted: this._submitted,
      accepted: this._accepted,
      errorsByStatus: { ...this.errors },
      achievedRatePerSec: elapsed > 0 ? this._submitted / elapsed : 0,
      latencyP50: percentile(sorted, 50),
      latencyP95: percentile(sorted, 95),
      latencyP99: percentile(sorted, 99),
      dlrReceived: this._dlrReceived,
      dlrUnmatched: this.correlator.unmatched.length,
    };
  }

  stop(): void {
    this.stopping = true;
  }

  async start(cfg: BurstConfig): Promise<void> {
    if (this.running) throw new Error("a burst is already running");
    if (this.session.state !== "bound") {
      throw new Error(`session is not bound (state is ${this.session.state})`);
    }
    if (cfg.ratePerSec > MAX_RATE) {
      throw new Error(`${cfg.ratePerSec}/sec is above the ${MAX_RATE}/sec ceiling of this build`);
    }
    if (cfg.ratePerSec < 1) throw new Error("the rate must be at least 1 message per second");

    // Resolving first means an over-ceiling count is refused before a single
    // PDU goes out.
    const { numbers } = resolveRecipients(cfg.recipients);

    this.running = true;
    this.stopping = false;
    this.lostConnection = false;
    this._rate = cfg.ratePerSec;
    this.startedAt = Date.now();
    this.finishedAt = 0;
    this.sendingEndedAt = 0;
    this.correlator = new DlrCorrelator(this.correlation);

    const onReceipt = (e: { pdu: Pdu }) => this.onInbound(e.pdu);
    const onState = (e: { state: string }) => {
      if (e.state !== "bound") {
        this.lostConnection = true;
        this.stopping = true;
      }
    };
    this.session.on("pdu-in", onReceipt);
    this.session.on("state", onState);

    const progressTimer = setInterval(
      () => this.emit("progress", { sent: this._submitted, total: numbers.length, stats: this.stats }),
      PROGRESS_MS
    );

    try {
      await this.pump(cfg, numbers);
      this.sendingEndedAt = Date.now();
      await this.awaitDlrs(cfg.awaitDlrMs);
    } finally {
      clearInterval(progressTimer);
      this.session.off("pdu-in", onReceipt);
      this.session.off("state", onState);
      this.finishedAt = Date.now();
      this.running = false;
    }

    const reason: BurstDoneReason = this.lostConnection
      ? "connection-lost"
      : this.stopping
        ? "stopped"
        : "complete";

    this.emit("progress", { sent: this._submitted, total: numbers.length, stats: this.stats });
    this.emit("done", { reason, stats: this.stats });
  }

  /** Pace the submits, `perTick` at a time, and wait for them to settle. */
  private async pump(cfg: BurstConfig, numbers: string[]): Promise<void> {
    const inflight: Promise<void>[] = [];
    let i = 0;

    while (i < numbers.length && !this.stopping) {
      const perTick = Math.max(1, Math.round((this._rate * TICK_MS) / 1000));
      const tickStarted = Date.now();

      for (let n = 0; n < perTick && i < numbers.length && !this.stopping; n++, i++) {
        inflight.push(this.sendOne(cfg, numbers[i], i));
      }

      const spent = Date.now() - tickStarted;
      if (spent < TICK_MS) await new Promise((r) => setTimeout(r, TICK_MS - spent));
    }

    await Promise.all(inflight);
  }

  private async sendOne(cfg: BurstConfig, recipient: string, index: number): Promise<void> {
    const row: BurstRow = {
      id: `r${index}`,
      recipient,
      body: cfg.text,
      pdus: 0,
      messageId: null,
      commandStatus: null,
      submitLatencyMs: null,
      dlrState: null,
      dlrLatencyMs: null,
    };
    this._rows.push(row);
    this.rowById.set(row.id, row);
    this._messages++;

    const segments = segment({
      text: cfg.text,
      dataCoding: cfg.dataCoding,
      mode: cfg.segmentation,
      placement: cfg.placement,
      packing: cfg.packing,
      esmClass: Number(cfg.template.fields?.esm_class ?? 0),
    });

    row.pdus = segments.length;

    for (const seg of segments) {
      if (this.stopping) break;

      const input: Omit<PduInput, "sequenceNumber"> = {
        ...cfg.template,
        fields: {
          ...cfg.template.fields,
          ...seg.fields,
          destination_addr: recipient,
          data_coding: cfg.dataCoding,
        },
        tlvs: [...(cfg.template.tlvs ?? []), ...seg.tlvs],
      };

      const sentAt = Date.now();
      try {
        const resp = await this.session.send(input);
        const latency = Date.now() - sentAt;
        this._pdus++;
        this._submitted++;
        this.latencies.push(latency);
        if (row.submitLatencyMs === null) row.submitLatencyMs = latency;

        if (resp.commandStatus === CommandStatus.ESME_ROK) {
          const id = String(resp.fields.message_id ?? "");
          if (row.messageId === null && id.length > 0) {
            row.messageId = id;
            this.correlator.register(id, row.id);
          }
          // A row is accepted only once every one of its parts is.
          if (row.commandStatus === null && row.pdus === this._partsDone(row) + 1) {
            row.commandStatus = CommandStatus.ESME_ROK;
          }
        } else {
          const name = statusName(resp.commandStatus);
          this.errors[name] = (this.errors[name] ?? 0) + 1;
          // The first non-zero status is the row's verdict and it is not overwritten.
          if (row.commandStatus === null || row.commandStatus === CommandStatus.ESME_ROK) {
            row.commandStatus = resp.commandStatus;
          }
          if (resp.commandStatus === CommandStatus.ESME_RTHROTTLED && cfg.backoffOnThrottle) {
            this._rate = Math.max(1, Math.floor(this._rate / 2));
          }
        }
        this.partsDone.set(row.id, this._partsDone(row) + 1);
      } catch (e) {
        // A timeout or a lost connection: recorded, never retried behind the
        // user's back.
        const name = (e as Error).message.match(/timed out/i) ? "TIMEOUT" : "CONNECTION_LOST";
        this.errors[name] = (this.errors[name] ?? 0) + 1;
        this._submitted++;
        if (row.commandStatus === null) row.commandStatus = -1;
        break;
      }
    }

    if (row.commandStatus === CommandStatus.ESME_ROK) this._accepted++;
    this.emit("row", row);
  }

  private _partsDone(row: BurstRow): number {
    return this.partsDone.get(row.id) ?? 0;
  }

  private onInbound(pdu: Pdu): void {
    const receipt = parseReceipt(pdu);
    if (!receipt) return;

    const token = this.correlator.match(receipt);
    if (token === null) return;

    const row = this.rowById.get(token);
    if (!row) return;
    row.dlrState = receipt.state;
    row.dlrLatencyMs = Math.max(0, Date.now() - this.startedAt - (row.submitLatencyMs ?? 0));
    this._dlrReceived++;
    this.emit("row", row);
  }

  private async awaitDlrs(ms: number): Promise<void> {
    if (ms <= 0) return;
    const deadline = Date.now() + ms;
    while (Date.now() < deadline) {
      if (this._dlrReceived >= this._messages) return;
      if (this.lostConnection) return;
      await new Promise((r) => setTimeout(r, 10));
    }
  }
}
