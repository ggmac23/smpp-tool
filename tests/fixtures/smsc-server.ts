import * as net from "node:net";
import * as tls from "node:tls";
import * as fs from "node:fs";
import * as path from "node:path";
import { PduFramer } from "../../src/smpp/frame";
import { decodePdu, Pdu } from "../../src/smpp/decode";
import { encodePdu, PduInput } from "../../src/smpp/encode";
import { CommandStatus, commandName, responseIdFor } from "../../src/smpp/constants";

const TLS_DIR = path.join(__dirname, "tls");
const read = (f: string) => fs.readFileSync(path.join(TLS_DIR, f));

/** Test-only key material. See tests/fixtures/tls/README.md. */
export const testCerts = {
  get ca() {
    return read("ca.crt");
  },
  get serverKey() {
    return read("server.key");
  },
  get serverCert() {
    return read("server.crt");
  },
  get clientKey() {
    return read("client.key");
  },
  get clientCert() {
    return read("client.crt");
  },
};

export type SmscHandler = (pdu: Pdu) => PduInput | PduInput[] | null;

export interface FakeSmscOptions {
  tls?: {
    key: Buffer;
    cert: Buffer;
    requestCert?: boolean;
    ca?: Buffer;
  };
}

interface Conn {
  socket: net.Socket | tls.TLSSocket;
  framer: PduFramer;
}

/**
 * A loopback SMSC for the test suite.
 *
 * This is a test fixture, not a product feature: the client is the product,
 * and it needs something to talk to. Handlers decide what comes back, so a
 * test can produce a throttle, a bind failure, or silence.
 */
export class FakeSmsc {
  readonly received: Pdu[] = [];
  lastPeerCertificateSubject = "";
  /** The SNI name the last TLS client asked for, or "" if it sent none. */
  lastServername = "";

  private server?: net.Server | tls.Server;
  private readonly conns = new Set<Conn>();
  private readonly handlers = new Map<string, SmscHandler>();
  private nextMessageId = 1;

  constructor(private readonly opts: FakeSmscOptions = {}) {}

  on(command: string, handler: SmscHandler): void {
    this.handlers.set(command, handler);
  }

  listen(): Promise<number> {
    return new Promise((resolve, reject) => {
      const onConnection = (socket: net.Socket | tls.TLSSocket) => this.accept(socket);

      this.server = this.opts.tls
        ? tls.createServer(
            {
              key: this.opts.tls.key,
              cert: this.opts.tls.cert,
              requestCert: this.opts.tls.requestCert ?? false,
              ca: this.opts.tls.ca,
              rejectUnauthorized: false,
            },
            onConnection as (s: tls.TLSSocket) => void
          )
        : net.createServer(onConnection);

      this.server.on("error", reject);
      this.server.listen(0, "127.0.0.1", () => {
        const addr = this.server!.address();
        if (addr && typeof addr === "object") resolve(addr.port);
        else reject(new Error("no port"));
      });
    });
  }

  async close(): Promise<void> {
    for (const c of this.conns) c.socket.destroy();
    this.conns.clear();
    const server = this.server;
    this.server = undefined;
    if (!server) return;
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }

  get connectionCount(): number {
    return this.conns.size;
  }

  async waitForConnection(timeoutMs = 3000): Promise<void> {
    const started = Date.now();
    while (this.conns.size === 0) {
      if (Date.now() - started > timeoutMs) throw new Error("no client connected");
      await new Promise((r) => setTimeout(r, 5));
    }
  }

  /** Push a PDU at every connected client. */
  send(pdu: PduInput): void {
    const bytes = encodePdu(pdu);
    for (const c of this.conns) c.socket.write(bytes);
  }

  /** Push arbitrary bytes at every connected client, malformed or not. */
  sendRaw(bytes: Buffer): void {
    for (const c of this.conns) c.socket.write(bytes);
  }

  private accept(socket: net.Socket | tls.TLSSocket): void {
    if (socket instanceof tls.TLSSocket) {
      const cert = socket.getPeerCertificate();
      this.lastPeerCertificateSubject = cert && cert.subject ? JSON.stringify(cert.subject) : "";
      this.lastServername = (socket as tls.TLSSocket & { servername?: string }).servername ?? "";
    }

    const conn: Conn = { socket, framer: new PduFramer() };
    this.conns.add(conn);

    socket.on("data", (chunk: Buffer) => {
      let raws: Buffer[];
      try {
        raws = conn.framer.push(chunk);
      } catch {
        conn.framer.reset();
        return;
      }
      for (const raw of raws) {
        let pdu: Pdu;
        try {
          pdu = decodePdu(raw);
        } catch {
          continue;
        }
        this.received.push(pdu);
        this.respond(conn, pdu);
      }
    });

    socket.on("error", () => this.conns.delete(conn));
    socket.on("close", () => this.conns.delete(conn));
  }

  private respond(conn: Conn, pdu: Pdu): void {
    const name = commandName(pdu.commandId);
    const handler = this.handlers.get(name);

    const replies = handler ? handler(pdu) : this.defaultReply(name, pdu);
    if (replies === null) return;

    for (const reply of Array.isArray(replies) ? replies : [replies]) {
      conn.socket.write(encodePdu(reply));
    }
  }

  private defaultReply(name: string, pdu: Pdu): PduInput | null {
    const ok = (fields?: PduInput["fields"]): PduInput => ({
      commandId: responseIdFor(pdu.commandId),
      commandStatus: CommandStatus.ESME_ROK,
      sequenceNumber: pdu.sequenceNumber,
      fields,
    });

    switch (name) {
      case "bind_transmitter":
      case "bind_receiver":
      case "bind_transceiver":
        return ok({ system_id: "FakeSmsc" });
      case "submit_sm":
      case "data_sm":
      case "submit_multi":
      case "broadcast_sm":
        return ok({ message_id: this.mintMessageId() });
      case "query_sm":
        return ok({ message_id: String(pdu.fields.message_id ?? ""), message_state: 2 });
      case "cancel_sm":
      case "replace_sm":
      case "unbind":
      case "enquire_link":
        return ok();
      default:
        // A response from the client, or a command with no reply of its own.
        return null;
    }
  }

  private mintMessageId(): string {
    return (this.nextMessageId++).toString(16).toUpperCase().padStart(4, "0");
  }
}
