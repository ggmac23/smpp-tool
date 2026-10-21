import { EventEmitter } from "node:events";
import { Transport } from "./transport";
import { decodePdu, Pdu, PduDecodeError } from "../smpp/decode";
import { encodePdu, PduInput } from "../smpp/encode";
import {
  CommandId,
  CommandStatus,
  commandName,
  isResponse,
  responseIdFor,
  statusName,
} from "../smpp/constants";

export type BindMode = "transceiver" | "transmitter" | "receiver";
export type BindState = "unbound" | "binding" | "bound" | "unbinding" | "closed";

export interface BindOptions {
  systemId: string;
  password: string;
  systemType: string;
  interfaceVersion: number;
  mode: BindMode;
  addrTon?: number;
  addrNpi?: number;
  addressRange?: string;
}

export interface SessionOptions {
  /** Requests allowed in flight at once. A further send waits for a slot. */
  windowSize: number;
  responseTimeoutMs: number;
}

const BIND_COMMAND: Record<BindMode, number> = {
  transmitter: CommandId.bind_transmitter,
  receiver: CommandId.bind_receiver,
  transceiver: CommandId.bind_transceiver,
};

const MAX_SEQUENCE = 0x7fffffff;

interface Pending {
  sequenceNumber: number;
  resolve: (pdu: Pdu) => void;
  reject: (err: Error) => void;
  timer: NodeJS.Timeout;
  windowed: boolean;
}

/**
 * One SMPP conversation: bind state, sequence numbers, request/response
 * matching and the automatic answers the protocol requires.
 *
 * It never reconnects and never re-binds. When the socket goes, the session is
 * `closed` and stays closed until the user starts a new one.
 */
export class Session extends EventEmitter {
  private _state: BindState = "unbound";
  private _mode: BindMode | null = null;
  private nextSequence = 1;
  private readonly inflight = new Map<number, Pending>();
  private windowUsed = 0;
  private readonly waiters: (() => void)[] = [];

  constructor(
    private readonly transport: Transport,
    private readonly opts: SessionOptions
  ) {
    super();
    transport.on("pdu", (raw: Buffer) => this.onRaw(raw));
    transport.on("decode-error", (e: PduDecodeError) => this.emit("decode-error", { error: e }));
    transport.on("close", (err?: Error) => this.onClose(err));
  }

  get state(): BindState {
    return this._state;
  }

  get mode(): BindMode | null {
    return this._mode;
  }

  /** Requests still waiting for a response. */
  get pending(): number {
    return this.inflight.size;
  }

  /** Test seam: the next sequence number to be allocated. */
  setNextSequenceNumberForTest(n: number): void {
    this.nextSequence = n;
  }

  async bind(o: BindOptions): Promise<Pdu> {
    if (this._state !== "unbound") {
      throw new Error(`cannot bind while ${this._state}`);
    }
    this.setState("binding", o.mode);

    let resp: Pdu;
    try {
      resp = await this.request(
        {
          commandId: BIND_COMMAND[o.mode],
          fields: {
            system_id: o.systemId,
            password: o.password,
            system_type: o.systemType,
            interface_version: o.interfaceVersion,
            addr_ton: o.addrTon ?? 0,
            addr_npi: o.addrNpi ?? 0,
            address_range: o.addressRange ?? "",
          },
        },
        false
      );
    } catch (e) {
      this.setState("unbound", null);
      throw e;
    }

    if (resp.commandStatus !== CommandStatus.ESME_ROK) {
      this.setState("unbound", null);
      throw new Error(`bind refused: ${statusName(resp.commandStatus)}`);
    }

    this.setState("bound", o.mode);
    return resp;
  }

  async unbind(): Promise<Pdu> {
    if (this._state !== "bound") throw new Error(`cannot unbind while ${this._state}`);
    this.setState("unbinding", this._mode);
    try {
      const resp = await this.request({ commandId: CommandId.unbind }, false);
      this.setState("closed", null);
      return resp;
    } catch (e) {
      this.setState("closed", null);
      throw e;
    }
  }

  /**
   * Send a request and resolve with its response.
   *
   * A non-zero command_status resolves — the caller needs to see it. Only a
   * timeout or a lost connection rejects.
   */
  async send(input: Omit<PduInput, "sequenceNumber">): Promise<Pdu> {
    if (this._state !== "bound") throw new Error(`not bound (state is ${this._state})`);
    await this.acquireWindowSlot();
    return this.request(input, true);
  }

