import * as fs from "node:fs";
import * as path from "node:path";
import type { BindOptions } from "./session";
import type { TlsOptions } from "./transport";
import type { CorrelationConfig } from "./dlr";

/**
 * A saved connection. Note what is absent: no password, no key passphrase.
 * Secrets live in the SecretStore and never in this record.
 */
export interface Profile {
  id: string;
  name: string;
  host: string;
  port: number;
  bind: Omit<BindOptions, "password">;
  tls: Omit<TlsOptions, "clientKeyPassphrase" | "ca" | "clientCert" | "clientKey">;
  correlation: CorrelationConfig;
  /** Paths, not contents: the files are read at connect time. */
  caPath?: string;
  clientCertPath?: string;
  clientKeyPath?: string;
}

/**
 * The seam over Electron's `safeStorage`, so the store's logic is testable
 * without a running app.
 */
export interface SecretStore {
  encrypt(s: string): Buffer;
  decrypt(b: Buffer): string;
  available(): boolean;
}

interface SecretRecord {
  password?: string;
  keyPassphrase?: string;
}

const PROFILES_FILE = "profiles.json";
const SECRETS_FILE = "secrets.bin";

/**
 * Profiles on disk as JSON; passwords and key passphrases as `safeStorage`
 * ciphertext beside them.
 *
 * If `safeStorage` is unavailable the store refuses to save a secret rather
 * than writing it in the clear — a profile with no password still saves fine.
 */
export class ProfileStore {
  constructor(
    private readonly dir: string,
    private readonly secrets: SecretStore
  ) {}

  list(): Profile[] {
    return this.readProfiles();
  }

  save(p: Profile, password?: string, keyPassphrase?: string): void {
    const hasSecret = password !== undefined || keyPassphrase !== undefined;
    if (hasSecret && !this.secrets.available()) {
      throw new Error(
        "safeStorage is unavailable on this system, so the password cannot be encrypted; " +
          "save the profile without one and type it at connect time"
      );
    }

    // Secrets first: a failure there must not leave a profile referring to a
    // password that was never stored.
    if (hasSecret) {
      const all = this.readSecrets();
      const record: SecretRecord = { ...all[p.id] };
      if (password !== undefined) record.password = password;
      if (keyPassphrase !== undefined) record.keyPassphrase = keyPassphrase;
      all[p.id] = record;
      this.writeSecrets(all);
    }

    const profiles = this.readProfiles();
    const index = profiles.findIndex((x) => x.id === p.id);
    if (index >= 0) profiles[index] = p;
    else profiles.push(p);
    this.writeProfiles(profiles);
  }

  password(id: string): string | null {
    return this.readSecrets()[id]?.password ?? null;
  }

  keyPassphrase(id: string): string | null {
    return this.readSecrets()[id]?.keyPassphrase ?? null;
  }

  remove(id: string): void {
    this.writeProfiles(this.readProfiles().filter((p) => p.id !== id));
    const all = this.readSecrets();
    delete all[id];
    this.writeSecrets(all);
  }

  /* ------------------------------------------------------------------ */

  private file(name: string): string {
    return path.join(this.dir, name);
  }

  private readProfiles(): Profile[] {
    try {
      const raw = fs.readFileSync(this.file(PROFILES_FILE), "utf8");
      const parsed = JSON.parse(raw);
      return Array.isArray(parsed) ? (parsed as Profile[]) : [];
    } catch {
      // Missing or unreadable: start empty rather than refuse to open.
      return [];
    }
  }

  private writeProfiles(profiles: Profile[]): void {
    fs.mkdirSync(this.dir, { recursive: true });
    fs.writeFileSync(this.file(PROFILES_FILE), JSON.stringify(profiles, null, 2), "utf8");
  }

  private readSecrets(): Record<string, SecretRecord> {
    try {
      const blob = fs.readFileSync(this.file(SECRETS_FILE));
      if (blob.length === 0) return {};
      const parsed = JSON.parse(this.secrets.decrypt(blob));
      return typeof parsed === "object" && parsed !== null
        ? (parsed as Record<string, SecretRecord>)
        : {};
    } catch {
      return {};
    }
  }

  private writeSecrets(all: Record<string, SecretRecord>): void {
    fs.mkdirSync(this.dir, { recursive: true });
    fs.writeFileSync(this.file(SECRETS_FILE), this.secrets.encrypt(JSON.stringify(all)));
  }
}

/**
 * The real SecretStore, backed by Electron's `safeStorage` (the system
 * keychain). Not unit-tested: it needs a running Electron app, so the logic
 * above is tested through a fake and this adapter stays a thin shim.
 */
export function electronSecretStore(safeStorage: {
  isEncryptionAvailable(): boolean;
  encryptString(s: string): Buffer;
  decryptString(b: Buffer): string;
}): SecretStore {
  return {
    available: () => safeStorage.isEncryptionAvailable(),
    encrypt: (s) => safeStorage.encryptString(s),
    decrypt: (b) => safeStorage.decryptString(b),
  };
}
