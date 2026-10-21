import { describe, it, expect, afterEach } from "vitest";
import { BurstRunner, BurstConfig } from "../../src/main/burst";
import { MAX_RATE } from "../../src/main/recipients";
import { Session, BindOptions } from "../../src/main/session";
import { Transport } from "../../src/main/transport";
import { DEFAULT_CORRELATION } from "../../src/main/dlr";
import { FakeSmsc } from "../fixtures/smsc-server";
import { CommandId, CommandStatus, InterfaceVersion, DataCoding } from "../../src/smpp/constants";

const BIND: BindOptions = {
  systemId: "esme",
  password: "pw",
  systemType: "",
  interfaceVersion: InterfaceVersion.V34,
  mode: "transceiver",
};

function config(over: Partial<BurstConfig> = {}): BurstConfig {
  return {
    recipients: { kind: "range", from: "393331000000", count: 5 },
    ratePerSec: 200,
    template: {
      commandId: CommandId.submit_sm,
      fields: { source_addr: "SENDER", data_coding: DataCoding.GSM_DEFAULT },
    },
    text: "hello",
    dataCoding: DataCoding.GSM_DEFAULT,
    segmentation: "udh",
    placement: "short_message",
    backoffOnThrottle: false,
    awaitDlrMs: 300,
    ...over,
  };
}

let smsc: FakeSmsc | undefined;
let transport: Transport | undefined;
let session: Session | undefined;

async function bound(windowSize = 20): Promise<Session> {
  const port = await smsc!.listen();
  transport = new Transport({
    host: "127.0.0.1",
    port,
    tls: { enabled: false, rejectUnauthorized: true },
    connectTimeoutMs: 3000,
  });
  session = new Session(transport, { windowSize, responseTimeoutMs: 2000 });
  await transport.connect();
  await session.bind(BIND);
  return session;
}

afterEach(async () => {
  transport?.destroy();
  transport = undefined;
  session = undefined;
  await smsc?.close();
  smsc = undefined;
});

