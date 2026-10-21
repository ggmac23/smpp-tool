/** Byte-building helpers for the codec tests. Deliberately independent of src/smpp/encode. */

export function cstr(s: string): Buffer {
  return Buffer.concat([Buffer.from(s, "latin1"), Buffer.from([0])]);
}

export function u8(...n: number[]): Buffer {
  return Buffer.from(n);
}

export function u32be(n: number): Buffer {
  const b = Buffer.alloc(4);
  b.writeUInt32BE(n >>> 0, 0);
  return b;
}

/** A complete PDU with a correct command_length. */
export function pdu(commandId: number, status: number, seq: number, body: Buffer = Buffer.alloc(0)): Buffer {
  return Buffer.concat([u32be(16 + body.length), u32be(commandId), u32be(status), u32be(seq), body]);
}

/** A PDU whose command_length is whatever you say, correct or not. */
export function pduWithLength(
  length: number,
  commandId: number,
  status: number,
  seq: number,
  body: Buffer = Buffer.alloc(0)
): Buffer {
  return Buffer.concat([u32be(length), u32be(commandId), u32be(status), u32be(seq), body]);
}

/** A minimal, valid submit_sm body carrying `text` in short_message. */
export function submitSmBody(
  text = "hello",
  opts: { dataCoding?: number; esmClass?: number; source?: string; dest?: string } = {}
): Buffer {
  const msg = Buffer.from(text, "latin1");
  return Buffer.concat([
    cstr(""), // service_type
    u8(1, 1), // source_addr_ton, source_addr_npi
    cstr(opts.source ?? "393331234567"),
    u8(1, 1), // dest_addr_ton, dest_addr_npi
    cstr(opts.dest ?? "393337654321"),
    u8(opts.esmClass ?? 0), // esm_class
    u8(0), // protocol_id
    u8(0), // priority_flag
    cstr(""), // schedule_delivery_time
    cstr(""), // validity_period
    u8(1), // registered_delivery
    u8(0), // replace_if_present_flag
    u8(opts.dataCoding ?? 0), // data_coding
    u8(0), // sm_default_msg_id
    u8(msg.length), // sm_length
    msg,
  ]);
}
