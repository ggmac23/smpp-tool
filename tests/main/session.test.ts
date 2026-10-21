import { describe, it, expect, afterEach } from "vitest";
import { Session, BindOptions } from "../../src/main/session";
import { Transport } from "../../src/main/transport";
import { FakeSmsc } from "../fixtures/smsc-server";
import { Pdu } from "../../src/smpp/decode";
import { encodePdu } from "../../src/smpp/encode";
import { CommandId, CommandStatus, InterfaceVersion } from "../../src/smpp/constants";

const BIND: BindOptions = {
  systemId: "esme",
  password: "pw",
  systemType: "SMPP",
  interfaceVersion: InterfaceVersion.V34,
  mode: "transceiver",
};

async function until(predicate: () => boolean, timeoutMs = 3000): Promise<void> {
  const started = Date.now();
  while (!predicate()) {
    if (Date.now() - started > timeoutMs) throw new Error("condition never became true");
    await new Promise((r) => setTimeout(r, 5));
  }
}

let smsc: FakeSmsc | undefined;
let session: Session | undefined;
let transport: Transport | undefined;

async function open(
  sessionOpts: { windowSize?: number; responseTimeoutMs?: number } = {}
): Promise<Session> {
  const port = await smsc!.listen();
  transport = new Transport({
    host: "127.0.0.1",
    port,
    tls: { enabled: false, rejectUnauthorized: true },
    connectTimeoutMs: 3000,
  });
  session = new Session(transport, {
    windowSize: sessionOpts.windowSize ?? 10,
    responseTimeoutMs: sessionOpts.responseTimeoutMs ?? 2000,
  });
  await transport.connect();
  return session;
}

afterEach(async () => {
  transport?.destroy();
  transport = undefined;
  session = undefined;
  await smsc?.close();
  smsc = undefined;
});

describe("bind", () => {
  it("binds as transceiver and reports state bound", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    expect(s.state).toBe("unbound");

    const resp = await s.bind(BIND);
    expect(resp.commandId).toBe(CommandId.bind_transceiver_resp);
    expect(s.state).toBe("bound");
    expect(s.mode).toBe("transceiver");
  });

  it("sends bind_transmitter for transmitter mode", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind({ ...BIND, mode: "transmitter" });
    expect(smsc.received[0].commandId).toBe(CommandId.bind_transmitter);
  });

  it("sends bind_receiver for receiver mode", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind({ ...BIND, mode: "receiver" });
    expect(smsc.received[0].commandId).toBe(CommandId.bind_receiver);
  });

  it("sends the credentials and interface version it was given", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind(BIND);
    const p = smsc.received[0];
    expect(p.fields.system_id).toBe("esme");
    expect(p.fields.password).toBe("pw");
    expect(p.fields.system_type).toBe("SMPP");
    expect(p.fields.interface_version).toBe(InterfaceVersion.V34);
  });

  it("reports state unbound and rejects when the bind is refused", async () => {
    smsc = new FakeSmsc();
    smsc.on("bind_transceiver", (p) => ({
      commandId: CommandId.bind_transceiver_resp,
      commandStatus: CommandStatus.ESME_RBINDFAIL,
      sequenceNumber: p.sequenceNumber,
    }));
    const s = await open();

    await expect(s.bind(BIND)).rejects.toThrow(/ESME_RBINDFAIL/);
    expect(s.state).toBe("unbound");
  });

  it("reports the states it passed through", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    const states: string[] = [];
    s.on("state", (st: { state: string }) => states.push(st.state));
    await s.bind(BIND);
    expect(states).toEqual(["binding", "bound"]);
  });

  it("carries the bind mode on the state event", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    const events: { state: string; mode: string | null }[] = [];
    s.on("state", (e: { state: string; mode: string | null }) => events.push(e));
    await s.bind(BIND);
    expect(events[events.length - 1]).toEqual({ state: "bound", mode: "transceiver" });
  });

  it("unbinds and reports state closed", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind(BIND);
    await s.unbind();
    expect(s.state).toBe("closed");
  });
});

