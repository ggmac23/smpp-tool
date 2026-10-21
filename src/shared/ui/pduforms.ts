import { CommandId } from "../../smpp/constants";
import { TLV_MESSAGE_PAYLOAD, BodyPlacement } from "../../smpp/segment";
import { encodeText } from "../../smpp/text";
import type { SendPduRequest } from "../api";
import { TON_NPI_PRESETS } from "./connection";
import { SCHEDULE_DELIVERY, VALIDITY_PERIODS, timeFor } from "./fields";

export interface Built {
  request: SendPduRequest;
  /** Always advisory. The request is returned regardless. */
  warnings: string[];
}

function tonNpi(label: string): { ton: number; npi: number } {
  const p = TON_NPI_PRESETS.find((x) => x.label === label);
  return p ? { ton: p.ton, npi: p.npi } : { ton: 0, npi: 0 };
}

const MESSAGE_ID_MAX = 64;

function messageIdWarnings(messageId: string): string[] {
  if (messageId.length === 0) return ["message_id is empty. Sending it anyway."];
  if (messageId.length > MESSAGE_ID_MAX) {
    return [
      `message_id is ${messageId.length} characters; the spec allows ${MESSAGE_ID_MAX}. Sending it anyway.`,
    ];
  }
  return [];
}

function hex(text: string, dataCoding: number): string {
  return encodeText(text, dataCoding).bytes.toString("hex");
}

/** Shared by query_sm, cancel_sm, replace_sm and the broadcast family. */
export interface MessageOpsFormValues {
  messageId: string;
  serviceType: string;
  sourceTonNpiLabel: string;
  sourceAddr: string;
  destTonNpiLabel: string;
  destinationAddr: string;
  scheduleDeliveryLabel: string;
  validityPeriodLabel: string;
  registeredDelivery: number;
  smDefaultMsgId: number;
  text: string;
  dataCoding: number;
}

export function buildQueryInput(v: MessageOpsFormValues): Built {
  const src = tonNpi(v.sourceTonNpiLabel);
  return {
    warnings: messageIdWarnings(v.messageId),
    request: {
      commandId: CommandId.query_sm,
      fields: {
        message_id: v.messageId,
        source_addr_ton: src.ton,
        source_addr_npi: src.npi,
        source_addr: v.sourceAddr,
      },
    },
  };
}

export function buildCancelInput(v: MessageOpsFormValues): Built {
  const src = tonNpi(v.sourceTonNpiLabel);
  const dst = tonNpi(v.destTonNpiLabel);
  return {
    warnings: messageIdWarnings(v.messageId),
    request: {
      commandId: CommandId.cancel_sm,
      fields: {
        service_type: v.serviceType,
        message_id: v.messageId,
        source_addr_ton: src.ton,
        source_addr_npi: src.npi,
        source_addr: v.sourceAddr,
        dest_addr_ton: dst.ton,
        dest_addr_npi: dst.npi,
        destination_addr: v.destinationAddr,
      },
    },
  };
}

export function buildReplaceInput(v: MessageOpsFormValues): Built {
  const src = tonNpi(v.sourceTonNpiLabel);
  return {
    warnings: messageIdWarnings(v.messageId),
    request: {
      commandId: CommandId.replace_sm,
      fields: {
        message_id: v.messageId,
        source_addr_ton: src.ton,
        source_addr_npi: src.npi,
        source_addr: v.sourceAddr,
        schedule_delivery_time: timeFor(SCHEDULE_DELIVERY, v.scheduleDeliveryLabel),
        validity_period: timeFor(VALIDITY_PERIODS, v.validityPeriodLabel),
        registered_delivery: v.registeredDelivery,
        sm_default_msg_id: v.smDefaultMsgId,
      },
      octetFields: { short_message: hex(v.text, v.dataCoding) },
    },
  };
}

export type BroadcastKind = "broadcast_sm" | "query_broadcast_sm" | "cancel_broadcast_sm";

export interface BroadcastFormValues extends MessageOpsFormValues {
  kind: BroadcastKind;
}

/**
 * The SMPP v5.0 broadcast family. Note what is absent: there is no
 * `broadcast_multi` command in SMPP, so the three real commands are offered
 * instead of inventing one.
 */
