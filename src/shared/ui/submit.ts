import { TON_NPI_PRESETS } from "./connection";
import { SCHEDULE_DELIVERY, VALIDITY_PERIODS, timeFor } from "./fields";
import {
  CommandId,
  DataCoding,
  SAMPLES,
  TLV_MESSAGE_PAYLOAD,
  type BodyPlacement,
  type Gsm7Packing,
  type Measurement,
  type SegmentationMode,
} from "./protocol";

export interface TlvRow {
  /** Tag as hex, with or without a 0x prefix. */
  tagHex: string;
  /** Value as hex, when the user typed hex. */
  valueHex?: string;
  /** Value as text, when the user typed text. Latin-1 encoded by the backend. */
  valueText?: string;
}

export interface SubmitFormValues {
  serviceType: string;
  sourceTonNpiLabel: string;
  sourceAddr: string;
  destTonNpiLabel: string;
  destinationAddr: string;
  esmClass: number;
  protocolId: number;
  priorityFlag: number;
  registeredDelivery: number;
  scheduleDeliveryLabel: string;
  validityPeriodLabel: string;
  dataCoding: number;
  replaceIfPresent: number;
  smDefaultMsgId: number;
  text: string;
  placement: BodyPlacement;
  segmentation: SegmentationMode;
  packing: Gsm7Packing;
  /** A hand-written UDH. Its presence turns automatic concatenation off. */
  udhHex: string;
  tlvs: TlvRow[];
}

/**
 * The submit form as `compose_submit` wants it.
 *
 * The labels the widgets hold become the numbers and time strings the
 * protocol uses; everything else passes straight through. No segmenting, no
 * encoding, no validation: the backend composes the PDUs and returns the
 * warnings, so there is exactly one place that decides what the form means.
 */
export interface ComposeArgs {
  serviceType: string;
  sourceAddrTon: number;
  sourceAddrNpi: number;
  sourceAddr: string;
  destAddrTon: number;
  destAddrNpi: number;
  destinationAddr: string;
  esmClass: number;
  protocolId: number;
  priorityFlag: number;
  registeredDelivery: number;
  scheduleDeliveryTime: string;
  validityPeriod: string;
  dataCoding: number;
  replaceIfPresent: number;
  smDefaultMsgId: number;
  text: string;
  placement: BodyPlacement;
  segmentation: SegmentationMode;
  packing: Gsm7Packing;
  udhHex: string;
  tlvs: TlvRow[];
}

/** What `compose_submit` gives back. */
export interface ComposedSubmit {
  requests: {
    commandId: number;
    fields: Record<string, string | number>;
    octetFields: Record<string, string>;
    tlvs: { tag: number; hex: string }[];
  }[];
  warnings: string[];
  measurement: Measurement;
}

function tonNpi(label: string): { ton: number; npi: number } {
  const preset = TON_NPI_PRESETS.find((p) => p.label === label);
  return preset ? { ton: preset.ton, npi: preset.npi } : { ton: 0, npi: 0 };
}

export function buildComposeArgs(v: SubmitFormValues): ComposeArgs {
  const source = tonNpi(v.sourceTonNpiLabel);
  const dest = tonNpi(v.destTonNpiLabel);

  return {
    serviceType: v.serviceType,
    sourceAddrTon: source.ton,
    sourceAddrNpi: source.npi,
    sourceAddr: v.sourceAddr,
    destAddrTon: dest.ton,
    destAddrNpi: dest.npi,
    destinationAddr: v.destinationAddr,
    esmClass: v.esmClass,
    protocolId: v.protocolId,
    priorityFlag: v.priorityFlag,
    registeredDelivery: v.registeredDelivery,
    scheduleDeliveryTime: timeFor(SCHEDULE_DELIVERY, v.scheduleDeliveryLabel),
    validityPeriod: timeFor(VALIDITY_PERIODS, v.validityPeriodLabel),
    dataCoding: v.dataCoding,
    replaceIfPresent: v.replaceIfPresent,
    smDefaultMsgId: v.smDefaultMsgId,
    text: v.text,
    placement: v.placement,
    segmentation: v.segmentation,
    packing: v.packing,
    udhHex: v.udhHex,
    tlvs: v.tlvs,
  };
}

/** A one-line counter for under the message box. */
export function counterText(m: Measurement): string {
  const base = `${m.parts} part${m.parts === 1 ? "" : "s"} · ${m.chars} chars · ${m.bits} bits · ${m.bytes} bytes`;
  return m.unmappable.length > 0 ? `${base} · cannot encode ${m.unmappable.join(" ")}` : base;
}

export interface SamplePreset {
  key: keyof typeof SAMPLES;
  label: string;
  dataCoding: number;
  dataCodingLabel: string;
  segmentation: SegmentationMode;
  segmentationLabel: string;
  parts: number;
}

/**
 * The reference tool's four sample messages.
 *
 * Each carries the segmentation its body needs: a three-part sample picked
 * while the form says "one PDU" would produce a body far past what sm_length
 * can describe, which is an error the SMSC reports and the user has to guess
 * at. Choosing a sample therefore sets the mode it implies.
 */
export const SAMPLE_PRESETS: SamplePreset[] = [
  {
    key: "gsm1",
    label: "GSM 1 part",
    dataCoding: DataCoding.GSM_DEFAULT,
    dataCodingLabel: "0 — GSM 03.38 default",
    segmentation: "none",
    segmentationLabel: "None — one PDU",
    parts: 1,
  },
  {
    key: "ucs2_1",
    label: "UCS2 1 part",
    dataCoding: DataCoding.UCS2,
    dataCodingLabel: "8 — UCS2",
    segmentation: "none",
    segmentationLabel: "None — one PDU",
    parts: 1,
  },
  {
    key: "gsm3",
    label: "GSM 3 parts",
    dataCoding: DataCoding.GSM_DEFAULT,
    dataCodingLabel: "0 — GSM 03.38 default",
    segmentation: "udh",
    segmentationLabel: "UDH concatenation",
    parts: 3,
  },
  {
    key: "ucs2_3",
    label: "UCS2 3 parts",
    dataCoding: DataCoding.UCS2,
    dataCodingLabel: "8 — UCS2",
    segmentation: "udh",
    segmentationLabel: "UDH concatenation",
    parts: 3,
  },
];

export { TLV_MESSAGE_PAYLOAD, DataCoding, SAMPLES, CommandId };