describe("sending", () => {
  it("allocates increasing sequence numbers", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind(BIND);
    await s.send({ commandId: CommandId.submit_sm, fields: {} });
    await s.send({ commandId: CommandId.submit_sm, fields: {} });
    const seqs = smsc.received.map((p) => p.sequenceNumber);
    expect(seqs).toEqual([1, 2, 3]);
  });

  it("resolves send() with the response that matches the sequence number", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind(BIND);
    const resp = await s.send({ commandId: CommandId.submit_sm, fields: {} });
    expect(resp.commandId).toBe(CommandId.submit_sm_resp);
    expect(typeof resp.fields.message_id).toBe("string");
  });

  it("resolves two concurrent sends with their own responses, out of order", async () => {
    smsc = new FakeSmsc();
    // Answer the second request first.
    const queued: Pdu[] = [];
    smsc.on("submit_sm", (p) => {
      queued.push(p);
      if (queued.length < 2) return null;
      return [
        {
          commandId: CommandId.submit_sm_resp,
          sequenceNumber: queued[1].sequenceNumber,
          fields: { message_id: "SECOND" },
        },
        {
          commandId: CommandId.submit_sm_resp,
          sequenceNumber: queued[0].sequenceNumber,
          fields: { message_id: "FIRST" },
        },
      ];
    });
    const s = await open();
    await s.bind(BIND);

    const [a, b] = await Promise.all([
      s.send({ commandId: CommandId.submit_sm, fields: {} }),
      s.send({ commandId: CommandId.submit_sm, fields: {} }),
    ]);
    expect(a.fields.message_id).toBe("FIRST");
    expect(b.fields.message_id).toBe("SECOND");
  });

  it("resolves rather than rejects on a non-zero command_status", async () => {
    smsc = new FakeSmsc();
    smsc.on("submit_sm", (p) => ({
      commandId: CommandId.submit_sm_resp,
      commandStatus: CommandStatus.ESME_RTHROTTLED,
      sequenceNumber: p.sequenceNumber,
    }));
    const s = await open();
    await s.bind(BIND);
    const resp = await s.send({ commandId: CommandId.submit_sm, fields: {} });
    expect(resp.commandStatus).toBe(CommandStatus.ESME_RTHROTTLED);
  });

  it("sends raw bytes verbatim through sendRaw", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind(BIND);

    const bytes = encodePdu({ commandId: CommandId.enquire_link, sequenceNumber: 4242 });
    bytes.writeUInt32BE(16, 0);
    s.sendRaw(bytes);
    await until(() => smsc!.received.length === 2);
    expect(smsc.received[1].sequenceNumber).toBe(4242);
  });

  it("emits pdu-out and pdu-in for every PDU", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    const out: Pdu[] = [];
    const inn: Pdu[] = [];
    s.on("pdu-out", (e: { pdu: Pdu }) => out.push(e.pdu));
    s.on("pdu-in", (e: { pdu: Pdu }) => inn.push(e.pdu));

    await s.bind(BIND);
    expect(out.map((p) => p.commandId)).toEqual([CommandId.bind_transceiver]);
    expect(inn.map((p) => p.commandId)).toEqual([CommandId.bind_transceiver_resp]);
  });

  it("refuses to send when it is not bound", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await expect(s.send({ commandId: CommandId.submit_sm, fields: {} })).rejects.toThrow(
      /not bound/i
    );
  });
});

describe("inbound requests", () => {
  it("answers deliver_sm with deliver_sm_resp automatically", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind(BIND);

    smsc.send({
      commandId: CommandId.deliver_sm,
      sequenceNumber: 77,
      fields: { esm_class: 4, short_message: Buffer.from("id:A stat:DELIVRD", "latin1") },
    });
    await until(() => smsc!.received.length === 2);

    expect(smsc.received[1].commandId).toBe(CommandId.deliver_sm_resp);
    expect(smsc.received[1].sequenceNumber).toBe(77);
    expect(smsc.received[1].commandStatus).toBe(CommandStatus.ESME_ROK);
  });

  it("answers an inbound enquire_link", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind(BIND);

    smsc.send({ commandId: CommandId.enquire_link, sequenceNumber: 55 });
    await until(() => smsc!.received.length === 2);
    expect(smsc.received[1].commandId).toBe(CommandId.enquire_link_resp);
    expect(smsc.received[1].sequenceNumber).toBe(55);
  });

  it("moves to closed on an inbound unbind", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind(BIND);

    smsc.send({ commandId: CommandId.unbind, sequenceNumber: 66 });
    await until(() => s.state === "closed");
    expect(smsc.received[1].commandId).toBe(CommandId.unbind_resp);
  });

  it("does not re-bind after the socket closes", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind(BIND);

    await smsc.close();
    smsc = undefined;

    await until(() => s.state === "closed");
    await new Promise((r) => setTimeout(r, 200));
    expect(s.state).toBe("closed");
  });
});

