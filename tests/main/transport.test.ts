import { describe, it, expect, afterEach } from "vitest";
import { Transport, TransportOptions, TlsOptions } from "../../src/main/transport";
import { FakeSmsc, testCerts } from "../fixtures/smsc-server";
import { decodePdu } from "../../src/smpp/decode";
import { encodePdu } from "../../src/smpp/encode";
import { CommandId } from "../../src/smpp/constants";

const NO_TLS: TlsOptions = { enabled: false, rejectUnauthorized: true };

function opts(port: number, tls: Partial<TlsOptions> = {}): TransportOptions {
  return {
    host: "127.0.0.1",
    port,
    tls: { ...NO_TLS, ...tls },
    connectTimeoutMs: 3000,
  };
}

/** Collects the events a test wants to assert on. */
function watch(t: Transport) {
  const pdus: Buffer[] = [];
  const errors: Error[] = [];
  const closes: (Error | undefined)[] = [];
  t.on("pdu", (raw: Buffer) => pdus.push(raw));
  t.on("decode-error", (e: Error) => errors.push(e));
  t.on("close", (e?: Error) => closes.push(e));
  return { pdus, errors, closes };
}

async function until(predicate: () => boolean, timeoutMs = 3000): Promise<void> {
  const started = Date.now();
  while (!predicate()) {
    if (Date.now() - started > timeoutMs) throw new Error("condition never became true");
    await new Promise((r) => setTimeout(r, 5));
  }
}

let smsc: FakeSmsc | undefined;
let transport: Transport | undefined;

afterEach(async () => {
  transport?.destroy();
  transport = undefined;
  await smsc?.close();
  smsc = undefined;
});

describe("Transport over plain TCP", () => {
  it("connects and emits framed PDUs", async () => {
    smsc = new FakeSmsc();
    const port = await smsc.listen();
    transport = new Transport(opts(port));
    const seen = watch(transport);

    await transport.connect();
    expect(transport.connected).toBe(true);

    transport.write(encodePdu({ commandId: CommandId.enquire_link, sequenceNumber: 1 }));
    await until(() => seen.pdus.length === 1);
    expect(decodePdu(seen.pdus[0]).commandId).toBe(CommandId.enquire_link_resp);
  });

  it("emits one pdu event per PDU when two arrive in one read", async () => {
    smsc = new FakeSmsc();
    smsc.on("enquire_link", (p) => [
      { commandId: CommandId.enquire_link_resp, sequenceNumber: p.sequenceNumber },
      { commandId: CommandId.deliver_sm, sequenceNumber: 500, fields: { esm_class: 4 } },
    ]);
    const port = await smsc.listen();
    transport = new Transport(opts(port));
    const seen = watch(transport);

    await transport.connect();
    transport.write(encodePdu({ commandId: CommandId.enquire_link, sequenceNumber: 1 }));
    await until(() => seen.pdus.length === 2);

    expect(seen.pdus.map((r) => decodePdu(r).commandId)).toEqual([
      CommandId.enquire_link_resp,
      CommandId.deliver_sm,
    ]);
  });

  it("emits decode-error and stays connected on malformed bytes", async () => {
    smsc = new FakeSmsc();
    const port = await smsc.listen();
    transport = new Transport(opts(port));
    const seen = watch(transport);

    await transport.connect();
    await smsc.waitForConnection();

    const bogus = Buffer.alloc(20);
    bogus.writeUInt32BE(0x7fffffff, 0); // a command_length that lies
    smsc.sendRaw(bogus);
    await until(() => seen.errors.length === 1);

    expect(transport.connected).toBe(true);
    expect(seen.closes).toHaveLength(0);

    // The framer was reset, so a well-formed PDU afterwards still arrives.
    smsc.send({ commandId: CommandId.deliver_sm, sequenceNumber: 7, fields: { esm_class: 4 } });
    await until(() => seen.pdus.length === 1);
    expect(decodePdu(seen.pdus[0]).sequenceNumber).toBe(7);
  });

  it("emits close and does not reconnect when the server goes away", async () => {
    smsc = new FakeSmsc();
    const port = await smsc.listen();
    transport = new Transport(opts(port));
    const seen = watch(transport);

    await transport.connect();
    await smsc.waitForConnection();
    await smsc.close();
    smsc = undefined;

    await until(() => seen.closes.length === 1);
    expect(transport.connected).toBe(false);

    // Still closed a moment later: nothing reconnected behind our back.
    await new Promise((r) => setTimeout(r, 150));
    expect(transport.connected).toBe(false);
    expect(seen.closes).toHaveLength(1);
  });

  it("emits close exactly once when destroyed", async () => {
    smsc = new FakeSmsc();
    const port = await smsc.listen();
    transport = new Transport(opts(port));
    const seen = watch(transport);

    await transport.connect();
    transport.destroy();
    transport.destroy();
    await until(() => seen.closes.length === 1);
    await new Promise((r) => setTimeout(r, 100));
    expect(seen.closes).toHaveLength(1);
  });

  it("counts bytes in and out", async () => {
    smsc = new FakeSmsc();
    const port = await smsc.listen();
    transport = new Transport(opts(port));
    const out: Buffer[] = [];
    const inn: Buffer[] = [];
    transport.on("bytes-out", (b: Buffer) => out.push(b));
    transport.on("bytes-in", (b: Buffer) => inn.push(b));

    await transport.connect();
    transport.write(encodePdu({ commandId: CommandId.enquire_link, sequenceNumber: 1 }));
    await until(() => inn.length > 0);
    expect(out[0].length).toBe(16);
    expect(inn[0].length).toBeGreaterThan(0);
  });

  it("rejects connect() against a refused port", async () => {
    smsc = new FakeSmsc();
    const port = await smsc.listen();
    await smsc.close();
    smsc = undefined;

    transport = new Transport(opts(port));
    await expect(transport.connect()).rejects.toThrow();
    expect(transport.connected).toBe(false);
  });

  it("rejects connect() after connectTimeoutMs against an unroutable address", async () => {
    // 192.0.2.0/24 is TEST-NET-1: reserved for documentation, never routed.
    transport = new Transport({
      host: "192.0.2.1",
      port: 2775,
      tls: NO_TLS,
      connectTimeoutMs: 200,
    });
    await expect(transport.connect()).rejects.toThrow(/timed out/i);
  });
});

