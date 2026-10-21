import { describe, it, expect } from "vitest";
import { encodeTlvs, decodeTlvs, tlvName } from "../../src/smpp/tlv";
import { CommandId, commandName, statusName, CommandStatus } from "../../src/smpp/constants";

describe("TLV codec", () => {
  it("encodes a TLV as tag, length, value big-endian", () => {
    const out = encodeTlvs([{ tag: 0x0424, value: Buffer.from("hi", "ascii") }]);
    expect(out).toEqual(Buffer.from([0x04, 0x24, 0x00, 0x02, 0x68, 0x69]));
  });

  it("round-trips multiple TLVs", () => {
    const tlvs = [
      { tag: 0x0424, value: Buffer.from("hello", "ascii") },
      { tag: 0x001e, value: Buffer.from("A7F2", "ascii") },
      { tag: 0x020c, value: Buffer.from([0x2a]) },
    ];
    expect(decodeTlvs(encodeTlvs(tlvs))).toEqual(tlvs);
  });

  it("decodes an unknown tag without losing its bytes", () => {
    const tlv = { tag: 0x9999, value: Buffer.from([0xde, 0xad, 0xbe, 0xef]) };
    const decoded = decodeTlvs(encodeTlvs([tlv]));
    expect(decoded).toEqual([tlv]);
  });

  it("decodes a zero-length TLV", () => {
    const decoded = decodeTlvs(Buffer.from([0x04, 0x26, 0x00, 0x00]));
    expect(decoded).toEqual([{ tag: 0x0426, value: Buffer.alloc(0) }]);
  });

  it("throws on a TLV whose declared length exceeds the buffer", () => {
    expect(() => decodeTlvs(Buffer.from([0x04, 0x24, 0x00, 0x10, 0x68, 0x69]))).toThrow();
  });

  it("names a known tag", () => {
    expect(tlvName(0x0424)).toBe("message_payload");
    expect(tlvName(0x001e)).toBe("receipted_message_id");
  });

  it("names an unknown tag by its hex value", () => {
    expect(tlvName(0x9999)).toBe("unknown(0x9999)");
  });
});

describe("command constants", () => {
  it("names responses from their request id", () => {
    expect(CommandId.submit_sm).toBe(0x00000004);
    expect(CommandId.submit_sm_resp).toBe(0x80000004);
    expect(commandName(0x80000004)).toBe("submit_sm_resp");
  });

  it("names an unrecognised command id", () => {
    expect(commandName(0x4242)).toBe("unknown(0x4242)");
  });

  it("names every command in the spec's PDU list", () => {
    for (const name of [
      "bind_transmitter",
      "bind_receiver",
      "bind_transceiver",
      "unbind",
      "submit_sm",
      "data_sm",
      "query_sm",
      "cancel_sm",
      "replace_sm",
      "submit_multi",
      "broadcast_sm",
      "query_broadcast_sm",
      "cancel_broadcast_sm",
      "enquire_link",
      "deliver_sm",
      "generic_nack",
    ]) {
      const id = CommandId[name as keyof typeof CommandId];
      expect(id, name).toBeTypeOf("number");
      expect(commandName(id)).toBe(name);
    }
  });

  it("names command statuses", () => {
    expect(statusName(CommandStatus.ESME_ROK)).toBe("ESME_ROK");
    expect(statusName(0x00000058)).toBe("ESME_RTHROTTLED");
    expect(statusName(0x12345)).toBe("unknown(0x12345)");
  });
});