describe("orphans and unknown commands (Review Focus 3)", () => {
  it("emits orphan for a response whose sequence number is not pending", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind(BIND);

    const orphans: Pdu[] = [];
    s.on("orphan", (p: Pdu) => orphans.push(p));

    smsc.send({
      commandId: CommandId.submit_sm_resp,
      sequenceNumber: 9999,
      fields: { message_id: "GHOST" },
    });
    await until(() => orphans.length === 1);
    expect(orphans[0].fields.message_id).toBe("GHOST");
    expect(s.state).toBe("bound");
  });

  it("emits orphan for an unknown command_id and stays bound", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind(BIND);

    const orphans: Pdu[] = [];
    s.on("orphan", (p: Pdu) => orphans.push(p));

    smsc.send({ commandId: 0x4242, sequenceNumber: 3, fields: { body: Buffer.from([1, 2]) } });
    await until(() => orphans.length === 1);
    expect(orphans[0].commandId).toBe(0x4242);
    expect(s.state).toBe("bound");
  });

  it("keeps answering enquire_link after an orphan", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind(BIND);

    smsc.send({ commandId: CommandId.submit_sm_resp, sequenceNumber: 9999, fields: {} });
    smsc.send({ commandId: CommandId.enquire_link, sequenceNumber: 56 });
    await until(() => smsc!.received.some((p) => p.commandId === CommandId.enquire_link_resp));
    expect(s.state).toBe("bound");
  });

  it("emits decode-error for a malformed inbound PDU and stays bound", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind(BIND);

    const errors: Error[] = [];
    s.on("decode-error", (e: { error: Error }) => errors.push(e.error));

    // Well-framed (command_length matches) but the body is nonsense for submit_sm.
    const bogus = Buffer.alloc(18);
    bogus.writeUInt32BE(18, 0);
    bogus.writeUInt32BE(CommandId.submit_sm, 4);
    bogus.writeUInt32BE(0, 8);
    bogus.writeUInt32BE(1, 12);
    bogus.writeUInt8(0x41, 16);
    bogus.writeUInt8(0x42, 17);
    smsc.sendRaw(bogus);

    await until(() => errors.length === 1);
    expect(s.state).toBe("bound");
  });
});

describe("response timeouts (Review Focus 5)", () => {
  it("rejects send() after responseTimeoutMs when the SMSC never answers", async () => {
    smsc = new FakeSmsc();
    smsc.on("submit_sm", () => null);
    const s = await open({ responseTimeoutMs: 150 });
    await s.bind(BIND);

    await expect(s.send({ commandId: CommandId.submit_sm, fields: {} })).rejects.toThrow(
      /timed out/i
    );
  });

  it("emits timeout with the sequence number", async () => {
    smsc = new FakeSmsc();
    smsc.on("submit_sm", () => null);
    const s = await open({ responseTimeoutMs: 150 });
    await s.bind(BIND);

    const timeouts: number[] = [];
    s.on("timeout", (e: { sequenceNumber: number }) => timeouts.push(e.sequenceNumber));
    await s.send({ commandId: CommandId.submit_sm, fields: {} }).catch(() => undefined);
    expect(timeouts).toEqual([2]);
  });

  it("frees the window slot after a timeout", async () => {
    smsc = new FakeSmsc();
    let answered = 0;
    smsc.on("submit_sm", (p) => {
      answered++;
      // Never answer the first; answer the second normally.
      return answered === 1
        ? null
        : {
            commandId: CommandId.submit_sm_resp,
            sequenceNumber: p.sequenceNumber,
            fields: { message_id: "OK" },
          };
    });
    const s = await open({ windowSize: 1, responseTimeoutMs: 150 });
    await s.bind(BIND);

    const first = s.send({ commandId: CommandId.submit_sm, fields: {} }).catch((e) => e as Error);
    const second = s.send({ commandId: CommandId.submit_sm, fields: {} });

    expect((await first).message).toMatch(/timed out/i);
    expect((await second).fields.message_id).toBe("OK");
  });

  it("leaves no pending requests behind after a timeout", async () => {
    smsc = new FakeSmsc();
    smsc.on("submit_sm", () => null);
    const s = await open({ responseTimeoutMs: 150 });
    await s.bind(BIND);
    await s.send({ commandId: CommandId.submit_sm, fields: {} }).catch(() => undefined);
    expect(s.pending).toBe(0);
  });

  it("rejects everything pending when the connection drops", async () => {
    smsc = new FakeSmsc();
    smsc.on("submit_sm", () => null);
    const s = await open({ responseTimeoutMs: 5000 });
    await s.bind(BIND);

    const inflight = s.send({ commandId: CommandId.submit_sm, fields: {} }).catch((e) => e as Error);
    await until(() => smsc!.received.length === 2);
    await smsc.close();
    smsc = undefined;

    expect((await inflight).message).toMatch(/connection closed/i);
    expect(s.pending).toBe(0);
  });
});

describe("sequence numbers", () => {
  it("wraps at 0x7fffffff back to 1", async () => {
    smsc = new FakeSmsc();
    const s = await open();
    await s.bind(BIND);
    s.setNextSequenceNumberForTest(0x7fffffff);
    await s.send({ commandId: CommandId.submit_sm, fields: {} });
    await s.send({ commandId: CommandId.submit_sm, fields: {} });
    const seqs = smsc.received.slice(1).map((p) => p.sequenceNumber);
    expect(seqs).toEqual([0x7fffffff, 1]);
  });
});