describe("Transport over TLS", () => {
  const serverTls = () => ({ key: testCerts.serverKey, cert: testCerts.serverCert });

  it("refuses an untrusted certificate when rejectUnauthorized is true", async () => {
    smsc = new FakeSmsc({ tls: serverTls() });
    const port = await smsc.listen();
    transport = new Transport(
      opts(port, { enabled: true, rejectUnauthorized: true, servername: "localhost" })
    );
    await expect(transport.connect()).rejects.toThrow();
  });

  it("accepts an untrusted certificate when rejectUnauthorized is false", async () => {
    smsc = new FakeSmsc({ tls: serverTls() });
    const port = await smsc.listen();
    transport = new Transport(
      opts(port, { enabled: true, rejectUnauthorized: false, servername: "localhost" })
    );
    const seen = watch(transport);

    await transport.connect();
    transport.write(encodePdu({ commandId: CommandId.enquire_link, sequenceNumber: 1 }));
    await until(() => seen.pdus.length === 1);
    expect(decodePdu(seen.pdus[0]).commandId).toBe(CommandId.enquire_link_resp);
  });

  it("accepts the certificate when its CA is supplied", async () => {
    smsc = new FakeSmsc({ tls: serverTls() });
    const port = await smsc.listen();
    transport = new Transport(
      opts(port, {
        enabled: true,
        rejectUnauthorized: true,
        ca: testCerts.ca,
        servername: "localhost",
      })
    );
    await transport.connect();
    expect(transport.connected).toBe(true);
  });

  it("sends the configured servername as SNI", async () => {
    smsc = new FakeSmsc({ tls: serverTls() });
    const port = await smsc.listen();
    transport = new Transport(
      opts(port, {
        enabled: true,
        rejectUnauthorized: true,
        ca: testCerts.ca,
        servername: "localhost",
      })
    );
    await transport.connect();
    await smsc.waitForConnection();
    expect(smsc.lastServername).toBe("localhost");
  });

  it("presents a client certificate when the server requests one", async () => {
    smsc = new FakeSmsc({
      tls: { ...serverTls(), requestCert: true, ca: testCerts.ca },
    });
    const port = await smsc.listen();
    transport = new Transport(
      opts(port, {
        enabled: true,
        rejectUnauthorized: true,
        ca: testCerts.ca,
        servername: "localhost",
        clientCert: testCerts.clientCert,
        clientKey: testCerts.clientKey,
      })
    );
    await transport.connect();
    await smsc.waitForConnection();
    expect(smsc.lastPeerCertificateSubject).toContain("smppclient test client");
  });
});
