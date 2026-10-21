import { describe, it, expect, beforeEach } from "vitest";
import { PduFramer, MAX_PDU_LENGTH } from "../../src/smpp/frame";
import { PduDecodeError } from "../../src/smpp/decode";
import { CommandId } from "../../src/smpp/constants";
import { pdu, pduWithLength, submitSmBody } from "../helpers/bytes";

describe("PduFramer", () => {
  let framer: PduFramer;
  beforeEach(() => {
    framer = new PduFramer();
  });

  it("returns one PDU from one chunk", () => {
    const p = pdu(CommandId.enquire_link, 0, 7);
    expect(framer.push(p)).toEqual([p]);
  });

  it("returns nothing until a PDU is complete", () => {
    const p = pdu(CommandId.submit_sm, 0, 1, submitSmBody());
    expect(framer.push(p.subarray(0, 10))).toEqual([]);
  });

  it("reassembles a PDU split across three chunks", () => {
    const p = pdu(CommandId.submit_sm, 0, 1, submitSmBody());
    expect(framer.push(p.subarray(0, 5))).toEqual([]);
    expect(framer.push(p.subarray(5, 20))).toEqual([]);
    expect(framer.push(p.subarray(20))).toEqual([p]);
  });

  it("returns two PDUs delivered in one chunk", () => {
    const a = pdu(CommandId.enquire_link, 0, 1);
    const b = pdu(CommandId.submit_sm, 0, 2, submitSmBody("second"));
    expect(framer.push(Buffer.concat([a, b]))).toEqual([a, b]);
  });

  it("keeps the tail of a chunk that ends mid-PDU", () => {
    const a = pdu(CommandId.enquire_link, 0, 1);
    const b = pdu(CommandId.submit_sm, 0, 2, submitSmBody("second"));
    const both = Buffer.concat([a, b]);
    expect(framer.push(both.subarray(0, a.length + 9))).toEqual([a]);
    expect(framer.push(both.subarray(a.length + 9))).toEqual([b]);
  });

  it("splits a PDU boundary inside the 16-byte header", () => {
    const a = pdu(CommandId.enquire_link, 0, 1);
    const b = pdu(CommandId.enquire_link, 0, 2);
    const both = Buffer.concat([a, b]);
    expect(framer.push(both.subarray(0, a.length + 3))).toEqual([a]);
    expect(framer.push(both.subarray(a.length + 3))).toEqual([b]);
  });

  it("rejects a command_length below 16", () => {
    expect(() => framer.push(pduWithLength(8, CommandId.enquire_link, 0, 1))).toThrow(
      PduDecodeError
    );
  });

  it("rejects a command_length above MAX_PDU_LENGTH", () => {
    expect(() =>
      framer.push(pduWithLength(MAX_PDU_LENGTH + 1, CommandId.submit_sm, 0, 1))
    ).toThrow(PduDecodeError);
  });

  it("rejects a lying command_length without allocating", () => {
    const buf = Buffer.alloc(32);
    buf.writeUInt32BE(0x7fffffff, 0);
    expect(() => framer.push(buf)).toThrow(PduDecodeError);
  });

  it("drops its buffer on reset and frames the next PDU cleanly", () => {
    const partial = pdu(CommandId.submit_sm, 0, 1, submitSmBody());
    framer.push(partial.subarray(0, 10));
    expect(framer.buffered).toBe(10);

    framer.reset();
    expect(framer.buffered).toBe(0);

    const next = pdu(CommandId.enquire_link, 0, 2);
    expect(framer.push(next)).toEqual([next]);
  });
});
