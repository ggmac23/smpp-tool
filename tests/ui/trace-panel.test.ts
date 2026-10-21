import { describe, it, expect } from "vitest";
import {
  hexDump,
  hexDumpRows,
  formatTime,
  directionArrow,
  traceRowText,
  droppedNotice,
} from "../../src/shared/ui/hexdump";
import type { WirePdu } from "../../src/shared/api";

const row: WirePdu = {
  seq: 1,
  at: Date.UTC(2026, 9, 21, 12, 4, 11, 250),
  direction: "out",
  commandName: "submit_sm",
  commandStatus: 0,
  statusName: "ESME_ROK",
  sequenceNumber: 42,
  summary: "submit_sm 393331234567 -> 393337654321 5 bytes",
  hex: "0000003d00000004",
};

describe("hexDump", () => {
  it("formats hex in 16-byte rows with an ASCII column", () => {
    const bytes = Buffer.from("0123456789abcdef0123", "latin1").toString("hex");
    const rows = hexDumpRows(bytes);
    expect(rows).toHaveLength(2);
    expect(rows[0].offset).toBe("00000000");
    expect(rows[0].hex.split(/\s+/).filter(Boolean)).toHaveLength(16);
    expect(rows[0].ascii).toBe("0123456789abcdef");
    expect(rows[1].offset).toBe("00000010");
    expect(rows[1].ascii).toBe("0123");
  });

  it("pads the hex column of a short last row so the ASCII column lines up", () => {
    const rows = hexDumpRows("41424344");
    expect(rows[0].hex).toHaveLength(16 * 3 - 1);
  });

  it("renders a non-printable byte as a dot in the ASCII column", () => {
    const rows = hexDumpRows("00017f41ff");
    expect(rows[0].ascii).toBe("...A.");
  });

  it("renders a space as a space, not a dot", () => {
    expect(hexDumpRows("412041").ascii ?? hexDumpRows("412041")[0].ascii).toBe("A A");
  });

  it("joins the rows into one block with offsets", () => {
    const text = hexDump("4142434445");
    expect(text).toContain("00000000");
    expect(text).toContain("41 42 43 44 45");
    expect(text).toContain("ABCDE");
  });

  it("returns an empty string for no bytes", () => {
    expect(hexDump("")).toBe("");
    expect(hexDumpRows("")).toEqual([]);
  });

  it("tolerates whitespace and uppercase in the input", () => {
    expect(hexDumpRows("41 42 43")[0].ascii).toBe("ABC");
    expect(hexDumpRows("4A4B")[0].ascii).toBe("JK");
  });
});

describe("trace row rendering", () => {
  it("formats the time to the millisecond", () => {
    expect(formatTime(row.at)).toMatch(/^\d{2}:\d{2}:\d{2}\.\d{3}$/);
  });

  it("marks direction with an arrow that reads at a glance", () => {
    expect(directionArrow("out")).toBe("→");
    expect(directionArrow("in")).toBe("←");
  });

  it("renders a row as time, sequence, direction and summary", () => {
    const text = traceRowText(row);
    expect(text).toContain("42");
    expect(text).toContain("→");
    expect(text).toContain("submit_sm 393331234567");
  });

  it("renders a decode-error row with the error instead of a summary", () => {
    const text = traceRowText({
      ...row,
      summary: "",
      decodeError: "unterminated C-string at offset 4",
    });
    expect(text).toContain("unterminated C-string");
    expect(text).toMatch(/decode error/i);
  });

  it("includes a non-zero status in the row", () => {
    expect(traceRowText({ ...row, commandStatus: 0x58, statusName: "ESME_RTHROTTLED" })).toContain(
      "ESME_RTHROTTLED"
    );
  });
});

describe("droppedNotice", () => {
  it("says nothing when nothing was dropped", () => {
    expect(droppedNotice(0)).toBe("");
  });

  it("names the number of entries dropped and why", () => {
    const notice = droppedNotice(1234);
    expect(notice).toContain("1234");
    expect(notice).toMatch(/cap|limit/i);
  });
});
