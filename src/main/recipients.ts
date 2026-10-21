/** The sizing ceiling the spec sets: results are held in memory up to here. */
export const MAX_BURST = 50_000;
export const MAX_RATE = 500;

export type RecipientSource =
  | { kind: "repeat"; number: string; count: number }
  | { kind: "range"; from: string; count: number }
  | { kind: "csv"; text: string; column?: string };

export interface ResolvedRecipients {
  numbers: string[];
  /** Problems worth telling the user about. Never a reason to drop a row. */
  warnings: string[];
}

/**
 * An address the tool is willing to send without comment. Letters are allowed,
 * because alphanumeric sender and destination addresses are legitimate.
 */
const PLAUSIBLE_ADDRESS = /^[+]?[0-9A-Za-z._-]{1,32}$/;

/** Column names that mean "this row is a header", whatever it contains. */
const HEADER_WORDS = new Set([
  "msisdn",
  "number",
  "numero",
  "phone",
  "mobile",
  "destination",
  "destination_addr",
  "dest",
  "dst",
  "to",
  "address",
  "addr",
  "recipient",
  "subscriber",
]);

/** Split one CSV line, honouring double quotes. */
function splitCsvLine(line: string): string[] {
  const out: string[] = [];
  let field = "";
  let quoted = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (quoted) {
      if (ch === '"') {
        if (line[i + 1] === '"') {
          field += '"';
          i++;
        } else {
          quoted = false;
        }
      } else {
        field += ch;
      }
    } else if (ch === '"') {
      quoted = true;
    } else if (ch === ",") {
      out.push(field);
      field = "";
    } else {
      field += ch;
    }
  }
  out.push(field);
  return out.map((f) => f.trim());
}

/** Increment the numeric tail of an address, keeping its width. */
function increment(address: string, by: number): string | null {
  const m = /^(.*?)(\d+)$/.exec(address);
  if (!m) return null;
  const [, head, digits] = m;
  const next = (BigInt(digits) + BigInt(by)).toString();
  return head + next.padStart(digits.length, "0");
}

function checkCount(n: number): void {
  if (n > MAX_BURST) {
    throw new Error(
      `${n} recipients is above the ${MAX_BURST}-message ceiling this build holds in memory`
    );
  }
  if (n < 1) throw new Error("a burst needs at least one recipient");
}

export function resolveRecipients(src: RecipientSource): ResolvedRecipients {
  const warnings: string[] = [];

  if (src.kind === "repeat") {
    checkCount(src.count);
    return { numbers: Array.from({ length: src.count }, () => src.number), warnings };
  }

  if (src.kind === "range") {
    checkCount(src.count);
    if (increment(src.from, 0) === null) {
      warnings.push(`"${src.from}" has no numeric tail to increment; every message uses it as is`);
      return { numbers: Array.from({ length: src.count }, () => src.from), warnings };
    }
    const numbers: string[] = [];
    for (let i = 0; i < src.count; i++) numbers.push(increment(src.from, i)!);
    return { numbers, warnings };
  }

  const lines = src.text.split(/\r?\n/).map((l) => l.trim());
  const rows: { line: string; number: number }[] = [];
  for (const [i, line] of lines.entries()) {
    if (line.length > 0) rows.push({ line, number: i + 1 });
  }
  if (rows.length === 0) throw new Error("the CSV has no rows");

  let columnIndex = 0;
  let start = 0;
  const first = splitCsvLine(rows[0].line);

  if (src.column) {
    // Naming a column says the file has a header row, whether or not the name
    // is in it.
    start = 1;
    const found = first.findIndex((h) => h.toLowerCase() === src.column!.toLowerCase());
    if (found >= 0) {
      columnIndex = found;
    } else {
      warnings.push(`no column named "${src.column}"; using the first column instead`);
    }
  } else {
    // No column named: the first row is a header if its cell is a known header
    // word, or if it holds no digits while some later row's cell does. A file
    // of purely alphanumeric senders therefore keeps all its rows.
    const cell = first[0] ?? "";
    const hasDigits = (s: string) => /\d/.test(s);
    const laterHasDigits = rows
      .slice(1)
      .some((r) => hasDigits(splitCsvLine(r.line)[0] ?? ""));
    if (HEADER_WORDS.has(cell.toLowerCase()) || (!hasDigits(cell) && laterHasDigits)) {
      start = 1;
    }
  }

  const body = rows.slice(start);
  checkCount(body.length);

  const numbers: string[] = [];
  for (const row of body) {
    const cells = splitCsvLine(row.line);
    const value = cells[columnIndex] ?? "";
    if (!PLAUSIBLE_ADDRESS.test(value)) {
      warnings.push(`row ${row.number}: "${value}" does not look like an address; sending it anyway`);
    }
    numbers.push(value);
  }

  return { numbers, warnings };
}