describe("BurstRunner", () => {
  it("sends one submit_sm per recipient", async () => {
    smsc = new FakeSmsc();
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    await runner.start(config());

    const submits = smsc.received.filter((p) => p.commandId === CommandId.submit_sm);
    expect(submits).toHaveLength(5);
    expect(runner.rows).toHaveLength(5);
    expect(runner.rows.map((r) => r.recipient)).toEqual([
      "393331000000",
      "393331000001",
      "393331000002",
      "393331000003",
      "393331000004",
    ]);
  });

  it("records the message_id from submit_sm_resp on the row", async () => {
    smsc = new FakeSmsc();
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    await runner.start(config({ recipients: { kind: "repeat", number: "3933", count: 2 } }));

    expect(runner.rows.every((r) => typeof r.messageId === "string" && r.messageId!.length > 0)).toBe(
      true
    );
    expect(runner.rows[0].messageId).not.toBe(runner.rows[1].messageId);
    expect(runner.rows.every((r) => r.commandStatus === CommandStatus.ESME_ROK)).toBe(true);
    expect(runner.rows.every((r) => (r.submitLatencyMs ?? -1) >= 0)).toBe(true);
  });

  it("sends the composed body as the row's text", async () => {
    smsc = new FakeSmsc();
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    await runner.start(config({ recipients: { kind: "repeat", number: "3933", count: 1 } }));

    const submit = smsc.received.find((p) => p.commandId === CommandId.submit_sm)!;
    expect((submit.fields.short_message as Buffer).toString("latin1")).toBe("hello");
    expect(submit.fields.destination_addr).toBe("3933");
    expect(submit.fields.source_addr).toBe("SENDER");
    expect(runner.rows[0].body).toBe("hello");
  });

  it("counts errors by status name", async () => {
    smsc = new FakeSmsc();
    let n = 0;
    smsc.on("submit_sm", (p) => ({
      commandId: CommandId.submit_sm_resp,
      commandStatus: ++n % 2 === 0 ? CommandStatus.ESME_RINVDSTADR : CommandStatus.ESME_ROK,
      sequenceNumber: p.sequenceNumber,
      fields: { message_id: `M${n}` },
    }));
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    await runner.start(config({ recipients: { kind: "repeat", number: "3933", count: 4 } }));

    expect(runner.stats.submitted).toBe(4);
    expect(runner.stats.accepted).toBe(2);
    expect(runner.stats.errorsByStatus).toEqual({ ESME_RINVDSTADR: 2 });
  });

  it("holds the requested rate when the SMSC answers ESME_RTHROTTLED", async () => {
    smsc = new FakeSmsc();
    smsc.on("submit_sm", (p) => ({
      commandId: CommandId.submit_sm_resp,
      commandStatus: CommandStatus.ESME_RTHROTTLED,
      sequenceNumber: p.sequenceNumber,
    }));
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    await runner.start(
      config({ recipients: { kind: "repeat", number: "3933", count: 6 }, ratePerSec: 300 })
    );

    expect(runner.currentRatePerSec).toBe(300);
    expect(runner.stats.submitted).toBe(6);
    expect(runner.stats.accepted).toBe(0);
    expect(runner.stats.errorsByStatus.ESME_RTHROTTLED).toBe(6);
  });

  it("backs off when backoffOnThrottle is set", async () => {
    smsc = new FakeSmsc();
    smsc.on("submit_sm", (p) => ({
      commandId: CommandId.submit_sm_resp,
      commandStatus: CommandStatus.ESME_RTHROTTLED,
      sequenceNumber: p.sequenceNumber,
    }));
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    await runner.start(
      config({
        recipients: { kind: "repeat", number: "3933", count: 6 },
        ratePerSec: 300,
        backoffOnThrottle: true,
      })
    );

    expect(runner.currentRatePerSec).toBeLessThan(300);
    expect(runner.stats.submitted).toBe(6);
  });

  it("computes achieved rate and latency percentiles", async () => {
    smsc = new FakeSmsc();
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    await runner.start(
      config({ recipients: { kind: "repeat", number: "3933", count: 20 }, ratePerSec: 100 })
    );

    expect(runner.stats.achievedRatePerSec).toBeGreaterThan(0);
    expect(runner.stats.achievedRatePerSec).toBeLessThanOrEqual(150);
    expect(runner.stats.latencyP50).toBeGreaterThanOrEqual(0);
    expect(runner.stats.latencyP95).toBeGreaterThanOrEqual(runner.stats.latencyP50);
    expect(runner.stats.latencyP99).toBeGreaterThanOrEqual(runner.stats.latencyP95);
  });

  it("rejects a rate above MAX_RATE before sending anything", async () => {
    smsc = new FakeSmsc();
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    await expect(runner.start(config({ ratePerSec: MAX_RATE + 1 }))).rejects.toThrow(
      new RegExp(String(MAX_RATE))
    );
    expect(smsc.received.filter((p) => p.commandId === CommandId.submit_sm)).toHaveLength(0);
    expect(runner.rows).toHaveLength(0);
  });

  it("rejects a rate of zero", async () => {
    smsc = new FakeSmsc();
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);
    await expect(runner.start(config({ ratePerSec: 0 }))).rejects.toThrow();
  });

  it("stops with connection-lost when the bind drops mid-burst", async () => {
    smsc = new FakeSmsc();
    let seen = 0;
    smsc.on("submit_sm", (p) => {
      if (++seen === 3) {
        setTimeout(() => void smsc?.close(), 0);
        return null;
      }
      return {
        commandId: CommandId.submit_sm_resp,
        sequenceNumber: p.sequenceNumber,
        fields: { message_id: `M${seen}` },
      };
    });
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    let reason = "";
    runner.on("done", (e: { reason: string }) => (reason = e.reason));

    await runner.start(
      config({ recipients: { kind: "repeat", number: "3933", count: 50 }, ratePerSec: 20 })
    );

    expect(reason).toBe("connection-lost");
    expect(runner.stats.submitted).toBeLessThan(50);
  });

  it("stops where it was when stop() is called", async () => {
    smsc = new FakeSmsc();
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    let reason = "";
    runner.on("done", (e: { reason: string }) => (reason = e.reason));
    runner.on("row", () => {
      if (runner.rows.length >= 3) runner.stop();
    });

    await runner.start(
      config({ recipients: { kind: "repeat", number: "3933", count: 100 }, ratePerSec: 50 })
    );

    expect(reason).toBe("stopped");
    expect(runner.stats.submitted).toBeLessThan(100);
  });

  it("correlates a deliver_sm back to its row", async () => {
    smsc = new FakeSmsc();
    smsc.on("submit_sm", (p) => {
      const id = `ID${p.sequenceNumber}`;
      setTimeout(
        () =>
          smsc?.send({
            commandId: CommandId.deliver_sm,
            sequenceNumber: 9000 + p.sequenceNumber,
            fields: {
              esm_class: 4,
              short_message: Buffer.from(`id:${id} stat:DELIVRD err:000`, "latin1"),
            },
          }),
        20
      );
      return {
        commandId: CommandId.submit_sm_resp,
        sequenceNumber: p.sequenceNumber,
        fields: { message_id: id },
      };
    });
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    await runner.start(
      config({ recipients: { kind: "repeat", number: "3933", count: 3 }, awaitDlrMs: 1000 })
    );

    expect(runner.rows.map((r) => r.dlrState)).toEqual(["DELIVRD", "DELIVRD", "DELIVRD"]);
    expect(runner.rows.every((r) => (r.dlrLatencyMs ?? -1) >= 0)).toBe(true);
    expect(runner.stats.dlrReceived).toBe(3);
    expect(runner.stats.dlrUnmatched).toBe(0);
  });

  it("counts an unmatched receipt in dlrUnmatched without touching a row", async () => {
    smsc = new FakeSmsc();
    smsc.on("submit_sm", (p) => {
      setTimeout(
        () =>
          smsc?.send({
            commandId: CommandId.deliver_sm,
            sequenceNumber: 9000,
            fields: {
              esm_class: 4,
              short_message: Buffer.from("id:SOMEONE-ELSE stat:DELIVRD err:000", "latin1"),
            },
          }),
        20
      );
      return {
        commandId: CommandId.submit_sm_resp,
        sequenceNumber: p.sequenceNumber,
        fields: { message_id: "MINE" },
      };
    });
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    await runner.start(
      config({ recipients: { kind: "repeat", number: "3933", count: 1 }, awaitDlrMs: 500 })
    );

    expect(runner.stats.dlrUnmatched).toBe(1);
    expect(runner.stats.dlrReceived).toBe(0);
    expect(runner.rows[0].dlrState).toBeNull();
  });

  it("reports a 3-part message as one row with pdus 3", async () => {
    smsc = new FakeSmsc();
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    await runner.start(
      config({
        recipients: { kind: "repeat", number: "3933", count: 2 },
        text: "a".repeat(400),
      })
    );

    expect(runner.rows).toHaveLength(2);
    expect(runner.rows.map((r) => r.pdus)).toEqual([3, 3]);
    expect(runner.stats.messages).toBe(2);
    expect(runner.stats.pdus).toBe(6);
    expect(smsc.received.filter((p) => p.commandId === CommandId.submit_sm)).toHaveLength(6);
  });

  it("marks the row with the first non-zero status when part 2 of 3 fails", async () => {
    smsc = new FakeSmsc();
    let n = 0;
    smsc.on("submit_sm", (p) => ({
      commandId: CommandId.submit_sm_resp,
      commandStatus: ++n === 2 ? CommandStatus.ESME_RSUBMITFAIL : CommandStatus.ESME_ROK,
      sequenceNumber: p.sequenceNumber,
      fields: { message_id: `M${n}` },
    }));
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    await runner.start(
      config({ recipients: { kind: "repeat", number: "3933", count: 1 }, text: "a".repeat(400) })
    );

    expect(runner.rows[0].pdus).toBe(3);
    expect(runner.rows[0].commandStatus).toBe(CommandStatus.ESME_RSUBMITFAIL);
    expect(runner.stats.accepted).toBe(0);
  });

  it("keeps every row for 1000 messages", async () => {
    smsc = new FakeSmsc();
    const s = await bound(100);
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    await runner.start(
      config({
        recipients: { kind: "range", from: "393330000000", count: 1000 },
        ratePerSec: MAX_RATE,
        awaitDlrMs: 0,
      })
    );

    expect(runner.rows).toHaveLength(1000);
    expect(runner.stats.submitted).toBe(1000);
    expect(new Set(runner.rows.map((r) => r.recipient)).size).toBe(1000);
  }, 30000);

  it("emits progress while it runs", async () => {
    smsc = new FakeSmsc();
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    const progress: { sent: number; total: number }[] = [];
    runner.on("progress", (p: { sent: number; total: number }) => progress.push(p));

    await runner.start(
      config({ recipients: { kind: "repeat", number: "3933", count: 40 }, ratePerSec: 100 })
    );

    expect(progress.length).toBeGreaterThan(0);
    expect(progress[progress.length - 1].total).toBe(40);
  });

  it("refuses to start when the session is not bound", async () => {
    smsc = new FakeSmsc();
    const s = await bound();
    await s.unbind();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);
    await expect(runner.start(config())).rejects.toThrow(/not bound/i);
  });

  it("puts the body in message_payload when the placement says so", async () => {
    smsc = new FakeSmsc();
    const s = await bound();
    const runner = new BurstRunner(s, DEFAULT_CORRELATION);

    await runner.start(
      config({
        recipients: { kind: "repeat", number: "3933", count: 1 },
        placement: "message_payload",
        segmentation: "none",
      })
    );

    const submit = smsc.received.find((p) => p.commandId === CommandId.submit_sm)!;
    expect(submit.tlvs.find((t) => t.tag === 0x0424)!.value.toString("latin1")).toBe("hello");
  });
});
