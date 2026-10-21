import { app, BrowserWindow, ipcMain, dialog, safeStorage } from "electron";
import * as fs from "node:fs";
import * as path from "node:path";
import { Controller, Dialogs } from "./controller";
import { ProfileStore, electronSecretStore } from "./profiles";
import { registerIpc, electronIpcIo, CommandTarget, IpcHandle } from "./ipc";
import { COMMAND_NAMES } from "../shared/api";

const UI_INDEX = path.join(__dirname, "..", "..", "compiled", "build", "index.html");
const PRELOAD = path.join(__dirname, "..", "preload", "index.js");

const FILE_FILTERS: Record<string, Electron.FileFilter[]> = {
  csv: [{ name: "Recipient list", extensions: ["csv", "txt"] }],
  ca: [{ name: "Certificate authority", extensions: ["crt", "pem", "cer"] }],
  cert: [{ name: "Client certificate", extensions: ["crt", "pem", "cer"] }],
  key: [{ name: "Private key", extensions: ["key", "pem"] }],
};

function makeDialogs(win: BrowserWindow): Dialogs {
  return {
    async openFile(kind) {
      const r = await dialog.showOpenDialog(win, {
        properties: ["openFile"],
        filters: FILE_FILTERS[kind] ?? [],
      } as Electron.OpenDialogOptions);
      return r.canceled || r.filePaths.length === 0 ? null : r.filePaths[0];
    },
    async saveFile(suggestedName) {
      const r = await dialog.showSaveDialog(win, { defaultPath: suggestedName });
      return r.canceled || !r.filePath ? null : r.filePath;
    },
  };
}

function createWindow(): BrowserWindow {
  const win = new BrowserWindow({
    width: 1440,
    height: 960,
    title: "SMPP Client",
    webPreferences: {
      preload: PRELOAD,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });

  // The controller needs somewhere to push events, and the IPC surface needs
  // something to dispatch to, so the target forwards by name and is filled in
  // once the controller exists.
  let controller: Controller | undefined;
  const target = {} as CommandTarget;
  for (const name of COMMAND_NAMES) {
    target[name] = (...args: never[]) => {
      if (!controller) throw new Error("the app is still starting up");
      const fn = controller[name] as (...a: never[]) => Promise<unknown>;
      return fn.apply(controller, args);
    };
  }

  const push: IpcHandle = registerIpc(target, electronIpcIo(ipcMain, win.webContents));

  controller = new Controller({
    profiles: new ProfileStore(path.join(app.getPath("userData"), "profiles"), electronSecretStore(safeStorage)),
    dialogs: makeDialogs(win),
    readFile: (p) => fs.readFileSync(p),
    writeFile: (p, d) => fs.writeFileSync(p, d, "utf8"),
    push,
  });

  win.on("closed", () => {
    push.dispose();
    void controller?.disconnect();
    controller = undefined;
  });

  void win.loadFile(UI_INDEX);
  return win;
}

app.whenReady().then(() => {
  createWindow();
  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});
