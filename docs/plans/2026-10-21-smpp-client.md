# SMPP Client Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use the implementation workflow (recommended) or the implementation workflow to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A desktop SMPP client for testing SMSCs and aggregators — full parity with the Melrose Labs SMPP Client Proxy, plus direct TCP/TLS, a raw hex PDU composer and burst sending with DLR correlation.

**Architecture:** Two processes. The main process (TypeScript, Node) owns the socket, the PDU codec, sequence numbering, the trace, the burst engine and profile storage. The renderer (qooxdoo) is presentation only: it sends intents and receives events over a narrow typed IPC surface exposed by a preload script. The codec is pure and I/O-free, so it is tested without a socket; the session and burst engine are tested against a loopback SMSC fixture.

**Tech Stack:** Electron, TypeScript (main + preload + codec), Vitest, qooxdoo 7.9.3 with the `qx` CLI (renderer, JavaScript), electron-builder.

**Spec:** `docs/design/2026-10-21-smpp-client-design.md`

## Global Constraints

- qooxdoo **7.9.3** (stable). Not 8.0.0-beta.
- SMPP interface versions: default `0x00`, v3.3 `0x33`, v3.4 `0x34`, v5.0 `0x50`.
- Electron renderer: `contextIsolation: true`, `nodeIntegration: false`, `sandbox: true`. All privileged work goes through the preload bridge.
- Field validation **warns, never blocks**. Every form must be able to send a value the app itself considers invalid.
- **No silent reconnect and no silent re-bind.** Displayed connection state is always the real one.
- Burst sizing target: up to **50,000 messages** per burst at up to **500 messages/sec**, results held in memory. Above that the burst UI warns or refuses; it does not silently struggle.
- The codec **never silently repairs** what it is handed. Bytes the user composed go on the wire as composed.
- UI language: English. Field labels are the SMPP spec names (`esm_class`, `registered_delivery`, …).
- Primary platform macOS; `electron-builder` configured for mac/win/linux but run by hand. No signing, notarization, auto-update or CI.
- Passwords and TLS key passphrases: `safeStorage` only, never plaintext on disk.

## Review Focus

Input classes the spec implies but which no task's happy-path tests would reach. Each has a test pinned to the task that owns the code.

1. **A PDU split across TCP reads, or several PDUs in one read.** The user expects the trace to show every PDU exactly once, framed by `command_length` and not by read boundaries. → Task 3, Step 7.
2. **A `command_length` that lies** — smaller than the 16-byte header, larger than the data present, or absurdly large. The user expects a visible error, not a hang or a multi-gigabyte allocation. → Task 3, Step 9.
3. **An unknown `command_id`, or a response whose sequence number matches no pending request.** The user expects it in the trace and the session still alive. → Task 10, Step 9.
4. **Emoji or other non-GSM7 characters in the body with `data_coding` 0 selected.** The user expects the size counter and the encoder to show what will actually go on the wire, not a silent mangle. → Task 5, Step 7.
5. **A request the SMSC never answers.** The user expects the pending request to time out visibly rather than leak and quietly fill the window. → Task 10, Step 11.

---

### Task 1: Project scaffold and build pipeline

**Files:**
- Create: `package.json`, `tsconfig.json`, `tsconfig.main.json`, `vitest.config.ts`, `.gitignore`
- Create: `src/main/index.ts`, `src/preload/index.ts`
- Create: `compile.json`, `src/ui/source/class/smppclient/Application.js`
- Test: `tests/scaffold.test.ts`

**Interfaces:**
- Consumes: nothing.
- Produces: `npm test` (vitest), `npm run build:ui` (`qx compile --target=build`), `npm run build:main` (`tsc -p tsconfig.main.json`), `npm start` (electron). Main-process TypeScript compiles to `dist/`; the qooxdoo bundle compiles to `compiled/build/`.

- [ ] **Step 1: `git init` and write `.gitignore`**

Ignore `node_modules/`, `dist/`, `compiled/`, `release/`, `.the implementation workflow/`.

- [ ] **Step 2: Write the failing test**

```ts
// tests/scaffold.test.ts
import { existsSync } from "node:fs";
it("builds the main process to dist/main/index.js", () => {
  expect(existsSync("dist/main/index.js")).toBe(true);
});
it("builds the qooxdoo bundle to compiled/build/index.html", () => {
  expect(existsSync("compiled/build/index.html")).toBe(true);
});
```

- [ ] **Step 3: Run test to verify it fails**

Run: `npx vitest run tests/scaffold.test.ts`
Expected: FAIL — both files missing.

- [ ] **Step 4: Create the Node side**

`npm init`; add `electron`, `electron-builder`, `typescript`, `vitest`, `@types/node` as devDependencies. `tsconfig.json` targets ES2022, `strict: true`, `module: node16`. `tsconfig.main.json` extends it with `include: ["src/main", "src/preload", "src/smpp"]` and `outDir: dist`.

`src/main/index.ts`: create a `BrowserWindow` with the Global Constraints' security flags and a preload pointing at `dist/preload/index.js`, then load `compiled/build/index.html` with `loadFile`. `src/preload/index.ts` is an empty module for now.

- [ ] **Step 5: Create the qooxdoo application**

Run `npx qx create smppclient --type desktop --out src/ui --noninteractive` with `@qooxdoo/framework@7.9.3`, then set `compile.json`'s build target `outputPath` to `compiled/build`. `Application.js` shows a single label. The build target must emit relative asset paths so `file://` loading works.

- [ ] **Step 6: Run both builds, then the test**

Run: `npm run build:main && npm run build:ui && npx vitest run tests/scaffold.test.ts`
Expected: PASS.

- [ ] **Step 7: Verify the window opens**

Run: `npm start`
Expected: a window showing the label. Close it.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "chore: scaffold electron + qooxdoo + vitest build pipeline"
```

---

### Task 2: SMPP constants and TLV codec

**Files:**
- Create: `src/smpp/constants.ts`, `src/smpp/tlv.ts`
- Test: `tests/smpp/tlv.test.ts`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `CommandId` — const object mapping every command name in the spec's PDU list (`bind_transmitter`, `bind_receiver`, `bind_transceiver`, `unbind`, `submit_sm`, `data_sm`, `query_sm`, `cancel_sm`, `replace_sm`, `submit_multi`, `broadcast_sm`, `broadcast_multi`, `enquire_link`, `deliver_sm`, `generic_nack`) and their `_resp` counterparts, to their numeric ids. Responses are `0x80000000 | request`.
  - `commandName(id: number): string` — returns `"unknown(0x…)"` for unrecognised ids.
  - `CommandStatus` — numeric → name (`ESME_ROK`, `ESME_RTHROTTLED`, …); `statusName(n: number): string`.
  - `Ton`, `Npi`, `DataCoding`, `EsmClass`, `RegisteredDelivery`, `PriorityFlag`, `InterfaceVersion` — the exact value sets listed in the spec's `submit_sm` section.
  - `KnownTlv` — tag number → `{ name, type }` for at least `message_payload` (0x0424), `receipted_message_id` (0x001E), `sar_msg_ref_num` (0x020C), `sar_total_segments` (0x020E), `sar_segment_seqnum` (0x020F), `more_messages_to_send` (0x0426).
  - `interface Tlv { tag: number; value: Buffer }`
  - `encodeTlvs(tlvs: Tlv[]): Buffer`
  - `decodeTlvs(buf: Buffer): Tlv[]`
  - `tlvName(tag: number): string`

- [ ] **Step 1: Write the failing tests**

```ts
it("encodes a TLV as tag, length, value big-endian")      // 0x0424, "hi" → 04 24 00 02 68 69
it("round-trips multiple TLVs")
it("decodes an unknown tag without losing its bytes")     // tag 0x9999 survives decode→encode
it("decodes a zero-length TLV")
it("throws on a TLV whose declared length exceeds the buffer")
it("names responses from their request id")               // commandName(0x80000004) === "submit_sm_resp"
it("names an unrecognised command id")                    // commandName(0x4242) === "unknown(0x4242)"
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/smpp/tlv.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `src/smpp/constants.ts`**

