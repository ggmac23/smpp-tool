import { EventEmitter } from "node:events";
import * as net from "node:net";
import * as tls from "node:tls";
import { PduFramer } from "../smpp/frame";
import { PduDecodeError } from "../smpp/decode";

export interface TlsOptions {
  enabled: boolean;
  /** A CA bundle to trust in addition to the system store. */
  ca?: Buffer;
  /** false accepts a certificate the client cannot verify. */
  rejectUnauthorized: boolean;
  /** The SNI name to send, when it differs from `host`. */
  servername?: string;
  clientCert?: Buffer;
  clientKey?: Buffer;
  clientKeyPassphrase?: string;
}

export interface TransportOptions {
  host: string;
  port: number;
  tls: TlsOptions;
  connectTimeoutMs: number;
}

/**
 * The socket, and nothing else.
 *
 * It frames the byte stream into PDUs and reports what happens. It does not
 * reconnect — ever. The connection state the UI shows is the real one, and a
 * dropped bind stays dropped until the user says otherwise.
 */
export class Transport extends EventEmitter {
  private socket?: net.Socket | tls.TLSSocket;
  private framer = new PduFramer();
  private closed = false;
  private open = false;

  constructor(private readonly opts: TransportOptions) {
    super();
  }

  get connected(): boolean {
    return this.open;
  }

  connect(): Promise<void> {
    if (this.socket) return Promise.reject(new Error("transport already used"));

    return new Promise<void>((resolve, reject) => {
      let settled = false;
      const timer = setTimeout(() => {
        if (settled) return;
        settled = true;
        this.socket?.destroy();
        reject(
          new Error(
            `connection to ${this.opts.host}:${this.opts.port} timed out after ${this.opts.connectTimeoutMs}ms`
          )
        );
      }, this.opts.connectTimeoutMs);

      const onReady = () => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        this.open = true;
        resolve();
      };

      const onEarlyError = (err: Error) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        this.socket?.destroy();
        reject(err);
      };

      const socket = this.opts.tls.enabled
        ? tls.connect(
            {
              host: this.opts.host,
              port: this.opts.port,
              ca: this.opts.tls.ca,
              rejectUnauthorized: this.opts.tls.rejectUnauthorized,
              servername: this.opts.tls.servername ?? this.opts.host,
              cert: this.opts.tls.clientCert,
              key: this.opts.tls.clientKey,
              passphrase: this.opts.tls.clientKeyPassphrase,
            },
            onReady
          )
        : net.connect({ host: this.opts.host, port: this.opts.port }, onReady);

      this.socket = socket;
      socket.once("error", onEarlyError);

      socket.on("data", (chunk: Buffer) => {
        this.emit("bytes-in", chunk);
        let raws: Buffer[];
        try {
          raws = this.framer.push(chunk);
        } catch (e) {
          // One unreadable PDU does not end the session: drop what is buffered
          // and keep listening, because an SMSC that sent one bad PDU is still
          // worth watching.
          this.framer.reset();
          this.emit("decode-error", e as PduDecodeError);
          return;
        }
        for (const raw of raws) this.emit("pdu", raw);
      });

      socket.on("close", () => this.finish());

      socket.on("error", (err: Error) => {
        if (!settled) return; // handled by onEarlyError
        this.finish(err);
      });
    });
  }

  write(bytes: Buffer): void {
    if (!this.socket || !this.open) throw new Error("transport is not connected");
    this.emit("bytes-out", bytes);
    this.socket.write(bytes);
  }

  destroy(): void {
    this.socket?.destroy();
    this.finish();
  }

  private finish(err?: Error): void {
    if (this.closed) return;
    this.closed = true;
    this.open = false;
    this.emit("close", err);
  }
}
