import { describe, it, expect } from "vitest";
import {
  encodeText,
  decodeText,
  measure,
  SAMPLES,
  septetsOf,
  septetsFor,
  splitSeptets,
} from "../../src/smpp/text";
import { DataCoding } from "../../src/smpp/constants";

const A160 = "a".repeat(160);

describe("GSM 03.38", () => {
  it("packs GSM7 septets", () => {
    const { bytes } = encodeText(A160, DataCoding.GSM_DEFAULT, "packed");
    expect(bytes.length).toBe(140);
  });

  it("sends one septet per octet when unpacked", () => {
    const { bytes } = encodeText(A160, DataCoding.GSM_DEFAULT, "unpacked");
    expect(bytes.length).toBe(160);
  });

  it("defaults to unpacked, which is what SMSCs expect over SMPP", () => {
    expect(encodeText(A160, DataCoding.GSM_DEFAULT).bytes.length).toBe(160);
  });

  it("round-trips packed GSM7", () => {
    const text = "Hello @£$¥ world";
    const { bytes } = encodeText(text, DataCoding.GSM_DEFAULT, "packed");
    expect(decodeText(bytes, DataCoding.GSM_DEFAULT, "packed")).toBe(text);
  });

  it("round-trips unpacked GSM7", () => {
    const text = "Hello Ä Ö Ñ Ü à";
    const { bytes } = encodeText(text, DataCoding.GSM_DEFAULT, "unpacked");
    expect(decodeText(bytes, DataCoding.GSM_DEFAULT, "unpacked")).toBe(text);
  });

  it("counts an escape-table character as two septets", () => {
    expect(septetsOf("€")).toBe(2);
    const m = measure("€", DataCoding.GSM_DEFAULT, "udh");
    expect(m.chars).toBe(1);
    expect(m.bits).toBe(14);
  });

  it("encodes every escape-table character", () => {
    for (const ch of "^{}\\[~]|€") {
      const r = encodeText(ch, DataCoding.GSM_DEFAULT, "unpacked");
      expect(r.unmappable, ch).toEqual([]);
      expect(r.bytes.length, ch).toBe(2);
      expect(r.bytes[0], ch).toBe(0x1b);
    }
  });
});

describe("other codings", () => {
  it("encodes UCS2 big-endian", () => {
    expect(encodeText("é", DataCoding.UCS2).bytes).toEqual(Buffer.from([0x00, 0xe9]));
  });

  it("round-trips UCS2 including an emoji", () => {
    const text = "ciao 🙂";
    const { bytes } = encodeText(text, DataCoding.UCS2);
    expect(decodeText(bytes, DataCoding.UCS2)).toBe(text);
  });

  it("passes octet data through unchanged", () => {
    const raw = "\u0000\u0001þÿ";
    const { bytes } = encodeText(raw, DataCoding.OCTET_UNSPECIFIED_4);
    expect(bytes).toEqual(Buffer.from([0x00, 0x01, 0xfe, 0xff]));
  });

  it("encodes Latin-1", () => {
    expect(encodeText("é", DataCoding.LATIN_1).bytes).toEqual(Buffer.from([0xe9]));
  });

  it("encodes Cyrillic", () => {
    const { bytes, unmappable } = encodeText("Привет", DataCoding.CYRILLIC);
    expect(unmappable).toEqual([]);
    expect(decodeText(bytes, DataCoding.CYRILLIC)).toBe("Привет");
  });

  it("encodes Latin/Hebrew", () => {
    const { bytes, unmappable } = encodeText("שלום", DataCoding.LATIN_HEBREW);
    expect(unmappable).toEqual([]);
    expect(decodeText(bytes, DataCoding.LATIN_HEBREW)).toBe("שלום");
  });

  it("encodes JIS", () => {
    const { bytes, unmappable } = encodeText("こんにちは", DataCoding.JIS);
    expect(unmappable).toEqual([]);
    expect(decodeText(bytes, DataCoding.JIS)).toBe("こんにちは");
  });
});

