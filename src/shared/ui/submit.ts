import { CommandId, DataCoding } from "../../smpp/constants";
import { segment, BodyPlacement, TLV_MESSAGE_PAYLOAD } from "../../smpp/segment";
import { measure, Measurement, SegmentationMode, Gsm7Packing, SAMPLES } from "../../smpp/text";
import type { SendPduRequest } from "../api";
import { TON_NPI_PRESETS } from "./connection";
import { DATA_CODINGS, SCHEDULE_DELIVERY, VALIDITY_PERIODS, timeFor } from "./fields";

export interface TlvRow {
  /** Tag as hex, with or without a 0x prefix. */
  tagHex: string;
  /** Value as hex, when the user typed hex. */
  valueHex?: string;
  /** Value as text, when the user typed text. Latin-1 encoded. */
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

export interface BuiltSubmit {
  /** One request per segment, ready for the `sendPdu` command. */
  requests: SendPduRequest[];
  /** Things worth saying. Never a reason to withhold the PDU. */
  warnings: string[];
  measurement: Measurement;
}

function cleanHex(s: string): string {
  return (s || "").replace(/^0x/i, "").replace(/[^0-9a-fA-F]/g, "");
}

function tonNpi(label: string): { ton: number; npi: number } {
  const preset = TON_NPI_PRESETS.find((p) => p.label === label);
  return preset ? { ton: preset.ton, npi: preset.npi } : { ton: 0, npi: 0 };
}

const MIN_ADDRESS_LENGTH = 8;

/**
 * Turn the submit form into the PDUs it means.
 *
 * Validation only ever warns. An over-long field, a short address and a
 * data_coding the spec does not list all go on the wire exactly as typed —
 * that is the point of the tool.
 */
export function buildSubmitInput(v: SubmitFormValues): BuiltSubmit {
  const warnings: string[] = [];

  const source = tonNpi(v.sourceTonNpiLabel);
  const dest = tonNpi(v.destTonNpiLabel);

  if (v.destinationAddr.length > 0 && v.destinationAddr.length < MIN_ADDRESS_LENGTH) {
    warnings.push(
      `destination_addr "${v.destinationAddr}" is ${v.destinationAddr.length} characters; ` +
        `the reference tool wants at least ${MIN_ADDRESS_LENGTH}. Sending it anyway.`
    );
  }
  if (v.destinationAddr.length === 0) {
    warnings.push("destination_addr is empty. Sending it anyway.");
  }
  if (v.serviceType.length > 5) {
    warnings.push(
      `service_type is ${v.serviceType.length} characters; the spec allows 5. Sending it anyway.`
    );
  }
  if (!DATA_CODINGS.some((o) => o.value === v.dataCoding)) {
    warnings.push(
      `data_coding ${v.dataCoding} is outside the 0-8 range the spec lists. Sending it anyway.`
    );
  }

  const udh = cleanHex(v.udhHex);
  const measurement = measure(v.text, v.dataCoding, v.segmentation, v.packing);

  if (measurement.unmappable.length > 0) {
    warnings.push(
      `data_coding ${v.dataCoding} cannot represent ${measurement.unmappable.join(" ")}; ` +
        "those characters go out as question marks."
    );
  }

  const segments = segment({
    text: v.text,
    dataCoding: v.dataCoding,
    mode: v.segmentation,
    placement: v.placement,
    packing: v.packing,
    esmClass: v.esmClass,
    userUdh: udh.length > 0 ? Buffer.from(udh, "hex") : undefined,
  });

  const manualTlvs = v.tlvs
    .filter((t) => cleanHex(t.tagHex).length > 0)
    .map((t) => ({
      tag: parseInt(cleanHex(t.tagHex), 16),
      hex:
        t.valueHex !== undefined && t.valueHex !== null && cleanHex(t.valueHex).length > 0
          ? cleanHex(t.valueHex)
          : Buffer.from(t.valueText ?? "", "latin1").toString("hex"),
    }));

  const scheduleDeliveryTime = timeFor(SCHEDULE_DELIVERY, v.scheduleDeliveryLabel);
  const validityPeriod = timeFor(VALIDITY_PERIODS, v.validityPeriodLabel);

  const requests: SendPduRequest[] = segments.map((seg) => {
    const octetFields: Record<string, string> = {};
    const body = seg.fields.short_message;
    if (Buffer.isBuffer(body)) octetFields.short_message = body.toString("hex");

    const tlvs = [
      ...seg.tlvs.map((t) => ({ tag: t.tag, hex: t.value.toString("hex") })),
      ...manualTlvs,
    ];

    return {
      commandId: CommandId.submit_sm,
      fields: {
        service_type: v.serviceType,
        source_addr_ton: source.ton,
        source_addr_npi: source.npi,
        source_addr: v.sourceAddr,
        dest_addr_ton: dest.ton,
        dest_addr_npi: dest.npi,
        destination_addr: v.destinationAddr,
        esm_class: Number(seg.fields.esm_class ?? v.esmClass),
        protocol_id: v.protocolId,
        priority_flag: v.priorityFlag,
        schedule_delivery_time: scheduleDeliveryTime,
        validity_period: validityPeriod,
        registered_delivery: v.registeredDelivery,
        replace_if_present_flag: v.replaceIfPresent,
        data_coding: v.dataCoding,
        sm_default_msg_id: v.smDefaultMsgId,
      },
      octetFields,
      tlvs,
    };
  });

  return { requests, warnings, measurement };
}

/** A one-line counter for under the message box. */
export function counterText(m: Measurement): string {
  const base = `${m.parts} part${m.parts === 1 ? "" : "s"} · ${m.chars} chars · ${m.bits} bits · ${m.bytes} bytes`;
  return m.unmappable.length > 0 ? `${base} · cannot encode ${m.unmappable.join(" ")}` : base;
}

export { TLV_MESSAGE_PAYLOAD, DataCoding, SAMPLES };