Plain const objects plus the three lookup functions. Values come from the spec's tables; do not invent values the spec does not list.

- [ ] **Step 4: Implement `src/smpp/tlv.ts`**

`encodeTlvs` / `decodeTlvs` / `tlvName` per the Interfaces block.

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run tests/smpp/tlv.test.ts`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/smpp/constants.ts src/smpp/tlv.ts tests/smpp/tlv.test.ts
git commit -m "feat(smpp): command constants and TLV codec"
```

---

### Task 3: PDU framing and decode

**Files:**
- Create: `src/smpp/pdu-defs.ts`, `src/smpp/decode.ts`, `src/smpp/frame.ts`
- Test: `tests/smpp/decode.test.ts`, `tests/smpp/frame.test.ts`

**Interfaces:**
- Consumes: Task 2's constants and `decodeTlvs`.
- Produces:
  - `src/smpp/pdu-defs.ts`: `PDU_FIELDS: Record<string, FieldDef[]>` where `FieldDef = { name: string; type: "cstring" | "u8" | "u32" | "octets"; maxLen?: number; lenFrom?: string }`. One entry per command in `CommandId`, listing mandatory parameters in wire order. `octets` fields take their length from the `lenFrom` field (`short_message` from `sm_length`).
  - `src/smpp/decode.ts`:
    - `interface Pdu { commandId: number; commandStatus: number; sequenceNumber: number; fields: Record<string, number | string | Buffer>; tlvs: Tlv[]; raw: Buffer }`
    - `decodePdu(buf: Buffer): Pdu` — `raw` is the exact input slice.
    - `class PduDecodeError extends Error { readonly raw: Buffer }`
  - `src/smpp/frame.ts`:
    - `class PduFramer { push(chunk: Buffer): Buffer[] }` — accumulates and returns complete PDU slices, framing by `command_length`. Leaves a partial PDU buffered.
    - `MAX_PDU_LENGTH = 1_048_576`

- [ ] **Step 1: Write the failing framer tests**

```ts
it("returns one PDU from one chunk")
it("returns nothing until a PDU is complete")             // push 10 of 29 bytes → []
it("reassembles a PDU split across three chunks")
it("returns two PDUs delivered in one chunk")
it("keeps the tail of a chunk that ends mid-PDU")
it("rejects a command_length below 16", ...)              // throws PduDecodeError
it("rejects a command_length above MAX_PDU_LENGTH")       // no allocation attempt
```

- [ ] **Step 2: Write the failing decode tests**

```ts
it("decodes the 16-byte header")
it("decodes submit_sm mandatory fields in wire order")
it("reads short_message using sm_length")
it("decodes trailing TLVs after the mandatory fields")
it("decodes a PDU with an unknown command_id, keeping the body as octets")
it("preserves the exact input bytes in raw")
it("throws PduDecodeError carrying raw when a cstring is unterminated")
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `npx vitest run tests/smpp/`
Expected: FAIL — `frame.ts` and `decode.ts` missing.

- [ ] **Step 4: Implement `src/smpp/pdu-defs.ts`**

Field tables only. Source them from the SMPP 3.4 spec for the shared commands, and the spec document's `submit_sm` section for the field set that must be exposed.

- [ ] **Step 5: Implement `PduFramer` in `src/smpp/frame.ts`**

Hold a single growing buffer; loop while it holds at least 16 bytes and at least `command_length` bytes. Validate `command_length` against both bounds **before** using it as a size.

- [ ] **Step 6: Implement `decodePdu` in `src/smpp/decode.ts`**

Walk `PDU_FIELDS[commandName(commandId)]`; for an unrecognised command, put the whole body in `fields.body` as a Buffer and parse no TLVs.

- [ ] **Step 7: Run the framing tests** *(Review Focus 1)*

Run: `npx vitest run tests/smpp/frame.test.ts`
Expected: PASS, including the split-chunk and two-in-one-chunk cases.

- [ ] **Step 8: Run the decode tests**

Run: `npx vitest run tests/smpp/decode.test.ts`
Expected: PASS.

- [ ] **Step 9: Add the lying-length test** *(Review Focus 2)*

```ts
it("rejects a lying command_length without allocating", () => {
  const buf = Buffer.alloc(32); buf.writeUInt32BE(0x7fffffff, 0);
  expect(() => framer.push(buf)).toThrow(PduDecodeError);
});
it("rejects command_length 8")
```

Run: `npx vitest run tests/smpp/frame.test.ts`
Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add src/smpp tests/smpp
git commit -m "feat(smpp): PDU field definitions, framer and decoder"
```

---

### Task 4: PDU encode and raw passthrough

**Files:**
- Create: `src/smpp/encode.ts`
- Test: `tests/smpp/encode.test.ts`

**Interfaces:**
- Consumes: Tasks 2 and 3.
- Produces:
  - `interface PduInput { commandId: number; commandStatus?: number; sequenceNumber: number; fields?: Record<string, number | string | Buffer>; tlvs?: Tlv[] }`
  - `encodePdu(input: PduInput): Buffer` — fills `command_length`, omits nothing, applies no validation and no clamping. A field absent from `fields` is written as its zero value (`0`, `""`, empty octets).
  - `encodeRaw(bytes: Buffer): Buffer` — returns `bytes` unchanged. This is the entry point for the hex composer; it must not rewrite `command_length` or anything else.

- [ ] **Step 1: Write the failing tests**

```ts
it("round-trips submit_sm through encode and decode")
it("writes command_length to match the produced buffer")
it("writes an over-long service_type verbatim")           // 12 chars, no truncation
it("writes a data_coding of 0xff verbatim")               // not in the spec's 0-8 set
it("writes sm_length and short_message consistently when sm_length is absent")
it("keeps a caller-supplied sm_length that disagrees with short_message")
it("returns raw bytes untouched from encodeRaw")          // a buffer with command_length 0xdeadbeef
it("appends TLVs after the mandatory fields")
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/smpp/encode.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `encodePdu` and `encodeRaw` in `src/smpp/encode.ts`**

Walk the same `PDU_FIELDS` table. The disagreeing-`sm_length` test fixes the rule: a caller-supplied length field wins over the derived one. That is the Global Constraint about never silently repairing, expressed in code.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run tests/smpp/encode.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/smpp/encode.ts tests/smpp/encode.test.ts
git commit -m "feat(smpp): PDU encoder with raw passthrough"
```

---

### Task 5: Text codecs and the size counter

**Files:**
- Create: `src/smpp/text.ts`
- Test: `tests/smpp/text.test.ts`

