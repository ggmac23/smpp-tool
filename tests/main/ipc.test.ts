import { describe, it, expect, vi } from "vitest";
import { registerIpc, IpcIo } from "../../src/main/ipc";
import { COMMAND_NAMES } from "../../src/shared/api";

/** A stand-in for ipcMain + webContents, so no Electron is launched. */
function fakeIo() {
  const handlers = new Map<string, (...args: unknown[]) => unknown>();
  const sent: { channel: string; payload: unknown }[] = [];
  const io: IpcIo = {
    handle: (channel, fn) => handlers.set(channel, fn),
    send: (channel, payload) => sent.push({ channel, payload }),
  };
  return { io, handlers, sent };
}

/** A controller whose every command resolves, so coverage is what is tested. */
function fakeController() {
  const calls: string[] = [];
  const target: Record<string, unknown> = { on: () => undefined, off: () => undefined };
  for (const name of COMMAND_NAMES) {
    target[name] = vi.fn(async () => {
      calls.push(name);
      return { ok: true };
    });
  }
  return { controller: target as never, calls };
}

describe("registerIpc", () => {
  it("registers a handler for every command", () => {
    const { io, handlers } = fakeIo();
    const { controller } = fakeController();
    registerIpc(controller, io);
    expect([...handlers.keys()].sort()).toEqual([...COMMAND_NAMES].sort());
  });

  it("forwards a command to the controller and returns its result", async () => {
    const { io, handlers } = fakeIo();
    const { controller, calls } = fakeController();
    registerIpc(controller, io);

    const result = await handlers.get("listProfiles")!();
    expect(calls).toEqual(["listProfiles"]);
    expect(result).toEqual({ _ok: true, value: { ok: true } });
  });

  it("returns an array result as an array, not as an object with numeric keys", async () => {
    const { io, handlers } = fakeIo();
    const { controller } = fakeController();
    (controller as unknown as Record<string, unknown>).trace = async () => [{ seq: 1 }, { seq: 2 }];
    registerIpc(controller, io);

    const result = (await handlers.get("trace")!()) as { _ok: boolean; value: unknown };
    expect(Array.isArray(result.value)).toBe(true);
    expect(result.value).toEqual([{ seq: 1 }, { seq: 2 }]);
  });

  it("returns an undefined result without inventing a value", async () => {
    const { io, handlers } = fakeIo();
    const { controller } = fakeController();
    (controller as unknown as Record<string, unknown>).unbind = async () => undefined;
    registerIpc(controller, io);

    expect(await handlers.get("unbind")!()).toEqual({ _ok: true, value: undefined });
  });

  it("returns a null result as null", async () => {
    const { io, handlers } = fakeIo();
    const { controller } = fakeController();
    (controller as unknown as Record<string, unknown>).exportTrace = async () => null;
    registerIpc(controller, io);

    expect(await handlers.get("exportTrace")!()).toEqual({ _ok: true, value: null });
  });

  it("forwards a main-process error as a failed result, not a throw", async () => {
    const { io, handlers } = fakeIo();
    const { controller } = fakeController();
    (controller as unknown as Record<string, unknown>).bind = async () => {
      throw new Error("bind refused: ESME_RBINDFAIL");
    };
    registerIpc(controller, io);

    const result = (await handlers.get("bind")!()) as { _ok: boolean; error: string };
    expect(result._ok).toBe(false);
    expect(result.error).toContain("ESME_RBINDFAIL");
  });

  it("also reports a failed command on the error event", async () => {
    const { io, handlers, sent } = fakeIo();
    const { controller } = fakeController();
    (controller as unknown as Record<string, unknown>).unbind = async () => {
      throw new Error("boom");
    };
    const handle = registerIpc(controller, io);
    await handlers.get("unbind")!();
    handle.flush();

    expect(sent.some((s) => s.channel === "error")).toBe(true);
    handle.dispose();
  });
});

describe("event batching", () => {
  function setup(batchMs = 1000) {
    const { io, sent } = fakeIo();
    const { controller } = fakeController();
    const handle = registerIpc(controller, io, { batchMs });
    return { handle, sent };
  }

  it("batches pdu events into one send instead of one per PDU", () => {
    const { handle, sent } = setup();
    for (let i = 0; i < 50; i++) handle.pushPdu({ seq: i } as never);
    expect(sent.filter((s) => s.channel === "pdu")).toHaveLength(0);

    handle.flush();
    const batches = sent.filter((s) => s.channel === "pdu");
    expect(batches).toHaveLength(1);
    expect((batches[0].payload as unknown[]).length).toBe(50);
    handle.dispose();
  });

  it("preserves PDU order inside a batch", () => {
    const { handle, sent } = setup();
    for (let i = 0; i < 10; i++) handle.pushPdu({ seq: i } as never);
    handle.flush();
    const payload = sent.find((s) => s.channel === "pdu")!.payload as { seq: number }[];
    expect(payload.map((p) => p.seq)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    handle.dispose();
  });

  it("never drops a PDU when batching", () => {
    const { handle, sent } = setup();
    for (let i = 0; i < 2000; i++) {
      handle.pushPdu({ seq: i } as never);
      if (i % 500 === 499) handle.flush();
    }
    handle.flush();

    const all = sent
      .filter((s) => s.channel === "pdu")
      .flatMap((s) => s.payload as { seq: number }[]);
    expect(all).toHaveLength(2000);
    expect(all.map((p) => p.seq)).toEqual(Array.from({ length: 2000 }, (_, i) => i));
    handle.dispose();
  });

  it("batches burst rows the same way", () => {
    const { handle, sent } = setup();
    for (let i = 0; i < 30; i++) handle.pushBurstRow({ id: `r${i}` } as never);
    handle.flush();
    const batch = sent.find((s) => s.channel === "burst-row")!;
    expect((batch.payload as unknown[]).length).toBe(30);
    handle.dispose();
  });

  it("sends nothing on a flush with nothing buffered", () => {
    const { handle, sent } = setup();
    handle.flush();
    expect(sent).toHaveLength(0);
    handle.dispose();
  });

  it("flushes on its own timer", async () => {
    const { handle, sent } = setup(20);
    handle.pushPdu({ seq: 1 } as never);
    await new Promise((r) => setTimeout(r, 80));
    expect(sent.filter((s) => s.channel === "pdu")).toHaveLength(1);
    handle.dispose();
  });

  it("batches counters instead of sending one message per PDU", () => {
    const { handle, sent } = setup();
    for (let i = 0; i < 50; i++) handle.pushCounters({ submit_sm: { sent: i, received: 0 } });
    expect(sent.filter((s) => s.channel === "counters")).toHaveLength(0);

    handle.flush();
    const batches = sent.filter((s) => s.channel === "counters");
    expect(batches).toHaveLength(1);
    // The latest snapshot wins; intermediate ones are not worth a round trip.
    expect(batches[0].payload).toEqual({ submit_sm: { sent: 49, received: 0 } });
    handle.dispose();
  });

  it("sends no counters when none were pushed", () => {
    const { handle, sent } = setup();
    handle.pushPdu({ seq: 1 } as never);
    handle.flush();
    expect(sent.filter((s) => s.channel === "counters")).toHaveLength(0);
    handle.dispose();
  });

  it("sends state changes immediately rather than batching them", () => {
    const { handle, sent } = setup();
    handle.pushState({ state: "bound", mode: "transceiver" });
    expect(sent.find((s) => s.channel === "state")).toBeDefined();
    handle.dispose();
  });
});
