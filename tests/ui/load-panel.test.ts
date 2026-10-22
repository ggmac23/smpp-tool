import { describe, it, expect } from "vitest";
import {
  MAX_CONNECTIONS,
  LOAD_MODES,
  checkLoadLimits,
  buildLoadMode,
  formatLoadReport,
  formatLoadProgress,
  formatRampStep,
  CONNECTION_COLUMNS,
  formatConnectionRow,
  endReasonText,
  type LoadFormValues,
  type LoadReport,
} from "../../src/shared/ui/load";

const FORM: LoadFormValues = {
  connections: 4,
  messages: 1000,
  modeLabel: "Fixed rate",
  ratePerSec: 200,
  startRate: 50,
  step: 50,
  stepSeconds: 10,
  maxRate: 500,
  errorSharePercent: 2,
  latencyP95Ms: 2000,
};

const REPORT: LoadReport = {
  connections: 4,
  requestedRatePerSec: 200,
  achievedRatePerSec: 187.4,
  sustainedRatePerSec: 150,
  messages: 1000,
  pdus: 1400,
  submitted: 1400,
  accepted: 1380,
  errorsByStatus: { ESME_RTHROTTLED: 20 },
  latencyP50: 12,
  latencyP95: 48,
  latencyP99: 130,
  durationMs: 7470,
  perConnection: [
    {
      index: 0,
      bound: true,
      bindError: null,
      submitted: 350,
      accepted: 350,
      errorsByStatus: {},
      latencyP50: 11,
      latencyP95: 40,
      latencyP99: 90,
      lost: false,
    },
    {
      index: 1,
      bound: false,
      bindError: "ESME_RBINDFAIL",
      submitted: 0,
      accepted: 0,
      errorsByStatus: {},
      latencyP50: 0,
      latencyP95: 0,
      latencyP99: 0,
      lost: false,
    },
  ],
  steps: [],
  ended: "breached",
};

const find = (label: string) => formatLoadReport(REPORT).find((e) => e.label === label)?.value;

describe("load-test limits", () => {
  it("refuses more connections than this build opens at once, naming the limit", () => {
    const check = checkLoadLimits({ ...FORM, connections: MAX_CONNECTIONS + 1 });
    expect(check.ok).toBe(false);
    expect(check.reason).toContain(String(MAX_CONNECTIONS));
  });

  it("refuses zero connections: there would be nothing to send on", () => {
    expect(checkLoadLimits({ ...FORM, connections: 0 }).ok).toBe(false);
  });

  it("refuses more messages than the build will hold", () => {
    const check = checkLoadLimits({ ...FORM, messages: 50001 });
    expect(check.ok).toBe(false);
    expect(check.reason).toContain("50000");
  });

  it("refuses a rate above what the connections can carry, and says so", () => {
    // The ceiling is per connection, so four connections carry four times it.
    const check = checkLoadLimits({ ...FORM, connections: 1, ratePerSec: 5001 });
    expect(check.ok).toBe(false);
    expect(check.reason).toMatch(/5000/);
  });

  it("allows the rate the connections together can carry", () => {
    expect(checkLoadLimits({ ...FORM, connections: 4, ratePerSec: 20000 }).ok).toBe(true);
  });

  it("checks the ramp's top rate, not its starting one", () => {
    const check = checkLoadLimits({
      ...FORM,
      modeLabel: "Ramp until it breaks",
      connections: 1,
      startRate: 10,
      maxRate: 9000,
    });
    expect(check.ok).toBe(false);
  });

  it("refuses a ramp that never climbs", () => {
    const check = checkLoadLimits({ ...FORM, modeLabel: "Ramp until it breaks", step: 0 });
    expect(check.ok).toBe(false);
    expect(check.reason).toMatch(/step/i);
  });

  it("accepts the default form", () => {
    expect(checkLoadLimits(FORM)).toEqual({ ok: true, reason: "" });
  });
});

describe("buildLoadMode", () => {
  it("builds a fixed mode from the rate field", () => {
    expect(buildLoadMode(FORM)).toEqual({ kind: "fixed", ratePerSec: 200 });
  });

  it("builds a ramp with its thresholds, turning the percentage into a share", () => {
    expect(buildLoadMode({ ...FORM, modeLabel: "Ramp until it breaks" })).toEqual({
      kind: "ramp",
      startRate: 50,
      step: 50,
      stepDurationMs: 10000,
      maxRate: 500,
      abandonOn: { errorShare: 0.02, latencyP95Ms: 2000 },
    });
  });

  it("offers exactly the two modes the engine has", () => {
    expect(LOAD_MODES.map((m) => m.value)).toEqual(["fixed", "ramp"]);
  });
});

