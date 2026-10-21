import { commandName, statusName, CommandStatus } from "./constants";
import { Pdu, FieldValue } from "./decode";
import { PDU_FIELDS } from "./pdu-defs";
import { tlvName } from "./tlv";
import { parseReceipt } from "./receipt";

function render(value: FieldValue): string {
  if (Buffer.isBuffer(value)) return value.toString("hex");
  if (Array.isArray(value)) return JSON.stringify(value);
  return String(value);
}

/**
 * The one-line decoded form shown in the trace table.
 *
 * It names what a reader scanning a trace needs: the command, who it is
 * between, how big the body is, the message id, the receipt state, and the
 * status when it is not OK.
 */
export function summarise(pdu: Pdu): string {
  const name = commandName(pdu.commandId);
  const parts: string[] = [name];

  const f = pdu.fields;
  const src = f.source_addr;
  const dst = f.destination_addr;

  if (typeof src === "string" && typeof dst === "string" && (src || dst)) {
    parts.push(`${src || "-"} -> ${dst || "-"}`);
  } else if (typeof f.system_id === "string" && f.system_id) {
    parts.push(f.system_id);
  }

  if (typeof f.message_id === "string" && f.message_id) parts.push(`id=${f.message_id}`);

  const body = Buffer.isBuffer(f.short_message) ? f.short_message : undefined;
  const payload = pdu.tlvs.find((t) => t.tag === 0x0424);
  const bodyLength = body?.length ?? payload?.value.length;
  if (bodyLength !== undefined && bodyLength > 0) parts.push(`${bodyLength} bytes`);

  const receipt = parseReceipt(pdu);
  if (receipt) {
    if (receipt.messageId) parts.push(`receipt id=${receipt.messageId}`);
    if (receipt.state) parts.push(receipt.state);
  }

  if (pdu.commandStatus !== CommandStatus.ESME_ROK) {
    parts.push(statusName(pdu.commandStatus));
  }

  if (pdu.tlvs.length > 0) parts.push(`${pdu.tlvs.length} TLV`);

  return parts.join(" ");
}

export interface DescribedField {
  name: string;
  value: string;
}

/** Every field of the PDU, header first, for the expanded view. */
export function describe(pdu: Pdu): DescribedField[] {
  const rows: DescribedField[] = [
    { name: "command_length", value: String(pdu.raw.length) },
    { name: "command_id", value: `${commandName(pdu.commandId)} (0x${(pdu.commandId >>> 0).toString(16)})` },
    { name: "command_status", value: `${statusName(pdu.commandStatus)} (${pdu.commandStatus})` },
    { name: "sequence_number", value: String(pdu.sequenceNumber) },
  ];

  const defs = PDU_FIELDS[commandName(pdu.commandId)];
  if (defs) {
    for (const def of defs) {
      const v = pdu.fields[def.name];
      if (v !== undefined) rows.push({ name: def.name, value: render(v) });
    }
  } else if (pdu.fields.body !== undefined) {
    rows.push({ name: "body", value: render(pdu.fields.body) });
  }

  for (const tlv of pdu.tlvs) {
    rows.push({ name: tlvName(tlv.tag), value: tlv.value.toString("hex") });
  }

  return rows;
}
