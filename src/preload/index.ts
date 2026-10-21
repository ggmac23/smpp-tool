import { contextBridge, ipcRenderer } from "electron";
import { COMMAND_NAMES, EVENT_NAMES, EventName, Events, Result } from "../shared/api";

/**
 * The only surface the renderer gets.
 *
 * Each command is an `invoke` that returns a `Result`: the main process never
 * throws across the boundary, so this unwraps the verdict and rejects on the
 * renderer side where a caller can catch it.
 */
function buildApi(): Record<string, unknown> {
  const api: Record<string, unknown> = {};

  for (const name of COMMAND_NAMES) {
    api[name] = async (...args: unknown[]) => {
      const result = (await ipcRenderer.invoke(name, ...args)) as Result<unknown>;
      if (result._ok === false) throw new Error(result.error);
      return result.value;
    };
  }

  api.on = <K extends EventName>(name: K, cb: (payload: Events[K]) => void): (() => void) => {
    if (!EVENT_NAMES.includes(name)) throw new Error(`unknown event "${String(name)}"`);
    const listener = (_event: unknown, payload: Events[K]) => cb(payload);
    ipcRenderer.on(name, listener);
    return () => ipcRenderer.off(name, listener);
  };

  return api;
}

contextBridge.exposeInMainWorld("smpp", buildApi());