**Interfaces:**
- Consumes: `DataCoding` from Task 2.
- Produces:
  - `type SegmentationMode = "none" | "udh" | "sar"` — declared here because `measure` needs it; Task 6 imports it from this module.
  - `encodeText(text: string, dataCoding: number): { bytes: Buffer; unmappable: string[] }` — `unmappable` lists characters the target coding cannot represent; they are encoded as `?` and reported, never dropped silently.
  - `decodeText(bytes: Buffer, dataCoding: number): string`
  - `measure(text: string, dataCoding: number, segmentation: SegmentationMode): { parts: number; chars: number; bits: number; bytes: number; unmappable: string[] }` — the spec's live counter: SMS parts, characters used, bits used, total size.
  - GSM 03.38 default alphabet plus its escape-table extensions (`^{}\[~]|€`), each escape costing 2 septets.

- [ ] **Step 1: Write the failing tests**

```ts
it("packs GSM7 septets")                                   // 160 chars → 140 bytes
it("counts an escape-table character as two septets")      // "€" → chars 1, bits 14
it("encodes UCS2 big-endian")                              // "é" → 00 e9
it("passes octet data through unchanged")                  // data_coding 4
it("encodes Latin-1, Cyrillic and Hebrew codings")
it("reports parts 1 for 160 GSM7 chars and 2 for 161")
it("reports parts 1 for 70 UCS2 chars and 2 for 71")
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/smpp/text.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `src/smpp/text.ts`**

GSM 03.38 tables as const maps; septet packing per the spec. Latin-1, Cyrillic (ISO-8859-5), Hebrew (ISO-8859-8) and JIS via Node's `Buffer`/`TextDecoder` where available, hand tables where not.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run tests/smpp/text.test.ts`
Expected: PASS.

- [ ] **Step 5: Add the sample-content presets**

`SAMPLES: Record<"gsm1" | "ucs2_1" | "gsm3" | "ucs2_3", string>` — the spec's four presets. `gsm1` must include characters from the GSM alphabet's escape table so the counter is exercised by the preset itself.

- [ ] **Step 6: Run tests to verify they still pass**

Run: `npx vitest run tests/smpp/text.test.ts`
Expected: PASS.

- [ ] **Step 7: Add the non-GSM7 input test** *(Review Focus 4)*

```ts
it("reports emoji as unmappable under GSM7 and does not drop them", () => {
  const r = encodeText("hi 🙂", DataCoding.GSM_DEFAULT);
  expect(r.unmappable).toEqual(["🙂"]);
  expect(r.bytes.length).toBeGreaterThan(0);
});
it("reports unmappable characters from measure() too")
```

Run: `npx vitest run tests/smpp/text.test.ts`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/smpp/text.ts tests/smpp/text.test.ts
git commit -m "feat(smpp): text codecs, size counter and content presets"
```

---

### Task 6: Segmentation

**Files:**
- Create: `src/smpp/segment.ts`
- Test: `tests/smpp/segment.test.ts`

**Interfaces:**
- Consumes: Tasks 2, 4 and 5, including `SegmentationMode` from `src/smpp/text.ts`.
- Produces:
  - `type BodyPlacement = "short_message" | "message_payload"`
  - `segment(opts: { text: string; dataCoding: number; mode: SegmentationMode; placement: BodyPlacement; userUdh?: Buffer; refNum?: number }): Segment[]` where `Segment = { fields: Record<string, number | string | Buffer>; tlvs: Tlv[] }` — ready to merge into a `submit_sm` `PduInput`.
  - Rules fixed by the spec: `"none"` returns exactly one segment however long the text is; `"udh"` emits an 8-bit-reference concatenation IE and sets `esm_class` UDHI; `"sar"` emits `sar_msg_ref_num` / `sar_total_segments` / `sar_segment_seqnum` TLVs and leaves `esm_class` alone. **A `userUdh` turns automatic concatenation off** — the user's bytes win — and the result is one segment carrying that UDH.

- [ ] **Step 1: Write the failing tests**

```ts
it("returns one oversized segment in none mode")           // 400 GSM7 chars → 1 segment
it("splits GSM7 into 153-septet segments in udh mode")
it("splits UCS2 into 67-character segments in udh mode")
it("sets the UDHI bit in esm_class in udh mode only")
it("emits sar_* TLVs with matching ref num in sar mode")
it("numbers sar segments from 1")
it("returns one segment carrying the user's UDH when userUdh is given")
it("does not add a concatenation IE when userUdh is given")
it("puts the body in message_payload when placement says so")
it("omits sm_length from a message_payload segment")
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/smpp/segment.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `segment` in `src/smpp/segment.ts`**

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run tests/smpp/segment.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/smpp/segment.ts tests/smpp/segment.test.ts
git commit -m "feat(smpp): segmentation with UDH, sar TLVs and user-UDH override"
```

---

### Task 7: Delivery receipt parsing and DLR correlation

**Files:**
- Create: `src/smpp/receipt.ts`, `src/main/dlr.ts`
- Test: `tests/smpp/receipt.test.ts`, `tests/main/dlr.test.ts`

**Interfaces:**
- Consumes: Tasks 2 and 3.
- Produces:
  - `src/smpp/receipt.ts`: `parseReceipt(pdu: Pdu): Receipt | null` where `Receipt = { messageId: string | null; source: "tlv" | "text"; state: string | null; errorCode: string | null; submitDate: string | null; doneDate: string | null }`. Prefers the `receipted_message_id` TLV; otherwise parses the `id:`/`stat:`/`err:` key-value form out of `short_message`.
  - `src/main/dlr.ts`:
    - `interface CorrelationConfig { preferTlv: boolean; parseText: boolean; normaliseRadix: boolean; stripLeadingZeros: boolean }`
    - `class DlrCorrelator { constructor(cfg: CorrelationConfig); register(messageId: string, token: string): void; match(receipt: Receipt): string | null; readonly unmatched: Receipt[] }` — `token` is the caller's handle (a burst row id). `match` returns the token or `null`, and a `null` match appends to `unmatched`. Nothing is dropped and nothing is guessed.
    - `normaliseId(id: string, cfg: CorrelationConfig): string[]` — the candidate keys for one id: as-given, hex↔decimal reinterpretation, leading zeros stripped.

- [ ] **Step 1: Write the failing receipt tests**

```ts
it("prefers the receipted_message_id TLV over the text")
it("parses id, stat and err out of the receipt text")
it("returns source 'text' when no TLV is present")
it("returns null for a deliver_sm that is not a receipt")   // esm_class without the receipt bits
it("tolerates receipt text with extra unknown keys")
```

- [ ] **Step 2: Write the failing correlator tests**

```ts
it("matches an id returned verbatim")
it("matches a decimal receipt id against a hex submit id")  // "1193046" vs "123456"
it("matches a hex receipt id against a decimal submit id")
it("matches an id that gained leading zeros")               // "00ABC" vs "ABC"
it("does not match when normalisation is switched off")
it("puts an unmatchable receipt in unmatched")
it("never returns a token it was not given")
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `npx vitest run tests/smpp/receipt.test.ts tests/main/dlr.test.ts`
Expected: FAIL — modules not found.

- [ ] **Step 4: Implement `parseReceipt`**

