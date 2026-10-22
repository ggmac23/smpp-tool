import { describe, it, expect } from "vitest";
import {
  REGISTERED_DELIVERY,
  ESM_CLASSES,
  PRIORITY_FLAGS,
  DATA_CODINGS,
  SCHEDULE_DELIVERY,
  VALIDITY_PERIODS,
  SEGMENTATION_MODES,
  BODY_PLACEMENTS,
  relativeTime,
  absoluteTime,
} from "../../src/shared/ui/fields";
import { buildSubmitInput, SubmitFormValues, SAMPLE_PRESETS } from "../../src/shared/ui/submit";
import { SAMPLES } from "../../src/smpp/text";
import { DataCoding, ESM_UDHI_BIT, CommandId } from "../../src/smpp/constants";

const FORM: SubmitFormValues = {
  serviceType: "",
  sourceTonNpiLabel: "Intl",
  sourceAddr: "393331234567",
  destTonNpiLabel: "Intl",
  destinationAddr: "393337654321",
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
  placement: "short_message",
  segmentation: "none",
  packing: "unpacked",
  udhHex: "",
  tlvs: [],
};

const build = (over: Partial<SubmitFormValues> = {}) => buildSubmitInput({ ...FORM, ...over });

describe("sample presets", () => {
  it("offers the reference tool's four samples", () => {
    expect(SAMPLE_PRESETS.map((p) => p.key)).toEqual(["gsm1", "ucs2_1", "gsm3", "ucs2_3"]);
  });

  it("sets a multi-part preset to a segmenting mode, or its body cannot be sent", () => {
    for (const preset of SAMPLE_PRESETS.filter((p) => p.parts > 1)) {
      expect(preset.segmentationLabel, preset.key).not.toMatch(/^None/);
    }
  });

  it("leaves a single-part preset on one PDU", () => {
    for (const preset of SAMPLE_PRESETS.filter((p) => p.parts === 1)) {
      expect(preset.segmentationLabel, preset.key).toMatch(/^None/);
    }
  });

  it("pairs each preset with the coding its text needs", () => {
    expect(SAMPLE_PRESETS.find((p) => p.key === "ucs2_3")!.dataCodingLabel).toMatch(/UCS2/);
    expect(SAMPLE_PRESETS.find((p) => p.key === "gsm3")!.dataCodingLabel).toMatch(/GSM/);
  });

  it("builds every preset without a length warning", () => {
    for (const preset of SAMPLE_PRESETS) {
      const built = buildSubmitInput({
        ...FORM,
        text: SAMPLES[preset.key],
        dataCoding: preset.dataCoding,
        segmentation: preset.segmentation,
      });
      expect(built.warnings.join(" "), preset.key).not.toMatch(/sm_length/i);
    }
  });
});

describe("field option lists", () => {
  it("offers registered_delivery 0-3 and 16-19", () => {
    expect(REGISTERED_DELIVERY.map((o) => o.value)).toEqual([0, 1, 2, 3, 16, 17, 18, 19]);
  });

  it("offers esm_class 0-3 and 64-67", () => {
    expect(ESM_CLASSES.map((o) => o.value)).toEqual([0, 1, 2, 3, 64, 65, 66, 67]);
  });

  it("labels the UDHI variants as such", () => {
    expect(ESM_CLASSES.find((o) => o.value === 64)!.label).toMatch(/UDHI/);
    expect(ESM_CLASSES.find((o) => o.value === 0)!.label).not.toMatch(/UDHI/);
  });

  it("offers priority_flag 0-4", () => {
    expect(PRIORITY_FLAGS.map((o) => o.value)).toEqual([0, 1, 2, 3, 4]);
  });

  it("offers data_coding 0-8", () => {
    expect(DATA_CODINGS.map((o) => o.value)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8]);
  });

  it("offers the reference tool's schedule options", () => {
    expect(SCHEDULE_DELIVERY.map((o) => o.label)).toEqual([
      "Immediate",
      "+1 minute",
      "+2 minutes",
      "+5 minutes",
      "+15 minutes",
      "Historic submit time",
    ]);
  });

  it("offers the reference tool's validity periods", () => {
    expect(VALIDITY_PERIODS.map((o) => o.label)).toEqual([
      "Default",
      "1 minute",
      "30 minutes",
      "1 hour",
      "4 hours",
      "1 day",
    ]);
  });

  it("offers the three segmentation modes and the two body placements", () => {
    expect(SEGMENTATION_MODES.map((o) => o.value)).toEqual(["none", "udh", "sar"]);
    expect(BODY_PLACEMENTS.map((o) => o.value)).toEqual(["short_message", "message_payload"]);
  });
});

