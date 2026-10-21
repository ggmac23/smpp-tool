import { describe, it, expect } from "vitest";
import {
  formatStats,
  checkBurstLimits,
  burstColumns,
  formatBurstRow,
  BURST_COLUMNS_WITH_DLR,
  BURST_COLUMNS_NO_DLR,
} from "../../src/shared/ui/stats";
import { MAX_BURST, MAX_RATE } from "../../src/main/recipients";
import type { BurstStats, BurstRow } from "../../src/main/burst";

const stats: BurstStats = {
  messages: 100,
  pdus: 140,
  submitted: 140,
  accepted: 120,
  errorsByStatus: { ESME_RTHROTTLED: 15, ESME_RINVDSTADR: 5 },
  achievedRatePerSec: 187.4,
  latencyP50: 12,
  latencyP95: 48,
  latencyP99: 130,
  dlrReceived: 95,
  dlrUnmatched: 3,
};

const find = (label: string) => formatStats(stats).find((e) => e.label === label)?.value;

describe("formatStats", () => {
  it("separates messages from PDUs, because a 3-part message is one of each", () => {
    expect(find("Messages")).toBe("100");
    expect(find("PDUs")).toBe("140");
  });

  it("reports submitted and accepted", () => {
    expect(find("Submitted")).toBe("140");
    expect(find("Accepted")).toBe("120");
  });

  it("formats errors by status as one entry per status name", () => {
    const entries = formatStats(stats);
    expect(entries.find((e) => e.label === "ESME_RTHROTTLED")?.value).toBe("15");
    expect(entries.find((e) => e.label === "ESME_RINVDSTADR")?.value).toBe("5");
  });

  it("formats an achieved rate below the requested rate without rounding it to the request", () => {
    expect(find("Achieved rate")).toBe("187.4/s");
  });

  it("formats percentiles in milliseconds", () => {
    expect(find("Latency p50")).toBe("12 ms");
    expect(find("Latency p95")).toBe("48 ms");
    expect(find("Latency p99")).toBe("130 ms");
  });

  it("reports delivery receipts received and unmatched", () => {
    expect(find("DLRs received")).toBe("95");
    expect(find("DLRs unmatched")).toBe("3");
  });

  it("says plainly when no errors came back", () => {
    const clean = formatStats({ ...stats, errorsByStatus: {} });
    expect(clean.find((e) => e.label === "Errors")?.value).toBe("none");
  });

  it("writes a zero achieved rate rather than dividing by nothing", () => {
    expect(formatStats({ ...stats, achievedRatePerSec: 0 }).find((e) => e.label === "Achieved rate")?.value).toBe(
      "0.0/s"
    );
  });
});

describe("checkBurstLimits", () => {
  it("accepts a count at exactly MAX_BURST", () => {
    expect(checkBurstLimits(MAX_BURST, 100)).toEqual({ ok: true });
  });

  it("accepts a rate at exactly MAX_RATE", () => {
    expect(checkBurstLimits(10, MAX_RATE)).toEqual({ ok: true });
  });

  it("reports a count above MAX_BURST as a refusal with the limit in the message", () => {
    const result = checkBurstLimits(MAX_BURST + 1, 100);
    expect(result.ok).toBe(false);
    expect(result.reason).toContain(String(MAX_BURST));
  });

  it("reports a rate above MAX_RATE as a refusal with the limit in the message", () => {
    const result = checkBurstLimits(10, MAX_RATE + 1);
    expect(result.ok).toBe(false);
    expect(result.reason).toContain(String(MAX_RATE));
  });

  it("refuses a count of zero", () => {
    expect(checkBurstLimits(0, 100).ok).toBe(false);
  });

  it("refuses a rate of zero", () => {
    expect(checkBurstLimits(10, 0).ok).toBe(false);
  });

  it("names the count when both are over, so the user fixes the bigger problem first", () => {
    expect(checkBurstLimits(MAX_BURST + 1, MAX_RATE + 1).reason).toContain(String(MAX_BURST));
  });
});

describe("burstColumns", () => {
  it("shows the DLR columns under a transceiver bind", () => {
    expect(burstColumns("transceiver")).toEqual(BURST_COLUMNS_WITH_DLR);
    expect(burstColumns("receiver")).toEqual(BURST_COLUMNS_WITH_DLR);
  });

  it("omits the DLR columns under a transmitter-only bind", () => {
    const columns = burstColumns("transmitter");
    expect(columns).toEqual(BURST_COLUMNS_NO_DLR);
    expect(columns.join(" ")).not.toMatch(/DLR/i);
  });

  it("omits the DLR columns when nothing is bound", () => {
    expect(burstColumns(null)).toEqual(BURST_COLUMNS_NO_DLR);
  });
});

describe("formatBurstRow", () => {
  const row: BurstRow = {
    id: "r0",
    recipient: "393331234567",
    body: "hello",
    pdus: 3,
    messageId: "A7F2",
    commandStatus: 0,
    submitLatencyMs: 14,
    dlrState: "DELIVRD",
    dlrLatencyMs: 220,
  };

  it("renders every column of a finished row", () => {
    expect(formatBurstRow(row, BURST_COLUMNS_WITH_DLR)).toEqual([
      "393331234567",
      "hello",
      "3",
      "A7F2",
      "ESME_ROK",
      "14",
      "DELIVRD",
      "220",
    ]);
  });

  it("leaves a column blank while its value has not arrived", () => {
    const pending = formatBurstRow(
      { ...row, messageId: null, commandStatus: null, submitLatencyMs: null, dlrState: null, dlrLatencyMs: null },
      BURST_COLUMNS_WITH_DLR
    );
    expect(pending.slice(3)).toEqual(["", "", "", "", ""]);
  });

  it("names a timeout rather than showing a negative status number", () => {
    expect(formatBurstRow({ ...row, commandStatus: -1 }, BURST_COLUMNS_WITH_DLR)[4]).toMatch(
      /timeout|no response/i
    );
  });

  it("drops the DLR cells when the columns do not include them", () => {
    expect(formatBurstRow(row, BURST_COLUMNS_NO_DLR)).toHaveLength(BURST_COLUMNS_NO_DLR.length);
  });
});