export function buildBroadcastInput(v: BroadcastFormValues): Built {
  const src = tonNpi(v.sourceTonNpiLabel);
  const warnings = messageIdWarnings(v.messageId);

  if (v.kind === "query_broadcast_sm" || v.kind === "cancel_broadcast_sm") {
    return {
      warnings,
      request: {
        commandId: CommandId[v.kind],
        fields: {
          ...(v.kind === "cancel_broadcast_sm" ? { service_type: v.serviceType } : {}),
          message_id: v.messageId,
          source_addr_ton: src.ton,
          source_addr_npi: src.npi,
          source_addr: v.sourceAddr,
        },
      },
    };
  }

  return {
    warnings,
    request: {
      commandId: CommandId.broadcast_sm,
      fields: {
        service_type: v.serviceType,
        source_addr_ton: src.ton,
        source_addr_npi: src.npi,
        source_addr: v.sourceAddr,
        message_id: v.messageId,
        priority_flag: 0,
        schedule_delivery_time: timeFor(SCHEDULE_DELIVERY, v.scheduleDeliveryLabel),
        validity_period: timeFor(VALIDITY_PERIODS, v.validityPeriodLabel),
        replace_if_present_flag: 0,
        data_coding: v.dataCoding,
        sm_default_msg_id: v.smDefaultMsgId,
      },
      tlvs: [{ tag: TLV_MESSAGE_PAYLOAD, hex: hex(v.text, v.dataCoding) }],
    },
  };
}

export interface MultiFormValues {
  serviceType: string;
  sourceTonNpiLabel: string;
  sourceAddr: string;
  destTonNpiLabel: string;
  /** One address per line. */
  destinations: string;
  /** One distribution-list name per line. */
  distributionLists: string;
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
}

function lines(text: string): string[] {
  return (text || "")
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
}

export function buildMultiInput(v: MultiFormValues): Built {
  const src = tonNpi(v.sourceTonNpiLabel);
  const dst = tonNpi(v.destTonNpiLabel);

  const addresses = lines(v.destinations).map((addr) => ({
    flag: 1,
    ton: dst.ton,
    npi: dst.npi,
    addr,
  }));
  const dls = lines(v.distributionLists).map((dlName) => ({ flag: 2, dlName }));
  const destList = [...addresses, ...dls];

  const warnings: string[] = [];
  if (destList.length === 0) {
    warnings.push("submit_multi has no destinations at all. Sending it anyway.");
  }

  return {
    warnings,
    request: {
      commandId: CommandId.submit_multi,
      fields: {
        service_type: v.serviceType,
        source_addr_ton: src.ton,
        source_addr_npi: src.npi,
        source_addr: v.sourceAddr,
        esm_class: v.esmClass,
        protocol_id: v.protocolId,
        priority_flag: v.priorityFlag,
        schedule_delivery_time: timeFor(SCHEDULE_DELIVERY, v.scheduleDeliveryLabel),
        validity_period: timeFor(VALIDITY_PERIODS, v.validityPeriodLabel),
        registered_delivery: v.registeredDelivery,
        replace_if_present_flag: v.replaceIfPresent,
        data_coding: v.dataCoding,
        sm_default_msg_id: v.smDefaultMsgId,
      },
      destList,
      octetFields: { short_message: hex(v.text, v.dataCoding) },
    },
  };
}

export interface DataSmFormValues {
  serviceType: string;
  sourceTonNpiLabel: string;
  sourceAddr: string;
  destTonNpiLabel: string;
  destinationAddr: string;
  esmClass: number;
  registeredDelivery: number;
  dataCoding: number;
  text: string;
  placement: BodyPlacement;
}

/**
 * data_sm has no short_message field at all, so its body always travels in
 * the message_payload TLV whatever the placement selector says.
 */
export function buildDataSmInput(v: DataSmFormValues): Built {
  const src = tonNpi(v.sourceTonNpiLabel);
  const dst = tonNpi(v.destTonNpiLabel);
  return {
    warnings: [],
    request: {
      commandId: CommandId.data_sm,
      fields: {
        service_type: v.serviceType,
        source_addr_ton: src.ton,
        source_addr_npi: src.npi,
        source_addr: v.sourceAddr,
        dest_addr_ton: dst.ton,
        dest_addr_npi: dst.npi,
        destination_addr: v.destinationAddr,
        esm_class: v.esmClass,
        registered_delivery: v.registeredDelivery,
        data_coding: v.dataCoding,
      },
      tlvs: [{ tag: TLV_MESSAGE_PAYLOAD, hex: hex(v.text, v.dataCoding) }],
    },
  };
}