- [ ] **Step 5: Implement `DlrCorrelator` and `normaliseId`**

Index registrations under every candidate key from `normaliseId`, so a lookup is one map hit per candidate. Ambiguity — two submits whose candidate keys collide — resolves to the earliest unmatched registration, and the plan's rule is that a collision must not silently steal another row's receipt: once a token is matched it is removed from the index.

- [ ] **Step 6: Run tests to verify they pass**

Run: `npx vitest run tests/smpp/receipt.test.ts tests/main/dlr.test.ts`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/smpp/receipt.ts src/main/dlr.ts tests/smpp/receipt.test.ts tests/main/dlr.test.ts
git commit -m "feat: delivery receipt parsing and configurable DLR correlation"
```

---

### Task 8: Loopback SMSC test fixture

**Files:**
- Create: `tests/fixtures/smsc-server.ts`
- Test: `tests/fixtures/smsc-server.test.ts`

**Interfaces:**
- Consumes: Tasks 2, 3 and 4.
- Produces:
  - `class FakeSmsc` with:
    - `constructor(opts?: { tls?: { key: Buffer; cert: Buffer; requestCert?: boolean; ca?: Buffer } })`
    - `listen(): Promise<number>` — resolves with the bound port
    - `close(): Promise<void>`
    - `readonly received: Pdu[]`
    - `on(command: string, handler: (pdu: Pdu) => PduInput | PduInput[] | null): void` — a handler returning `null` sends nothing, which is how the never-answers case is tested
    - `send(pdu: PduInput): void` — push a `deliver_sm` or an unsolicited PDU at the client
    - `sendRaw(bytes: Buffer): void` — push malformed bytes
    - default behaviour: answer every bind with `ESME_ROK`, every `submit_sm` with an incrementing `message_id`, every `enquire_link` with `ESME_ROK`
  - `generateSelfSignedCert(): { key: Buffer; cert: Buffer; ca: Buffer }` — for the TLS tests. Use `node:crypto`'s `X509Certificate` generation or a committed fixture pair; a committed pair is acceptable and simpler, but must carry a long expiry and a note that it is test-only.

This is a test fixture, not a product feature; it does not contradict the spec's non-goal about acting as an SMSC.

- [ ] **Step 1: Write the failing test**

```ts
it("accepts a bind_transceiver and answers ESME_ROK")
it("records every PDU it received")
it("answers submit_sm with an incrementing message_id")
it("sends nothing when the handler returns null")
it("delivers an unsolicited deliver_sm to the client")
it("accepts a TLS connection with a self-signed cert")
```

Drive these with a bare `net.Socket` plus `encodePdu` / `PduFramer`, not with the session class — which does not exist yet.

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/fixtures/smsc-server.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `FakeSmsc`**

A `net.Server` or `tls.Server` per connection, each with its own `PduFramer`.

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/fixtures/smsc-server.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add tests/fixtures
git commit -m "test: loopback SMSC fixture"
```

---

### Task 9: Transport

**Files:**
- Create: `src/main/transport.ts`
- Test: `tests/main/transport.test.ts`

**Interfaces:**
- Consumes: Task 3's `PduFramer`, Task 8's fixture.
- Produces:
  - `interface TlsOptions { enabled: boolean; ca?: Buffer; rejectUnauthorized: boolean; servername?: string; clientCert?: Buffer; clientKey?: Buffer; clientKeyPassphrase?: string }`
  - `interface TransportOptions { host: string; port: number; tls: TlsOptions; connectTimeoutMs: number }`
  - `class Transport extends EventEmitter` — `connect(): Promise<void>`, `write(bytes: Buffer): void`, `destroy(): void`, `readonly connected: boolean`. Events: `"pdu"` with the raw PDU slice, `"decode-error"` with a `PduDecodeError`, `"close"` with an optional `Error`, `"bytes-out"` / `"bytes-in"` with Buffers.
  - The transport does **not** reconnect. Ever.

- [ ] **Step 1: Write the failing tests**

```ts
it("connects over plain TCP and emits framed PDUs")
it("connects over TLS to a self-signed cert when rejectUnauthorized is false")
it("refuses a self-signed cert when rejectUnauthorized is true")
it("accepts a self-signed cert when its CA is supplied")
it("presents a client certificate when the server requests one")
it("sends the configured servername as SNI")
it("emits decode-error and stays connected on malformed bytes")
it("emits close and does not reconnect when the server goes away")
it("rejects connect() after connectTimeoutMs against a dead port")
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/main/transport.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `Transport`**

`net.connect` or `tls.connect` by `tls.enabled`. One `PduFramer` per connection. A `PduDecodeError` from the framer becomes a `"decode-error"` event and the framer is reset; the socket stays open, because an SMSC that sent one bad PDU is still worth watching.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run tests/main/transport.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/main/transport.ts tests/main/transport.test.ts
git commit -m "feat(main): TCP/TLS transport with CA, mTLS and SNI options"
```

---

### Task 10: Session

**Files:**
- Create: `src/main/session.ts`
- Test: `tests/main/session.test.ts`

**Interfaces:**
- Consumes: Tasks 2–4, 7, 9; Task 8's fixture.
- Produces:
  - `type BindMode = "transceiver" | "transmitter" | "receiver"`
  - `type BindState = "unbound" | "binding" | "bound" | "unbinding" | "closed"`
  - `interface BindOptions { systemId: string; password: string; systemType: string; interfaceVersion: number; mode: BindMode; addrTon?: number; addrNpi?: number; addressRange?: string }`
  - `class Session extends EventEmitter`:
    - `constructor(transport: Transport, opts: { windowSize: number; responseTimeoutMs: number })`
    - `bind(o: BindOptions): Promise<Pdu>`
    - `unbind(): Promise<Pdu>`
    - `send(input: Omit<PduInput, "sequenceNumber">): Promise<Pdu>` — allocates the sequence number, resolves on the matching response, rejects on timeout
    - `sendRaw(bytes: Buffer): void` — fire and forget, no sequence allocation, no response matching; the hex composer's path
    - `readonly state: BindState`
    - Events: `"pdu-in"` / `"pdu-out"` with `{ pdu: Pdu, at: number }`, `"state"` with a `BindState`, `"orphan"` with a `Pdu`, `"timeout"` with `{ sequenceNumber: number }`
    - Answers an inbound `deliver_sm` with `deliver_sm_resp` automatically, and an inbound `enquire_link` with `enquire_link_resp`. An inbound `unbind` is answered and moves the state to `closed`.
  - Sequence numbers start at 1 and wrap at `0x7fffffff`.

- [ ] **Step 1: Write the failing tests**

```ts
it("binds as transceiver and reports state bound")
it("sends bind_transmitter for transmitter mode")
it("reports state unbound and rejects when the bind is refused")  // ESME_RBINDFAIL
it("allocates increasing sequence numbers")
it("resolves send() with the response that matches the sequence number")
it("resolves two concurrent sends with their own responses, out of order")
it("answers deliver_sm with deliver_sm_resp automatically")
it("answers an inbound enquire_link")
it("moves to closed on an inbound unbind")
it("does not re-bind after the socket closes")                    // state closed, no new connect
it("sends raw bytes verbatim through sendRaw")
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/main/session.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `Session`**

A `Map<number, { resolve, reject, timer }>` of pending requests. The window limit applies to `send`; a `send` beyond `windowSize` waits for a slot rather than failing, because a burst at a high rate relies on that backpressure.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run tests/main/session.test.ts`
Expected: PASS.