describe("measure", () => {
  it("reports parts 1 for 160 GSM7 chars and 2 for 161", () => {
    expect(measure(A160, DataCoding.GSM_DEFAULT, "udh").parts).toBe(1);
    expect(measure("a".repeat(161), DataCoding.GSM_DEFAULT, "udh").parts).toBe(2);
  });

  it("reports parts 1 for 70 UCS2 chars and 2 for 71", () => {
    expect(measure("a".repeat(70), DataCoding.UCS2, "udh").parts).toBe(1);
    expect(measure("a".repeat(71), DataCoding.UCS2, "udh").parts).toBe(2);
  });

  it("reports one part in none mode however long the text is", () => {
    expect(measure("a".repeat(1000), DataCoding.GSM_DEFAULT, "none").parts).toBe(1);
  });

  it("reports chars, bits and bytes for unpacked GSM7", () => {
    const m = measure("hello", DataCoding.GSM_DEFAULT, "udh");
    expect(m.chars).toBe(5);
    expect(m.bits).toBe(35);
    expect(m.bytes).toBe(5);
  });

  it("reports the packed byte count when packing is packed", () => {
    expect(measure(A160, DataCoding.GSM_DEFAULT, "udh", "packed").bytes).toBe(140);
  });

  it("never splits an escape pair across parts", () => {
    // The 153-septet boundary lands exactly on the euro sign's ESC, so the pair
    // must move to the next segment whole and the first segment comes up short.
    const text = "a".repeat(152) + "€" + "b".repeat(100);
    const runs = splitSeptets(septetsFor(text), 153);
    expect(runs[0]).toHaveLength(152);
    expect(runs[1][0]).toBe(0x1b);
    expect(measure(text, DataCoding.GSM_DEFAULT, "udh").parts).toBe(2);
  });

  it("counts an emoji as one character but two UCS2 code units", () => {
    const m = measure("🙂", DataCoding.UCS2, "udh");
    expect(m.chars).toBe(1);
    expect(m.bytes).toBe(4);
  });
});

describe("samples", () => {
  it("offers the spec's four presets", () => {
    expect(Object.keys(SAMPLES).sort()).toEqual(["gsm1", "gsm3", "ucs2_1", "ucs2_3"]);
  });

  it("exercises the GSM escape table in the gsm1 preset", () => {
    expect(septetsOf(SAMPLES.gsm1)).toBeGreaterThan(Array.from(SAMPLES.gsm1).length);
  });

  it("measures each preset as its name claims", () => {
    expect(measure(SAMPLES.gsm1, DataCoding.GSM_DEFAULT, "udh").parts).toBe(1);
    expect(measure(SAMPLES.gsm3, DataCoding.GSM_DEFAULT, "udh").parts).toBe(3);
    expect(measure(SAMPLES.ucs2_1, DataCoding.UCS2, "udh").parts).toBe(1);
    expect(measure(SAMPLES.ucs2_3, DataCoding.UCS2, "udh").parts).toBe(3);
  });

  it("maps every preset character in its own coding", () => {
    expect(encodeText(SAMPLES.gsm1, DataCoding.GSM_DEFAULT).unmappable).toEqual([]);
    expect(encodeText(SAMPLES.gsm3, DataCoding.GSM_DEFAULT).unmappable).toEqual([]);
    expect(encodeText(SAMPLES.ucs2_1, DataCoding.UCS2).unmappable).toEqual([]);
    expect(encodeText(SAMPLES.ucs2_3, DataCoding.UCS2).unmappable).toEqual([]);
  });
});

describe("unmappable characters (Review Focus 4)", () => {
  it("reports emoji as unmappable under GSM7 and does not drop them", () => {
    const r = encodeText("hi 🙂", DataCoding.GSM_DEFAULT);
    expect(r.unmappable).toEqual(["🙂"]);
    expect(r.bytes.length).toBeGreaterThan(0);
  });

  it("substitutes a question mark for an unmappable character", () => {
    const r = encodeText("hi 🙂", DataCoding.GSM_DEFAULT, "unpacked");
    expect(decodeText(r.bytes, DataCoding.GSM_DEFAULT, "unpacked")).toBe("hi ?");
  });

  it("reports unmappable characters from measure() too", () => {
    expect(measure("hi 🙂", DataCoding.GSM_DEFAULT, "udh").unmappable).toEqual(["🙂"]);
  });

  it("reports each distinct unmappable character once", () => {
    expect(encodeText("🙂🙂🙃", DataCoding.GSM_DEFAULT).unmappable).toEqual(["🙂", "🙃"]);
  });

  it("reports a Cyrillic character as unmappable under Latin-1", () => {
    expect(encodeText("Привет", DataCoding.LATIN_1).unmappable.length).toBeGreaterThan(0);
  });
});
