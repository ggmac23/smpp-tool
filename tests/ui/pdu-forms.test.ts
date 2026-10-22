import { describe, it, expect } from "vitest";
import {
  buildQueryInput,
  buildCancelInput,
  buildReplaceInput,
  buildMultiInput,
  buildBroadcastInput,
  buildDataSmInput,
  MessageOpsFormValues,
  MultiFormValues,
  DataSmFormValues,
} from "../../src/shared/ui/pduforms";
import { CommandId, DataCoding } from "../../src/shared/ui/protocol";

/**
 * The body arrives already encoded: the backend's `encode_body` command does
 * that, so no widget holds a codec. These tests check where the builders put
 * it, which is their job; that the bytes then encode into a PDU is the
 * backend's, and is tested there.
 */
const BODY_HEX = "7265706c6163656d656e742074657874";

const OPS: MessageOpsFormValues = {
  messageId: "A".repeat(64),
  serviceType: "CMT",
  sourceTonNpiLabel: "Intl",
  sourceAddr: "393331234567",
  destTonNpiLabel: "Intl",
  destinationAddr: "393337654321",
  scheduleDeliveryLabel: "Immediate",
  validityPeriodLabel: "4 hours",
  registeredDelivery: 1,
  smDefaultMsgId: 0,
  text: "replacement text",
  dataCoding: DataCoding.GSM_DEFAULT,
};

describe("query_sm", () => {
  it("builds query_sm with a 64-character message_id", () => {
    const { request, warnings } = buildQueryInput(OPS);
    expect(request.commandId).toBe(CommandId.query_sm);
    expect(request.fields!.message_id).toBe("A".repeat(64));
    expect(warnings).toEqual([]);
  });

  it("carries the source address and its TON/NPI", () => {
    const { request } = buildQueryInput(OPS);
    expect(request.fields).toMatchObject({
      source_addr: "393331234567",
      source_addr_ton: 1,
      source_addr_npi: 1,
    });
  });

  it("warns but still builds a message_id longer than 64 characters", () => {
    const { request, warnings } = buildQueryInput({ ...OPS, messageId: "B".repeat(70) });
    expect(request.fields!.message_id).toBe("B".repeat(70));
    expect(warnings.join(" ")).toMatch(/message_id/);
  });

  it("warns about an empty message_id", () => {
    expect(buildQueryInput({ ...OPS, messageId: "" }).warnings.join(" ")).toMatch(/message_id/);
  });
});

describe("cancel_sm", () => {
  it("builds cancel_sm with source address and service_type", () => {
    const { request } = buildCancelInput(OPS);
    expect(request.commandId).toBe(CommandId.cancel_sm);
    expect(request.fields).toMatchObject({
      service_type: "CMT",
      message_id: "A".repeat(64),
      source_addr: "393331234567",
      destination_addr: "393337654321",
    });
  });

});

describe("replace_sm", () => {
  it("builds replace_sm with its own validity period", () => {
    const { request } = buildReplaceInput(OPS, BODY_HEX);
    expect(request.commandId).toBe(CommandId.replace_sm);
    expect(request.fields!.validity_period).toBe("000000040000000R");
  });

  it("carries the replacement body as hex", () => {
    const { request } = buildReplaceInput(OPS, BODY_HEX);
    expect(request.octetFields!.short_message).toBe(BODY_HEX);
  });

  it("carries registered_delivery and sm_default_msg_id", () => {
    const { request } = buildReplaceInput({ ...OPS, registeredDelivery: 2, smDefaultMsgId: 7 }, BODY_HEX);
    expect(request.fields).toMatchObject({ registered_delivery: 2, sm_default_msg_id: 7 });
  });

});

