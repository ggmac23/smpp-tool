import { commandName } from "./constants";
import { PDU_FIELDS, FieldDef } from "./pdu-defs";
import { Tlv, decodeTlvs } from "./tlv";

export class PduDecodeError extends Error {
  readonly raw: Buffer;
  constructor(message: string, raw: Buffer) {
    super(message);
    this.name = "PduDecodeError";
    this.raw = raw;
  }
}

/** One entry of submit_multi's dest_address list. */
export interface DestEntry {
  flag: number;
  ton?: number;
  npi?: number;
  addr?: string;
  dlName?: string;
}

/** One entry of submit_multi_resp's unsuccess_sme list. */
export interface UnsuccessEntry {
  ton: number;
  npi: number;
  addr: string;
  errorStatus: number;
}

export type FieldValue = number | string | Buffer | DestEntry[] | UnsuccessEntry[];

export interface Pdu {
  commandId: number;
  commandStatus: number;
  sequenceNumber: number;
  fields: Record<string, FieldValue>;
  tlvs: Tlv[];
  /** The exact bytes this PDU was decoded from. */
  raw: Buffer;
}

export const PDU_HEADER_LENGTH = 16;

class Reader {
  offset = 0;
  constructor(
    private readonly body: Buffer,
    private readonly raw: Buffer
  ) {}

  private fail(message: string): never {
    throw new PduDecodeError(message, this.raw);
  }

  get remaining(): number {
    return this.body.length - this.offset;
  }

  u8(name: string): number {
    if (this.remaining < 1) this.fail(`${name}: ran out of bytes reading a u8`);
    return this.body.readUInt8(this.offset++);
  }

  u16(name: string): number {
    if (this.remaining < 2) this.fail(`${name}: ran out of bytes reading a u16`);
    const v = this.body.readUInt16BE(this.offset);
    this.offset += 2;
    return v;
  }

  u32(name: string): number {
    if (this.remaining < 4) this.fail(`${name}: ran out of bytes reading a u32`);
    const v = this.body.readUInt32BE(this.offset);
    this.offset += 4;
    return v;
  }

  cstring(name: string): string {
    const end = this.body.indexOf(0, this.offset);
    if (end < 0) this.fail(`${name}: unterminated C-string at offset ${this.offset}`);
    const s = this.body.toString("latin1", this.offset, end);
    this.offset = end + 1;
    return s;
  }

  octets(name: string, length: number): Buffer {
    if (length > this.remaining) {
      this.fail(`${name}: declares ${length} byte(s) but only ${this.remaining} remain`);
    }
    const v = Buffer.from(this.body.subarray(this.offset, this.offset + length));
    this.offset += length;
    return v;
  }

  tail(): Buffer {
    return this.body.subarray(this.offset);
  }
}

function readDestList(r: Reader, count: number): DestEntry[] {
  const out: DestEntry[] = [];
  for (let i = 0; i < count; i++) {
    const flag = r.u8(`dest_address[${i}].dest_flag`);
    if (flag === 2) {
      out.push({ flag, dlName: r.cstring(`dest_address[${i}].dl_name`) });
    } else {
      out.push({
        flag,
        ton: r.u8(`dest_address[${i}].dest_addr_ton`),
        npi: r.u8(`dest_address[${i}].dest_addr_npi`),
        addr: r.cstring(`dest_address[${i}].destination_addr`),
      });
    }
  }
  return out;
}

function readUnsuccessList(r: Reader, count: number): UnsuccessEntry[] {
  const out: UnsuccessEntry[] = [];
  for (let i = 0; i < count; i++) {
    out.push({
      ton: r.u8(`unsuccess_sme[${i}].dest_addr_ton`),
      npi: r.u8(`unsuccess_sme[${i}].dest_addr_npi`),
      addr: r.cstring(`unsuccess_sme[${i}].destination_addr`),
      errorStatus: r.u32(`unsuccess_sme[${i}].error_status`),
    });
  }
  return out;
}

function readField(r: Reader, def: FieldDef, fields: Record<string, FieldValue>): FieldValue {
  switch (def.type) {
    case "u8":
      return r.u8(def.name);
    case "u16":
      return r.u16(def.name);
    case "u32":
      return r.u32(def.name);
    case "cstring":
      return r.cstring(def.name);
    case "octets": {
      const n = def.lenFrom ? Number(fields[def.lenFrom] ?? 0) : r.remaining;
      return r.octets(def.name, n);
    }
    case "dest_list":
      return readDestList(r, def.lenFrom ? Number(fields[def.lenFrom] ?? 0) : 0);
    case "unsuccess_list":
      return readUnsuccessList(r, def.lenFrom ? Number(fields[def.lenFrom] ?? 0) : 0);
  }
}

/**
 * Decode one complete PDU. `buf` must be exactly the PDU: its command_length
 * must match its own size, because that is what the framer hands over. A
 * command_length that lies is a decode error, never something to repair.
 */
export function decodePdu(buf: Buffer): Pdu {
  if (buf.length < PDU_HEADER_LENGTH) {
    throw new PduDecodeError(
      `PDU shorter than the ${PDU_HEADER_LENGTH}-byte header: ${buf.length} byte(s)`,
      buf
    );
  }
  const commandLength = buf.readUInt32BE(0);
  if (commandLength !== buf.length) {
    throw new PduDecodeError(
      `command_length ${commandLength} disagrees with the ${buf.length} byte(s) present`,
      buf
    );
  }

  const commandId = buf.readUInt32BE(4);
  const commandStatus = buf.readUInt32BE(8);
  const sequenceNumber = buf.readUInt32BE(12);
  const body = buf.subarray(PDU_HEADER_LENGTH);
  const name = commandName(commandId);
  const defs = PDU_FIELDS[name];

  const fields: Record<string, FieldValue> = {};

  if (!defs) {
    // An id the protocol does not define: keep the body intact and parse no TLVs.
    return {
      commandId,
      commandStatus,
      sequenceNumber,
      fields: { body: Buffer.from(body) },
      tlvs: [],
      raw: buf,
    };
  }

  const r = new Reader(body, buf);
  for (const def of defs) {
    fields[def.name] = readField(r, def, fields);
  }

  let tlvs: Tlv[] = [];
  const tail = r.tail();
  if (tail.length > 0) {
    try {
      tlvs = decodeTlvs(tail);
    } catch (e) {
      throw new PduDecodeError(`${name}: ${(e as Error).message}`, buf);
    }
  }

  return { commandId, commandStatus, sequenceNumber, fields, tlvs, raw: buf };
}
