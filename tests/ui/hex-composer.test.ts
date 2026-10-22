import { describe, it, expect } from "vitest";
import { parseHex, formatHex, compactHex } from "../../src/shared/ui/hex";

/**
 * The hex composer is where a tester types bytes by hand. Parsing is tolerant
 * of what people paste and strict about what it cannot read: a typo must be
 * reported, never silently dropped, because the whole point is that the bytes
 * on the wire are the bytes that were typed.
 *
 * Decoding is not here any more. The backend decodes, so the renderer holds
 * no codec and no Buffer.
 */
describe("parseHex", () => {
  it("parses hex with spaces, newlines and 0x prefixes", () => {
    expect(Array.from(parseHex("0x00 00\n0x00 10"))).toEqual([0x00, 0x00, 0x00, 0x10]);
  });

  it("parses uppercase and lowercase alike", () => {
    expect(Array.from(parseHex("DEadBEef"))).toEqual([0xde, 0xad, 0xbe, 0xef]);
  });

  it("parses the separators hex dumps use", () => {
    // Pasting de:ad:be:ef out of a dump is normal; telling the user a colon
    // is not a hex digit would be technically true and useless.
    expect(Array.from(parseHex("de:ad-be_ef"))).toEqual([0xde, 0xad, 0xbe, 0xef]);
    expect(Array.from(parseHex("de,ad\tbe ef"))).toEqual([0xde, 0xad, 0xbe, 0xef]);
  });

  it("parses an empty string as no bytes", () => {
    expect(parseHex("").length).toBe(0);
    expect(parseHex("   \n ").length).toBe(0);
  });

  it("names the character it cannot read and where it is", () => {
    expect(() => parseHex("dead beeg")).toThrow(/"g" is not a hex digit \(at position 8\)/);
  });

  it("refuses an odd number of digits rather than guessing a nibble", () => {
    // Guessing a zero would send a byte the user never typed.
    expect(() => parseHex("abc")).toThrow(/odd number of hex digits \(3\)/);
  });

  it("returns bytes, not a Buffer: the renderer has no Node globals", () => {
    expect(parseHex("ff")).toBeInstanceOf(Uint8Array);
  });
});

describe("formatHex", () => {
  it("pads every byte to two digits", () => {
    expect(formatHex(new Uint8Array([0x00, 0x0f, 0xff]))).toBe("00 0f ff");
  });

  it("re-spaces hex text it is handed", () => {
    expect(formatHex("0x000000100x00000015")).toBe("00 00 00 10 00 00 00 15");
  });

  it("formats no bytes as an empty string", () => {
    expect(formatHex(new Uint8Array(0))).toBe("");
  });

  it("round-trips a traced PDU's hex through parse and format", () => {
    const hex = "0000001000000015000000000000000c";
    expect(compactHex(formatHex(hex))).toBe(hex);
  });
});

describe("compactHex", () => {
  it("is what the commands take: no separators", () => {
    expect(compactHex("de ad be ef")).toBe("deadbeef");
  });
});
