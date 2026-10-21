import * as fs from "node:fs";

export interface TraceEntry {
  /** Monotonic row ordinal, stable across eviction. */
  seq: number;
  at: number;
  direction: "in" | "out";
  commandName: string;
  commandStatus: number;
  /** The PDU's own sequence number, not the row ordinal. */
  sequenceNumber: number;
  /** The decoded one-liner shown in the table. Empty when the PDU would not parse. */
  summary: string;
  raw: Buffer;
  /** Set instead of a summary when the PDU would not decode. */
  decodeError?: string;
}

export interface Counter {
  sent: number;
  received: number;
}

export const DEFAULT_TRACE_CAP = 10_000;

/**
 * The PDU trace: a bounded ring of entries plus unbounded per-command counters.
 *
 * When the cap is reached the oldest entries go and `dropped` counts them, so
 * the UI can say so rather than quietly showing a partial history. Counters are
 * never reduced by eviction — they answer "how much traffic was there", which
 * is not the same question as "what is still on screen".
 */
export class TraceStore {
  private items: TraceEntry[] = [];
  private _dropped = 0;
  private _counters: Record<string, Counter> = {};

  constructor(private cap: number = DEFAULT_TRACE_CAP) {}

  add(e: TraceEntry): void {
    const c = (this._counters[e.commandName] ??= { sent: 0, received: 0 });
    if (e.direction === "out") c.sent++;
    else c.received++;

    this.items.push(e);
    this.trim();
  }

  /** Entries from row ordinal `from` onwards, at most `limit` of them. */
  entries(from?: number, limit?: number): TraceEntry[] {
    let out = this.items;
    if (from !== undefined) out = out.filter((e) => e.seq >= from);
    if (limit !== undefined) out = out.slice(0, limit);
    return out;
  }

  get size(): number {
    return this.items.length;
  }

  /** Entries discarded to stay inside the cap. */
  get dropped(): number {
    return this._dropped;
  }

  get counters(): Record<string, Counter> {
    return this._counters;
  }

  setCap(n: number): void {
    this.cap = Math.max(1, n);
    this.trim();
  }

  clear(): void {
    this.items = [];
    this._dropped = 0;
    this._counters = {};
  }

  private trim(): void {
    if (this.items.length <= this.cap) return;
    const excess = this.items.length - this.cap;
    this.items.splice(0, excess);
    this._dropped += excess;
  }
}

function stamp(at: number): string {
  return new Date(at).toISOString();
}

export function formatTraceLine(e: TraceEntry): string {
  const arrow = e.direction === "out" ? "out >>" : "in  <<";
  const what = e.decodeError ? `DECODE ERROR: ${e.decodeError}` : e.summary;
  return `${stamp(e.at)} ${arrow} seq=${e.sequenceNumber} ${e.commandName} status=${e.commandStatus} ${what} [${e.raw.toString("hex")}]`;
}

/** Appends the trace to a file, for sessions longer than the in-memory cap. */
export class TraceFileSink {
  private fd: number;

  constructor(readonly path: string) {
    this.fd = fs.openSync(path, "a");
  }

  write(e: TraceEntry): void {
    fs.writeSync(this.fd, formatTraceLine(e) + "\n");
  }

  close(): void {
    if (this.fd >= 0) {
      fs.closeSync(this.fd);
      this.fd = -1;
    }
  }
}
