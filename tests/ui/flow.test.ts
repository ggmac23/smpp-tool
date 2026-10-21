import { describe, it, expect } from "vitest";
import {
  formatOffset,
  flowSteps,
  arrowColour,
  buildFlowSvg,
  FlowInput,
} from "../../src/shared/ui/flow";

const base = Date.UTC(2026, 9, 21, 12, 0, 0);

const rows: FlowInput[] = [
  { at: base, direction: "out", commandName: "submit_sm", sequenceNumber: 1 },
  { at: base + 113, direction: "in", commandName: "submit_sm_resp", sequenceNumber: 1 },
  { at: base + 1131, direction: "in", commandName: "deliver_sm", sequenceNumber: 900 },
  { at: base + 1131, direction: "out", commandName: "deliver_sm_resp", sequenceNumber: 900 },
];

describe("formatOffset", () => {
  it("writes seconds with six decimals, like a packet trace", () => {
    expect(formatOffset(0)).toBe("0.000000");
    expect(formatOffset(0.113201)).toBe("0.113201");
    expect(formatOffset(5.1439)).toBe("5.143900");
  });

  it("never writes a negative zero", () => {
    expect(formatOffset(-0)).toBe("0.000000");
  });
});

describe("flowSteps", () => {
  it("measures the first step from itself, so it reads zero", () => {
    const steps = flowSteps(rows);
    expect(steps[0].offsetFirst).toBe("0.000000");
    expect(steps[0].offsetPrev).toBe("0.000000");
  });

  it("measures each later step from the first packet and from the previous one", () => {
    const steps = flowSteps(rows);
    expect(steps[1].offsetFirst).toBe("0.113000");
    expect(steps[1].offsetPrev).toBe("0.113000");
    expect(steps[2].offsetFirst).toBe("1.131000");
    expect(steps[2].offsetPrev).toBe("1.018000");
  });

  it("keeps the command name and sequence number on each step", () => {
    const steps = flowSteps(rows);
    expect(steps[0].label).toBe("submit_sm");
    expect(steps[0].sequenceNumber).toBe(1);
  });

  it("points a sent PDU to the right and a received one to the left", () => {
    const steps = flowSteps(rows);
    expect(steps[0].toRight).toBe(true);
    expect(steps[1].toRight).toBe(false);
  });

  it("labels an undecodable PDU as such instead of leaving it blank", () => {
    const steps = flowSteps([
      { at: base, direction: "in", commandName: "(undecodable)", sequenceNumber: 0, decodeError: "bad length" },
    ]);
    expect(steps[0].label).toMatch(/undecodable|decode/i);
  });

  it("returns nothing for no rows", () => {
    expect(flowSteps([])).toEqual([]);
  });
});

describe("arrowColour", () => {
  it("gives submits, receipts and responses each their own colour", () => {
    const submit = arrowColour("submit_sm");
    const resp = arrowColour("submit_sm_resp");
    const deliver = arrowColour("deliver_sm");
    expect(new Set([submit, resp, deliver]).size).toBe(3);
  });

  it("colours every response the same way", () => {
    expect(arrowColour("submit_sm_resp")).toBe(arrowColour("query_sm_resp"));
  });

  it("has a colour for a command it does not know", () => {
    expect(arrowColour("unknown(0x4242)")).toMatch(/^#[0-9a-f]{6}$/i);
  });
});

describe("buildFlowSvg", () => {
  it("draws one arrow per PDU", () => {
    const svg = buildFlowSvg(rows);
    expect(svg.match(/<line [^>]*class="flow-arrow"/g) ?? []).toHaveLength(4);
  });

  it("writes both offset columns for every step", () => {
    const svg = buildFlowSvg(rows);
    expect(svg).toContain("0.113000");
    expect(svg).toContain("1.018000");
  });

  it("labels each arrow with its command", () => {
    const svg = buildFlowSvg(rows);
    for (const name of ["submit_sm", "submit_sm_resp", "deliver_sm", "deliver_sm_resp"]) {
      expect(svg, name).toContain(">" + name + "<");
    }
  });

  it("names the two sides", () => {
    const svg = buildFlowSvg(rows, { leftLabel: "ESME", rightLabel: "smsc.example.com:2775" });
    expect(svg).toContain("ESME");
    expect(svg).toContain("smsc.example.com:2775");
  });

  it("escapes a label that contains markup characters", () => {
    const svg = buildFlowSvg([
      { at: base, direction: "out", commandName: "<script>&", sequenceNumber: 1 },
    ]);
    expect(svg).not.toContain("<script>");
    expect(svg).toContain("&lt;script&gt;&amp;");
  });

  it("grows its height with the number of steps", () => {
    const tall = buildFlowSvg(
      Array.from({ length: 40 }, (_, i) => ({
        at: base + i * 10,
        direction: "out" as const,
        commandName: "submit_sm",
        sequenceNumber: i,
      }))
    );
    const short = buildFlowSvg(rows);
    const heightOf = (svg: string) => Number(/height="(\d+)"/.exec(svg)![1]);
    expect(heightOf(tall)).toBeGreaterThan(heightOf(short));
  });

  it("says so plainly when there is nothing to draw", () => {
    const svg = buildFlowSvg([]);
    expect(svg).toMatch(/nothing|no PDU/i);
  });

  it("produces a well-formed svg root", () => {
    const svg = buildFlowSvg(rows);
    expect(svg.startsWith("<svg")).toBe(true);
    expect(svg.trimEnd().endsWith("</svg>")).toBe(true);
  });
});