- [ ] **Step 5: Verify no reconnect**

Run: `npx vitest run tests/main/session.test.ts -t "does not re-bind"`
Expected: PASS. This pins a Global Constraint; do not weaken it later for convenience.

- [ ] **Step 6: Commit**

```bash
git add src/main/session.ts tests/main/session.test.ts
git commit -m "feat(main): SMPP session with bind, windowing and response matching"
```

- [ ] **Step 7: Write the orphan and unknown-command tests** *(Review Focus 3)*

```ts
it("emits orphan for a response whose sequence number is not pending", () => { /* fixture sends submit_sm_resp seq 9999 */ });
it("emits orphan for an unknown command_id and stays bound");
it("keeps answering enquire_link after an orphan");
```

- [ ] **Step 8: Run them to verify they fail**

Run: `npx vitest run tests/main/session.test.ts -t orphan`
Expected: FAIL — no `"orphan"` event yet.

- [ ] **Step 9: Emit `"orphan"` instead of throwing**

Unmatched responses and unknown commands are traced, not fatal.

- [ ] **Step 10: Write the response-timeout test** *(Review Focus 5)*

```ts
it("rejects send() after responseTimeoutMs when the SMSC never answers", async () => { /* handler returns null */ });
it("emits timeout with the sequence number");
it("frees the window slot after a timeout");   // windowSize 1, two sends, second must proceed
```

- [ ] **Step 11: Run the timeout tests to verify they pass**

Run: `npx vitest run tests/main/session.test.ts -t timeout`
Expected: PASS.

- [ ] **Step 12: Commit**

```bash
git add src/main/session.ts tests/main/session.test.ts
git commit -m "feat(main): orphan PDU events and response timeouts"
```

---

### Task 11: Trace store, counters and export

**Files:**
- Create: `src/main/trace.ts`, `src/main/export.ts`
- Test: `tests/main/trace.test.ts`, `tests/main/export.test.ts`

**Interfaces:**
- Consumes: Tasks 2, 3, 10.
- Produces:
  - `interface TraceEntry { seq: number; at: number; direction: "in" | "out"; commandName: string; commandStatus: number; sequenceNumber: number; summary: string; raw: Buffer; decodeError?: string }` — `summary` is the one-line decoded form shown in the table.
  - `class TraceStore` — `add(e: TraceEntry): void`, `entries(from?: number, limit?: number): TraceEntry[]`, `clear(): void`, `readonly dropped: number`, `readonly counters: Record<string, { sent: number; received: number }>`, `setCap(n: number): void`. A full store evicts the oldest and increments `dropped`, which the UI must show; counters are never reset by eviction.
  - `class TraceFileSink { constructor(path: string); write(e: TraceEntry): void; close(): void }` — the optional append-to-file for long sessions.
  - `src/main/export.ts`: `toCsv(rows: Record<string, unknown>[], columns: string[]): string` and `traceToText(entries: TraceEntry[]): string`. CSV quoting per RFC 4180.

- [ ] **Step 1: Write the failing tests**

```ts
it("keeps entries in insertion order")
it("evicts the oldest entry past the cap and counts it in dropped")
it("counts sent and received per command name")
it("does not decrement counters when an entry is evicted")
it("clears entries and counters on clear()")
it("appends one line per entry to a file sink")
it("quotes a CSV field containing a comma, a quote and a newline")
it("renders a decode-error entry with its raw bytes and no summary")
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/main/trace.test.ts tests/main/export.test.ts`
Expected: FAIL — modules not found.

- [ ] **Step 3: Implement `TraceStore`, `TraceFileSink` and the exporters**

A ring buffer for the store. Default cap 10,000 entries.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run tests/main/trace.test.ts tests/main/export.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/main/trace.ts src/main/export.ts tests/main/trace.test.ts tests/main/export.test.ts
git commit -m "feat(main): trace store, per-PDU counters and CSV/text export"
```

---

### Task 12: Burst engine

**Files:**
- Create: `src/main/burst.ts`, `src/main/recipients.ts`
- Test: `tests/main/burst.test.ts`, `tests/main/recipients.test.ts`

**Interfaces:**
- Consumes: Tasks 4, 6, 7, 10, 11.
- Produces:
  - `src/main/recipients.ts`:
    - `type RecipientSource = { kind: "repeat"; number: string; count: number } | { kind: "range"; from: string; count: number } | { kind: "csv"; text: string; column?: string }`
    - `resolveRecipients(src: RecipientSource): { numbers: string[]; warnings: string[] }` — the spec's three sources. `range` increments the numeric tail of `from`. `csv` takes the named column or the first, skipping a header row it recognises. Warnings, not errors: a malformed row is reported and kept.
    - `MAX_BURST = 50_000`, `MAX_RATE = 500`
  - `src/main/burst.ts`:
    - `interface BurstConfig { recipients: RecipientSource; ratePerSec: number; template: Omit<PduInput, "sequenceNumber">; segmentation: SegmentationMode; placement: BodyPlacement; backoffOnThrottle: boolean; awaitDlrMs: number }`
    - `interface BurstRow { id: string; recipient: string; body: string; pdus: number; messageId: string | null; commandStatus: number | null; submitLatencyMs: number | null; dlrState: string | null; dlrLatencyMs: number | null }`
    - `interface BurstStats { messages: number; pdus: number; submitted: number; accepted: number; errorsByStatus: Record<string, number>; achievedRatePerSec: number; latencyP50: number; latencyP95: number; latencyP99: number; dlrReceived: number; dlrUnmatched: number }`
    - `class BurstRunner extends EventEmitter` — `start(cfg: BurstConfig): Promise<void>`, `stop(): void`, `readonly rows: BurstRow[]`, `readonly stats: BurstStats`. Events: `"progress"` with `{ sent, total, stats }` throttled to about 10/sec, `"row"` with an updated `BurstRow`, `"done"` with `{ reason: "complete" | "stopped" | "connection-lost" }`.
    - Fixed behaviour from the spec: on `ESME_RTHROTTLED` the row records the status and the requested rate is **held**, unless `backoffOnThrottle`; if the session leaves `bound` mid-burst the runner **stops and reports** `"connection-lost"`; a request over `MAX_BURST` or `MAX_RATE` is rejected before any PDU is sent, with the reason in the error.
    - A multi-part message is one row with `pdus > 1`; `stats.messages` and `stats.pdus` are separate figures, and a part failing marks the row's `commandStatus` with the first non-zero status.

- [ ] **Step 1: Write the failing recipient tests**

```ts
it("repeats one number count times")
it("increments the numeric tail for a range")
it("reads a CSV column by name")
it("reads the first column when no name is given")
it("skips a header row")
it("warns about a malformed row and keeps it")
it("rejects a count above MAX_BURST")
```

- [ ] **Step 2: Write the failing burst tests**

```ts
it("sends one submit_sm per recipient")
it("records the message_id from submit_sm_resp on the row")
it("holds the requested rate when the SMSC answers ESME_RTHROTTLED")
it("backs off when backoffOnThrottle is set")
it("counts errors by status name")
it("computes achieved rate and latency percentiles")
it("stops with connection-lost when the bind drops mid-burst")
it("stops where it was when stop() is called")
it("rejects a rate above MAX_RATE before sending anything")
it("correlates a deliver_sm back to its row")
it("counts an unmatched receipt in dlrUnmatched without touching a row")
it("reports a 3-part message as one row with pdus 3")
it("marks the row with the first non-zero status when part 2 of 3 fails")
it("keeps every row for 1000 messages")        // the never-cap anti-goal
```

Drive these against `FakeSmsc` with handlers that return the statuses each test needs.

- [ ] **Step 3: Run tests to verify they fail**

Run: `npx vitest run tests/main/burst.test.ts tests/main/recipients.test.ts`
Expected: FAIL — modules not found.

- [ ] **Step 4: Implement `resolveRecipients`**

- [ ] **Step 5: Implement `BurstRunner`**

Pace with a token bucket refilled on an interval, bounded by the session's window so backpressure is the session's job and not duplicated here. Rows live in a plain array indexed by a `Map` from row id; the array is never capped (anti-goal).

- [ ] **Step 6: Run tests to verify they pass**

Run: `npx vitest run tests/main/burst.test.ts tests/main/recipients.test.ts`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/main/burst.ts src/main/recipients.ts tests/main/burst.test.ts tests/main/recipients.test.ts
git commit -m "feat(main): burst engine with three recipient sources, pacing and DLR correlation"
```

