import { describe, it, expect } from "vitest";
import { decodePdu, PduDecodeError } from "../../src/smpp/decode";
import { CommandId, CommandStatus } from "../../src/smpp/constants";
import { PDU_FIELDS } from "../../src/smpp/pdu-defs";
import { cstr, u8, pdu, submitSmBody } from "../helpers/bytes";
import { encodeTlvs } from "../../src/smpp/tlv";

describe("decodePdu", () => {
  it("decodes the 16-byte header", () => {
    const p = decodePdu(pdu(CommandId.enquire_link, CommandStatus.ESME_ROK, 42));
    expect(p.commandId).toBe(CommandId.enquire_link);
    expect(p.commandStatus).toBe(0);
    expect(p.sequenceNumber).toBe(42);
  });

  it("decodes submit_sm mandatory fields in wire order", () => {
    const p = decodePdu(pdu(CommandId.submit_sm, 0, 1, submitSmBody("hello")));
    expect(p.fields.service_type).toBe("");
    expect(p.fields.source_addr_ton).toBe(1);
    expect(p.fields.source_addr).toBe("393331234567");
    expect(p.fields.destination_addr).toBe("393337654321");
    expect(p.fields.registered_delivery).toBe(1);
    expect(p.fields.sm_length).toBe(5);
  });

  it("reads short_message using sm_length", () => {
    const p = decodePdu(pdu(CommandId.submit_sm, 0, 1, submitSmBody("hello")));
    expect(p.fields.short_message).toEqual(Buffer.from("hello", "latin1"));
  });

  it("decodes trailing TLVs after the mandatory fields", () => {
    const body = Buffer.concat([
      submitSmBody("hi"),
      encodeTlvs([{ tag: 0x0424, value: Buffer.from("payload") }]),
    ]);
    const p = decodePdu(pdu(CommandId.submit_sm, 0, 1, body));
    expect(p.tlvs).toEqual([{ tag: 0x0424, value: Buffer.from("payload") }]);
  });

  it("decodes submit_sm_resp", () => {
    const p = decodePdu(pdu(CommandId.submit_sm_resp, 0, 1, cstr("A7F2")));
    expect(p.fields.message_id).toBe("A7F2");
  });

  it("decodes a PDU with an unknown command_id, keeping the body as octets", () => {
    const body = Buffer.from([1, 2, 3, 4]);
    const p = decodePdu(pdu(0x4242, 0, 1, body));
    expect(p.fields.body).toEqual(body);
    expect(p.tlvs).toEqual([]);
  });

  it("preserves the exact input bytes in raw", () => {
    const bytes = pdu(CommandId.submit_sm, 0, 1, submitSmBody("hello"));
    expect(decodePdu(bytes).raw).toEqual(bytes);
  });

  it("throws PduDecodeError carrying raw when a cstring is unterminated", () => {
    // service_type with no NUL and nothing after it
    const bytes = pdu(CommandId.submit_sm, 0, 1, Buffer.from("abc", "latin1"));
    try {
      decodePdu(bytes);
      expect.unreachable("should have thrown");
    } catch (e) {
      expect(e).toBeInstanceOf(PduDecodeError);
      expect((e as PduDecodeError).raw).toEqual(bytes);
    }
  });

  it("throws PduDecodeError when sm_length runs past the body", () => {
    const body = Buffer.concat([
      cstr(""),
      u8(1, 1),
      cstr("111"),
      u8(1, 1),
      cstr("222"),
      u8(0, 0, 0),
      cstr(""),
      cstr(""),
      u8(0, 0, 0, 0),
      u8(200), // sm_length lies
      Buffer.from("hi"),
    ]);
    expect(() => decodePdu(pdu(CommandId.submit_sm, 0, 1, body))).toThrow(PduDecodeError);
  });

  it("throws PduDecodeError when command_length disagrees with the buffer", () => {
    const bytes = pdu(CommandId.enquire_link, 0, 1);
    expect(() => decodePdu(bytes.subarray(0, 15))).toThrow(PduDecodeError);
  });
});

describe("PDU_FIELDS", () => {
  it("defines a field table for every command the client sends", () => {
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
      expect(PDU_FIELDS[name], name).toBeDefined();
    }
  });

  it("defines a field table for every response", () => {
    for (const name of ["submit_sm_resp", "bind_transceiver_resp", "query_sm_resp", "deliver_sm_resp"]) {
      expect(PDU_FIELDS[name], name).toBeDefined();
    }
  });

  it("gives short_message its length from sm_length", () => {
    const f = PDU_FIELDS.submit_sm.find((x) => x.name === "short_message");
    expect(f).toMatchObject({ type: "octets", lenFrom: "sm_length" });
  });
});
