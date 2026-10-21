import { DataCoding } from "./constants";

export type SegmentationMode = "none" | "udh" | "sar";

/**
 * How GSM 03.38 text is laid out in the octets.
 *
 * Over SMPP, `data_coding 0` is conventionally sent one septet per octet,
 * because sm_length counts octets — that is what SMSCs expect, so it is the
 * default. Packed 7-bit is the GSM air-interface layout; a tester still needs
 * to be able to send it, so it is offered, never assumed.
 */
export type Gsm7Packing = "unpacked" | "packed";

const ESC = 0x1b;

// GSM 03.38 default alphabet, index = septet value.
const GSM_BASIC =
  "@£$¥èéùìòÇ\nØø\rÅå" +
  "Δ_ΦΓΛΩΠΨΣΘΞ\u001bÆæßÉ" +
  " !\"#¤%&'()*+,-./" +
  "0123456789:;<=>?" +
  "¡ABCDEFGHIJKLMNO" +
  "PQRSTUVWXYZÄÖÑÜ§" +
  "¿abcdefghijklmno" +
  "pqrstuvwxyzäöñüà";

// GSM 03.38 extension table: ESC followed by these septet values.
const GSM_EXT: Record<string, number> = {
  "\f": 0x0a,
  "^": 0x14,
  "{": 0x28,
  "}": 0x29,
  "\\": 0x2f,
  "[": 0x3c,
  "~": 0x3d,
  "]": 0x3e,
  "|": 0x40,
  "€": 0x65,
};

const GSM_EXT_BY_SEPTET = new Map<number, string>(
  Object.entries(GSM_EXT).map(([ch, v]) => [v, ch])
);

const GSM_BY_CHAR = new Map<string, number>();
for (let i = 0; i < GSM_BASIC.length; i++) {
  if (i === ESC) continue; // the escape marker is not a character
  GSM_BY_CHAR.set(GSM_BASIC[i], i);
}

/** The septet sequence for `text`, with `onUnmappable` called for each character that has none. */
function toSeptets(text: string, onUnmappable?: (ch: string) => void): number[] {
  const out: number[] = [];
  for (const ch of text) {
    const basic = GSM_BY_CHAR.get(ch);
    if (basic !== undefined) {
      out.push(basic);
      continue;
    }
    const ext = GSM_EXT[ch];
    if (ext !== undefined) {
      out.push(ESC, ext);
      continue;
    }
    onUnmappable?.(ch);
    out.push(GSM_BY_CHAR.get("?")!);
  }
  return out;
}

/** Septets `text` occupies in GSM 03.38, counting an escape-table character as two. */
export function septetsOf(text: string): number {
  return toSeptets(text).length;
}

/** The septet sequence for `text`, escape pairs included. */
export function septetsFor(text: string): number[] {
  return toSeptets(text);
}

export const GSM_ESCAPE = ESC;

function packSeptets(septets: number[]): Buffer {
  const out: number[] = [];
  let acc = 0;
  let bits = 0;
  for (const s of septets) {
    acc |= (s & 0x7f) << bits;
    bits += 7;
    while (bits >= 8) {
      out.push(acc & 0xff);
      acc >>>= 8;
      bits -= 8;
    }
  }
  if (bits > 0) out.push(acc & 0xff);
  return Buffer.from(out);
}

function unpackSeptets(buf: Buffer): number[] {
  const out: number[] = [];
  let acc = 0;
  let bits = 0;
  for (const byte of buf) {
    acc |= byte << bits;
    bits += 8;
    while (bits >= 7) {
      out.push(acc & 0x7f);
      acc >>>= 7;
      bits -= 7;
    }
  }
  // A trailing all-zero septet is padding, not an "@".
  if (out.length > 0 && bits === 0 && out[out.length - 1] === 0) out.pop();
  return out;
}

function septetsToText(septets: number[]): string {
  let s = "";
  for (let i = 0; i < septets.length; i++) {
    if (septets[i] === ESC) {
      const ch = GSM_EXT_BY_SEPTET.get(septets[i + 1]);
      if (ch !== undefined) {
        s += ch;
        i++;
        continue;
      }
      continue; // a lone escape carries no character
    }
    s += GSM_BASIC[septets[i]] ?? "";
  }
  return s;
}

