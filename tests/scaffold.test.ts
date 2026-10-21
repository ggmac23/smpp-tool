import { existsSync } from "node:fs";
import { it, expect } from "vitest";

it("builds the main process to dist/main/index.js", () => {
  expect(existsSync("dist/main/index.js")).toBe(true);
});

it("builds the qooxdoo bundle to compiled/build/index.html", () => {
  expect(existsSync("compiled/build/index.html")).toBe(true);
});
