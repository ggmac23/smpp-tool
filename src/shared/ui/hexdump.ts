import type { WirePdu } from "../api";

const BYTES_PER_ROW = 16;

export interface HexDumpRow {
  offset: string;
  /** `BYTES_PER_ROW` hex pairs, space separated and padded so columns line up. */
  hex: string;
  ascii: string;
}

function cleanHex(hex: string): string {
  return (hex || "").replace(/[^0-9a-fA-F]/g, "");
}

function printable(byte: number): string {
  return byte >= 0x20 && byte <= 0x7e ? String.fromCharCode(byte) : ".";
}

export function hexDumpRows(hex: string): HexDumpRow[] {
  const clean = cleanHex(hex);
  const bytes: number[] = [];
  for (let i = 0; i < clean.length; i += 2) {
    bytes.push(parseInt(clean.slice(i, i + 2).padEnd(2, "0"), 16));
  }
  if (bytes.length === 0) return [];

  const rows: HexDumpRow[] = [];
  for (let start = 0; start < bytes.length; start += BYTES_PER_ROW) {
    const slice = bytes.slice(start, start + BYTES_PER_ROW);
    rows.push({
      offset: start.toString(16).padStart(8, "0"),
      hex: slice
        .map((b) => b.toString(16).padStart(2, "0"))
        .join(" ")
        .padEnd(BYTES_PER_ROW * 3 - 1, " "),
      ascii: slice.map(printable).join(""),
    });
  }
  return rows;
}

/** The whole dump as one block: offset, hex, then ASCII. */
export function hexDump(hex: string): string {
  return hexDumpRows(hex)
    .map((r) => `${r.offset}  ${r.hex}  ${r.ascii}`)
    .join("\n");
}

/** `hh:mm:ss.mmm` in local time — a trace is read against a wall clock. */
export function formatTime(at: number): string {
  const d = new Date(at);
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
}

export function directionArrow(direction: "in" | "out"): string {
  return direction === "out" ? "→" : "←";
}

/** The one-line form for the trace table's PDU column. */
export function traceRowText(row: WirePdu): string {
  const what = row.decodeError ? `decode error: ${row.decodeError}` : row.summary;
  const status = row.commandStatus !== 0 ? ` ${row.statusName}` : "";
  return `${formatTime(row.at)}  ${row.sequenceNumber}  ${directionArrow(row.direction)}  ${what}${status}`;
}

/** Shown above the trace when the cap has started discarding history. */
export function droppedNotice(dropped: number): string {
  if (dropped <= 0) return "";
  return `${dropped} earlier entr${dropped === 1 ? "y has" : "ies have"} been discarded to stay inside the trace cap.`;
}

export const TRACE_COLUMNS = ["Time", "SeqNo", "Dir", "SMPP PDU", "Status"];

/**
 * Shown when the framer is holding bytes for a PDU that has not finished
 * arriving. An SMSC that declares more than it sends otherwise stalls the
 * trace silently, with no row and no error.
 */
export function pendingNotice(pending: { buffered: number; declared: number | null }): string {
  if (!pending || pending.buffered <= 0) return "";
  if (pending.declared === null) {
    return `${pending.buffered} byte(s) held: not yet a complete PDU header.`;
  }
  return `${pending.buffered} of ${pending.declared} byte(s) held, waiting for the rest of a PDU.`;
}