describe("SMPP time formats", () => {
  it("writes a relative time as 16 characters ending in R", () => {
    const t = relativeTime({ minutes: 15 });
    expect(t).toHaveLength(16);
    expect(t.endsWith("R")).toBe(true);
    expect(t).toBe("000000001500000R");
  });

  it("writes a relative hour and a relative day", () => {
    expect(relativeTime({ hours: 4 })).toBe("000000040000000R");
    expect(relativeTime({ days: 1 })).toBe("000001000000000R");
  });

  it("writes an absolute time as 16 characters ending in +", () => {
    const t = absoluteTime(new Date(Date.UTC(2026, 9, 21, 12, 4, 11)));
    expect(t).toBe("261021120411000+");
  });
});

describe("buildSubmitInput", () => {
  it("builds a submit_sm with the form's TON/NPI and addresses", () => {
    const { requests } = build();
    expect(requests).toHaveLength(1);
    expect(requests[0].commandId).toBe(CommandId.submit_sm);
    expect(requests[0].fields).toMatchObject({
      source_addr_ton: 1,
      source_addr_npi: 1,
      source_addr: "393331234567",
      dest_addr_ton: 1,
      dest_addr_npi: 1,
      destination_addr: "393337654321",
      registered_delivery: 1,
    });
  });

  it("uses the Alpha preset's alphanumeric TON for the source", () => {
    const { requests } = build({ sourceTonNpiLabel: "Alpha", sourceAddr: "MYBRAND" });
    expect(requests[0].fields).toMatchObject({ source_addr_ton: 5, source_addr: "MYBRAND" });
  });

  it("carries the body as hex in octetFields", () => {
    const { requests } = build();
    expect(requests[0].octetFields!.short_message).toBe("68656c6c6f");
  });

  it("puts the body in message_payload when that placement is chosen", () => {
    const { requests } = build({ placement: "message_payload" });
    expect(requests[0].octetFields!.short_message).toBeUndefined();
    const payload = requests[0].tlvs!.find((t) => t.tag === 0x0424);
    expect(payload!.hex).toBe("68656c6c6f");
  });

  it("includes an arbitrary unknown TLV", () => {
    const { requests } = build({ tlvs: [{ tagHex: "9999", valueHex: "deadbeef" }] });
    expect(requests[0].tlvs).toEqual(
      expect.arrayContaining([{ tag: 0x9999, hex: "deadbeef" }])
    );
  });

  it("encodes a TLV given as text rather than hex", () => {
    const { requests } = build({ tlvs: [{ tagHex: "1400", valueText: "hi" }] });
    expect(requests[0].tlvs![0]).toEqual({ tag: 0x1400, hex: "6869" });
  });

  it("returns a PDU and a warning for a 3-character destination, not an error", () => {
    const { requests, warnings } = build({ destinationAddr: "123" });
    expect(requests).toHaveLength(1);
    expect(requests[0].fields!.destination_addr).toBe("123");
    expect(warnings.join(" ")).toMatch(/destination/i);
  });

  it("returns a PDU for a data_coding the spec does not list", () => {
    const { requests, warnings } = build({ dataCoding: 0xff });
    expect(requests[0].fields!.data_coding).toBe(0xff);
    expect(warnings.join(" ")).toMatch(/data_coding/);
  });

  it("warns about an over-long service_type without truncating it", () => {
    const { requests, warnings } = build({ serviceType: "ABCDEFGHIJKL" });
    expect(requests[0].fields!.service_type).toBe("ABCDEFGHIJKL");
    expect(warnings.join(" ")).toMatch(/service_type/);
  });

  it("warns about nothing for a well-formed message", () => {
    expect(build().warnings).toEqual([]);
  });

  it("produces three segments in udh mode for the 3-part GSM7 preset", () => {
    const { requests } = build({ text: SAMPLES.gsm3, segmentation: "udh" });
    expect(requests).toHaveLength(3);
  });

  it("sets esm_class UDHI in udh mode", () => {
    const { requests } = build({ text: SAMPLES.gsm3, segmentation: "udh" });
    expect(Number(requests[0].fields!.esm_class) & ESM_UDHI_BIT).toBe(ESM_UDHI_BIT);
  });

  it("leaves esm_class alone and emits sar_* TLVs in sar mode", () => {
    const { requests } = build({ text: SAMPLES.gsm3, segmentation: "sar" });
    expect(Number(requests[0].fields!.esm_class) & ESM_UDHI_BIT).toBe(0);
    expect(requests[0].tlvs!.some((t) => t.tag === 0x020c)).toBe(true);
    expect(requests[0].tlvs!.some((t) => t.tag === 0x020e)).toBe(true);
  });

  it("sends one PDU in none mode however long the text is", () => {
    expect(build({ text: SAMPLES.gsm3, segmentation: "none" }).requests).toHaveLength(1);
  });

  it("honours a hand-written UDH over automatic concatenation", () => {
    const { requests } = build({ text: SAMPLES.gsm3, segmentation: "udh", udhHex: "03 20 01 05" });
    expect(requests).toHaveLength(1);
    expect(requests[0].octetFields!.short_message.startsWith("03200105")).toBe(true);
  });

  it("reports the counter for the ucs2_3 preset as 3 parts", () => {
    const m = build({
      text: SAMPLES.ucs2_3,
      dataCoding: DataCoding.UCS2,
      segmentation: "udh",
    }).measurement;
    expect(m.parts).toBe(3);
  });

  it("reports chars, bits and bytes in the counter", () => {
    const m = build().measurement;
    expect(m.chars).toBe(5);
    expect(m.bits).toBe(35);
    expect(m.bytes).toBe(5);
  });

  it("reports unmappable characters in the counter", () => {
    const built = build({ text: "hi 🙂" });
    expect(built.measurement.unmappable).toEqual(["🙂"]);
    expect(built.warnings.join(" ")).toMatch(/🙂/);
  });

  it("warns when the body will not fit short_message in one PDU", () => {
    // sm_length is a single octet: a 300-byte body in one PDU writes 44.
    const built = build({ text: "a".repeat(300), segmentation: "none" });
    expect(built.warnings.join(" ")).toMatch(/sm_length|254|one PDU/i);
    expect(built.requests).toHaveLength(1);
  });

  it("does not warn about length when the body travels in message_payload", () => {
    const built = build({
      text: "a".repeat(300),
      segmentation: "none",
      placement: "message_payload",
    });
    expect(built.warnings.join(" ")).not.toMatch(/sm_length/i);
  });

  it("does not warn about length when the body is segmented", () => {
    expect(build({ text: "a".repeat(300), segmentation: "udh" }).warnings.join(" ")).not.toMatch(
      /sm_length/i
    );
  });

  it("counts one part when a hand-written UDH turns concatenation off", () => {
    const built = build({ text: "a".repeat(400), segmentation: "udh", udhHex: "03 20 01 05" });
    expect(built.requests).toHaveLength(1);
    expect(built.measurement.parts).toBe(1);
  });

  it("writes the relative validity period the label asks for", () => {
    const { requests } = build({ validityPeriodLabel: "4 hours" });
    expect(requests[0].fields!.validity_period).toBe("000000040000000R");
  });

  it("writes an empty validity period for Default", () => {
    expect(build().requests[0].fields!.validity_period).toBe("");
  });

  it("writes the relative schedule the label asks for", () => {
    const { requests } = build({ scheduleDeliveryLabel: "+15 minutes" });
    expect(requests[0].fields!.schedule_delivery_time).toBe("000000001500000R");
  });

  it("writes an absolute past time for the historic option", () => {
    const { requests } = build({ scheduleDeliveryLabel: "Historic submit time" });
    const t = String(requests[0].fields!.schedule_delivery_time);
    expect(t).toHaveLength(16);
    expect(t.endsWith("+")).toBe(true);
  });

  it("carries protocol_id, priority_flag, replace_if_present and sm_default_msg_id", () => {
    const { requests } = build({
      protocolId: 0x7f,
      priorityFlag: 3,
      replaceIfPresent: 1,
      smDefaultMsgId: 9,
    });
    expect(requests[0].fields).toMatchObject({
      protocol_id: 0x7f,
      priority_flag: 3,
      replace_if_present_flag: 1,
      sm_default_msg_id: 9,
    });
  });
});
