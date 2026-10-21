import { describe, it, expect } from "vitest";
import { segment } from "../../src/smpp/segment";
import { decodeText } from "../../src/smpp/text";
import { DataCoding, ESM_UDHI_BIT, EsmClass } from "../../src/smpp/constants";

const MESSAGE_PAYLOAD = 0x0424;
const SAR_REF = 0x020c;
const SAR_TOTAL = 0x020e;
const SAR_SEQ = 0x020f;

const gsm = (text: string, over: Record<string, unknown> = {}) =>
  segment({
    text,
    dataCoding: DataCoding.GSM_DEFAULT,
    mode: "udh",
    placement: "short_message",
    ...over,
  } as Parameters<typeof segment>[0]);

describe("segment", () => {
  it("returns one oversized segment in none mode", () => {
    const segs = gsm("a".repeat(400), { mode: "none" });
    expect(segs).toHaveLength(1);
    expect((segs[0].fields.short_message as Buffer).length).toBe(400);
  });

  it("splits GSM7 into 153-septet segments in udh mode", () => {
    const segs = gsm("a".repeat(400));
    expect(segs).toHaveLength(3);
    // 6-byte concatenation UDH plus 153 unpacked septets.
    expect((segs[0].fields.short_message as Buffer).length).toBe(6 + 153);
    expect((segs[2].fields.short_message as Buffer).length).toBe(6 + (400 - 306));
  });

  it("splits UCS2 into 67-character segments in udh mode", () => {
    const segs = gsm("a".repeat(200), { dataCoding: DataCoding.UCS2 });
    expect(segs).toHaveLength(3);
    expect((segs[0].fields.short_message as Buffer).length).toBe(6 + 67 * 2);
  });

  it("reassembles to the original text across segments", () => {
    const text = "a".repeat(400);
    const segs = gsm(text);
    const joined = segs
      .map((s) => decodeText((s.fields.short_message as Buffer).subarray(6), DataCoding.GSM_DEFAULT))
      .join("");
    expect(joined).toBe(text);
  });

  it("writes a concatenation UDH with a shared reference and rising sequence", () => {
    const segs = gsm("a".repeat(400), { refNum: 0x2a });
    for (const [i, s] of segs.entries()) {
      const udh = (s.fields.short_message as Buffer).subarray(0, 6);
      expect(udh[0]).toBe(0x05); // UDH length
      expect(udh[1]).toBe(0x00); // IE: concatenated short message, 8-bit ref
      expect(udh[2]).toBe(0x03); // IE length
      expect(udh[3]).toBe(0x2a); // reference
      expect(udh[4]).toBe(3); // total
      expect(udh[5]).toBe(i + 1); // sequence, from 1
    }
  });

  it("sets the UDHI bit in esm_class in udh mode only", () => {
    expect(Number(gsm("a".repeat(400))[0].fields.esm_class) & ESM_UDHI_BIT).toBe(ESM_UDHI_BIT);
    expect(Number(gsm("a".repeat(400), { mode: "sar" })[0].fields.esm_class) & ESM_UDHI_BIT).toBe(0);
    expect(Number(gsm("hi", { mode: "none" })[0].fields.esm_class) & ESM_UDHI_BIT).toBe(0);
  });

  it("keeps the caller's messaging mode when it sets UDHI", () => {
    const segs = gsm("a".repeat(400), { esmClass: EsmClass.STORE_AND_FORWARD });
    expect(segs[0].fields.esm_class).toBe(EsmClass.STORE_AND_FORWARD | ESM_UDHI_BIT);
  });

  it("emits sar_* TLVs with matching ref num in sar mode", () => {
    const segs = gsm("a".repeat(400), { mode: "sar", refNum: 0x1234 });
    expect(segs).toHaveLength(3);
    for (const s of segs) {
      const ref = s.tlvs.find((t) => t.tag === SAR_REF)!;
      expect(ref.value).toEqual(Buffer.from([0x12, 0x34]));
      expect(s.tlvs.find((t) => t.tag === SAR_TOTAL)!.value).toEqual(Buffer.from([3]));
    }
  });

  it("numbers sar segments from 1", () => {
    const segs = gsm("a".repeat(400), { mode: "sar" });
    expect(segs.map((s) => s.tlvs.find((t) => t.tag === SAR_SEQ)!.value[0])).toEqual([1, 2, 3]);
  });

  it("adds no UDH to the body in sar mode", () => {
    const segs = gsm("a".repeat(400), { mode: "sar" });
    expect((segs[0].fields.short_message as Buffer).length).toBe(153);
  });

  it("returns one segment carrying the user's UDH when userUdh is given", () => {
    const udh = Buffer.from([0x03, 0x20, 0x01, 0x05]);
    const segs = gsm("a".repeat(400), { userUdh: udh });
    expect(segs).toHaveLength(1);
    expect((segs[0].fields.short_message as Buffer).subarray(0, 4)).toEqual(udh);
  });

  it("does not add a concatenation IE when userUdh is given", () => {
    const udh = Buffer.from([0x03, 0x20, 0x01, 0x05]);
    const body = gsm("a".repeat(400), { userUdh: udh })[0].fields.short_message as Buffer;
    expect(body.length).toBe(udh.length + 400);
  });

  it("still sets UDHI for a user-supplied UDH", () => {
    const segs = gsm("hi", { userUdh: Buffer.from([0x00]), mode: "none" });
    expect(Number(segs[0].fields.esm_class) & ESM_UDHI_BIT).toBe(ESM_UDHI_BIT);
  });

  it("puts the body in message_payload when placement says so", () => {
    const segs = gsm("hello", { placement: "message_payload", mode: "none" });
    expect(segs[0].tlvs.find((t) => t.tag === MESSAGE_PAYLOAD)!.value).toEqual(
      Buffer.from("hello", "latin1")
    );
  });

  it("omits sm_length and short_message from a message_payload segment", () => {
    const segs = gsm("hello", { placement: "message_payload", mode: "none" });
    expect(segs[0].fields.short_message).toBeUndefined();
    expect(segs[0].fields.sm_length).toBeUndefined();
  });

  it("reports unmappable characters on each segment", () => {
    const segs = gsm("hi 🙂", { mode: "none" });
    expect(segs[0].unmappable).toEqual(["🙂"]);
  });

  it("segments packed GSM7 into 134-octet bodies", () => {
    const segs = gsm("a".repeat(400), { packing: "packed" });
    expect((segs[0].fields.short_message as Buffer).length).toBe(6 + 134);
  });
});
