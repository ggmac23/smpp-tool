/**
 * Hand-typed hex, in and out. No Buffer, no codec: the renderer parses what
 * the user typed and hands the backend a hex string, which is the only form
 * bytes take on this side.
 */

/**
 * Parse hand-typed hex into bytes.
 *
 * Tolerant of the separators hex dumps use — spaces, newlines, tabs, commas,
 * colons, dashes, underscores, `0x` prefixes — and strict about anything else,
 * so a typo is reported rather than silently dropped. The backend is the
 * lenient one; here the user gets told.
 */
export function parseHex(text: string): Uint8Array {
  const cleaned = (text || "").replace(/0x/gi, "").replace(/[\s,:_-]+/g, "");
  if (cleaned.length === 0) return new Uint8Array(0);

  const bad = /[^0-9a-fA-F]/.exec(cleaned);
  if (bad) {
    throw new Error(`"${bad[0]}" is not a hex digit (at position ${bad.index + 1})`);
  }
  if (cleaned.length % 2 !== 0) {
    throw new Error(`an odd number of hex digits (${cleaned.length}): every byte needs two`);
  }

  const out = new Uint8Array(cleaned.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(cleaned.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

/** Bytes as space-separated hex pairs. Accepts hex text and re-spaces it. */
export function formatHex(bytes: Uint8Array | string): string {
  const buf = typeof bytes === "string" ? parseHex(bytes) : bytes;
  return Array.from(buf)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join(" ");
}

/** Hex with no separators, which is what the commands take. */
export function compactHex(bytes: Uint8Array | string): string {
  const buf = typeof bytes === "string" ? parseHex(bytes) : bytes;
  return Array.from(buf)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

/**
 * What `preview_pdu` gives back. The decode happens in the backend: a PDU
 * that will not decode is often exactly what a tester wants to send, so the
 * preview informs and never gates.
 */
export interface PduPreview {
  ok: boolean;
  error?: string | null;
  summary: string;
  fields: { name: string; value: string }[];
  byteCount: number;
  sendable: boolean;
}
