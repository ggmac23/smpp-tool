import { DataCoding, ESM_UDHI_BIT } from "./constants";
import { Tlv } from "./tlv";
import { FieldValue } from "./decode";
import {
  SegmentationMode,
  Gsm7Packing,
  encodeText,
  septetsFor,
  splitSeptets,
  splitByCodeUnits,
  GSM7_SINGLE,
  GSM7_CONCAT,
  UCS2_SINGLE_UNITS,
  UCS2_CONCAT_UNITS,
  OCTET_SINGLE,
  OCTET_CONCAT,
} from "./text";

export type BodyPlacement = "short_message" | "message_payload";

export const TLV_MESSAGE_PAYLOAD = 0x0424;
export const TLV_SAR_MSG_REF_NUM = 0x020c;
export const TLV_SAR_TOTAL_SEGMENTS = 0x020e;
export const TLV_SAR_SEGMENT_SEQNUM = 0x020f;

export interface Segment {
  /** The fields this segment owns; merge over the form's template. */
  fields: Record<string, FieldValue>;
  tlvs: Tlv[];
  /** Characters the chosen data_coding could not represent. */
  unmappable: string[];
}

export interface SegmentOptions {
  text: string;
  dataCoding: number;
  mode: SegmentationMode;
  placement: BodyPlacement;
  /** A UDH the user composed. Its presence turns automatic concatenation off. */
  userUdh?: Buffer;
  /** Concatenation reference. Random when omitted: 8-bit for udh, 16-bit for sar. */
  refNum?: number;
  /** The messaging mode the form selected; UDHI is OR-ed into it. */
  esmClass?: number;
  packing?: Gsm7Packing;
}

function concatUdh(refNum: number, total: number, seq: number): Buffer {
  // UDH length 5, IE 0x00 (concatenated short message, 8-bit reference), IE length 3.
  return Buffer.from([0x05, 0x00, 0x03, refNum & 0xff, total & 0xff, seq & 0xff]);
}

/** Split `text` into the per-segment chunks its coding calls for. */
function chunk(text: string, dataCoding: number, packing: Gsm7Packing): string[] {
  if (dataCoding === DataCoding.GSM_DEFAULT) {
    // Split on septets so an escape pair is never cut in half, then map the
    // runs back to text. 153 septets is 134 packed octets or 153 unpacked ones.
    const septets = septetsFor(text);
    // A body that fits one segment is one segment: the concatenation limit
    // only applies once the message actually has to be split. The live
    // counter decides the same way, and the two must agree.
    if (septets.length <= GSM7_SINGLE) return [text];

    const runs = splitSeptets(septets, GSM7_CONCAT);
    const chars = Array.from(text);
    const chunks: string[] = [];
    let charIndex = 0;
    for (const run of runs) {
      let used = 0;
      let s = "";
      while (charIndex < chars.length) {
        const cost = septetsFor(chars[charIndex]).length;
        if (used + cost > run.length) break;
        s += chars[charIndex];
        used += cost;
        charIndex++;
      }
      chunks.push(s);
    }
    return chunks;
  }

  if (dataCoding === DataCoding.UCS2) {
    const units = text.length;
    return units <= UCS2_SINGLE_UNITS ? [text] : splitByCodeUnits(text, UCS2_CONCAT_UNITS);
  }

  const bytes = encodeText(text, dataCoding, packing).bytes.length;
  if (bytes <= OCTET_SINGLE) return [text];
  const chars = Array.from(text);
  const out: string[] = [];
  for (let i = 0; i < chars.length; i += OCTET_CONCAT) {
    out.push(chars.slice(i, i + OCTET_CONCAT).join(""));
  }
  return out;
}

function u16(n: number): Buffer {
  const b = Buffer.alloc(2);
  b.writeUInt16BE(n & 0xffff, 0);
  return b;
}

/**
 * Turn a message body into the segments that carry it.
 *
 * `none` returns exactly one segment however long the text is — the user asked
 * for one PDU and gets one PDU. A `userUdh` wins over automatic concatenation:
 * the user's bytes go on the wire and the app adds no information element of
 * its own.
 */
export function segment(opts: SegmentOptions): Segment[] {
  const packing: Gsm7Packing = opts.packing ?? "unpacked";
  const baseEsm = opts.esmClass ?? 0;

  const build = (
    text: string,
    udh: Buffer | undefined,
    tlvs: Tlv[],
    setUdhi: boolean
  ): Segment => {
    const { bytes, unmappable } = encodeText(text, opts.dataCoding, packing);
    const body = udh ? Buffer.concat([udh, bytes]) : bytes;
    const fields: Record<string, FieldValue> = {
      esm_class: setUdhi ? baseEsm | ESM_UDHI_BIT : baseEsm,
    };
    const all = [...tlvs];
    if (opts.placement === "message_payload") {
      all.push({ tag: TLV_MESSAGE_PAYLOAD, value: body });
    } else {
      fields.short_message = body;
    }
    return { fields, tlvs: all, unmappable };
  };

  // The user's own UDH turns automatic concatenation off: one segment, their bytes.
  if (opts.userUdh && opts.userUdh.length > 0) {
    return [build(opts.text, opts.userUdh, [], true)];
  }

  if (opts.mode === "none") {
    return [build(opts.text, undefined, [], false)];
  }

  const chunks = chunk(opts.text, opts.dataCoding, packing);

  if (chunks.length === 1) {
    return [build(chunks[0], undefined, [], false)];
  }

  if (opts.mode === "sar") {
    const ref = opts.refNum ?? 1 + Math.floor(Math.random() * 0xfffe);
    return chunks.map((c, i) =>
      build(
        c,
        undefined,
        [
          { tag: TLV_SAR_MSG_REF_NUM, value: u16(ref) },
          { tag: TLV_SAR_TOTAL_SEGMENTS, value: Buffer.from([chunks.length]) },
          { tag: TLV_SAR_SEGMENT_SEQNUM, value: Buffer.from([i + 1]) },
        ],
        false
      )
    );
  }

  const ref = opts.refNum ?? 1 + Math.floor(Math.random() * 0xfe);
  return chunks.map((c, i) => build(c, concatUdh(ref, chunks.length, i + 1), [], true));
}
