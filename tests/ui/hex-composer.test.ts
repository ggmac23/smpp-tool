import { describe, it, expect } from "vitest";
import { parseHex, formatHex, previewPdu } from "../../src/shared/ui/hex";
import { encodePdu } from "../../src/smpp/encode";
import { CommandId } from "../../src/smpp/constants";

describe("parseHex", () => {
  it("parses hex with spaces, newlines and 0x prefixes", () => {
    expect(parseHex("00 00\n00 10\t0x15")).toEqual(
      Buffer.from([0x00, 0x00, 0x00, 0x10, 0x15])
    );
  });

  it("parses uppercase and lowercase alike", () => {
    expect(parseHex("DeAdBeEf")).toEqual(Buffer.from([0xde, 0xad, 0xbe, 0xef]));
  });

  it("parses an empty string as no bytes", () => {
    expect(parseHex("   \n  ")).toEqual(Buffer.alloc(0));
  });

  it("throws on an odd number of hex digits", () => {
    expect(() => parseHex("4142 4")).toThrow(/odd/i);
  });

  it("throws on a non-hex character", () => {
    expect(() => parseHex("41 4g")).toThrow(/g/);
  });

  it("names the offending character rather than failing vaguely", () => {
    expect(() => parseHex("zz")).toThrow(/"z"/);
  });
});

describe("formatHex", () => {
  it("formats a buffer as space-separated hex", () => {
    expect(formatHex(Buffer.from([0x00, 0x0a, 0xff]))).toBe("00 0a ff");
  });

  it("formats no bytes as an empty string", () => {
    expect(formatHex(Buffer.alloc(0))).toBe("");
  });

  it("round-trips a traced PDU's raw bytes through format and parse", () => {
    const bytes = encodePdu({
      commandId: CommandId.submit_sm,
      sequenceNumber: 42,
      fields: { source_addr: "3933", destination_addr: "3934" },
    });
    expect(parseHex(formatHex(bytes))).toEqual(bytes);
  });

  it("accepts a plain hex string as well as a buffer", () => {
    expect(formatHex("000aff")).toBe("00 0a ff");
  });
});

describe("previewPdu", () => {
  const good = encodePdu({
    commandId: CommandId.submit_sm,
    sequenceNumber: 42,
    fields: { source_addr: "393331234567", destination_addr: "393337654321" },
  });

  it("previews a valid PDU's decoded form", () => {
    const p = previewPdu(good);
    expect(p.ok).toBe(true);
    expect(p.summary).toContain("submit_sm");
    expect(p.fields.find((f) => f.name === "source_addr")?.value).toBe("393331234567");
    expect(p.byteCount).toBe(good.length);
  });

  it("reports the bytes as sendable when it decodes", () => {
    expect(previewPdu(good).sendable).toBe(true);
  });

  it("previews a decode error and still reports the bytes as sendable", () => {
    const broken = Buffer.from(good);
    broken.writeUInt32BE(0xdeadbeef, 0);
    const p = previewPdu(broken);
    expect(p.ok).toBe(false);
    expect(p.error).toMatch(/command_length/i);
    expect(p.sendable).toBe(true);
    expect(p.byteCount).toBe(broken.length);
  });

  it("reports a truncated PDU as undecodable but sendable", () => {
    const p = previewPdu(good.subarray(0, 8));
    expect(p.ok).toBe(false);
    expect(p.sendable).toBe(true);
  });

  it("refuses to call no bytes sendable", () => {
    const p = previewPdu(Buffer.alloc(0));
    expect(p.sendable).toBe(false);
    expect(p.byteCount).toBe(0);
  });

  it("previews bytes given as a hex string", () => {
    expect(previewPdu(formatHex(good)).summary).toContain("submit_sm");
  });
});
