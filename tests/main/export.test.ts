import { describe, it, expect } from "vitest";
import { toCsv, traceToText } from "../../src/main/export";
import { TraceEntry } from "../../src/main/trace";

describe("toCsv", () => {
  it("writes a header row from the columns", () => {
    expect(toCsv([{ a: 1, b: 2 }], ["a", "b"]).split("\r\n")[0]).toBe("a,b");
  });

  it("writes one row per record in column order", () => {
    const csv = toCsv([{ b: "two", a: "one" }], ["a", "b"]);
    expect(csv.split("\r\n")[1]).toBe("one,two");
  });

  it("quotes a field containing a comma", () => {
    expect(toCsv([{ a: "x,y" }], ["a"]).split("\r\n")[1]).toBe('"x,y"');
  });

  it("doubles a quote inside a quoted field", () => {
    expect(toCsv([{ a: 'say "hi"' }], ["a"]).split("\r\n")[1]).toBe('"say ""hi"""');
  });

  it("quotes a field containing a newline", () => {
    expect(toCsv([{ a: "line1\nline2" }], ["a"])).toContain('"line1\nline2"');
  });

  it("writes an empty field for a missing or null value", () => {
    expect(toCsv([{ a: null, b: undefined }], ["a", "b"]).split("\r\n")[1]).toBe(",");
  });

  it("renders a Buffer as hex", () => {
    expect(toCsv([{ a: Buffer.from([0x0a, 0xff]) }], ["a"]).split("\r\n")[1]).toBe("0aff");
  });

  it("writes only a header row for no records", () => {
    expect(toCsv([], ["a", "b"])).toBe("a,b");
  });
});

describe("traceToText", () => {
  const entry: TraceEntry = {
    seq: 1,
    at: Date.UTC(2026, 9, 21, 12, 4, 11),
    direction: "out",
    commandName: "submit_sm",
    commandStatus: 0,
    sequenceNumber: 42,
    summary: "submit_sm 393331234567 -> 393337654321 5 bytes",
    raw: Buffer.from([0x00, 0x00, 0x00, 0x3d]),
  };

  it("writes one line per entry with direction, sequence and summary", () => {
    const text = traceToText([entry]);
    expect(text).toContain("42");
    expect(text).toContain("submit_sm 393331234567");
    expect(text).toMatch(/->|-->|out/);
  });

  it("includes the hex bytes", () => {
    expect(traceToText([entry])).toContain("0000003d");
  });

  it("marks a decode error instead of a summary", () => {
    const text = traceToText([{ ...entry, summary: "", decodeError: "unterminated C-string" }]);
    expect(text).toContain("unterminated C-string");
  });

  it("returns an empty string for no entries", () => {
    expect(traceToText([])).toBe("");
  });
});