  /**
   * Put bytes on the wire exactly as given: no sequence number allocated, no
   * response awaited, nothing validated. The hex composer's path.
   */
  sendRaw(bytes: Buffer): void {
    this.transport.write(bytes);
    try {
      this.emit("pdu-out", { pdu: decodePdu(bytes), at: Date.now() });
    } catch {
      this.emit("raw-out", { bytes, at: Date.now() });
    }
  }

  /* ------------------------------------------------------------------ */

  private allocateSequence(): number {
    const n = this.nextSequence;
    this.nextSequence = n >= MAX_SEQUENCE ? 1 : n + 1;
    return n;
  }

  private acquireWindowSlot(): Promise<void> {
    if (this.windowUsed < this.opts.windowSize) {
      this.windowUsed++;
      return Promise.resolve();
    }
    return new Promise<void>((resolve) => {
      this.waiters.push(() => {
        this.windowUsed++;
        resolve();
      });
    });
  }

  private releaseWindowSlot(): void {
    this.windowUsed = Math.max(0, this.windowUsed - 1);
    const next = this.waiters.shift();
    if (next) next();
  }

  private request(input: Omit<PduInput, "sequenceNumber">, windowed: boolean): Promise<Pdu> {
    const sequenceNumber = this.allocateSequence();
    const bytes = encodePdu({ ...input, sequenceNumber });

    return new Promise<Pdu>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.settle(sequenceNumber, (p) => {
          this.emit("timeout", { sequenceNumber });
          p.reject(
            new Error(
              `${commandName(input.commandId)} seq ${sequenceNumber} timed out after ${this.opts.responseTimeoutMs}ms`
            )
          );
        });
      }, this.opts.responseTimeoutMs);

      this.inflight.set(sequenceNumber, { sequenceNumber, resolve, reject, timer, windowed });

      try {
        this.transport.write(bytes);
      } catch (e) {
        this.settle(sequenceNumber, (p) => p.reject(e as Error));
        return;
      }
      this.emit("pdu-out", { pdu: decodePdu(bytes), at: Date.now() });
    });
  }

  /** Remove a pending request, stop its timer, free its window slot, then act. */
  private settle(sequenceNumber: number, act: (p: Pending) => void): void {
    const p = this.inflight.get(sequenceNumber);
    if (!p) return;
    this.inflight.delete(sequenceNumber);
    clearTimeout(p.timer);
    if (p.windowed) this.releaseWindowSlot();
    act(p);
  }

  private onRaw(raw: Buffer): void {
    let pdu: Pdu;
    try {
      pdu = decodePdu(raw);
    } catch (e) {
      this.emit("decode-error", { error: e as PduDecodeError });
      return;
    }

    this.emit("pdu-in", { pdu, at: Date.now() });

    if (isResponse(pdu.commandId)) {
      const had = this.inflight.has(pdu.sequenceNumber);
      if (!had) {
        this.emit("orphan", pdu);
        return;
      }
      this.settle(pdu.sequenceNumber, (p) => p.resolve(pdu));
      return;
    }

    this.answer(pdu);
  }

  /** The automatic answers the protocol requires of a bound ESME. */
  private answer(pdu: Pdu): void {
    const name = commandName(pdu.commandId);

    switch (name) {
      case "deliver_sm":
      case "data_sm":
        this.reply(pdu);
        return;
      case "enquire_link":
        this.reply(pdu);
        return;
      case "unbind":
        this.reply(pdu);
        this.setState("closed", null);
        return;
      case "alert_notification":
        // Defined with no response.
        return;
      default:
        // An id the protocol does not define, or a request this client does not
        // answer. It goes in the trace as an orphan; the session stays bound,
        // and nothing is sent back, because a test client should not invent
        // traffic the user did not ask for.
        this.emit("orphan", pdu);
        return;
    }
  }

  private reply(pdu: Pdu): void {
    const bytes = encodePdu({
      commandId: responseIdFor(pdu.commandId),
      commandStatus: CommandStatus.ESME_ROK,
      sequenceNumber: pdu.sequenceNumber,
    });
    try {
      this.transport.write(bytes);
      this.emit("pdu-out", { pdu: decodePdu(bytes), at: Date.now() });
    } catch {
      // The socket went while we were answering; onClose will report it.
    }
  }

  private onClose(err?: Error): void {
    const reason = err ? `connection closed: ${err.message}` : "connection closed";
    for (const sequenceNumber of [...this.inflight.keys()]) {
      this.settle(sequenceNumber, (p) => p.reject(new Error(reason)));
    }
    this.waiters.length = 0;
    this.windowUsed = 0;
    if (this._state !== "closed") this.setState("closed", null);
  }

  private setState(state: BindState, mode: BindMode | null): void {
    this._state = state;
    this._mode = mode;
    this.emit("state", { state, mode });
  }
}
