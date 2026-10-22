import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";

const yml = readFileSync("electron-builder.yml", "utf8");
const pkg = JSON.parse(readFileSync("package.json", "utf8")) as {
  main: string;
  scripts: Record<string, string>;
};

describe("electron-builder configuration", () => {
  it("lists dist/, compiled/build/ and package.json in the builder's files", () => {
    expect(yml).toMatch(/dist\/\*\*/);
    expect(yml).toMatch(/compiled\/build\/\*\*/);
    expect(yml).toMatch(/package\.json/);
  });

  it("declares mac, win and linux targets", () => {
    expect(yml).toMatch(/^mac:/m);
    expect(yml).toMatch(/^win:/m);
    expect(yml).toMatch(/^linux:/m);
  });

  it("builds a dmg and a zip for mac, nsis for win and AppImage for linux", () => {
    expect(yml).toContain("dmg");
    expect(yml).toContain("zip");
    expect(yml).toContain("nsis");
    expect(yml).toContain("AppImage");
  });

  it("does not enable signing or notarization", () => {
    expect(yml).toMatch(/identity:\s*null/);
    expect(yml).not.toMatch(/notarize:\s*true/);
    expect(yml).not.toMatch(/^\s*(cscLink|certificateFile|certificateSubjectName):/m);
  });

  it("keeps the test-only TLS keys out of the package", () => {
    expect(yml).toMatch(/!.*tests/);
  });

  it("points at the compiled main process as the entry point", () => {
    expect(pkg.main).toBe("dist/main/index.js");
  });

  it("offers a dist script that runs electron-builder", () => {
    expect(pkg.scripts.dist).toContain("electron-builder");
  });

  it("builds everything the package needs before packaging it", () => {
    expect(pkg.scripts.dist).toMatch(/npm run build|^build/);
  });
});
