import { describe, it, expect, beforeEach } from "vitest";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { ProfileStore, Profile, SecretStore } from "../../src/main/profiles";
import { DEFAULT_CORRELATION } from "../../src/main/dlr";

/** A stand-in for Electron's safeStorage, so the logic is testable off-app. */
class FakeSecrets implements SecretStore {
  constructor(private readonly works = true) {}
  available(): boolean {
    return this.works;
  }
  encrypt(s: string): Buffer {
    if (!this.works) throw new Error("unavailable");
    return Buffer.from("enc:" + s, "utf8");
  }
  decrypt(b: Buffer): string {
    if (!this.works) throw new Error("unavailable");
    return b.toString("utf8").replace(/^enc:/, "");
  }
}

const PROFILE: Profile = {
  id: "p1",
  name: "Aggregator A",
  host: "smsc.example.com",
  port: 2775,
  bind: {
    systemId: "esme",
    systemType: "SMPP",
    interfaceVersion: 0x34,
    mode: "transceiver",
  },
  tls: { enabled: true, rejectUnauthorized: false, servername: "smsc.example.com" },
  correlation: DEFAULT_CORRELATION,
};

let dir: string;
beforeEach(() => {
  dir = fs.mkdtempSync(path.join(os.tmpdir(), "profiles-"));
});

describe("ProfileStore", () => {
  it("returns an empty list for a fresh directory", () => {
    expect(new ProfileStore(dir, new FakeSecrets()).list()).toEqual([]);
  });

  it("round-trips a profile through save and list", () => {
    const s = new ProfileStore(dir, new FakeSecrets());
    s.save(PROFILE, "hunter2");
    expect(new ProfileStore(dir, new FakeSecrets()).list()).toEqual([PROFILE]);
  });

  it("never writes the password into profiles.json", () => {
    const s = new ProfileStore(dir, new FakeSecrets());
    s.save(PROFILE, "hunter2");
    const raw = fs.readFileSync(path.join(dir, "profiles.json"), "utf8");
    expect(raw).not.toContain("hunter2");
  });

  it("returns the password through the secret store", () => {
    const s = new ProfileStore(dir, new FakeSecrets());
    s.save(PROFILE, "hunter2");
    expect(new ProfileStore(dir, new FakeSecrets()).password("p1")).toBe("hunter2");
  });

  it("stores a TLS key passphrase separately from the password", () => {
    const s = new ProfileStore(dir, new FakeSecrets());
    s.save(PROFILE, "hunter2", "keypass");
    const reopened = new ProfileStore(dir, new FakeSecrets());
    expect(reopened.password("p1")).toBe("hunter2");
    expect(reopened.keyPassphrase("p1")).toBe("keypass");
  });

  it("never writes the key passphrase into profiles.json", () => {
    const s = new ProfileStore(dir, new FakeSecrets());
    s.save(PROFILE, "hunter2", "keypass");
    expect(fs.readFileSync(path.join(dir, "profiles.json"), "utf8")).not.toContain("keypass");
  });

  it("throws rather than storing a password when safeStorage is unavailable", () => {
    const s = new ProfileStore(dir, new FakeSecrets(false));
    expect(() => s.save(PROFILE, "hunter2")).toThrow(/safestorage|encrypt/i);
    expect(fs.existsSync(path.join(dir, "profiles.json"))).toBe(false);
  });

  it("saves a profile with no password when none is given", () => {
    const s = new ProfileStore(dir, new FakeSecrets(false));
    s.save(PROFILE);
    expect(s.list()).toEqual([PROFILE]);
    expect(s.password("p1")).toBeNull();
  });

  it("returns null for the password of an unknown profile", () => {
    expect(new ProfileStore(dir, new FakeSecrets()).password("nope")).toBeNull();
  });

  it("replaces a profile saved under the same id", () => {
    const s = new ProfileStore(dir, new FakeSecrets());
    s.save(PROFILE, "a");
    s.save({ ...PROFILE, name: "Renamed" }, "b");
    expect(s.list()).toHaveLength(1);
    expect(s.list()[0].name).toBe("Renamed");
    expect(s.password("p1")).toBe("b");
  });

  it("removes a profile and its secrets", () => {
    const s = new ProfileStore(dir, new FakeSecrets());
    s.save(PROFILE, "hunter2");
    s.remove("p1");
    expect(s.list()).toEqual([]);
    expect(s.password("p1")).toBeNull();
    expect(fs.readFileSync(path.join(dir, "secrets.bin")).toString()).not.toContain("hunter2");
  });

  it("keeps several profiles apart", () => {
    const s = new ProfileStore(dir, new FakeSecrets());
    s.save(PROFILE, "a");
    s.save({ ...PROFILE, id: "p2", name: "Staging" }, "b");
    expect(s.list().map((p) => p.id)).toEqual(["p1", "p2"]);
    expect(s.password("p1")).toBe("a");
    expect(s.password("p2")).toBe("b");
  });

  it("survives a corrupt profiles.json by starting empty", () => {
    fs.writeFileSync(path.join(dir, "profiles.json"), "{ not json");
    expect(new ProfileStore(dir, new FakeSecrets()).list()).toEqual([]);
  });

  it("keeps the password when a profile is saved again without one", () => {
    const s = new ProfileStore(dir, new FakeSecrets());
    s.save(PROFILE, "hunter2");
    s.save({ ...PROFILE, name: "Edited" });
    expect(s.password("p1")).toBe("hunter2");
  });
});
