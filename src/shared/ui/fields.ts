import { DataCoding, RegisteredDelivery, EsmClass } from "../../smpp/constants";
import type { SegmentationMode, Gsm7Packing } from "../../smpp/text";
import type { BodyPlacement } from "../../smpp/segment";
import type { Option } from "./connection";

/** registered_delivery: 0-3 plus the 16-19 intermediate-notification variants. */
export const REGISTERED_DELIVERY: Option<number>[] = [
  { label: "0 — no receipt", value: RegisteredDelivery.NONE },
  { label: "1 — on failure and success", value: RegisteredDelivery.FAILURE_AND_SUCCESS },
  { label: "2 — on failure only", value: RegisteredDelivery.FAILURE_ONLY },
  { label: "3 — on success only", value: RegisteredDelivery.SUCCESS_ONLY },
  { label: "16 — intermediate, no receipt", value: RegisteredDelivery.INTERMEDIATE_NONE },
  {
    label: "17 — intermediate, on failure and success",
    value: RegisteredDelivery.INTERMEDIATE_FAILURE_AND_SUCCESS,
  },
  { label: "18 — intermediate, on failure only", value: RegisteredDelivery.INTERMEDIATE_FAILURE_ONLY },
  { label: "19 — intermediate, on success only", value: RegisteredDelivery.INTERMEDIATE_SUCCESS_ONLY },
];

/** esm_class: the four messaging modes, each with and without UDHI. */
export const ESM_CLASSES: Option<number>[] = [
  { label: "0 — Default", value: EsmClass.DEFAULT },
  { label: "1 — Datagram", value: EsmClass.DATAGRAM },
  { label: "2 — Forward / transaction", value: EsmClass.FORWARD },
  { label: "3 — Store and forward", value: EsmClass.STORE_AND_FORWARD },
  { label: "64 — Default + UDHI", value: EsmClass.DEFAULT_UDHI },
  { label: "65 — Datagram + UDHI", value: EsmClass.DATAGRAM_UDHI },
  { label: "66 — Forward / transaction + UDHI", value: EsmClass.FORWARD_UDHI },
  { label: "67 — Store and forward + UDHI", value: EsmClass.STORE_AND_FORWARD_UDHI },
];

export const PRIORITY_FLAGS: Option<number>[] = [
  { label: "0 — non-priority", value: 0 },
  { label: "1", value: 1 },
  { label: "2", value: 2 },
  { label: "3", value: 3 },
  { label: "4 — highest", value: 4 },
];

export const DATA_CODINGS: Option<number>[] = [
  { label: "0 — GSM 03.38 default", value: DataCoding.GSM_DEFAULT },
  { label: "1 — IA5 / ASCII", value: DataCoding.IA5_ASCII },
  { label: "2 — octet, unspecified", value: DataCoding.OCTET_UNSPECIFIED_2 },
  { label: "3 — Latin-1", value: DataCoding.LATIN_1 },
  { label: "4 — octet, 8-bit binary", value: DataCoding.OCTET_UNSPECIFIED_4 },
  { label: "5 — JIS", value: DataCoding.JIS },
  { label: "6 — Cyrillic", value: DataCoding.CYRILLIC },
  { label: "7 — Latin / Hebrew", value: DataCoding.LATIN_HEBREW },
  { label: "8 — UCS2", value: DataCoding.UCS2 },
];

export const SEGMENTATION_MODES: Option<SegmentationMode>[] = [
  { label: "None — one PDU", value: "none" },
  { label: "UDH concatenation", value: "udh" },
  { label: "sar_* TLVs", value: "sar" },
];

export const BODY_PLACEMENTS: Option<BodyPlacement>[] = [
  { label: "short_message", value: "short_message" },
  { label: "message_payload TLV", value: "message_payload" },
];

export const GSM7_PACKINGS: Option<Gsm7Packing>[] = [
  { label: "Unpacked (one septet per octet)", value: "unpacked" },
  { label: "Packed 7-bit", value: "packed" },
];

function pad2(n: number): string {
  return String(Math.abs(Math.trunc(n)) % 100).padStart(2, "0");
}

export interface RelativeOffset {
  years?: number;
  months?: number;
  days?: number;
  hours?: number;
  minutes?: number;
  seconds?: number;
}

/**
 * An SMPP relative time: `YYMMDDhhmmss` offsets, then tenths, then `00R`.
 * Sixteen characters, the trailing `R` marking it relative.
 */
export function relativeTime(o: RelativeOffset): string {
  return (
    pad2(o.years ?? 0) +
    pad2(o.months ?? 0) +
    pad2(o.days ?? 0) +
    pad2(o.hours ?? 0) +
    pad2(o.minutes ?? 0) +
    pad2(o.seconds ?? 0) +
    "000R"
  );
}

/**
 * An SMPP absolute time in UTC: `YYMMDDhhmmss`, tenths, then `00+` — the
 * quarter-hour offset from UTC being zero.
 */
export function absoluteTime(d: Date): string {
  return (
    pad2(d.getUTCFullYear() % 100) +
    pad2(d.getUTCMonth() + 1) +
    pad2(d.getUTCDate()) +
    pad2(d.getUTCHours()) +
    pad2(d.getUTCMinutes()) +
    pad2(d.getUTCSeconds()) +
    "000+"
  );
}

/** How far in the past "Historic submit time" points. */
export const HISTORIC_OFFSET_MS = 60 * 60 * 1000;

export interface TimeOption {
  label: string;
  /** Built fresh on each call, because the historic option depends on "now". */
  value: () => string;
}

export const SCHEDULE_DELIVERY: TimeOption[] = [
  { label: "Immediate", value: () => "" },
  { label: "+1 minute", value: () => relativeTime({ minutes: 1 }) },
  { label: "+2 minutes", value: () => relativeTime({ minutes: 2 }) },
  { label: "+5 minutes", value: () => relativeTime({ minutes: 5 }) },
  { label: "+15 minutes", value: () => relativeTime({ minutes: 15 }) },
  {
    label: "Historic submit time",
    value: () => absoluteTime(new Date(Date.now() - HISTORIC_OFFSET_MS)),
  },
];

export const VALIDITY_PERIODS: TimeOption[] = [
  { label: "Default", value: () => "" },
  { label: "1 minute", value: () => relativeTime({ minutes: 1 }) },
  { label: "30 minutes", value: () => relativeTime({ minutes: 30 }) },
  { label: "1 hour", value: () => relativeTime({ hours: 1 }) },
  { label: "4 hours", value: () => relativeTime({ hours: 4 }) },
  { label: "1 day", value: () => relativeTime({ days: 1 }) },
];

export function timeFor(options: TimeOption[], label: string): string {
  const found = options.find((o) => o.label === label);
  return found ? found.value() : "";
}
