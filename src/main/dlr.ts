import type { Receipt } from "../smpp/receipt";

/**
 * How hard to work at matching a delivery receipt to the submit that caused it.
 *
 * Aggregators get this wrong constantly: the receipt id comes back in a
 * different radix, or with padding, or only inside the receipt text. Every
 * step is switchable so a profile can be pinned to what one SMSC actually does.
 */
export interface CorrelationConfig {
  /** Prefer the receipted_message_id TLV over the receipt text. */
  preferTlv: boolean;
  /** Fall back to parsing `id:` out of the receipt text. */
  parseText: boolean;
  /** Try the hex reading of a decimal id and vice versa. */
  normaliseRadix: boolean;
  /** Try the id without leading zeros. */
  stripLeadingZeros: boolean;
}

export const DEFAULT_CORRELATION: CorrelationConfig = {
  preferTlv: true,
  parseText: true,
  normaliseRadix: true,
  stripLeadingZeros: true,
};

const HEX = /^[0-9a-fA-F]+$/;
const DEC = /^[0-9]+$/;

/**
 * Every form of `id` worth trying. The id as given always comes first, so a
 * verbatim match is found without any guessing.
 */
export function normaliseId(id: string, cfg: CorrelationConfig): string[] {
  const out: string[] = [id];
  const add = (s: string) => {
    if (s.length > 0 && !out.includes(s)) out.push(s);
  };

  const seeds = [id];
  if (cfg.stripLeadingZeros) {
    const stripped = id.replace(/^0+(?=.)/, "");
    add(stripped);
    seeds.push(stripped);
  }

  if (cfg.normaliseRadix) {
    for (const seed of seeds) {
      if (HEX.test(seed)) {
        add(seed.toLowerCase());
        add(seed.toUpperCase());
        try {
          add(BigInt("0x" + seed).toString(10));
        } catch {
          /* not representable; nothing to add */
        }
      }
      if (DEC.test(seed)) {
        try {
          const n = BigInt(seed);
          add(n.toString(16));
          add(n.toString(16).toUpperCase());
        } catch {
          /* not representable; nothing to add */
        }
      }
    }
  }

  return out;
}

interface Registration {
  token: string;
  keys: string[];
  matched: boolean;
}

/**
 * Matches receipts to the submits that caused them.
 *
 * Nothing is dropped and nothing is guessed: a receipt that matches no
 * registration goes to `unmatched`, where the UI shows it, and a registration
 * that has been matched is retired so a second receipt cannot steal another
 * row's match.
 */
export class DlrCorrelator {
  private readonly index = new Map<string, Registration[]>();
  private readonly registrations: Registration[] = [];
  private readonly _unmatched: Receipt[] = [];

  constructor(private readonly cfg: CorrelationConfig) {}

  register(messageId: string, token: string): void {
    const keys = normaliseId(messageId, this.cfg);
    const reg: Registration = { token, keys, matched: false };
    this.registrations.push(reg);
    for (const k of keys) {
      const bucket = this.index.get(k);
      if (bucket) bucket.push(reg);
      else this.index.set(k, [reg]);
    }
  }

  match(receipt: Receipt): string | null {
    const id = receipt.messageId;
    if (id === null || id === "") {
      this._unmatched.push(receipt);
      return null;
    }

    for (const candidate of normaliseId(id, this.cfg)) {
      const bucket = this.index.get(candidate);
      if (!bucket) continue;
      // Earliest unmatched registration wins, so a collision cannot steal a row.
      const reg = bucket.find((r) => !r.matched);
      if (!reg) continue;
      reg.matched = true;
      return reg.token;
    }

    this._unmatched.push(receipt);
    return null;
  }

  /** Receipts that matched no submit. Visible, never discarded. */
  get unmatched(): Receipt[] {
    return this._unmatched;
  }

  /** Registrations still waiting for a receipt. */
  get pending(): number {
    return this.registrations.reduce((n, r) => n + (r.matched ? 0 : 1), 0);
  }

  reset(): void {
    this.index.clear();
    this.registrations.length = 0;
    this._unmatched.length = 0;
  }
}
