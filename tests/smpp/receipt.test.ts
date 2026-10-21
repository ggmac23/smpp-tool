import { describe, it, expect } from "vitest";
import { parseReceipt } from "../../src/smpp/receipt";
import { decodePdu } from "../../src/smpp/decode";
import { encodePdu } from "../../src/smpp/encode";
import { CommandId } from "../../src/smpp/constants";

const RECEIPT_TEXT =
  "id:0A1B2C3D sub:001 dlvrd:001 submit date:2610211200 done date:2610211201 stat:DELIVRD err:000 Text:hello";

function deliverSm(
  text: string,
  opts: { esmClass?: number; tlvs?: { tag: number; value: Buffer }[] } = {}
) {
  return decodePdu(
    encodePdu({
      commandId: CommandId.deliver_sm,
      sequenceNumber: 1,
      fields: {
        source_addr: "393331234567",
        destination_addr: "393337654321",
        esm_class: opts.esmClass ?? 0x04,
        short_message: Buffer.from(text, "latin1"),
      },
      tlvs: opts.tlvs,
    })
  );
}

describe("parseReceipt", () => {
  it("prefers the receipted_message_id TLV over the text", () => {
    const r = parseReceipt(
      deliverSm(RECEIPT_TEXT, {
        tlvs: [{ tag: 0x001e, value: Buffer.from("FROM-TLV\0", "latin1") }],
      })
    );
    expect(r?.messageId).toBe("FROM-TLV");
    expect(r?.source).toBe("tlv");
  });

  it("parses id, stat and err out of the receipt text", () => {
    const r = parseReceipt(deliverSm(RECEIPT_TEXT));
    expect(r?.messageId).toBe("0A1B2C3D");
    expect(r?.state).toBe("DELIVRD");
    expect(r?.errorCode).toBe("000");
  });

  it("parses the submit and done dates", () => {
    const r = parseReceipt(deliverSm(RECEIPT_TEXT));
    expect(r?.submitDate).toBe("2610211200");
    expect(r?.doneDate).toBe("2610211201");
  });

  it("returns source 'text' when no TLV is present", () => {
    expect(parseReceipt(deliverSm(RECEIPT_TEXT))?.source).toBe("text");
  });

  it("returns null for a deliver_sm that is not a receipt", () => {
    expect(parseReceipt(deliverSm("just an inbound message", { esmClass: 0 }))).toBeNull();
  });

  it("recognises an intermediate notification", () => {
    const r = parseReceipt(deliverSm(RECEIPT_TEXT, { esmClass: 0x20 }));
    expect(r?.state).toBe("DELIVRD");
  });

  it("tolerates receipt text with extra unknown keys", () => {
    const r = parseReceipt(
      deliverSm("id:XYZ vendor:acme ratio:7 stat:EXPIRED err:042 nonsense")
    );
    expect(r?.messageId).toBe("XYZ");
    expect(r?.state).toBe("EXPIRED");
    expect(r?.errorCode).toBe("042");
  });

  it("is case-insensitive about the receipt keys", () => {
    const r = parseReceipt(deliverSm("ID:ABC STAT:UNDELIV ERR:001"));
    expect(r?.messageId).toBe("ABC");
    expect(r?.state).toBe("UNDELIV");
  });

  it("falls back to the message_state TLV when the text has no stat", () => {
    const r = parseReceipt(
      deliverSm("id:ABC", { tlvs: [{ tag: 0x0427, value: Buffer.from([2]) }] })
    );
    expect(r?.state).toBe("DELIVERED");
  });

  it("reads the body from message_payload when short_message is empty", () => {
    const pdu = decodePdu(
      encodePdu({
        commandId: CommandId.deliver_sm,
        sequenceNumber: 1,
        fields: { esm_class: 0x04 },
        tlvs: [{ tag: 0x0424, value: Buffer.from(RECEIPT_TEXT, "latin1") }],
      })
    );
    expect(parseReceipt(pdu)?.messageId).toBe("0A1B2C3D");
  });

  it("returns null for a PDU that is not a deliver_sm", () => {
    const pdu = decodePdu(
      encodePdu({ commandId: CommandId.submit_sm, sequenceNumber: 1, fields: {} })
    );
    expect(parseReceipt(pdu)).toBeNull();
  });
});
