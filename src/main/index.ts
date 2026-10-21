import { app, BrowserWindow } from "electron";
import * as path from "node:path";

const UI_INDEX = path.join(__dirname, "..", "..", "compiled", "build", "index.html");
const PRELOAD = path.join(__dirname, "..", "preload", "index.js");

function createWindow(): BrowserWindow {
  const win = new BrowserWindow({
    width: 1400,
    height: 900,
    title: "SMPP Client",
    webPreferences: {
      preload: PRELOAD,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
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
