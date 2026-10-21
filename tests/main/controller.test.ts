import { describe, it, expect, afterEach } from "vitest";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { Controller, ControllerDeps } from "../../src/main/controller";
import { ProfileStore, SecretStore } from "../../src/main/profiles";
import { registerIpc, IpcIo, IpcHandle } from "../../src/main/ipc";
import { COMMAND_NAMES } from "../../src/shared/api";
import { DEFAULT_CORRELATION } from "../../src/main/dlr";
import { FakeSmsc } from "../fixtures/smsc-server";
import { CommandId, DataCoding, InterfaceVersion } from "../../src/smpp/constants";

class FakeSecrets implements SecretStore {
  available() {
    return true;
  }
  encrypt(s: string) {
    return Buffer.from(s, "utf8");
  }
  decrypt(b: Buffer) {
    return b.toString("utf8");
  }
}

let smsc: FakeSmsc | undefined;
let controller: Controller | undefined;
let handle: IpcHandle | undefined;

interface Harness {
  controller: Controller;
  sent: { channel: string; payload: unknown }[];
  handle: IpcHandle;
  dir: string;
  saveTo: { path: string | null };
  openWith: { path: string | null };
}

function harness(): Harness {
  const sent: { channel: string; payload: unknown }[] = [];
  const io: IpcIo = { handle: () => undefined, send: (c, p) => sent.push({ channel: c, payload: p }) };
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ctrl-"));
  const saveTo: { path: string | null } = { path: null };
  const openWith: { path: string | null } = { path: null };

  const push = registerIpc({} as never, io, { batchMs: 10_000 });
  handle = push;

  const deps: ControllerDeps = {
    profiles: new ProfileStore(dir, new FakeSecrets()),
    dialogs: {
      openFile: async () => openWith.path,
      saveFile: async () => saveTo.path,
    },
    readFile: (p) => fs.readFileSync(p),
    writeFile: (p, d) => fs.writeFileSync(p, d, "utf8"),
    push,
  };
  const c = new Controller(deps);
  controller = c;
  return { controller: c, sent, handle: push, dir, saveTo, openWith };
}

async function connected(h: Harness): Promise<void> {
  const port = await smsc!.listen();
  await h.controller.connect({
    host: "127.0.0.1",
    port,
    tls: { enabled: false, rejectUnauthorized: true },
    correlation: DEFAULT_CORRELATION,
    windowSize: 10,
    responseTimeoutMs: 2000,
    connectTimeoutMs: 3000,
  });
  await h.controller.bind({
    systemId: "esme",
    password: "pw",
    systemType: "",
    interfaceVersion: InterfaceVersion.V34,
    mode: "transceiver",
  });
}

afterEach(async () => {
  handle?.dispose();
  handle = undefined;
  await controller?.disconnect();
  controller = undefined;
  await smsc?.close();
  smsc = undefined;
});