describe("submit_multi", () => {
  const MULTI: MultiFormValues = {
    serviceType: "",
    sourceTonNpiLabel: "Intl",
    sourceAddr: "393331234567",
    destinations: "393331111111\n393332222222\n393333333333",
    distributionLists: "",
    esmClass: 0,
    protocolId: 0,
    priorityFlag: 0,
    registeredDelivery: 1,
    scheduleDeliveryLabel: "Immediate",
    validityPeriodLabel: "Default",
    dataCoding: DataCoding.GSM_DEFAULT,
    replaceIfPresent: 0,
    smDefaultMsgId: 0,
    text: "hello",
    destTonNpiLabel: "Intl",
  };

  it("builds submit_multi with three destination addresses", () => {
    const { request } = buildMultiInput(MULTI, BODY_HEX);
    expect(request.commandId).toBe(CommandId.submit_multi);
    expect(request.destList).toHaveLength(3);
    expect(request.destList!.map((d) => d.addr)).toEqual([
      "393331111111",
      "393332222222",
      "393333333333",
    ]);
  });

  it("names every destination, leaving the count to the backend to derive", () => {
    const { request } = buildMultiInput(MULTI, BODY_HEX);
    expect(request.destList).toEqual([
      { flag: 1, ton: 1, npi: 1, addr: "393331111111" },
      { flag: 1, ton: 1, npi: 1, addr: "393332222222" },
      { flag: 1, ton: 1, npi: 1, addr: "393333333333" },
    ]);
  });

  it("adds distribution lists as flag-2 entries", () => {
    const { request } = buildMultiInput({ ...MULTI, distributionLists: "my-list\nother-list" }, BODY_HEX);
    expect(request.destList).toHaveLength(5);
    expect(request.destList!.filter((d) => d.flag === 2).map((d) => d.dlName)).toEqual([
      "my-list",
      "other-list",
    ]);
  });

  it("ignores blank lines in the destination list", () => {
    const { request } = buildMultiInput({ ...MULTI, destinations: "3933\n\n\n3934\n" }, BODY_HEX);
    expect(request.destList).toHaveLength(2);
  });

  it("warns when there are no destinations at all", () => {
    const { warnings } = buildMultiInput({ ...MULTI, destinations: "", distributionLists: "" }, BODY_HEX);
    expect(warnings.join(" ")).toMatch(/destination/i);
  });
});

describe("broadcast_sm", () => {
  it("builds broadcast_sm", () => {
    const { request } = buildBroadcastInput({ ...OPS, kind: "broadcast_sm" }, BODY_HEX);
    expect(request.commandId).toBe(CommandId.broadcast_sm);
    expect(request.fields).toMatchObject({ message_id: "A".repeat(64), service_type: "CMT" });
  });

  it("builds query_broadcast_sm", () => {
    const { request } = buildBroadcastInput({ ...OPS, kind: "query_broadcast_sm" }, BODY_HEX);
    expect(request.commandId).toBe(CommandId.query_broadcast_sm);
  });

  it("builds cancel_broadcast_sm", () => {
    const { request } = buildBroadcastInput({ ...OPS, kind: "cancel_broadcast_sm" }, BODY_HEX);
    expect(request.commandId).toBe(CommandId.cancel_broadcast_sm);
  });

});

describe("data_sm", () => {
  const DATA: DataSmFormValues = {
    serviceType: "",
    sourceTonNpiLabel: "Intl",
    sourceAddr: "393331234567",
    destTonNpiLabel: "Intl",
    destinationAddr: "393337654321",
    esmClass: 0,
    registeredDelivery: 1,
    dataCoding: DataCoding.GSM_DEFAULT,
    text: "hello",
    placement: "message_payload",
  };

  it("builds data_sm", () => {
    const { request } = buildDataSmInput(DATA, BODY_HEX);
    expect(request.commandId).toBe(CommandId.data_sm);
    expect(request.fields).toMatchObject({
      source_addr: "393331234567",
      destination_addr: "393337654321",
      registered_delivery: 1,
    });
  });

  it("carries the body in message_payload, since data_sm has no short_message", () => {
    const { request } = buildDataSmInput(DATA, BODY_HEX);
    expect(request.tlvs!.find((t) => t.tag === 0x0424)!.hex).toBe(BODY_HEX);
  });

});
