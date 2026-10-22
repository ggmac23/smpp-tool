import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";

/**
 * The renderer's declarations, checked against what the backend actually
 * sends.
 *
 * Three of the worst defects in this application were the same shape: a
 * widget read a field the backend does not have. `result.path` off a bare
 * string, `status.pending` where the report says `pendingBuffered`, three TLS
 * paths written beside `tls` instead of inside it. Each compiled, passed
 * every test, and failed at the button.
 *
 * `src-tauri/tests/contract.rs` serialises one sample of every type that
 * crosses and fails if the dump drifts. This reads the same file and checks
 * that every field the renderer declares is in it — so a rename on either
 * side has to be a rename on both.
 */
const dump = JSON.parse(readFileSync("tests/fixtures/wire-contract.json", "utf8")) as Record<
  string,
  Record<string, unknown>
>;

const source = readFileSync("src/shared/ui/protocol.ts", "utf8");
const loadSource = readFileSync("src/shared/ui/load.ts", "utf8");
const hexSource = readFileSync("src/shared/ui/hex.ts", "utf8");

/** The keys an interface declares, read from the declaration itself. */
function declaredKeys(text: string, name: string): string[] {
  const start = text.indexOf(`export interface ${name} {`);
  if (start < 0) throw new Error(`no interface ${name}`);
  let depth = 0;
  let end = start;
  for (let i = text.indexOf("{", start); i < text.length; i++) {
    if (text[i] === "{") depth++;
    if (text[i] === "}") {
      depth--;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }
  const body = text.slice(text.indexOf("{", start) + 1, end);

  // Top-level keys only: a nested object's own keys are checked through the
  // value they sit in.
  const keys: string[] = [];
  let nesting = 0;
  for (const line of body.split("\n")) {
    const trimmed = line.trim();
    if (nesting === 0) {
      const m = /^([A-Za-z_][A-Za-z0-9_]*)\??\s*:/.exec(trimmed);
      if (m) keys.push(m[1]);
    }
    nesting += (line.match(/\{/g) || []).length - (line.match(/\}/g) || []).length;
  }
  return keys;
}

const PAIRS: [string, string, string][] = [
  [source, "WirePdu", "WirePdu"],
  [source, "BurstRow", "BurstRow"],
  [source, "BurstStats", "BurstStats"],
  [source, "StatusReport", "StatusReport"],
  [source, "Measurement", "Measurement"],
  [source, "SendOutcome", "SendOutcome"],
  [source, "ProfileSecrets", "ProfileSecrets"],
  [source, "WrittenFile", "WrittenFile"],
  [source, "EncodedBody", "EncodedBody"],
  [source, "Counter", "Counter"],
  [source, "Profile", "Profile"],
  [source, "DescribedField", "DescribedField"],
  [loadSource, "LoadReport", "LoadReport"],
  [loadSource, "ConnectionStats", "ConnectionStats"],
  [loadSource, "RampStep", "RampStep"],
  [loadSource, "LoadProgress", "LoadProgress"],
];

describe("what the renderer declares against what the backend sends", () => {
  for (const [text, name, shape] of PAIRS) {
    it(`${name} declares nothing the backend does not send`, () => {
      const sent = dump[shape];
      expect(sent, `${shape} is missing from the dump`).toBeDefined();
      for (const key of declaredKeys(text, name)) {
        expect(
          Object.prototype.hasOwnProperty.call(sent, key),
          `${name}.${key} is declared here and absent from what the backend sends`
        ).toBe(true);
      }
    });
  }

  it("covers every shape the backend dumps", () => {
    // A new crossing type with no counterpart here is a shape nothing checks.
    const checked = new Set(PAIRS.map(([, , shape]) => shape));
    const uncovered = Object.keys(dump).filter((k) => !checked.has(k) && k !== "BurstDone");
    expect(uncovered, "these shapes cross and nothing here checks them").toEqual([]);
  });

  it("keeps the TLS paths inside tls, where the backend reads them", () => {
    const profile = dump.Profile as Record<string, Record<string, unknown>>;
    expect(profile.tls.caPath).toBeDefined();
    expect(profile.caPath).toBeUndefined();
  });

  it("reports the stall as two fields, not as a pending object", () => {
    const status = dump.StatusReport;
    expect(status.pendingBuffered).toBeDefined();
    expect(status.pending).toBeUndefined();
  });

  it("returns an object from an export, so result.path reads", () => {
    expect(dump.WrittenFile.path).toBeDefined();
  });

  it("names the status on a burst row, so the renderer needs no status table", () => {
    expect(dump.BurstRow.statusName).toBeDefined();
  });
});

describe("the preview the hex composer reads", () => {
  it("declares nothing the backend does not send", () => {
    // PduPreview is produced by a command rather than carried in the dump,
    // so its shape is checked where it is declared.
    const keys = declaredKeys(hexSource, "PduPreview");
    expect(keys).toEqual(["ok", "error", "summary", "fields", "byteCount", "sendable"]);
  });
});