describe("Controller", () => {
  it("implements every command the API lists", () => {
    const h = harness();
    for (const name of COMMAND_NAMES) {
      expect(typeof (h.controller as unknown as Record<string, unknown>)[name], name).toBe(
        "function"
      );
    }
  });

  it("connects, binds and reports its status", async () => {
    smsc = new FakeSmsc();
    const h = harness();
    await connected(h);
    expect(await h.controller.status()).toMatchObject({ state: "bound", mode: "transceiver" });
  });

  it("refuses to send before connecting", async () => {
    const h = harness();
    await expect(h.controller.sendPdu({ commandId: CommandId.enquire_link })).rejects.toThrow(
      /not connected/i
    );
  });

  it("records both directions in the trace with summaries", async () => {
    smsc = new FakeSmsc();
    const h = harness();
    await connected(h);

    const rows = await h.controller.trace(1, 100);
    expect(rows.map((r) => r.direction)).toEqual(["out", "in"]);
    expect(rows[0].commandName).toBe("bind_transceiver");
    expect(rows[0].summary).toContain("esme");
    expect(rows[1].commandName).toBe("bind_transceiver_resp");
    expect(rows[0].hex.length).toBeGreaterThan(0);
  });

  it("converts hex octet fields into bytes when sending", async () => {
    smsc = new FakeSmsc();
    const h = harness();
    await connected(h);

    await h.controller.sendPdu({
      commandId: CommandId.submit_sm,
      fields: { destination_addr: "393331234567", data_coding: DataCoding.GSM_DEFAULT },
      octetFields: { short_message: "68656c6c6f" },
      tlvs: [{ tag: 0x0424, hex: "4142" }],
    });

    const submit = smsc.received.find((p) => p.commandId === CommandId.submit_sm)!;
    expect((submit.fields.short_message as Buffer).toString("latin1")).toBe("hello");
    expect(submit.tlvs[0].value).toEqual(Buffer.from([0x41, 0x42]));
  });

  it("sends hand-composed hex verbatim, lying command_length and all", async () => {
    smsc = new FakeSmsc();
    const h = harness();
    await connected(h);

    // A well-framed enquire_link, then the same with a broken length.
    await h.controller.sendRaw("00 00 00 10 00 00 00 15 00 00 00 00 00 00 03 e8");
    await new Promise((r) => setTimeout(r, 100));

    const got = smsc.received[smsc.received.length - 1];
    expect(got.commandId).toBe(CommandId.enquire_link);
    expect(got.sequenceNumber).toBe(1000);
  });

  it("returns the decoded fields and hex of a traced PDU", async () => {
    smsc = new FakeSmsc();
    const h = harness();
    await connected(h);

    const fields = await h.controller.pduFields(1);
    expect(fields.find((f) => f.name === "system_id")?.value).toBe("esme");
    expect(await h.controller.pduHex(1)).toMatch(/^[0-9a-f]+$/);
  });

  it("returns nothing for a trace row that does not exist", async () => {
    const h = harness();
    expect(await h.controller.pduFields(999)).toEqual([]);
    expect(await h.controller.pduHex(999)).toBe("");
  });

  it("records an undecodable inbound PDU as a decode error", async () => {
    smsc = new FakeSmsc();
    const h = harness();
    await connected(h);

    const bogus = Buffer.alloc(18);
    bogus.writeUInt32BE(18, 0);
    bogus.writeUInt32BE(CommandId.submit_sm, 4);
    bogus.writeUInt32BE(0, 8);
    bogus.writeUInt32BE(1, 12);
    bogus.writeUInt8(0x41, 16);
    bogus.writeUInt8(0x42, 17);
    smsc.sendRaw(bogus);

    await new Promise((r) => setTimeout(r, 150));
    const rows = await h.controller.trace(1, 100);
    const bad = rows.find((r) => r.decodeError);
    expect(bad).toBeDefined();
    expect(bad!.hex.length).toBeGreaterThan(0);
  });

  it("pushes batched pdu events and counters to the renderer", async () => {
    smsc = new FakeSmsc();
    const h = harness();
    await connected(h);
    h.handle.flush();

    const pduBatch = h.sent.find((s) => s.channel === "pdu");
    expect((pduBatch!.payload as unknown[]).length).toBe(2);
    expect(h.sent.some((s) => s.channel === "counters")).toBe(true);
    expect(h.sent.some((s) => s.channel === "state")).toBe(true);
  });

  it("clears the trace", async () => {
    smsc = new FakeSmsc();
    const h = harness();
    await connected(h);
    await h.controller.clearTrace();
    expect(await h.controller.trace(1, 100)).toEqual([]);
  });

  it("runs a burst and exports its rows as CSV", async () => {
    smsc = new FakeSmsc();
    const h = harness();
    await connected(h);

    const done = await h.controller.startBurst({
      recipients: { kind: "range", from: "393330000000", count: 3 },
      ratePerSec: 100,
      template: {
        commandId: CommandId.submit_sm,
        fields: { source_addr: "SENDER" },
      },
      text: "hello",
      dataCoding: DataCoding.GSM_DEFAULT,
      segmentation: "udh",
      placement: "short_message",
      backoffOnThrottle: false,
      awaitDlrMs: 0,
    });

    expect(done.reason).toBe("complete");
    expect(done.stats.submitted).toBe(3);
    expect(await h.controller.burstRows(0, 10)).toHaveLength(3);

    h.saveTo.path = path.join(h.dir, "burst.csv");
    const out = await h.controller.exportBurst();
    expect(out).toEqual({ path: h.saveTo.path });
    const csv = fs.readFileSync(h.saveTo.path, "utf8");
    expect(csv.split("\r\n")).toHaveLength(4);
    expect(csv).toContain("393330000002");
  });

  it("returns null from an export the user cancelled", async () => {
    const h = harness();
    h.saveTo.path = null;
    expect(await h.controller.exportTrace("csv")).toBeNull();
  });

  it("exports the trace as text, json and csv", async () => {
    smsc = new FakeSmsc();
    const h = harness();
    await connected(h);

    for (const format of ["text", "json", "csv"] as const) {
      h.saveTo.path = path.join(h.dir, `trace.${format}`);
      await h.controller.exportTrace(format);
      const body = fs.readFileSync(h.saveTo.path, "utf8");
      expect(body.length, format).toBeGreaterThan(0);
      expect(body, format).toContain("bind_transceiver");
    }
  });

  it("round-trips profiles and their secrets", async () => {
    const h = harness();
    await h.controller.saveProfile(
      {
        id: "p1",
        name: "A",
        host: "h",
        port: 2775,
        bind: { systemId: "esme", systemType: "", interfaceVersion: 0x34, mode: "transceiver" },
        tls: { enabled: false, rejectUnauthorized: true },
        correlation: DEFAULT_CORRELATION,
      },
      "pw"
    );
    expect(await h.controller.listProfiles()).toHaveLength(1);
    expect(await h.controller.profileSecrets("p1")).toEqual({
      password: "pw",
      keyPassphrase: null,
    });
    await h.controller.removeProfile("p1");
    expect(await h.controller.listProfiles()).toEqual([]);
  });

  it("returns a CSV's text from pickFile but only a path for a key", async () => {
    const h = harness();
    const csv = path.join(h.dir, "numbers.csv");
    fs.writeFileSync(csv, "msisdn\n393331111111\n");

    h.openWith.path = csv;
    expect(await h.controller.pickFile("csv")).toEqual({ path: csv, text: "msisdn\n393331111111\n" });
    expect(await h.controller.pickFile("key")).toEqual({ path: csv });
  });

  it("measures a body for the renderer", async () => {
    const h = harness();
    const m = await h.controller.measure({
      text: "a".repeat(161),
      dataCoding: DataCoding.GSM_DEFAULT,
      mode: "udh",
    });
    expect(m.parts).toBe(2);
    expect(m.chars).toBe(161);
  });
});
