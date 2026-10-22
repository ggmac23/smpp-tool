/**
 * The load-test section: what the form means, and how its report reads.
 *
 * A load test answers one question — how much does this SMSC take before it
 * gives way — so the report leads with the sustained rate and everything else
 * is there to explain it.
 *
 * The limits here are this build's, not the protocol's, and the backend
 * enforces them again. Checking in the renderer is only so the refusal
 * arrives before a single bind goes out.
 */
import { MAX_BURST, MAX_RATE } from "./protocol";
import type { LimitCheck, StatEntry } from "./stats";

/** Connections this build opens at once. The backend holds the same number. */
export const MAX_CONNECTIONS = 16;

export const LOAD_MODES = [
  { label: "Fixed rate", value: "fixed" },
  { label: "Ramp until it breaks", value: "ramp" },
] as const;

export type LoadModeKind = (typeof LOAD_MODES)[number]["value"];

export interface LoadFormValues {
  connections: number;
  messages: number;
  modeLabel: string;
  /** Fixed mode. */
  ratePerSec: number;
  /** Ramp mode. */
  startRate: number;
  step: number;
  stepSeconds: number;
  maxRate: number;
  /** The share of non-ROK responses that counts as giving way, as a percent. */
  errorSharePercent: number;
  /** The p95 latency that counts as giving way. */
  latencyP95Ms: number;
}

export type LoadMode =
  | { kind: "fixed"; ratePerSec: number }
  | {
      kind: "ramp";
      startRate: number;
      step: number;
      stepDurationMs: number;
      maxRate: number;
      abandonOn: { errorShare: number; latencyP95Ms: number };
    };

export interface ConnectionStats {
  index: number;
  bound: boolean;
  bindError: string | null;
  submitted: number;
  accepted: number;
  errorsByStatus: Record<string, number>;
  latencyP50: number;
  latencyP95: number;
  latencyP99: number;
  lost: boolean;
}

export interface RampStep {
  requestedRate: number;
  achievedRate: number;
  submitted: number;
  accepted: number;
  errorShare: number;
  latencyP95: number;
  breached: boolean;
  reason: string | null;
}

export type LoadEndReason =
  | "complete"
  | "stopped"
  | "breached"
  | "reached-max-rate"
  | "no-connections"
  | "connections-lost";

export interface LoadReport {
  connections: number;
  requestedRatePerSec: number;
  achievedRatePerSec: number;
  sustainedRatePerSec: number;
  messages: number;
  pdus: number;
  submitted: number;
  accepted: number;
  errorsByStatus: Record<string, number>;
  latencyP50: number;
  latencyP95: number;
  latencyP99: number;
  durationMs: number;
  perConnection: ConnectionStats[];
  steps: RampStep[];
  ended: LoadEndReason;
}

export interface LoadProgress {
  submitted: number;
  total: number;
  currentRatePerSec: number;
  achievedRatePerSec: number;
  errors: number;
}

function kindOf(modeLabel: string): LoadModeKind {
  const mode = LOAD_MODES.find((m) => m.label === modeLabel);
  return mode ? mode.value : "fixed";
}

/**
 * Whether the form can be started. Refusing here, with the limit named, beats
 * opening sixteen binds and failing on the first send.
 */
export function checkLoadLimits(v: LoadFormValues): LimitCheck {
  const refuse = (reason: string): LimitCheck => ({ ok: false, reason });

  if (v.connections < 1) {
    return refuse("At least one connection: there would be nothing to send on.");
  }
  if (v.connections > MAX_CONNECTIONS) {
    return refuse(
      `${v.connections} connections is above the ${MAX_CONNECTIONS} this build opens at once.`
    );
  }
  if (v.messages < 1) {
    return refuse("At least one message.");
  }
  if (v.messages > MAX_BURST) {
    return refuse(`${v.messages} messages is above the ${MAX_BURST} this build holds.`);
  }

  // The ceiling is per connection, so connections multiply it.
  const ceiling = MAX_RATE * v.connections;
  const kind = kindOf(v.modeLabel);

  if (kind === "fixed") {
    if (v.ratePerSec < 1) {
      return refuse("At least one message per second.");
    }
    if (v.ratePerSec > ceiling) {
      return refuse(
        `${v.ratePerSec}/s is above what ${v.connections} connection(s) carry in this ` +
          `build: ${MAX_RATE}/s each, so ${ceiling}/s.`
      );
    }
    return { ok: true, reason: "" };
  }

  if (v.startRate < 1) {
    return refuse("The ramp has to start at one message per second or more.");
  }
  if (v.step < 1) {
    return refuse("A ramp with a step of 0 never climbs: give it a step.");
  }
  if (v.maxRate < v.startRate) {
    return refuse("The ramp's top rate is below where it starts.");
  }
  if (v.maxRate > ceiling) {
    return refuse(
      `A top rate of ${v.maxRate}/s is above what ${v.connections} connection(s) carry in ` +
        `this build: ${MAX_RATE}/s each, so ${ceiling}/s.`
    );
  }
  if (v.stepSeconds < 1) {
    return refuse("Each step needs at least a second to measure anything.");
  }
  if (v.errorSharePercent <= 0 && v.latencyP95Ms <= 0) {
    return refuse(
      'A ramp needs a definition of "gave way": set an error share, a p95 latency, or both.'
    );
  }
  return { ok: true, reason: "" };
}

