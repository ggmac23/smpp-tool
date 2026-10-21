import { existsSync, readFileSync } from "node:fs";
import { it, expect } from "vitest";

it("builds the main process to dist/main/index.js", () => {
  expect(existsSync("dist/main/index.js")).toBe(true);
});

it("builds the qooxdoo bundle to compiled/build/index.html", () => {
  expect(existsSync("compiled/build/index.html")).toBe(true);
});

it("bundles the shared form logic to compiled/build/forms.js", () => {
  expect(existsSync("compiled/build/forms.js")).toBe(true);
});

it("loads forms.js from the generated page before the application", () => {
  const html = readFileSync("compiled/build/index.html", "utf8");
  expect(html).toContain('src="forms.js"');
  expect(html.indexOf('src="forms.js"')).toBeLessThan(html.indexOf("index.js"));
});
