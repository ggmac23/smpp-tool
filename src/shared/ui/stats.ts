import { MAX_BURST, MAX_RATE } from "../../main/recipients";
import { statusName } from "../../smpp/constants";
import type { BurstStats, BurstRow } from "../../main/burst";
import type { BindMode } from "../../main/session";

export interface StatEntry {
  label: string;
  value: string;
}

/**
 * The aggregates, in reading order. Messages and PDUs are separate figures:
 * a three-part message is one message and three PDUs, and conflating them
 * would make the throughput number a lie.
 */
export function formatStats(s: BurstStats): StatEntry[] {
  const entries: StatEntry[] = [
    { label: "Messages", value: String(s.messages) },
    { label: "PDUs", value: String(s.pdus) },
    { label: "Submitted", value: String(s.submitted) },
    { label: "Accepted", value: String(s.accepted) },
    { label: "Achieved rate", value: `${s.achievedRatePerSec.toFixed(1)}/s` },
    { label: "Latency p50", value: `${Math.round(s.latencyP50)} ms` },
    { label: "Latency p95", value: `${Math.round(s.latencyP95)} ms` },
    { label: "Latency p99", value: `${Math.round(s.latencyP99)} ms` },
    { label: "DLRs received", value: String(s.dlrReceived) },
    { label: "DLRs unmatched", value: String(s.dlrUnmatched) },
  ];

  const statuses = Object.keys(s.errorsByStatus).sort();
  if (statuses.length === 0) {
    entries.push({ label: "Errors", value: "none" });
  } else {
    for (const name of statuses) {
      entries.push({ label: name, value: String(s.errorsByStatus[name]) });
    }
  }

  return entries;
}

export interface LimitCheck {
  ok: boolean;
  reason?: string;
}

/**
 * Refuse before anything is sent, with the limit in the message.
 *
 * The ceiling is what this build holds in memory, not a protocol limit, so
 * the refusal says so rather than pretending the SMSC would object.
 */
export function checkBurstLimits(count: number, ratePerSec: number): LimitCheck {
  if (count > MAX_BURST) {
    return {
      ok: false,
      reason: `${count} messages is above the ${MAX_BURST} this build holds in memory. Lower the count.`,
    };
  }
  if (count < 1) {
    return { ok: false, reason: "A burst needs at least one message." };
  }
  if (ratePerSec > MAX_RATE) {
    return {
      ok: false,
      reason: `${ratePerSec}/sec is above the ${MAX_RATE}/sec ceiling of this build. Lower the rate.`,
    };
  }
  if (ratePerSec < 1) {
    return { ok: false, reason: "The rate must be at least 1 message per second." };
  }
  return { ok: true };
}

export const BURST_COLUMNS_NO_DLR = [
  "Recipient",
  "Body",
  "PDUs",
  "message_id",
  "Status",
  "Submit ms",
];

export const BURST_COLUMNS_WITH_DLR = [...BURST_COLUMNS_NO_DLR, "DLR", "DLR ms"];

/**
 * Delivery receipts arrive on a receiver path, so a transmitter-only bind can
 * never fill those columns. Leaving them out beats showing columns that stay
 * permanently empty.
 */
export function burstColumns(mode: BindMode | null): string[] {
  return mode === "transceiver" || mode === "receiver"
    ? BURST_COLUMNS_WITH_DLR
    : BURST_COLUMNS_NO_DLR;
}

const TIMEOUT_STATUS = -1;

function statusCell(commandStatus: number | null): string {
  if (commandStatus === null) return "";
  if (commandStatus === TIMEOUT_STATUS) return "no response";
  return statusName(commandStatus);
}

function blankIfNull(n: number | null): string {
  return n === null ? "" : String(n);
}

/** One table row, in the order `columns` names. */
export function formatBurstRow(row: BurstRow, columns: string[]): string[] {
  const cells: Record<string, string> = {
    Recipient: row.recipient,
    Body: row.body,
    PDUs: String(row.pdus),
    message_id: row.messageId ?? "",
    Status: statusCell(row.commandStatus),
    "Submit ms": blankIfNull(row.submitLatencyMs),
    DLR: row.dlrState ?? "",
    "DLR ms": blankIfNull(row.dlrLatencyMs),
  };
  return columns.map((c) => cells[c] ?? "");
}

export { MAX_BURST, MAX_RATE };
