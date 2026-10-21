import { PduDecodeError, PDU_HEADER_LENGTH } from "./decode";

/**
 * Upper bound on a single PDU. A command_length above this is rejected before
 * it is ever used as an allocation size.
 */
export const MAX_PDU_LENGTH = 1_048_576;

/**
 * Frames a TCP byte stream into PDUs by command_length — never by read
 * boundaries. A chunk may hold a fraction of a PDU, several PDUs, or a
 * boundary falling inside the 16-byte header.
 */
export class PduFramer {
  private buf: Buffer = Buffer.alloc(0);

  push(chunk: Buffer): Buffer[] {
    this.buf = this.buf.length === 0 ? chunk : Buffer.concat([this.buf, chunk]);

    const out: Buffer[] = [];
    for (;;) {
      if (this.buf.length < PDU_HEADER_LENGTH) break;

      const commandLength = this.buf.readUInt32BE(0);

      if (commandLength < PDU_HEADER_LENGTH) {
        throw new PduDecodeError(
          `command_length ${commandLength} is below the ${PDU_HEADER_LENGTH}-byte header`,
          Buffer.from(this.buf.subarray(0, Math.min(this.buf.length, PDU_HEADER_LENGTH)))
        );
      }
      if (commandLength > MAX_PDU_LENGTH) {
        throw new PduDecodeError(
          `command_length ${commandLength} exceeds the ${MAX_PDU_LENGTH}-byte maximum`,
          Buffer.from(this.buf.subarray(0, Math.min(this.buf.length, PDU_HEADER_LENGTH)))
        );
      }

      if (this.buf.length < commandLength) break;

      out.push(Buffer.from(this.buf.subarray(0, commandLength)));
      this.buf = this.buf.subarray(commandLength);
    }
    return out;
  }

  /** Bytes held back waiting for the rest of their PDU. */
  get buffered(): number {
    return this.buf.length;
  }

  reset(): void {
    this.buf = Buffer.alloc(0);
  }
}
