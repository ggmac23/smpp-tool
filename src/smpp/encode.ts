import { commandName } from "./constants";
import { PDU_FIELDS, FieldDef } from "./pdu-defs";
import { Tlv, encodeTlvs } from "./tlv";
import { DestEntry, UnsuccessEntry, FieldValue, PDU_HEADER_LENGTH } from "./decode";

export interface PduInput {
  commandId: number;
  commandStatus?: number;
  sequenceNumber: number;
  fields?: Record<string, FieldValue | undefined>;
  tlvs?: Tlv[];
}

function cstring(v: FieldValue | undefined): Buffer {
  const s = v === undefined || v === null ? "" : String(v);
  return Buffer.concat([Buffer.from(s, "latin1"), Buffer.from([0])]);
}

function uint(v: FieldValue | undefined, bytes: 1 | 2 | 4): Buffer {
  const n = Number(v ?? 0);
  const b = Buffer.alloc(bytes);
  if (bytes === 1) b.writeUInt8(n & 0xff, 0);
  else if (bytes === 2) b.writeUInt16BE(n & 0xffff, 0);
  else b.writeUInt32BE(n >>> 0, 0);
  return b;
}

function octets(v: FieldValue | undefined): Buffer {
  if (v === undefined || v === null) return Buffer.alloc(0);
  if (Buffer.isBuffer(v)) return v;
  if (typeof v === "string") return Buffer.from(v, "latin1");
  return Buffer.alloc(0);
}

function destList(v: FieldValue | undefined): Buffer {
  const entries = (Array.isArray(v) ? v : []) as DestEntry[];
  return Buffer.concat(
    entries.map((e) =>
      e.flag === 2
        ? Buffer.concat([uint(2, 1), cstring(e.dlName ?? "")])
        : Buffer.concat([
            uint(e.flag ?? 1, 1),
            uint(e.ton ?? 0, 1),
            uint(e.npi ?? 0, 1),
            cstring(e.addr ?? ""),
          ])
    )
  );
}

function unsuccessList(v: FieldValue | undefined): Buffer {
  const entries = (Array.isArray(v) ? v : []) as UnsuccessEntry[];
  return Buffer.concat(
    entries.map((e) =>
      Buffer.concat([
        uint(e.ton ?? 0, 1),
        uint(e.npi ?? 0, 1),
        cstring(e.addr ?? ""),
        uint(e.errorStatus ?? 0, 4),
      ])
    )
  );
}

function writeField(def: FieldDef, value: FieldValue | undefined): Buffer {
  switch (def.type) {
    case "cstring":
      return cstring(value);
    case "u8":
      return uint(value, 1);
    case "u16":
      return uint(value, 2);
    case "u32":
      return uint(value, 4);
    case "octets":
      return octets(value);
    case "dest_list":
      return destList(value);
    case "unsuccess_list":
      return unsuccessList(value);
  }
}

/**
 * Encode a PDU from a field model.
 *
 * No validation, no clamping, no truncation: an over-long string, an
 * out-of-range enum value and a length field that contradicts its payload all
 * go on the wire exactly as given. The only field this function computes is
 * `command_length`, and a length field is derived only when the caller left it
 * out — a supplied one always wins.
 */
export function encodePdu(input: PduInput): Buffer {
  const name = commandName(input.commandId);
  const defs = PDU_FIELDS[name];
  const given = input.fields ?? {};

  let body: Buffer;

  if (!defs) {
    // An id the protocol does not define: whatever the caller called the body.
    body = octets(given.body);
  } else {
    const resolved: Record<string, FieldValue | undefined> = { ...given };

    // Derive a count/length field only when the caller omitted it.
    for (const def of defs) {
      if (!def.lenFrom || resolved[def.lenFrom] !== undefined) continue;
      const v = resolved[def.name];
      if (def.type === "octets") resolved[def.lenFrom] = octets(v).length;
      else if (Array.isArray(v)) resolved[def.lenFrom] = v.length;
      else resolved[def.lenFrom] = 0;
    }

    body = Buffer.concat(defs.map((def) => writeField(def, resolved[def.name])));
  }

  const tlvs = input.tlvs && input.tlvs.length > 0 ? encodeTlvs(input.tlvs) : Buffer.alloc(0);
  const header = Buffer.alloc(PDU_HEADER_LENGTH);
  header.writeUInt32BE(PDU_HEADER_LENGTH + body.length + tlvs.length, 0);
  header.writeUInt32BE(input.commandId >>> 0, 4);
  header.writeUInt32BE((input.commandStatus ?? 0) >>> 0, 8);
  header.writeUInt32BE(input.sequenceNumber >>> 0, 12);

  return Buffer.concat([header, body, tlvs]);
}

/**
 * The hex composer's path onto the wire: the bytes go out exactly as composed.
 * Nothing is validated, nothing is rewritten — not even command_length.
 */
export function encodeRaw(bytes: Buffer): Buffer {
  return bytes;
}
