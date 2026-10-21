import { describe, it, expect } from "vitest";
import { summarise, describe as describePdu } from "../../src/smpp/summary";
import { decodePdu } from "../../src/smpp/decode";
import { encodePdu } from "../../src/smpp/encode";
import { CommandId, CommandStatus } from "../../src/smpp/constants";

const submit = decodePdu(
  encodePdu({
    commandId: CommandId.submit_sm,
    sequenceNumber: 42,
    fields: {
      source_addr: "393331234567",
      destination_addr: "393337654321",
      short_message: Buffer.from("hello", "latin1"),
    },
  })
);

describe("summarise", () => {
  it("names the command and the two addresses for submit_sm", () => {
    const s = summarise(submit);
    expect(s).toContain("submit_sm");
    expect(s).toContain("393331234567");
    expect(s).toContain("393337654321");
  });

  it("reports the body length for submit_sm", () => {
    expect(summarise(submit)).toMatch(/5\s*(bytes|B)/i);
  });

  it("names submit_sm_resp with its message_id", () => {
    const pdu = decodePdu(
      encodePdu({
        commandId: CommandId.submit_sm_resp,
        sequenceNumber: 42,
        fields: { message_id: "A7F2" },
      })
    );
    expect(summarise(pdu)).toContain("A7F2");
  });

  it("names the status when it is non-zero", () => {
    const pdu = decodePdu(
      encodePdu({
        commandId: CommandId.submit_sm_resp,
        commandStatus: CommandStatus.ESME_RTHROTTLED,
        sequenceNumber: 1,
        fields: {},
      })
    );
    expect(summarise(pdu)).toContain("ESME_RTHROTTLED");
  });

  it("does not clutter a successful response with ESME_ROK", () => {
    const pdu = decodePdu(
      encodePdu({ commandId: CommandId.enquire_link_resp, sequenceNumber: 1 })
    );
    expect(summarise(pdu)).not.toContain("ESME_ROK");
  });

  it("reports the receipt state for a deliver_sm that is one", () => {
    const pdu = decodePdu(
      encodePdu({
        commandId: CommandId.deliver_sm,
        sequenceNumber: 7,
        fields: {
          esm_class: 4,
          short_message: Buffer.from("id:A7F2 stat:DELIVRD err:000", "latin1"),
        },
      })
    );
    const s = summarise(pdu);
    expect(s).toContain("DELIVRD");
    expect(s).toContain("A7F2");
  });

  it("names an unknown command by its id", () => {
    const pdu = decodePdu(encodePdu({ commandId: 0x4242, sequenceNumber: 1, fields: {} }));
    expect(summarise(pdu)).toContain("0x4242");
  });

  it("names the bind system_id", () => {
    const pdu = decodePdu(
      encodePdu({
        commandId: CommandId.bind_transceiver,
        sequenceNumber: 1,
        fields: { system_id: "esme" },
      })
    );
    expect(summarise(pdu)).toContain("esme");
  });

  it("mentions the TLV count when a PDU carries TLVs", () => {
    const pdu = decodePdu(
      encodePdu({
        commandId: CommandId.submit_sm,
        sequenceNumber: 1,
        fields: {},
        tlvs: [
          { tag: 0x0424, value: Buffer.from("x") },
          { tag: 0x020c, value: Buffer.from([1, 2]) },
        ],
      })
    );
    expect(summarise(pdu)).toMatch(/2 TLV/);
  });
});

describe("describe", () => {
  it("lists every mandatory field as a name/value pair", () => {
    const rows = describePdu(submit);
    expect(rows.find((r) => r.name === "source_addr")?.value).toBe("393331234567");
    expect(rows.find((r) => r.name === "destination_addr")?.value).toBe("393337654321");
  });

  it("includes the header fields", () => {
    const rows = describePdu(submit);
    expect(rows.find((r) => r.name === "command_id")?.value).toContain("submit_sm");
    expect(rows.find((r) => r.name === "sequence_number")?.value).toBe("42");
    expect(rows.find((r) => r.name === "command_status")?.value).toContain("ESME_ROK");
  });

  it("renders an octet field as hex", () => {
    expect(describePdu(submit).find((r) => r.name === "short_message")?.value).toBe("68656c6c6f");
  });

  it("names each TLV by its tag name", () => {
    const pdu = decodePdu(
      encodePdu({
        commandId: CommandId.submit_sm,
        sequenceNumber: 1,
        fields: {},
        tlvs: [{ tag: 0x0424, value: Buffer.from("hi") }],
      })
    );
    expect(describePdu(pdu).some((r) => r.name === "message_payload")).toBe(true);
  });

  it("names an unknown TLV by its hex tag", () => {
    const pdu = decodePdu(
      encodePdu({
        commandId: CommandId.submit_sm,
        sequenceNumber: 1,
        fields: {},
        tlvs: [{ tag: 0x9999, value: Buffer.from([1]) }],
      })
    );
    expect(describePdu(pdu).some((r) => r.name === "unknown(0x9999)")).toBe(true);
  });
});