export function buildLoadMode(v: LoadFormValues): LoadMode {
  if (kindOf(v.modeLabel) === "fixed") {
    return { kind: "fixed", ratePerSec: v.ratePerSec };
  }
  return {
    kind: "ramp",
    startRate: v.startRate,
    step: v.step,
    stepDurationMs: v.stepSeconds * 1000,
    maxRate: v.maxRate,
    abandonOn: {
      errorShare: v.errorSharePercent / 100,
      latencyP95Ms: v.latencyP95Ms,
    },
  };
}

const END_REASONS: Record<string, string> = {
  complete: "every message sent",
  stopped: "stopped by you",
  breached: "the SMSC gave way",
  "reached-max-rate": "reached the top rate without giving way",
  "no-connections": "not one connection bound",
  "connections-lost": "every connection dropped",
};

export function endReasonText(reason: LoadEndReason): string {
  return END_REASONS[reason] || reason;
}

function rate(n: number): string {
  return `${Number.isInteger(n) ? n : n.toFixed(1)}/s`;
}

/**
 * The report, leading with the number the test exists to produce.
 *
 * Requested, achieved and sustained stay apart on purpose: asking for 500,
 * managing 480 and holding 150 without errors are three different facts, and
 * collapsing them would hide the only one that matters.
 */
export function formatLoadReport(r: LoadReport): StatEntry[] {
  // A ramp's "requested rate" is the ceiling it was allowed to climb to, and
  // a run that broke at 60/s never asked for 240/s: calling that the
  // requested rate reads as though the SMSC nearly managed it.
  const ramped = (r.steps || []).length > 0;

  const entries: StatEntry[] = [
    { label: "Sustained rate", value: rate(r.sustainedRatePerSec) },
    {
      label: ramped ? "Top rate allowed" : "Requested rate",
      value: rate(r.requestedRatePerSec),
    },
    { label: "Achieved rate", value: rate(r.achievedRatePerSec) },
    { label: "Connections", value: String(r.connections) },
    { label: "Messages", value: String(r.messages) },
    { label: "PDUs", value: String(r.pdus) },
    { label: "Submitted", value: String(r.submitted) },
    { label: "Accepted", value: String(r.accepted) },
    { label: "Latency p50", value: `${r.latencyP50} ms` },
    { label: "Latency p95", value: `${r.latencyP95} ms` },
    { label: "Latency p99", value: `${r.latencyP99} ms` },
    { label: "Duration", value: `${(r.durationMs / 1000).toFixed(2)} s` },
    { label: "Ended", value: endReasonText(r.ended) },
  ];

  for (const [status, count] of Object.entries(r.errorsByStatus)) {
    entries.push({ label: status, value: String(count) });
  }
  return entries;
}

export function formatLoadProgress(p: LoadProgress): string {
  return (
    `${p.submitted} / ${p.total} · ${p.currentRatePerSec}/s requested · ` +
    `${rate(p.achievedRatePerSec).replace("/s", "")}/s achieved · ${p.errors} errors`
  );
}

export function formatRampStep(step: RampStep): string {
  const verdict = step.breached ? "gave way" : "held";
  const because = step.breached && step.reason ? ` (${step.reason})` : "";
  return (
    `${step.requestedRate}/s requested, ${rate(step.achievedRate)} achieved, ` +
    `p95 ${step.latencyP95} ms — ${verdict}${because}`
  );
}

export const CONNECTION_COLUMNS = [
  "#",
  "Bound",
  "Submitted",
  "Accepted",
  "p95",
  "Errors",
  "Note",
];

function errorCount(errors: Record<string, number>): number {
  return Object.values(errors).reduce((a, b) => a + b, 0);
}

function note(c: ConnectionStats): string {
  if (c.bindError) return c.bindError;
  if (c.lost) return "connection lost";
  return "";
}

/** One row of the per-connection table, in the order `columns` names. */
export function formatConnectionRow(c: ConnectionStats, columns: string[]): string[] {
  const cells: Record<string, string> = {
    "#": String(c.index),
    Bound: c.bound ? "yes" : "no",
    Submitted: String(c.submitted),
    Accepted: String(c.accepted),
    p95: `${c.latencyP95} ms`,
    Errors: String(errorCount(c.errorsByStatus)),
    Note: note(c),
  };
  return columns.map((column) => cells[column] ?? "");
}
