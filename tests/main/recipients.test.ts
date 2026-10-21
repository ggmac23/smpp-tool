import { describe, it, expect } from "vitest";
import { resolveRecipients, MAX_BURST, MAX_RATE } from "../../src/main/recipients";

describe("resolveRecipients", () => {
  it("repeats one number count times", () => {
    const r = resolveRecipients({ kind: "repeat", number: "393331234567", count: 3 });
    expect(r.numbers).toEqual(["393331234567", "393331234567", "393331234567"]);
    expect(r.warnings).toEqual([]);
  });

  it("increments the numeric tail for a range", () => {
    const r = resolveRecipients({ kind: "range", from: "393331234567", count: 3 });
    expect(r.numbers).toEqual(["393331234567", "393331234568", "393331234569"]);
  });

  it("keeps the width when a range carries over", () => {
    const r = resolveRecipients({ kind: "range", from: "393331234599", count: 2 });
    expect(r.numbers).toEqual(["393331234599", "393331234600"]);
  });

  it("increments only the numeric tail of a mixed address", () => {
    const r = resolveRecipients({ kind: "range", from: "SENDER007", count: 2 });
    expect(r.numbers).toEqual(["SENDER007", "SENDER008"]);
  });

  it("warns when a range has no numeric tail to increment", () => {
    const r = resolveRecipients({ kind: "range", from: "ALPHA", count: 2 });
    expect(r.warnings.join(" ")).toMatch(/numeric/i);
    expect(r.numbers).toEqual(["ALPHA", "ALPHA"]);
  });

  it("reads a CSV column by name", () => {
    const text = "name,msisdn\nalice,393331111111\nbob,393332222222\n";
    const r = resolveRecipients({ kind: "csv", text, column: "msisdn" });
    expect(r.numbers).toEqual(["393331111111", "393332222222"]);
  });

  it("reads the first column when no name is given", () => {
    const text = "393331111111,alice\n393332222222,bob\n";
    const r = resolveRecipients({ kind: "csv", text });
    expect(r.numbers).toEqual(["393331111111", "393332222222"]);
  });

  it("skips a header row", () => {
    const text = "msisdn\n393331111111\n393332222222\n";
    const r = resolveRecipients({ kind: "csv", text });
    expect(r.numbers).toEqual(["393331111111", "393332222222"]);
  });

  it("keeps a numeric first row that is not a header", () => {
    const r = resolveRecipients({ kind: "csv", text: "393331111111\n393332222222\n" });
    expect(r.numbers).toHaveLength(2);
  });

  it("warns about a malformed row and keeps it", () => {
    const text = "msisdn\n393331111111\nnot a number!\n393332222222\n";
    const r = resolveRecipients({ kind: "csv", text });
    expect(r.numbers).toEqual(["393331111111", "not a number!", "393332222222"]);
    expect(r.warnings).toHaveLength(1);
    expect(r.warnings[0]).toMatch(/row 3/i);
  });

  it("ignores blank lines", () => {
    const r = resolveRecipients({ kind: "csv", text: "393331111111\n\n\n393332222222\n" });
    expect(r.numbers).toEqual(["393331111111", "393332222222"]);
  });

  it("handles quoted CSV fields", () => {
    const text = 'msisdn,note\n"393331111111","hello, world"\n';
    const r = resolveRecipients({ kind: "csv", text, column: "msisdn" });
    expect(r.numbers).toEqual(["393331111111"]);
  });

  it("warns when a named column is missing and falls back to the first", () => {
    const text = "name,other\nalice,393331111111\n";
    const r = resolveRecipients({ kind: "csv", text, column: "msisdn" });
    expect(r.warnings.join(" ")).toMatch(/msisdn/);
    expect(r.numbers).toEqual(["alice"]);
  });

  it("rejects a count above MAX_BURST", () => {
    expect(() =>
      resolveRecipients({ kind: "repeat", number: "1", count: MAX_BURST + 1 })
    ).toThrow(new RegExp(String(MAX_BURST)));
  });

  it("accepts a count at exactly MAX_BURST", () => {
    expect(resolveRecipients({ kind: "repeat", number: "1", count: MAX_BURST }).numbers).toHaveLength(
      MAX_BURST
    );
  });

  it("rejects a CSV with more rows than MAX_BURST", () => {
    const text = Array.from({ length: MAX_BURST + 2 }, (_, i) => String(i)).join("\n");
    expect(() => resolveRecipients({ kind: "csv", text })).toThrow(new RegExp(String(MAX_BURST)));
  });

  it("exposes the rate ceiling", () => {
    expect(MAX_RATE).toBe(500);
  });
});
