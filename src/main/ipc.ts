import { COMMAND_NAMES, CommandName, Events, EventName, WirePdu } from "../shared/api";
import type { BurstRow } from "./burst";

/**
 * The seam over `ipcMain` and `webContents`, so the IPC surface is testable
 * without launching Electron.
 */
export interface IpcIo {
  handle(channel: string, fn: (...args: unknown[]) => unknown): void;
  send(channel: string, payload: unknown): void;
}

/** Anything that implements the commands. The controller does. */
export type CommandTarget = Record<CommandName, (...args: never[]) => Promise<unknown>>;

export interface IpcOptions {
  /** How often batched events are flushed. */
  batchMs?: number;
}

export interface IpcHandle {
  pushPdu(pdu: WirePdu): void;
  pushBurstRow(row: BurstRow): void;
  pushState(payload: Events["state"]): void;
  pushEvent<K extends EventName>(name: K, payload: Events[K]): void;
  /** Send whatever is buffered now. */
  flush(): void;
  dispose(): void;
}

export const DEFAULT_BATCH_MS = 100;

/**
 * Wire the commands up and return the push side.
 *
 * PDU and burst-row events are batched on a timer: 500 PDUs a second must not
 * be 500 IPC messages a second. Batching may delay an event; it never drops
 * one and never reorders one, which the tests pin.
 */
export function registerIpc(
  target: CommandTarget,
  io: IpcIo,
  opts: IpcOptions = {}
): IpcHandle {
  const batchMs = opts.batchMs ?? DEFAULT_BATCH_MS;

  let pdus: WirePdu[] = [];
  let rows: BurstRow[] = [];

  const flush = (): void => {
    if (pdus.length > 0) {
      io.send("pdu", pdus);
      pdus = [];
    }
    if (rows.length > 0) {
      io.send("burst-row", rows);
      rows = [];
    }
  };

  const timer = setInterval(flush, batchMs);

  for (const name of COMMAND_NAMES) {
    io.handle(name, async (...args: unknown[]) => {
      try {
        const fn = target[name] as (...a: unknown[]) => Promise<unknown>;
        // The result goes in `value`, never spread into the envelope: an array
        // result (a trace page, a profile list) must stay an array.
        return { _ok: true, value: await fn.apply(target, args) };
      } catch (e) {
        const message = (e as Error).message;
        // A command never throws across the boundary: the renderer gets a
        // verdict it can show, and the error also surfaces as an event so the
        // UI can log it even when nobody awaited the call.
        io.send("error", { message, where: name });
        return { _ok: false, error: message };
      }
    });
  }

  return {
    pushPdu(pdu) {
      pdus.push(pdu);
    },
    pushBurstRow(row) {
      rows.push(row);
    },
    pushState(payload) {
      io.send("state", payload);
    },
    pushEvent(name, payload) {
      io.send(name, payload);
    },
    flush,
    dispose() {
      clearInterval(timer);
      flush();
    },
  };
}

/** The production `IpcIo`, over Electron's ipcMain and a window's webContents. */
export function electronIpcIo(
  ipcMain: { handle(channel: string, fn: (event: unknown, ...args: unknown[]) => unknown): void },
  webContents: { send(channel: string, payload: unknown): void; isDestroyed(): boolean }
): IpcIo {
  return {
    handle: (channel, fn) => ipcMain.handle(channel, (_event, ...args) => fn(...args)),
    send: (channel, payload) => {
      if (!webContents.isDestroyed()) webContents.send(channel, payload);
    },
  };
}
