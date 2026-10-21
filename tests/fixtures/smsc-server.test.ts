import { describe, it, expect, afterEach } from "vitest";
import * as net from "node:net";
import * as tls from "node:tls";
import { FakeSmsc, testCerts } from "./smsc-server";
import { encodePdu } from "../../src/smpp/encode";
import { decodePdu, Pdu } from "../../src/smpp/decode";
import { PduFramer } from "../../src/smpp/frame";
import { CommandId, CommandStatus } from "../../src/smpp/constants";

/** A bare client: a socket plus a framer. Deliberately not the Session class. */
class BareClient {
  readonly received: Pdu[] = [];
  private framer = new PduFramer();
  constructor(private socket: net.Socket | tls.TLSSocket) {
    socket.on("data", (chunk) => {
      for (const raw of this.framer.push(chunk)) this.received.push(decodePdu(raw));
    });
  }
  send(pdu: Parameters<typeof encodePdu>[0]): void {
    this.socket.write(encodePdu(pdu));
  }
  end(): void {
    this.socket.destroy();
  }
  async waitFor(n: number, timeoutMs = 3000): Promise<void> {
    const started = Date.now();
    while (this.received.length < n) {
      if (Date.now() - started > timeoutMs) {
        throw new Error(`timed out waiting for ${n} PDU(s); got ${this.received.length}`);
      }
      await new Promise((r) => setTimeout(r, 5));
    }
  }
}

function connectPlain(port: number): Promise<BareClient> {
  return new Promise((resolve, reject) => {
    const s = net.connect({ port, host: "127.0.0.1" }, () => resolve(new BareClient(s)));
    s.on("error", reject);
  });
}

function connectTls(port: number, opts: tls.ConnectionOptions = {}): Promise<BareClient> {
  return new Promise((resolve, reject) => {
    const s = tls.connect(
      { port, host: "127.0.0.1", servername: "localhost", ...opts },
      () => resolve(new BareClient(s))
    );
    s.on("error", reject);
  });
}

let smsc: FakeSmsc | undefined;
let client: BareClient | undefined;

afterEach(async () => {
  client?.end();
  client = undefined;
  await smsc?.close();
  smsc = undefined;
});

describe("FakeSmsc", () => {
  it("accepts a bind_transceiver and answers ESME_ROK", async () => {
    smsc = new FakeSmsc();
    const port = await smsc.listen();
    client = await connectPlain(port);

    client.send({
      commandId: CommandId.bind_transceiver,
      sequenceNumber: 1,
      fields: { system_id: "esme", password: "pw" },
    });
    await client.waitFor(1);

    expect(client.received[0].commandId).toBe(CommandId.bind_transceiver_resp);
    expect(client.received[0].commandStatus).toBe(CommandStatus.ESME_ROK);
    expect(client.received[0].sequenceNumber).toBe(1);
  });

  it("records every PDU it received", async () => {
    smsc = new FakeSmsc();
    const port = await smsc.listen();
    client = await connectPlain(port);

    client.send({ commandId: CommandId.enquire_link, sequenceNumber: 1 });
    client.send({ commandId: CommandId.enquire_link, sequenceNumber: 2 });
    await client.waitFor(2);

    expect(smsc.received.map((p) => p.commandId)).toEqual([
      CommandId.enquire_link,
      CommandId.enquire_link,
    ]);
  });

  it("answers submit_sm with an incrementing message_id", async () => {
    smsc = new FakeSmsc();
    const port = await smsc.listen();
    client = await connectPlain(port);

    client.send({ commandId: CommandId.submit_sm, sequenceNumber: 1, fields: {} });
    client.send({ commandId: CommandId.submit_sm, sequenceNumber: 2, fields: {} });
    await client.waitFor(2);

    const ids = client.received.map((p) => p.fields.message_id);
    expect(ids[0]).not.toBe(ids[1]);
    expect(ids.every((id) => typeof id === "string" && id.length > 0)).toBe(true);
  });

  it("sends nothing when the handler returns null", async () => {
    smsc = new FakeSmsc();
    smsc.on("submit_sm", () => null);
    const port = await smsc.listen();
    client = await connectPlain(port);

    client.send({ commandId: CommandId.submit_sm, sequenceNumber: 1, fields: {} });
    await new Promise((r) => setTimeout(r, 150));
    expect(client.received).toHaveLength(0);
    expect(smsc.received).toHaveLength(1);
  });

  it("lets a handler choose the command status", async () => {
    smsc = new FakeSmsc();
    smsc.on("submit_sm", (p) => ({
      commandId: CommandId.submit_sm_resp,
      commandStatus: CommandStatus.ESME_RTHROTTLED,
      sequenceNumber: p.sequenceNumber,
    }));
    const port = await smsc.listen();
    client = await connectPlain(port);

    client.send({ commandId: CommandId.submit_sm, sequenceNumber: 1, fields: {} });
    await client.waitFor(1);
    expect(client.received[0].commandStatus).toBe(CommandStatus.ESME_RTHROTTLED);
  });

  it("delivers an unsolicited deliver_sm to the client", async () => {
    smsc = new FakeSmsc();
    const port = await smsc.listen();
    client = await connectPlain(port);
    await smsc.waitForConnection();

    smsc.send({
      commandId: CommandId.deliver_sm,
      sequenceNumber: 99,
      fields: { short_message: Buffer.from("id:ABC stat:DELIVRD err:000", "latin1"), esm_class: 4 },
    });
    await client.waitFor(1);

    expect(client.received[0].commandId).toBe(CommandId.deliver_sm);
    expect(client.received[0].sequenceNumber).toBe(99);
  });

  it("pushes malformed bytes with sendRaw", async () => {
    smsc = new FakeSmsc();
    const port = await smsc.listen();
    client = await connectPlain(port);
    await smsc.waitForConnection();

    const bogus = Buffer.alloc(20);
    bogus.writeUInt32BE(20, 0);
    bogus.writeUInt32BE(0x4242, 4);
    smsc.sendRaw(bogus);
    await client.waitFor(1);

    expect(client.received[0].commandId).toBe(0x4242);
  });

  it("accepts a TLS connection with its test certificate", async () => {
    smsc = new FakeSmsc({ tls: { key: testCerts.serverKey, cert: testCerts.serverCert } });
    const port = await smsc.listen();
    client = await connectTls(port, { ca: testCerts.ca });

    client.send({ commandId: CommandId.enquire_link, sequenceNumber: 1 });
    await client.waitFor(1);
    expect(client.received[0].commandId).toBe(CommandId.enquire_link_resp);
  });

  it("requests a client certificate when asked to", async () => {
    smsc = new FakeSmsc({
      tls: {
        key: testCerts.serverKey,
        cert: testCerts.serverCert,
        requestCert: true,
        ca: testCerts.ca,
      },
    });
    const port = await smsc.listen();
    client = await connectTls(port, {
      ca: testCerts.ca,
      key: testCerts.clientKey,
      cert: testCerts.clientCert,
    });

    client.send({ commandId: CommandId.enquire_link, sequenceNumber: 1 });
    await client.waitFor(1);
    expect(smsc.lastPeerCertificateSubject).toContain("smppclient test client");
  });

  it("reports the number of live connections", async () => {
    smsc = new FakeSmsc();
    const port = await smsc.listen();
    client = await connectPlain(port);
    await smsc.waitForConnection();
    expect(smsc.connectionCount).toBe(1);
  });
});