/**
 * Encode/decode tables for the single- and multi-byte codings, built by asking
 * the platform's decoder what each byte sequence means. Built on first use.
 */
const singleByteCache = new Map<string, Map<string, number>>();
const multiByteCache = new Map<string, Map<string, Buffer>>();

function singleByteTable(label: string): Map<string, number> {
  let t = singleByteCache.get(label);
  if (t) return t;
  const dec = new TextDecoder(label);
  t = new Map<string, number>();
  for (let b = 0; b < 256; b++) {
    const ch = dec.decode(Uint8Array.from([b]));
    if (ch.length === 1 && ch !== "�" && !t.has(ch)) t.set(ch, b);
  }
  singleByteCache.set(label, t);
  return t;
}

function multiByteTable(label: string): Map<string, Buffer> {
  let t = multiByteCache.get(label);
  if (t) return t;
  const dec = new TextDecoder(label);
  t = new Map<string, Buffer>();
  for (let b = 0; b < 256; b++) {
    const ch = dec.decode(Uint8Array.from([b]));
    if (ch.length === 1 && ch !== "�" && !t.has(ch)) t.set(ch, Buffer.from([b]));
  }
  for (let hi = 0x81; hi <= 0xfc; hi++) {
    for (let lo = 0x40; lo <= 0xfc; lo++) {
      const ch = dec.decode(Uint8Array.from([hi, lo]));
      if (ch.length >= 1 && !ch.includes("�") && !t.has(ch)) {
        t.set(ch, Buffer.from([hi, lo]));
      }
    }
  }
  multiByteCache.set(label, t);
  return t;
}

const LABELS: Partial<Record<number, string>> = {
  [DataCoding.IA5_ASCII]: "iso-8859-1",
  [DataCoding.LATIN_1]: "iso-8859-1",
  [DataCoding.CYRILLIC]: "iso-8859-5",
  [DataCoding.LATIN_HEBREW]: "iso-8859-8",
};

function isOctet(dataCoding: number): boolean {
  return (
    dataCoding === DataCoding.OCTET_UNSPECIFIED_2 || dataCoding === DataCoding.OCTET_UNSPECIFIED_4
  );
}

export interface EncodedText {
  bytes: Buffer;
  /** Each distinct character the coding cannot represent, in first-seen order. */
  unmappable: string[];
}

export function encodeText(
  text: string,
  dataCoding: number,
  packing: Gsm7Packing = "unpacked"
): EncodedText {
  const seen = new Set<string>();
  const unmappable: string[] = [];
  const note = (ch: string) => {
    if (!seen.has(ch)) {
      seen.add(ch);
      unmappable.push(ch);
    }
  };

  if (dataCoding === DataCoding.GSM_DEFAULT) {
    const septets = toSeptets(text, note);
    const bytes = packing === "packed" ? packSeptets(septets) : Buffer.from(septets);
    return { bytes, unmappable };
  }

  if (dataCoding === DataCoding.UCS2) {
    return { bytes: Buffer.from(text, "utf16le").swap16(), unmappable };
  }

  if (isOctet(dataCoding)) {
    const bytes = Buffer.alloc(Array.from(text).length);
    let i = 0;
    for (const ch of text) {
      const cp = ch.codePointAt(0)!;
      if (cp > 0xff) note(ch);
      bytes[i++] = cp & 0xff;
    }
    return { bytes: bytes.subarray(0, i), unmappable };
  }

  if (dataCoding === DataCoding.JIS) {
    const table = multiByteTable("shift_jis");
    const parts: Buffer[] = [];
    for (const ch of text) {
      const b = table.get(ch);
      if (b) parts.push(b);
      else {
        note(ch);
        parts.push(Buffer.from("?", "ascii"));
      }
    }
    return { bytes: Buffer.concat(parts), unmappable };
  }

  const label = LABELS[dataCoding];
  if (label) {
    const table = singleByteTable(label);
    const bytes = Buffer.alloc(Array.from(text).length);
    let i = 0;
    for (const ch of text) {
      const b = table.get(ch);
      if (b === undefined) {
        note(ch);
        bytes[i++] = 0x3f; // '?'
      } else {
        bytes[i++] = b;
      }
    }
    return { bytes: bytes.subarray(0, i), unmappable };
  }

  // A data_coding the tool has no table for: treat the text as octets and say so.
  return encodeText(text, DataCoding.OCTET_UNSPECIFIED_4, packing);
}