describe("formatLoadReport", () => {
  it("leads with the sustained rate, which is the number the test is for", () => {
    expect(formatLoadReport(REPORT)[0].label).toBe("Sustained rate");
    expect(find("Sustained rate")).toBe("150/s");
  });

  it("keeps the requested, achieved and sustained rates apart", () => {
    expect(find("Requested rate")).toBe("200/s");
    expect(find("Achieved rate")).toBe("187.4/s");
  });

  it("separates messages from PDUs, because a 3-part message is one of each", () => {
    expect(find("Messages")).toBe("1000");
    expect(find("PDUs")).toBe("1400");
  });

  it("reports submitted and accepted", () => {
    expect(find("Submitted")).toBe("1400");
    expect(find("Accepted")).toBe("1380");
  });

  it("names each error status with its count", () => {
    expect(find("ESME_RTHROTTLED")).toBe("20");
  });

  it("formats the percentiles in milliseconds", () => {
    expect(find("Latency p50")).toBe("12 ms");
    expect(find("Latency p95")).toBe("48 ms");
    expect(find("Latency p99")).toBe("130 ms");
  });

  it("reports how long the run took and how many connections carried it", () => {
    expect(find("Duration")).toBe("7.47 s");
    expect(find("Connections")).toBe("4");
  });

  it("says in words why the run ended", () => {
    expect(find("Ended")).toMatch(/gave way|breached/i);
  });
});

describe("endReasonText", () => {
  it("has words for every reason the engine reports", () => {
    for (const reason of [
      "complete",
      "stopped",
      "breached",
      "reached-max-rate",
      "no-connections",
      "connections-lost",
    ] as const) {
      expect(endReasonText(reason), reason).toBeTruthy();
      expect(endReasonText(reason), reason).not.toContain("-");
    }
  });

  it("falls back to the raw reason rather than showing nothing", () => {
    expect(endReasonText("something-new" as never)).toBe("something-new");
  });
});

describe("the per-connection table", () => {
  it("has a column for the things that differ between connections", () => {
    expect(CONNECTION_COLUMNS).toEqual([
      "#",
      "Bound",
      "Submitted",
      "Accepted",
      "p95",
      "Errors",
      "Note",
    ]);
  });

  it("renders a healthy connection", () => {
    expect(formatConnectionRow(REPORT.perConnection[0], CONNECTION_COLUMNS)).toEqual([
      "0",
      "yes",
      "350",
      "350",
      "40 ms",
      "0",
      "",
    ]);
  });

  it("shows why a connection never bound, since that is the whole story for it", () => {
    const row = formatConnectionRow(REPORT.perConnection[1], CONNECTION_COLUMNS);
    expect(row[1]).toBe("no");
    expect(row[6]).toContain("ESME_RBINDFAIL");
  });
});

describe("live progress", () => {
  it("reads out what has gone and at what rate", () => {
    expect(
      formatLoadProgress({
        submitted: 400,
        total: 1000,
        currentRatePerSec: 200,
        achievedRatePerSec: 193.5,
        errors: 7,
      })
    ).toBe("400 / 1000 · 200/s requested · 193.5/s achieved · 7 errors");
  });
});

describe("ramp steps", () => {
  it("marks the step that gave way", () => {
    const line = formatRampStep({
      requestedRate: 250,
      achievedRate: 240.2,
      submitted: 2400,
      accepted: 2100,
      errorShare: 0.125,
      latencyP95: 3100,
      breached: true,
      reason: "error share 12.5% above 2%",
    });
    expect(line).toContain("250/s");
    expect(line).toContain("gave way");
    expect(line).toContain("error share 12.5% above 2%");
  });

  it("reads a step that held as holding", () => {
    const line = formatRampStep({
      requestedRate: 100,
      achievedRate: 99.8,
      submitted: 1000,
      accepted: 1000,
      errorShare: 0,
      latencyP95: 30,
      breached: false,
      reason: null,
    });
    expect(line).toContain("held");
  });
});

describe("a ramp's report", () => {
  const ramped: LoadReport = {
    ...REPORT,
    requestedRatePerSec: 240,
    sustainedRatePerSec: 0,
    steps: [
      {
        requestedRate: 60,
        achievedRate: 45.6,
        submitted: 46,
        accepted: 30,
        errorShare: 0.35,
        latencyP95: 1,
        breached: true,
        reason: "35% of submits came back with an error, above the 2% allowed",
      },
    ],
  };

  it("calls the ramp's ceiling a ceiling, not a rate that was requested", () => {
    // A ramp that broke at 60/s never asked for 240/s. Calling 240 the
    // requested rate reads as though the SMSC nearly managed it.
    const labels = formatLoadReport(ramped).map((e) => e.label);
    expect(labels).toContain("Top rate allowed");
    expect(labels).not.toContain("Requested rate");
  });

  it("still leads with the sustained rate, zero and all", () => {
    const first = formatLoadReport(ramped)[0];
    expect(first.label).toBe("Sustained rate");
    expect(first.value).toBe("0/s");
  });

  it("keeps calling it the requested rate on a fixed-rate run", () => {
    expect(formatLoadReport(REPORT).map((e) => e.label)).toContain("Requested rate");
  });
});