---

### Task 13: Profile store

**Files:**
- Create: `src/main/profiles.ts`
- Test: `tests/main/profiles.test.ts`

**Interfaces:**
- Consumes: Task 9's `TlsOptions`, Task 10's `BindOptions`.
- Produces:
  - `interface Profile { id: string; name: string; host: string; port: number; bind: Omit<BindOptions, "password">; tls: Omit<TlsOptions, "clientKeyPassphrase">; correlation: CorrelationConfig; caPath?: string; clientCertPath?: string; clientKeyPath?: string }` — note no secret fields.
  - `interface SecretStore { encrypt(s: string): Buffer; decrypt(b: Buffer): string; available(): boolean }` — the seam over Electron's `safeStorage`, so tests inject a fake.
  - `class ProfileStore { constructor(dir: string, secrets: SecretStore); list(): Profile[]; save(p: Profile, password?: string, keyPassphrase?: string): void; password(id: string): string | null; keyPassphrase(id: string): string | null; remove(id: string): void }` — profiles in `profiles.json`, secrets in `secrets.bin` as `safeStorage` ciphertext. If `secrets.available()` is false, `save` with a password throws rather than writing plaintext.

- [ ] **Step 1: Write the failing tests**

```ts
it("round-trips a profile through save and list")
it("never writes the password into profiles.json")      // read the file, assert the string is absent
it("returns the password through the secret store")
it("throws rather than storing a password when safeStorage is unavailable")
it("saves a profile with no password when none is given")
it("removes a profile and its secrets")
it("returns an empty list for a fresh directory")
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/main/profiles.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `ProfileStore` and a `safeStorage`-backed `SecretStore`**

The `safeStorage` implementation lives behind the interface and is not unit-tested (it needs a live Electron app); the fake covers the logic.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run tests/main/profiles.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/main/profiles.ts tests/main/profiles.test.ts
git commit -m "feat(main): profile store with safeStorage-backed secrets"
```

---

### Task 14: IPC surface and preload bridge

**Files:**
- Create: `src/main/ipc.ts`, `src/shared/api.ts`
- Modify: `src/main/index.ts`, `src/preload/index.ts`
- Test: `tests/main/ipc.test.ts`

**Interfaces:**
- Consumes: Tasks 9–13.
- Produces:
  - `src/shared/api.ts` — the one type both sides import:
    - `interface Commands` — `connect(profile, secrets)`, `bind(opts)`, `unbind()`, `disconnect()`, `sendPdu(input)`, `sendRaw(hex)`, `startBurst(cfg)`, `stopBurst()`, `trace(from, limit)`, `clearTrace()`, `exportTrace(format)`, `exportBurst()`, `listProfiles()`, `saveProfile(p, secrets)`, `removeProfile(id)`, `measure(text, dataCoding, mode)`, `pickFile(kind)`. Each returns a `Promise`.
    - `interface Events` — `"state"`, `"pdu"`, `"counters"`, `"burst-progress"`, `"burst-row"`, `"burst-done"`, `"error"`. Payload types named here.
  - `src/main/ipc.ts`: `registerIpc(win: BrowserWindow, deps: Deps): void` — one `ipcMain.handle` per command, and a push of each event to the renderer. PDU and burst-row events are **batched** on a ~100 ms tick into arrays, because 500 PDUs/sec must not be 500 IPC messages/sec; batching may delay, never drop or reorder.
  - `src/preload/index.ts`: `contextBridge.exposeInMainWorld("smpp", api)` where `api` implements `Commands` via `ipcRenderer.invoke` and offers `on<K extends keyof Events>(name: K, cb)`. No other surface is exposed.

- [ ] **Step 1: Write the failing tests**

```ts
it("registers a handler for every key of Commands")        // compare Object.keys against the handler map
it("batches pdu events on a tick instead of one per PDU")  // 50 PDUs → 1 send, 50 entries
it("preserves PDU order inside a batch")
it("never drops a PDU when batching")                      // 2000 PDUs → 2000 entries across batches
it("forwards a main-process error as an error event, not a throw")
```

Stub `BrowserWindow.webContents.send` and `ipcMain.handle`; this task does not launch Electron.

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/main/ipc.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Define `src/shared/api.ts`**

- [ ] **Step 4: Implement `registerIpc`**

- [ ] **Step 5: Implement the preload bridge**

- [ ] **Step 6: Run tests to verify they pass**

Run: `npx vitest run tests/main/ipc.test.ts`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/shared src/main/ipc.ts src/main/index.ts src/preload/index.ts tests/main/ipc.test.ts
git commit -m "feat: typed IPC surface with batched event delivery"
```

---

### Task 15: UI shell and connection panel

**Files:**
- Create: `src/ui/source/class/smppclient/Shell.js`, `src/ui/source/class/smppclient/ConnectionPanel.js`, `src/ui/source/class/smppclient/Api.js`
- Modify: `src/ui/source/class/smppclient/Application.js`
- Test: `tests/ui/connection-panel.test.ts`

**Interfaces:**
- Consumes: `window.smpp` from Task 14.
- Produces:
  - `smppclient.Api` — a thin qooxdoo singleton wrapping `window.smpp`, so no widget touches the bridge directly and tests can swap it.
  - `smppclient.Shell` — the spec's layout A: a `qx.ui.layout.Dock`; `ConnectionPanel` west, a tab view centre, the trace centre-south in a vertical splitpane.
  - `smppclient.ConnectionPanel` — host, port, TLS checkbox, `system_id`, `password`, `system_type`, interface-version selector (the four spec values), bind-mode radio (TRX / TX / RX), Bind and Unbind buttons, a state indicator reading `Unbound` when unbound, a profile dropdown with save and delete, a TLS detail group (custom CA file, accept self-signed, client cert and key with passphrase, SNI), and the per-PDU counters list below.
  - `getBindOptions()` and `getTlsOptions()` returning the Task 9/10 shapes.

- [ ] **Step 1: Write the failing test**

```ts
it("maps the four interface-version labels to 0x00, 0x33, 0x34, 0x50")
it("maps TRX, TX and RX to transceiver, transmitter, receiver")
it("reads back TLS options including CA path, SNI and client cert")
it("reports state Unbound before any bind")
```

Test the pure mapping functions by importing the panel's `getBindOptions` / `getTlsOptions` logic; the plan deliberately keeps that logic in functions callable without a DOM.

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/ui/connection-panel.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `Api`, `Shell` and `ConnectionPanel`**

Keep every value-mapping function free of widget references so it stays testable.

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/ui/connection-panel.test.ts`
Expected: PASS.

