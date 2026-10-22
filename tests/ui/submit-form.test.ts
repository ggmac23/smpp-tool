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
import {
  buildComposeArgs,
  counterText,
  SubmitFormValues,
  SAMPLE_PRESETS,
} from "../../src/shared/ui/submit";
import { SAMPLES, DataCoding } from "../../src/shared/ui/protocol";

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

const args = (over: Partial<SubmitFormValues> = {}) => buildComposeArgs({ ...FORM, ...over });

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

  it("has a text for every preset it offers", () => {
    // Whether that text really takes the number of parts the preset claims
    // is the measurer's answer, and the measurer now lives in the backend:
    // the packaged-app check exercises it against the real one.
    for (const preset of SAMPLE_PRESETS) {
      expect(SAMPLES[preset.key], preset.key).toBeTruthy();
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

describe("buildComposeArgs", () => {
  /**
   * The renderer's only job here is to turn the labels its widgets hold into
   * the numbers and time strings the protocol uses, and to pass the rest
   * through untouched. Segmenting, encoding and the warnings all happen in
   * the backend, so there is one place that decides what a form means.
   */

  it("resolves the TON/NPI labels into numbers", () => {
    expect(args()).toMatchObject({
      sourceAddrTon: 1,
      sourceAddrNpi: 1,
      destAddrTon: 1,
      destAddrNpi: 1,
    });
  });

  it("resolves an alphanumeric source to TON 5, which is what a sender name is", () => {
    expect(args({ sourceTonNpiLabel: "Alpha" })).toMatchObject({
      sourceAddrTon: 5,
      sourceAddrNpi: 0,
    });
  });

  it("falls back to unknown for a label it does not recognise", () => {
    // A label the user cannot pick, so this is about never throwing here.
    expect(args({ sourceTonNpiLabel: "nonsense" })).toMatchObject({
      sourceAddrTon: 0,
      sourceAddrNpi: 0,
    });
  });

  it("resolves the schedule and validity labels into SMPP time strings", () => {
    const out = args({ scheduleDeliveryLabel: "Immediate", validityPeriodLabel: "Default" });
    expect(out.scheduleDeliveryTime).toBe("");
    expect(out.validityPeriod).toBe("");

    const later = args({ validityPeriodLabel: "4 hours" });
    expect(later.validityPeriod).toBe("000000040000000R");
  });

  it("passes the addresses through exactly as typed", () => {
    const out = args({ sourceAddr: "InfoSMS", destinationAddr: "12345" });
    expect(out.sourceAddr).toBe("InfoSMS");
    expect(out.destinationAddr).toBe("12345");
  });

  it("passes every flag field through", () => {
    const out = args({
      protocolId: 3,
      priorityFlag: 2,
      replaceIfPresent: 1,
      smDefaultMsgId: 7,
      registeredDelivery: 17,
      esmClass: 0x40,
      serviceType: "CMT",
    });
    expect(out).toMatchObject({
      protocolId: 3,
      priorityFlag: 2,
      replaceIfPresent: 1,
      smDefaultMsgId: 7,
      registeredDelivery: 17,
      esmClass: 0x40,
      serviceType: "CMT",
    });
  });

  it("passes the body and its options through without touching them", () => {
    const out = args({
      text: "hello",
      dataCoding: DataCoding.UCS2,
      segmentation: "sar",
      placement: "message_payload",
      packing: "packed",
      udhHex: "050003010201",
    });
    expect(out).toMatchObject({
      text: "hello",
      dataCoding: 8,
      segmentation: "sar",
      placement: "message_payload",
      packing: "packed",
      udhHex: "050003010201",
    });
  });

  it("passes the TLV rows through as typed, tag and all", () => {
    const rows = [{ tagHex: "0x1400", valueText: "Ab" }];
    expect(args({ tlvs: rows }).tlvs).toEqual(rows);
  });

  it("builds nothing that looks like a PDU: no fields, no octets, no command id", () => {
    // If any of these appear here, the renderer has started composing again.
    const out = args() as unknown as Record<string, unknown>;
    expect(out.commandId).toBeUndefined();
    expect(out.fields).toBeUndefined();
    expect(out.octetFields).toBeUndefined();
  });
});

describe("counterText", () => {
  it("reads out the measurement the backend returned", () => {
    expect(
      counterText({ parts: 1, chars: 5, bits: 35, bytes: 5, unmappable: [] })
    ).toBe("1 part \u00b7 5 chars \u00b7 35 bits \u00b7 5 bytes");
  });

  it("pluralises parts", () => {
    expect(
      counterText({ parts: 3, chars: 400, bits: 2800, bytes: 350, unmappable: [] })
    ).toContain("3 parts");
  });

  it("names the characters the coding cannot hold", () => {
    expect(
      counterText({ parts: 1, chars: 2, bits: 14, bytes: 2, unmappable: ["\u2744"] })
    ).toContain("cannot encode \u2744");
  });
});
