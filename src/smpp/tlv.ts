import { KnownTlv } from "./constants";

export interface Tlv {
  tag: number;
  value: Buffer;
}

export function encodeTlvs(tlvs: Tlv[]): Buffer {
  const parts: Buffer[] = [];
  for (const { tag, value } of tlvs) {
    const head = Buffer.alloc(4);
    head.writeUInt16BE(tag & 0xffff, 0);
    head.writeUInt16BE(value.length & 0xffff, 2);
    parts.push(head, value);
  }
  return Buffer.concat(parts);
}

export function decodeTlvs(buf: Buffer): Tlv[] {
  const out: Tlv[] = [];
  let i = 0;
  while (i < buf.length) {
    if (i + 4 > buf.length) {
      throw new Error(`truncated TLV header at offset ${i}: ${buf.length - i} byte(s) left`);
    }
    const tag = buf.readUInt16BE(i);
    const len = buf.readUInt16BE(i + 2);
    const start = i + 4;
    if (start + len > buf.length) {
      throw new Error(
        `TLV ${tlvName(tag)} declares ${len} byte(s) but only ${buf.length - start} remain`
      );
    }
    out.push({ tag, value: buf.subarray(start, start + len) });
    i = start + len;
  }
  return out;
}

function hex4(n: number): string {
  return "0x" + (n & 0xffff).toString(16).padStart(4, "0");
}

export function tlvName(tag: number): string {
  return KnownTlv[tag]?.name ?? `unknown(${hex4(tag)})`;
}