- [ ] **Step 5: Bind against the fixture by hand**

Start `FakeSmsc` on a known port, run `npm run build:ui && npm run build:main && npm start`, enter the port, click Bind.
Expected: the indicator reads `Bound`, and the bind counter increments.

- [ ] **Step 6: Commit**

```bash
git add src/ui tests/ui/connection-panel.test.ts
git commit -m "feat(ui): shell layout and connection panel"
```

---

### Task 16: submit_sm form

**Files:**
- Create: `src/ui/source/class/smppclient/SubmitForm.js`, `src/ui/source/class/smppclient/TlvEditor.js`, `src/ui/source/class/smppclient/fields.js`
- Test: `tests/ui/submit-form.test.ts`

**Interfaces:**
- Consumes: Tasks 2, 5, 6, 14, 15.
- Produces:
  - `fields.js` — the option lists for every selector in the spec's `submit_sm` table: TON/NPI (Auto, Intl, Nat, Alpha, E164), `registered_delivery` (0–3, 16–19), `esm_class` (0–3, 64–67 with their labels), `priority_flag` (0–4), `data_coding` (0–8), `schedule_delivery_time` (immediate, +1, +2, +5, +15 min, historic), `validity_period` (default, 1 min, 30 min, 1 h, 4 h, 1 day).
  - `smppclient.TlvEditor` — add, edit and remove rows of `{ tag, value }` with hex or text value entry, including tags not in `KnownTlv`.
  - `smppclient.SubmitForm` — the short form plus a "Show all" toggle revealing `service_type`, `esm_class`, `protocol_id`, `priority_flag`, `data_coding`, TLVs and UDH; a body placement selector (`short_message` / `message_payload`); a segmentation selector (`none` / `udh` / `sar`); the four sample presets and Clear; the live counter showing parts, characters, bits, bytes and any unmappable characters; a Send button.
  - `buildSubmitInput(formValues): PduInput` — the pure function from form values to a PDU, composing Task 6's `segment` output. Validation produces warnings alongside the PDU; it never withholds it.

- [ ] **Step 1: Write the failing tests**

```ts
it("builds a submit_sm with the form's TON/NPI and addresses")
it("puts the body in message_payload when that placement is chosen")
it("includes an arbitrary unknown TLV")
it("returns a PDU and a warning for a 3-character destination, not an error")
it("returns a PDU for a data_coding the spec does not list")
it("produces three segments in udh mode for a 3-part GSM7 preset")
it("sets esm_class UDHI in udh mode")
it("leaves esm_class alone and emits sar_* TLVs in sar mode")
it("reports the counter for the ucs2_3 preset as 3 parts")
it("reports unmappable characters in the counter")
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/ui/submit-form.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `fields.js` and `buildSubmitInput`**

- [ ] **Step 4: Implement `TlvEditor` and `SubmitForm`**

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run tests/ui/submit-form.test.ts`
Expected: PASS.

- [ ] **Step 6: Send against the fixture by hand**

Bind to `FakeSmsc`, load the `gsm3` preset, pick `udh`, Send.
Expected: three `submit_sm` PDUs reach the fixture and three responses come back.

- [ ] **Step 7: Commit**

```bash
git add src/ui tests/ui/submit-form.test.ts
git commit -m "feat(ui): submit_sm form with full field set, TLV editor and live counter"
```

---

### Task 17: Remaining PDU tabs

**Files:**
- Create: `src/ui/source/class/smppclient/MessageOpsForm.js`, `src/ui/source/class/smppclient/MultiForm.js`, `src/ui/source/class/smppclient/DataSmForm.js`
- Modify: `src/ui/source/class/smppclient/Shell.js`
- Test: `tests/ui/pdu-forms.test.ts`

**Interfaces:**
- Consumes: Tasks 2, 14, 16.
- Produces:
  - `smppclient.MessageOpsForm` — `query_sm`, `cancel_sm` and `replace_sm`: `message_id` up to 64 characters, source address with TON/NPI, `service_type`, plus `replace_sm`'s own `schedule_delivery_time`, `validity_period`, `registered_delivery` and message body.
  - `smppclient.MultiForm` — `submit_multi`, `broadcast_sm`, `broadcast_multi`: a destination-address list editor plus the `submit_sm` field set.
  - `smppclient.DataSmForm` — `data_sm`.
  - `buildQueryInput`, `buildCancelInput`, `buildReplaceInput`, `buildMultiInput`, `buildBroadcastInput`, `buildDataSmInput` — pure form-values-to-`PduInput` functions, one per command.
  - An `enquire_link` button in the shell.

- [ ] **Step 1: Write the failing tests**

```ts
it("builds query_sm with a 64-character message_id")
it("builds cancel_sm with source address and service_type")
it("builds replace_sm with its own validity period")
it("builds submit_multi with three destination addresses")
it("builds broadcast_sm")
it("builds data_sm")
it("warns but still builds a message_id longer than 64 characters")
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/ui/pdu-forms.test.ts`
Expected: FAIL — modules not found.

- [ ] **Step 3: Implement the three form classes and their builders**

- [ ] **Step 4: Add the tabs and the enquire_link button to `Shell`**

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run tests/ui/pdu-forms.test.ts`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/ui tests/ui/pdu-forms.test.ts
git commit -m "feat(ui): data_sm, query/cancel/replace and multi/broadcast forms"
```

---

### Task 18: Trace table and hex view

**Files:**
- Create: `src/ui/source/class/smppclient/TracePanel.js`, `src/ui/source/class/smppclient/HexView.js`, `src/ui/source/class/smppclient/summary.js`
- Test: `tests/ui/trace-panel.test.ts`

**Interfaces:**
- Consumes: Tasks 3, 11, 14.
- Produces:
  - `summary.js`: `summarise(pdu: Pdu): string` — the decoded one-liner for the table, and `describe(pdu: Pdu): Array<{ name: string; value: string }>` for the expanded view.
  - `smppclient.HexView` — offset, hex and ASCII columns for a Buffer.
  - `smppclient.TracePanel` — a virtual table with the spec's columns (time, sequence number, direction, SMPP PDU), fed from batched `"pdu"` events; the view controls Top, Compact, Expand and Clear; a hex pane for the selected row; and a visible notice when `TraceStore.dropped > 0`.

- [ ] **Step 1: Write the failing tests**

