import { describe, it, expect } from "vitest";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { TraceStore, TraceFileSink, TraceEntry } from "../../src/main/trace";

let nextSeq = 1;
function entry(over: Partial<TraceEntry> = {}): TraceEntry {
  return {
    seq: nextSeq++,
    at: 1_700_000_000_000,
    direction: "out",
    commandName: "submit_sm",
    commandStatus: 0,
    sequenceNumber: 1,
    summary: "submit_sm 39333 -> 39333 5 bytes",
    raw: Buffer.from([0, 0, 0, 16]),
    ...over,
  };
}

describe("TraceStore", () => {
  it("keeps entries in insertion order", () => {
    const s = new TraceStore();
    s.add(entry({ seq: 1, summary: "first" }));
    s.add(entry({ seq: 2, summary: "second" }));
    expect(s.entries().map((e) => e.summary)).toEqual(["first", "second"]);
  });

  it("evicts the oldest entry past the cap and counts it in dropped", () => {
    const s = new TraceStore(2);
    s.add(entry({ seq: 1, summary: "a" }));
    s.add(entry({ seq: 2, summary: "b" }));
    s.add(entry({ seq: 3, summary: "c" }));
    expect(s.entries().map((e) => e.summary)).toEqual(["b", "c"]);
    expect(s.dropped).toBe(1);
  });

  it("counts sent and received per command name", () => {
    const s = new TraceStore();
    s.add(entry({ direction: "out", commandName: "submit_sm" }));
    s.add(entry({ direction: "out", commandName: "submit_sm" }));
    s.add(entry({ direction: "in", commandName: "submit_sm_resp" }));
    s.add(entry({ direction: "in", commandName: "deliver_sm" }));
    expect(s.counters.submit_sm).toEqual({ sent: 2, received: 0 });
    expect(s.counters.submit_sm_resp).toEqual({ sent: 0, received: 1 });
    expect(s.counters.deliver_sm).toEqual({ sent: 0, received: 1 });
  });

  it("does not decrement counters when an entry is evicted", () => {
    const s = new TraceStore(1);
    s.add(entry({ direction: "out", commandName: "submit_sm" }));
    s.add(entry({ direction: "out", commandName: "submit_sm" }));
    expect(s.entries()).toHaveLength(1);
    expect(s.counters.submit_sm.sent).toBe(2);
  });

  it("clears entries and counters on clear()", () => {
    const s = new TraceStore();
    s.add(entry());
    s.clear();
    expect(s.entries()).toEqual([]);
    expect(s.counters).toEqual({});
    expect(s.dropped).toBe(0);
  });

  it("returns a page starting at a row ordinal", () => {
    const s = new TraceStore();
    for (let i = 1; i <= 10; i++) s.add(entry({ seq: i, summary: `row-${i}` }));
    expect(s.entries(4, 3).map((e) => e.summary)).toEqual(["row-4", "row-5", "row-6"]);
  });

  it("returns an empty page past the end", () => {
    const s = new TraceStore();
    s.add(entry({ seq: 1 }));
    expect(s.entries(99, 10)).toEqual([]);
  });

  it("raises its cap without losing what it holds", () => {
    const s = new TraceStore(2);
    s.add(entry({ seq: 1, summary: "a" }));
    s.add(entry({ seq: 2, summary: "b" }));
    s.setCap(10);
    s.add(entry({ seq: 3, summary: "c" }));
    expect(s.entries().map((e) => e.summary)).toEqual(["a", "b", "c"]);
  });

  it("trims to a lowered cap and counts what it dropped", () => {
    const s = new TraceStore(10);
    for (let i = 1; i <= 5; i++) s.add(entry({ seq: i, summary: `row-${i}` }));
    s.setCap(2);
    expect(s.entries().map((e) => e.summary)).toEqual(["row-4", "row-5"]);
    expect(s.dropped).toBe(3);
  });

  it("holds a decode-error entry with its raw bytes and no summary", () => {
    const s = new TraceStore();
    const raw = Buffer.from([0xde, 0xad, 0xbe, 0xef]);
    s.add(
      entry({
        direction: "in",
        commandName: "unknown(0x4242)",
        summary: "",
        decodeError: "command_length 4 is below the 16-byte header",
        raw,
      })
    );
    const got = s.entries()[0];
    expect(got.decodeError).toMatch(/below the 16-byte header/);
    expect(got.summary).toBe("");
    expect(got.raw).toEqual(raw);
  });

  it("reports how many entries it is holding", () => {
    const s = new TraceStore(5);
    s.add(entry());
    s.add(entry());
    expect(s.size).toBe(2);
  });
});

describe("TraceFileSink", () => {
  it("appends one line per entry", () => {
    const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "trace-")), "trace.log");
    const sink = new TraceFileSink(file);
    sink.write(entry({ seq: 1, summary: "first" }));
    sink.write(entry({ seq: 2, summary: "second" }));
    sink.close();

    const lines = fs.readFileSync(file, "utf8").trimEnd().split("\n");
    expect(lines).toHaveLength(2);
    expect(lines[0]).toContain("first");
    expect(lines[1]).toContain("second");
  });

  it("records the direction and the hex bytes", () => {
    const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "trace-")), "trace.log");
    const sink = new TraceFileSink(file);
    sink.write(entry({ direction: "in", raw: Buffer.from([0x00, 0xff]) }));
    sink.close();
    const line = fs.readFileSync(file, "utf8");
    expect(line).toContain("in");
    expect(line).toContain("00ff");
  });
});
