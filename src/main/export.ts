import { TraceEntry, formatTraceLine } from "./trace";

function cell(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (Buffer.isBuffer(value)) return value.toString("hex");
  return String(value);
}

function quote(s: string): string {
  return /[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

/** RFC 4180 CSV: CRLF rows, quotes doubled, fields with a separator quoted. */
export function toCsv(rows: Record<string, unknown>[], columns: string[]): string {
  const lines = [columns.map(quote).join(",")];
  for (const row of rows) {
    lines.push(columns.map((c) => quote(cell(row[c]))).join(","));
  }
  return lines.join("\r\n");
}

export function traceToText(entries: TraceEntry[]): string {
  return entries.map(formatTraceLine).join("\n");
}