```ts
it("summarises submit_sm with its addresses and message length")
it("summarises submit_sm_resp with its message_id and status name")
it("summarises deliver_sm with the receipt state when it is a receipt")
it("summarises an unknown command by id")
it("summarises a decode error with the byte count and no field list")
it("formats hex in 16-byte rows with an ASCII column")
it("renders a non-printable byte as a dot in the ASCII column")
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/ui/trace-panel.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `summary.js` and `HexView`**

- [ ] **Step 4: Implement `TracePanel`**

`qx.ui.table.Table` with a remote model paging through `trace(from, limit)`, so a 10,000-entry trace does not build 10,000 widgets.

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run tests/ui/trace-panel.test.ts`
Expected: PASS.

- [ ] **Step 6: Check the trace against the fixture by hand**

Bind, send a `submit_sm`, have the fixture push a `deliver_sm` and then `sendRaw` a malformed PDU.
Expected: four rows in order, the hex pane matching each selection, and the malformed one shown as a decode error with its bytes.

- [ ] **Step 7: Commit**

```bash
git add src/ui tests/ui/trace-panel.test.ts
git commit -m "feat(ui): PDU trace table with hex view and decode-error rows"
```

---

### Task 19: Burst panel

**Files:**
- Create: `src/ui/source/class/smppclient/BurstPanel.js`, `src/ui/source/class/smppclient/StatsBar.js`
- Modify: `src/ui/source/class/smppclient/Shell.js`
- Test: `tests/ui/burst-panel.test.ts`

**Interfaces:**
- Consumes: Tasks 12, 14, 16.
- Produces:
  - `smppclient.BurstPanel` — a recipient-source selector covering the spec's three sources (repeat a number, generate a range, load a CSV via `pickFile`), a rate field, a count field, the `submit_sm` template taken from `SubmitForm`, a backoff-on-throttle checkbox, Start and Stop, a results table (recipient, body, `message_id`, status, submit latency, DLR state, DLR latency), and an Export CSV button.
  - `smppclient.StatsBar` — the spec's aggregates: submitted, accepted, errors by status, achieved rate, p50/p95/p99 latency, DLRs received and unmatched.
  - `formatStats(s: BurstStats): Array<{ label: string; value: string }>`
  - A refusal path: a count or rate over `MAX_BURST` / `MAX_RATE` shows the limit and the Start button stays disabled, per the Global Constraint about degrading honestly.
  - DLR columns are available only under a transceiver bind, per the spec's session model. Under a TX-only bind the panel says so and leaves the DLR columns out rather than showing them permanently empty.

- [ ] **Step 1: Write the failing tests**

```ts
it("formats errors by status as one entry per status name")
it("formats an achieved rate below the requested rate without rounding it to the request")
it("formats percentiles in milliseconds")
it("reports a count above MAX_BURST as a refusal with the limit in the message")
it("reports a rate above MAX_RATE as a refusal")
it("accepts a count at exactly MAX_BURST")
it("omits the DLR columns under a transmitter-only bind")
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/ui/burst-panel.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `formatStats` and the refusal check**

- [ ] **Step 4: Implement `StatsBar` and `BurstPanel`**

The results table is a virtual table over `rows`, paged, since 50,000 rows must not become 50,000 widgets. The rows themselves are never capped.

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run tests/ui/burst-panel.test.ts`
Expected: PASS.

- [ ] **Step 6: Run a burst against the fixture by hand**

5,000 messages at 200/sec at `FakeSmsc` with a handler that answers `ESME_RTHROTTLED` for one message in fifty and pushes a `deliver_sm` for the rest.
Expected: 5,000 rows, the throttled ones marked, DLR states filled in, achieved rate and percentiles plausible, CSV export containing 5,000 data rows.

- [ ] **Step 7: Commit**

```bash
git add src/ui tests/ui/burst-panel.test.ts
git commit -m "feat(ui): burst panel with three recipient sources, live stats and CSV export"
```

---

### Task 20: Hex composer

**Files:**
- Create: `src/ui/source/class/smppclient/HexComposer.js`, `src/ui/source/class/smppclient/hex.js`
- Modify: `src/ui/source/class/smppclient/Shell.js`, `src/ui/source/class/smppclient/TracePanel.js`
- Test: `tests/ui/hex-composer.test.ts`

**Interfaces:**
- Consumes: Tasks 4, 10, 14, 18.
- Produces:
  - `hex.js`: `parseHex(text: string): Buffer` — tolerant of whitespace, newlines and `0x` prefixes; throws on an odd digit count or a non-hex character. `formatHex(buf: Buffer): string`.
  - `smppclient.HexComposer` — a hex text area, a byte count, a decode preview showing what the bytes would parse as (or the decode error), and a Send button wired to `sendRaw`. The preview is informational: a PDU that fails to decode is still sendable.
  - A "Edit as hex" action on a trace row that opens the composer pre-filled with that PDU's `raw`.

- [ ] **Step 1: Write the failing tests**

```ts
it("parses hex with spaces, newlines and 0x prefixes")
it("throws on an odd number of hex digits")
it("throws on a non-hex character")
it("formats a buffer as space-separated hex")
it("previews a valid PDU's decoded form")
it("previews a decode error and still reports the bytes as sendable")
it("round-trips a traced PDU's raw bytes through format and parse")
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/ui/hex-composer.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `hex.js` and `HexComposer`**

- [ ] **Step 4: Add the "Edit as hex" action to `TracePanel`**

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run tests/ui/hex-composer.test.ts`
Expected: PASS.

- [ ] **Step 6: Send a deliberately broken PDU by hand**

Bind to `FakeSmsc`, open the composer, take a traced `submit_sm`, change its `command_length` to `0xdeadbeef`, Send.
Expected: the fixture's `received` shows the bytes arrived exactly as composed, and the app did not rewrite the length. This is the spec's central anti-goal; if the bytes differ, the codec is repairing and must be fixed.

- [ ] **Step 7: Commit**

```bash
git add src/ui tests/ui/hex-composer.test.ts
git commit -m "feat(ui): raw hex PDU composer with edit-from-trace"
```

---

### Task 21: Packaging

**Files:**
- Create: `electron-builder.yml`
- Modify: `package.json`
- Test: `tests/packaging.test.ts`

**Interfaces:**
- Consumes: every prior task.
- Produces: `npm run dist` producing an unsigned macOS app in `release/`, with Windows and Linux targets configured but not built by default.

- [ ] **Step 1: Write the failing test**

```ts
it("lists dist/, compiled/build/ and package.json in the builder's files")
it("declares mac, win and linux targets")
it("does not enable signing or notarization")   // no identity, no notarize block
```

Read and assert on `electron-builder.yml`.

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/packaging.test.ts`
Expected: FAIL — file missing.

- [ ] **Step 3: Write `electron-builder.yml`**

`files` covers `dist/**`, `compiled/build/**` and `package.json`. `mac.identity: null` so an unsigned build succeeds. Targets: `dmg` and `zip` for mac, `nsis` for win, `AppImage` for linux.

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/packaging.test.ts`
Expected: PASS.

- [ ] **Step 5: Build and launch the packaged app**

Run: `npm run dist`, then open the app from `release/`.
Expected: it starts, the window renders, and a bind against `FakeSmsc` succeeds — which proves the qooxdoo bundle's relative paths survive packaging.

- [ ] **Step 6: Run the whole suite**

Run: `npx vitest run`
Expected: PASS, no skipped tests.

- [ ] **Step 7: Commit**

```bash
git add electron-builder.yml package.json tests/packaging.test.ts
git commit -m "chore: electron-builder packaging for mac, win and linux"
```