export function decodeText(
  bytes: Buffer,
  dataCoding: number,
  packing: Gsm7Packing = "unpacked"
): string {
  if (dataCoding === DataCoding.GSM_DEFAULT) {
    const septets = packing === "packed" ? unpackSeptets(bytes) : Array.from(bytes);
    return septetsToText(septets);
  }
  if (dataCoding === DataCoding.UCS2) {
    return Buffer.from(bytes).swap16().toString("utf16le");
  }
  if (isOctet(dataCoding)) return bytes.toString("latin1");
  if (dataCoding === DataCoding.JIS) return new TextDecoder("shift_jis").decode(bytes);
  const label = LABELS[dataCoding];
  if (label) return new TextDecoder(label).decode(bytes);
  return bytes.toString("latin1");
}

/* ---------------------------------------------------------------------------
 * Splitting. Exported because segmentation (segment.ts) and the live counter
 * must agree on where the boundaries fall.
 * ------------------------------------------------------------------------ */

export const GSM7_SINGLE = 160;
export const GSM7_CONCAT = 153;
export const UCS2_SINGLE_UNITS = 70;
export const UCS2_CONCAT_UNITS = 67;
export const OCTET_SINGLE = 140;
export const OCTET_CONCAT = 134;

/** Split septets into runs of at most `limit`, never between an ESC and its pair. */
export function splitSeptets(septets: number[], limit: number): number[][] {
  const out: number[][] = [];
  let i = 0;
  while (i < septets.length) {
    let end = Math.min(i + limit, septets.length);
    // An ESC as the last septet of a run would be orphaned: push it to the next run.
    if (end < septets.length && septets[end - 1] === ESC) end -= 1;
    out.push(septets.slice(i, end));
    i = end;
  }
  return out;
}

/** Split text into chunks of at most `limitUnits` UTF-16 code units, never inside a surrogate pair. */
export function splitByCodeUnits(text: string, limitUnits: number): string[] {
  const out: string[] = [];
  let chunk = "";
  let units = 0;
  for (const ch of text) {
    if (units + ch.length > limitUnits) {
      out.push(chunk);
      chunk = "";
      units = 0;
    }
    chunk += ch;
    units += ch.length;
  }
  if (chunk.length > 0 || out.length === 0) out.push(chunk);
  return out;
}

export interface Measurement {
  /** Characters the user typed, counting an astral character as one. */
  chars: number;
  /** Bits the encoded body occupies. */
  bits: number;
  /** Octets the encoded body occupies. */
  bytes: number;
  /** PDUs this body will take under the chosen segmentation mode. */
  parts: number;
  unmappable: string[];
}

export function measure(
  text: string,
  dataCoding: number,
  mode: SegmentationMode,
  packing: Gsm7Packing = "unpacked"
): Measurement {
  const { bytes, unmappable } = encodeText(text, dataCoding, packing);
  const chars = Array.from(text).length;

  let parts = 1;
  let bits: number;

  if (dataCoding === DataCoding.GSM_DEFAULT) {
    const septets = toSeptets(text);
    bits = septets.length * 7;
    if (mode !== "none") {
      parts =
        septets.length <= GSM7_SINGLE
          ? 1
          : splitSeptets(septets, GSM7_CONCAT).length;
    }
  } else if (dataCoding === DataCoding.UCS2) {
    bits = bytes.length * 8;
    const units = bytes.length / 2;
    if (mode !== "none") {
      parts =
        units <= UCS2_SINGLE_UNITS ? 1 : splitByCodeUnits(text, UCS2_CONCAT_UNITS).length;
    }
  } else {
    bits = bytes.length * 8;
    if (mode !== "none") {
      parts =
        bytes.length <= OCTET_SINGLE ? 1 : Math.ceil(bytes.length / OCTET_CONCAT);
    }
  }

  return { chars, bits, bytes: bytes.length, parts, unmappable };
}

/** The reference tool's sample messages. `gsm1` exercises the escape table on purpose. */
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
