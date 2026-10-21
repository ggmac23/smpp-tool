import { describe, it, expect } from "vitest";
import { DlrCorrelator, normaliseId, CorrelationConfig } from "../../src/main/dlr";
import type { Receipt } from "../../src/smpp/receipt";

const ALL: CorrelationConfig = {
  preferTlv: true,
  parseText: true,
  normaliseRadix: true,
  stripLeadingZeros: true,
};

const OFF: CorrelationConfig = {
  preferTlv: true,
  parseText: true,
  normaliseRadix: false,
  stripLeadingZeros: false,
};

const receipt = (messageId: string | null, state = "DELIVRD"): Receipt => ({
  messageId,
  source: "text",
  state,
  errorCode: "000",
  submitDate: null,
  doneDate: null,
});

describe("normaliseId", () => {
  it("includes the id as given", () => {
    expect(normaliseId("ABC", ALL)).toContain("ABC");
  });

  it("includes the decimal reading of a hex id", () => {
    expect(normaliseId("123456", ALL)).toContain("1193046");
  });

  it("includes the hex reading of a decimal id", () => {
    expect(normaliseId("1193046", ALL)).toContain("123456");
  });

  it("includes the form without leading zeros", () => {
    expect(normaliseId("00ABC", ALL)).toContain("ABC");
  });

  it("returns only the id itself when both switches are off", () => {
    expect(normaliseId("00123456", OFF)).toEqual(["00123456"]);
  });
});

describe("DlrCorrelator", () => {
  it("matches an id returned verbatim", () => {
    const c = new DlrCorrelator(ALL);
    c.register("A7F2", "row-1");
    expect(c.match(receipt("A7F2"))).toBe("row-1");
  });

  it("matches a decimal receipt id against a hex submit id", () => {
    const c = new DlrCorrelator(ALL);
    c.register("123456", "row-1");
    expect(c.match(receipt("1193046"))).toBe("row-1");
  });

  it("matches a hex receipt id against a decimal submit id", () => {
    const c = new DlrCorrelator(ALL);
    c.register("1193046", "row-1");
    expect(c.match(receipt("123456"))).toBe("row-1");
  });

  it("matches an id that gained leading zeros", () => {
    const c = new DlrCorrelator(ALL);
    c.register("ABC", "row-1");
    expect(c.match(receipt("00ABC"))).toBe("row-1");
  });

  it("does not match when normalisation is switched off", () => {
    const c = new DlrCorrelator(OFF);
    c.register("123456", "row-1");
    expect(c.match(receipt("1193046"))).toBeNull();
  });

  it("puts an unmatchable receipt in unmatched", () => {
    const c = new DlrCorrelator(ALL);
    const r = receipt("NOPE");
    expect(c.match(r)).toBeNull();
    expect(c.unmatched).toEqual([r]);
  });

  it("puts a receipt with no message id in unmatched", () => {
    const c = new DlrCorrelator(ALL);
    const r = receipt(null);
    expect(c.match(r)).toBeNull();
    expect(c.unmatched).toEqual([r]);
  });

  it("never returns a token it was not given", () => {
    const c = new DlrCorrelator(ALL);
    c.register("A", "row-1");
    expect(c.match(receipt("B"))).toBeNull();
  });

  it("does not let a second receipt steal the first row's match", () => {
    const c = new DlrCorrelator(ALL);
    c.register("A7F2", "row-1");
    expect(c.match(receipt("A7F2"))).toBe("row-1");
    expect(c.match(receipt("A7F2"))).toBeNull();
    expect(c.unmatched).toHaveLength(1);
  });

  it("gives colliding registrations to the earliest unmatched row", () => {
    const c = new DlrCorrelator(ALL);
    c.register("A7F2", "row-1");
    c.register("A7F2", "row-2");
    expect(c.match(receipt("A7F2"))).toBe("row-1");
    expect(c.match(receipt("A7F2"))).toBe("row-2");
  });

  it("keeps every candidate form of one registration pointing at the same row", () => {
    const c = new DlrCorrelator(ALL);
    c.register("123456", "row-1");
    expect(c.match(receipt("00123456"))).toBe("row-1");
  });

  it("reports how many registrations are still waiting", () => {
    const c = new DlrCorrelator(ALL);
    c.register("A", "row-1");
    c.register("B", "row-2");
    expect(c.pending).toBe(2);
    c.match(receipt("A"));
    expect(c.pending).toBe(1);
  });
});
