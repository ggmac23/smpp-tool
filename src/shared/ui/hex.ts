import { decodePdu } from "../../smpp/decode";
import { summarise, describe as describePdu, DescribedField } from "../../smpp/summary";

/**
 * Parse hand-typed hex into bytes.
 *
 * Tolerant of whatever a person pastes — spaces, newlines, tabs, `0x`
 * prefixes — and strict about what it cannot read, so a typo is reported
 * rather than silently dropped.
 */
export function parseHex(text: string): Buffer {
  const cleaned = (text || "").replace(/0x/gi, "").replace(/[\s,]+/g, "");
  if (cleaned.length === 0) return Buffer.alloc(0);

  const bad = /[^0-9a-fA-F]/.exec(cleaned);
  if (bad) {
    throw new Error(`"${bad[0]}" is not a hex digit (at position ${bad.index + 1})`);
  }
  if (cleaned.length % 2 !== 0) {
    throw new Error(`an odd number of hex digits (${cleaned.length}): every byte needs two`);
  }
  return Buffer.from(cleaned, "hex");
}

export function formatHex(bytes: Buffer | string): string {
  const buf = typeof bytes === "string" ? parseHex(bytes) : bytes;
  return Array.from(buf)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join(" ");
}

export interface PduPreview {
  /** Whether the bytes decode as a PDU. */
  ok: boolean;
  /** Why they do not, when they do not. */
  error?: string;
  summary: string;
  fields: DescribedField[];
  byteCount: number;
  /**
   * Whether there is anything to send. Deliberately independent of `ok`: a
   * PDU that will not decode is exactly what a tester often wants to send,
   * so the preview is informational and never a gate.
   */
  sendable: boolean;
}

export function previewPdu(bytes: Buffer | string): PduPreview {
  const buf = typeof bytes === "string" ? parseHex(bytes) : bytes;
  const sendable = buf.length > 0;

  try {
    const pdu = decodePdu(buf);
    return {
      ok: true,
      summary: summarise(pdu),
      fields: describePdu(pdu),
      byteCount: buf.length,
      sendable,
    };
  } catch (e) {
    return {
      ok: false,
      error: (e as Error).message,
      summary: "",
      fields: [],
      byteCount: buf.length,
      sendable,
    };
  }
}
