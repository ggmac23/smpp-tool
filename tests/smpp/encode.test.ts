import { describe, it, expect } from "vitest";
import { encodePdu, encodeRaw } from "../../src/smpp/encode";
import { decodePdu } from "../../src/smpp/decode";
import { CommandId } from "../../src/smpp/constants";

const SUBMIT = {
  commandId: CommandId.submit_sm,
  sequenceNumber: 1,
  fields: {
    service_type: "",
    source_addr_ton: 1,
    source_addr_npi: 1,
    source_addr: "393331234567",
    dest_addr_ton: 1,
    dest_addr_npi: 1,
    destination_addr: "393337654321",
    esm_class: 0,
    registered_delivery: 1,
    data_coding: 0,
    short_message: Buffer.from("hello", "latin1"),
  },
};

describe("encodePdu", () => {
  it("round-trips submit_sm through encode and decode", () => {
    const p = decodePdu(encodePdu(SUBMIT));
    expect(p.commandId).toBe(CommandId.submit_sm);
    expect(p.sequenceNumber).toBe(1);
    expect(p.fields.source_addr).toBe("393331234567");
    expect(p.fields.destination_addr).toBe("393337654321");
    expect(p.fields.registered_delivery).toBe(1);
    expect(p.fields.short_message).toEqual(Buffer.from("hello", "latin1"));
  });

  it("writes command_length to match the produced buffer", () => {
    const buf = encodePdu(SUBMIT);
    expect(buf.readUInt32BE(0)).toBe(buf.length);
  });

  it("writes an over-long service_type verbatim", () => {
    const buf = encodePdu({
      ...SUBMIT,
      fields: { ...SUBMIT.fields, service_type: "ABCDEFGHIJKL" },
    });
    expect(decodePdu(buf).fields.service_type).toBe("ABCDEFGHIJKL");
  });

  it("writes a data_coding of 0xff verbatim", () => {
    const buf = encodePdu({ ...SUBMIT, fields: { ...SUBMIT.fields, data_coding: 0xff } });
    expect(decodePdu(buf).fields.data_coding).toBe(0xff);
  });

  it("writes sm_length and short_message consistently when sm_length is absent", () => {
    const p = decodePdu(encodePdu(SUBMIT));
    expect(p.fields.sm_length).toBe(5);
    expect(p.fields.short_message).toEqual(Buffer.from("hello", "latin1"));
  });

  it("keeps a caller-supplied sm_length that disagrees with short_message", () => {
    const buf = encodePdu({
      ...SUBMIT,
      fields: { ...SUBMIT.fields, sm_length: 200, short_message: Buffer.from("hi", "latin1") },
    });
    // The declared length goes on the wire as given; the bytes are the two supplied.
    const smLengthOffset = buf.length - 3;
    expect(buf.readUInt8(smLengthOffset)).toBe(200);
    expect(buf.subarray(smLengthOffset + 1).toString("latin1")).toBe("hi");
    // And such a PDU is exactly as broken as the caller asked for.
    expect(() => decodePdu(buf)).toThrow();
  });

  it("writes a zero value for every omitted field", () => {
    const p = decodePdu(
      encodePdu({ commandId: CommandId.submit_sm, sequenceNumber: 9, fields: {} })
    );
    expect(p.fields.source_addr).toBe("");
    expect(p.fields.esm_class).toBe(0);
    expect(p.fields.sm_length).toBe(0);
    expect(p.fields.short_message).toEqual(Buffer.alloc(0));
  });

  it("appends TLVs after the mandatory fields", () => {
    const tlvs = [{ tag: 0x0424, value: Buffer.from("payload", "latin1") }];
    const p = decodePdu(encodePdu({ ...SUBMIT, tlvs }));
    expect(p.tlvs).toEqual(tlvs);
    expect(p.fields.short_message).toEqual(Buffer.from("hello", "latin1"));
  });

  it("encodes a default commandStatus of 0", () => {
    expect(decodePdu(encodePdu(SUBMIT)).commandStatus).toBe(0);
  });

  it("encodes bind_transceiver", () => {
    const p = decodePdu(
      encodePdu({
        commandId: CommandId.bind_transceiver,
        sequenceNumber: 1,
        fields: {
          system_id: "esme",
          password: "secret",
          system_type: "SMPP",
          interface_version: 0x34,
        },
      })
    );
    expect(p.fields.system_id).toBe("esme");
    expect(p.fields.interface_version).toBe(0x34);
  });

  it("encodes submit_multi with a destination list", () => {
    const p = decodePdu(
      encodePdu({
        commandId: CommandId.submit_multi,
        sequenceNumber: 1,
        fields: {
          source_addr: "111",
          dest_address: [
            { flag: 1, ton: 1, npi: 1, addr: "393331111111" },
            { flag: 1, ton: 1, npi: 1, addr: "393332222222" },
            { flag: 2, dlName: "my-list" },
          ],
          short_message: Buffer.from("hi", "latin1"),
        },
      })
    );
    expect(p.fields.number_of_dests).toBe(3);
    expect(p.fields.dest_address).toEqual([
      { flag: 1, ton: 1, npi: 1, addr: "393331111111" },
      { flag: 1, ton: 1, npi: 1, addr: "393332222222" },
      { flag: 2, dlName: "my-list" },
    ]);
  });

  it("encodes an unknown command id from a raw body", () => {
    const body = Buffer.from([1, 2, 3, 4]);
    const p = decodePdu(
      encodePdu({ commandId: 0x4242, sequenceNumber: 1, fields: { body } })
    );
    expect(p.fields.body).toEqual(body);
  });
});

describe("encodeRaw", () => {
  it("returns raw bytes untouched", () => {
    const bytes = Buffer.alloc(20);
    bytes.writeUInt32BE(0xdeadbeef, 0);
    bytes.writeUInt32BE(CommandId.submit_sm, 4);
    const out = encodeRaw(bytes);
    expect(out).toEqual(bytes);
    expect(out.readUInt32BE(0)).toBe(0xdeadbeef);
  });

  it("does not rewrite a truncated PDU", () => {
    const bytes = Buffer.from([0x00, 0x00, 0x00, 0x10, 0x00]);
    expect(encodeRaw(bytes)).toEqual(bytes);
  });
});
