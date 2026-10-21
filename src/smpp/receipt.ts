import { CommandId } from "./constants";
import { Pdu } from "./decode";

export interface Receipt {
  messageId: string | null;
  /** Where the message id came from: the TLV, or the receipt text. */
  source: "tlv" | "text";
  state: string | null;
  errorCode: string | null;
  submitDate: string | null;
  doneDate: string | null;
}

const TLV_RECEIPTED_MESSAGE_ID = 0x001e;
const TLV_MESSAGE_PAYLOAD = 0x0424;
const TLV_MESSAGE_STATE = 0x0427;

/** esm_class message-type bits: 0x04 delivery receipt, 0x20 intermediate notification. */
const ESM_MESSAGE_TYPE_MASK = 0x3c;
const ESM_DELIVERY_RECEIPT = 0x04;
const ESM_INTERMEDIATE_NOTIFICATION = 0x20;

const MESSAGE_STATE_NAMES: Record<number, string> = {
  1: "ENROUTE",
  2: "DELIVERED",
  3: "EXPIRED",
  4: "DELETED",
  5: "UNDELIVERABLE",
  6: "ACCEPTED",
  7: "UNKNOWN",
  8: "REJECTED",
};

function cstr(v: unknown): string | null {
  if (typeof v !== "string") return null;
  const end = v.indexOf("\0");
  return end >= 0 ? v.slice(0, end) : v;
}

function bodyText(pdu: Pdu): string {
  const sm = pdu.fields.short_message;
  if (Buffer.isBuffer(sm) && sm.length > 0) return sm.toString("latin1");
  const payload = pdu.tlvs.find((t) => t.tag === TLV_MESSAGE_PAYLOAD);
  return payload ? payload.value.toString("latin1") : "";
}

/** `key:value` out of receipt text, case-insensitively, tolerating unknown keys. */
function field(text: string, key: string): string | null {
  const m = new RegExp(`(?:^|\\s)${key}:\\s*([^\\s]*)`, "i").exec(text);
  return m ? m[1] : null;
}

/** `submit date:` / `done date:` carry a space inside the key. */
function dateField(text: string, which: "submit" | "done"): string | null {
  const m = new RegExp(`(?:^|\\s)${which}\\s+date:\\s*([^\\s]*)`, "i").exec(text);
  return m ? m[1] : null;
}

/**
 * Read a delivery receipt out of a deliver_sm, or return null if this PDU is
 * not one.
 *
 * A PDU counts as a receipt when esm_class says so, or when it carries a
 * receipted_message_id TLV, or when its body reads as receipt text. Real
 * SMSCs do all three, and some set esm_class to 0 while sending receipt text.
 */
export function parseReceipt(pdu: Pdu): Receipt | null {
  if (pdu.commandId !== CommandId.deliver_sm && pdu.commandId !== CommandId.data_sm) return null;

  const esm = Number(pdu.fields.esm_class ?? 0) & ESM_MESSAGE_TYPE_MASK;
  const byEsm = esm === ESM_DELIVERY_RECEIPT || esm === ESM_INTERMEDIATE_NOTIFICATION;

  const tlv = pdu.tlvs.find((t) => t.tag === TLV_RECEIPTED_MESSAGE_ID);
  const tlvId = tlv ? cstr(tlv.value.toString("latin1")) : null;

  const text = bodyText(pdu);
  const textId = field(text, "id");
  const state = field(text, "stat");

  const looksLikeReceipt = textId !== null && (state !== null || byEsm);

  if (!byEsm && !tlvId && !looksLikeReceipt) return null;

  const stateTlv = pdu.tlvs.find((t) => t.tag === TLV_MESSAGE_STATE);
  const stateFromTlv =
    stateTlv && stateTlv.value.length > 0
      ? MESSAGE_STATE_NAMES[stateTlv.value[0]] ?? `state(${stateTlv.value[0]})`
      : null;

  return {
    messageId: tlvId ?? textId,
    source: tlvId ? "tlv" : "text",
    state: state ?? stateFromTlv,
    errorCode: field(text, "err"),
    submitDate: dateField(text, "submit"),
    doneDate: dateField(text, "done"),
  };
}
