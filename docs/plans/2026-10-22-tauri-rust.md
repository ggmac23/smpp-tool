# SMPP Client on Tauri and Rust — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use the implementation workflow (recommended) or the implementation workflow to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Electron main process with a Rust backend under Tauri v2, keeping the qooxdoo renderer and every behaviour the Electron build was verified to have.

**Architecture:** One Rust crate under `src-tauri`. `smpp::*` is the pure protocol layer (codec, text, segmentation, receipts) with no I/O; `app::*` owns the socket, the session, the trace, the burst engine and the profile store; `commands::*` is the Tauri bridge. The renderer is the existing qooxdoo application, served from `compiled/build`, talking to Rust through `invoke` and `listen` instead of a preload bridge. Bytes cross as lowercase hex strings, never binary.

**Tech Stack:** Rust (stable, 1.99), Tauri v2, tokio, serde, rustls (custom CA, client certs, optional skip-verify, SNI), encoding_rs (Latin-1, Cyrillic, Hebrew, Shift-JIS), keyring (macOS Keychain), qooxdoo 7.9.3 unchanged.

**Spec:** `docs/design/2026-10-22-tauri-rust-design.md`, which amends `docs/design/2026-10-21-smpp-client-design.md`. Both apply.

**Source of truth:** the TypeScript implementation on this branch is a working, tested reference. Each task below names the module it ports. Where this plan and the TypeScript disagree, the TypeScript wins unless the plan says otherwise — it is the version that was verified against a socket.

## Global Constraints

- Every anti-goal from the original spec holds: never hide the bytes, never block invalid input, never lose traffic under load, never silently reconnect or re-bind.
- The renderer **never builds or parses a PDU**. It sends intents; Rust composes. This is a correction of a drift in the Electron build.
- Tauri v2, stable Rust, tokio. No `unsafe` outside a documented, tested reason.
- A command returns `Result<T, String>`; errors cross as messages, never panics. A panic in a command is a defect.
- Bytes cross the bridge as lowercase hex strings.
- Trace and burst-row events are batched on a ~100 ms tick. Batching may delay; it may never drop or reorder.
- The burst result table is never capped. The trace is capped and says so.
- `cargo clippy -- -D warnings` is clean; `cargo fmt --check` is clean.
- macOS is the only target that must build and run.

## Review Focus

Input classes the happy path will not reach. Each is pinned to the task that owns the code.

1. **A PDU split across TCP reads, or several in one read, or a good PDU sharing a read with a malformed one.** Every PDU appears exactly once; the good ones survive the bad one. → Task 4, Step 7.
2. **A `command_length` that lies** — below 16, above the cap, or larger than the data that ever arrives. The first two are visible errors with no allocation; the third is visible as held bytes, not a silent stall. → Task 4, Step 9.
3. **An unknown `command_id`, or a response matching no pending request.** Traced, session still bound, nothing invented in reply. → Task 11, Step 8.
4. **Emoji under `data_coding` 0, and bodies at the 160/70/140 boundaries.** The counter and the segmenter agree, and unmappable characters are reported rather than silently replaced. → Task 6, Step 7 and Task 7, Step 5.
5. **A request the SMSC never answers, and a send queued behind a full window when the connection drops.** Both settle visibly; neither leaks. → Task 11, Step 10.

---

### Task 1: Tauri scaffold serving the existing renderer

**Files:**
- Create: `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/build.rs`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`
- Modify: `package.json` (scripts), `.gitignore` (`src-tauri/target`)
- Test: `src-tauri/tests/scaffold.rs`

**Interfaces:**
- Consumes: the existing `compiled/build` produced by `npm run build:ui`.
- Produces: `npm run tauri:dev` and `npm run tauri:build`; `cargo test` inside `src-tauri`.

- [ ] **Step 1: Write the failing test**

```rust
// src-tauri/tests/scaffold.rs
#[test]
fn tauri_config_serves_the_qooxdoo_bundle() {
    let conf: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("tauri.conf.json").unwrap()).unwrap();
    assert_eq!(conf["build"]["frontendDist"], "../compiled/build");
}

#[test]
fn the_renderer_bundle_exists() {
    assert!(std::path::Path::new("../compiled/build/index.html").exists());
    assert!(std::path::Path::new("../compiled/build/forms.js").exists());
}
```

- [ ] **Step 2: Run it to watch it fail**

Run: `cd src-tauri && cargo test --test scaffold`
Expected: FAIL — no `tauri.conf.json`.

- [ ] **Step 3: Create the crate**

`cargo init --name smppclient src-tauri`. Dependencies: `tauri` (v2, features `protocol-asset`), `tauri-build`, `tauri-plugin-dialog` (the open/save dialogs behind `pick_file`, `export_trace` and `export_burst`), `tokio` (`rt-multi-thread`, `net`, `time`, `sync`, `macros`), `serde`/`serde_json`, `thiserror`. Dev-dependency: `tokio` with `test-util`.

`tauri.conf.json` sets `build.frontendDist` to `../compiled/build`, `build.beforeBuildCommand` to `npm run build:ui`, one window 1440×960 titled "SMPP Client", and `app.security.csp` to a policy allowing only `self` — the renderer loads nothing remote.

- [ ] **Step 4: Run the test to watch it pass**

Run: `cd src-tauri && cargo test --test scaffold`
Expected: PASS.

- [ ] **Step 5: Verify the real window renders the real application**

Run `npm run tauri:dev`, then confirm from the webview that qooxdoo started: the application object exists and the six tab labels are present. A WKWebView harness already proved the bundle runs on this engine; this step proves Tauri serves it.
Expected: `submit_sm, data_sm, query / cancel / replace, submit_multi, burst, hex`.

- [ ] **Step 6: Commit**

```bash
git add src-tauri package.json .gitignore
git commit -m "feat(tauri): scaffold a Rust backend serving the qooxdoo renderer"
```

---

### Task 2: `smpp::constants`

**Files:**
- Create: `src-tauri/src/smpp/mod.rs`, `src-tauri/src/smpp/constants.rs`
- Test: inline `#[cfg(test)]` module

**Ports:** `src/smpp/constants.ts`.

**Interfaces:**
- Produces: `CommandId` constants and `command_name(u32) -> String` (`"unknown(0x…)"` for undefined ids); `command_status_name(u32) -> String`; `response_id_for(u32) -> u32`; `is_response(u32) -> bool`; enums/consts for `Ton`, `Npi`, `DataCoding`, `EsmClass` (with `ESM_UDHI_BIT`), `RegisteredDelivery`, `PriorityFlag`, `InterfaceVersion`; `known_tlv(u16) -> Option<&'static str>` and `tlv_name(u16) -> String`.
- The v5.0 broadcast family is `broadcast_sm`, `query_broadcast_sm`, `cancel_broadcast_sm`. **There is no `broadcast_multi`**; see the Task 2 ruling in the decision record.

- [ ] **Step 1: Write the failing tests**

Port from `tests/smpp/tlv.test.ts`'s `describe("command constants")`:

```rust
fn names_responses_from_their_request_id()   // 0x80000004 -> "submit_sm_resp"
fn names_an_unrecognised_command_id()        // 0x4242 -> "unknown(0x4242)"
fn names_every_command_in_the_spec_list()    // the 15 names, round-tripped
fn names_command_statuses()                  // ESME_ROK, 0x58 -> ESME_RTHROTTLED, unknown
```

- [ ] **Step 2: Run them to watch them fail**

Run: `cd src-tauri && cargo test smpp::constants`
Expected: FAIL — module does not exist.

- [ ] **Step 3: Implement `constants.rs`**

Tables as `const` arrays plus a lazily built reverse map. Values from the SMPP 3.4 and 5.0 specifications; nothing invented.

- [ ] **Step 4: Run them to watch them pass**

Run: `cd src-tauri && cargo test smpp::constants`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/smpp
git commit -m "feat(smpp): command, status and field constants in Rust"
```

---

### Task 3: `smpp::tlv`

**Files:** Create `src-tauri/src/smpp/tlv.rs`
**Ports:** `src/smpp/tlv.ts`.

**Interfaces:**
- Produces: `struct Tlv { tag: u16, value: Vec<u8> }`; `encode_tlvs(&[Tlv]) -> Vec<u8>`; `decode_tlvs(&[u8]) -> Result<Vec<Tlv>, TlvError>`.

- [ ] **Step 1: Write the failing tests**

Port `tests/smpp/tlv.test.ts`'s TLV cases: tag/length/value big-endian; round-trip; an unknown tag survives; a zero-length TLV; a declared length past the end is an error naming the tag.

- [ ] **Step 2: Run them to watch them fail**

Run: `cd src-tauri && cargo test smpp::tlv`
Expected: FAIL.

- [ ] **Step 3: Implement `tlv.rs`**

- [ ] **Step 4: Run them to watch them pass**

Run: `cd src-tauri && cargo test smpp::tlv`
Expected: PASS.

- [ ] **Step 5: Commit**

---

### Task 4: `smpp::defs`, `smpp::decode`, `smpp::frame`

**Files:** Create `src-tauri/src/smpp/defs.rs`, `decode.rs`, `frame.rs`
**Ports:** `src/smpp/pdu-defs.ts`, `src/smpp/decode.ts`, `src/smpp/frame.ts`.

**Interfaces:**
- `defs`: `FieldType { CString, U8, U16, U32, Octets, DestList, UnsuccessList }`, `FieldDef { name, ty, max_len, len_from }`, `fields_for(&str) -> Option<&'static [FieldDef]>`, covering every command in `CommandId` including both bind directions and the broadcast family.
- `decode`: `FieldValue` (an enum over `U32`, `Str`, `Bytes`, `DestList(Vec<DestEntry>)`, `UnsuccessList(Vec<UnsuccessEntry>)`); `Pdu { command_id, command_status, sequence_number, fields: IndexMap<String, FieldValue>, tlvs: Vec<Tlv>, raw: Vec<u8> }`; `decode_pdu(&[u8]) -> Result<Pdu, PduDecodeError>`; `PduDecodeError { message, raw, framed: Vec<Vec<u8>> }`.
  - A `command_length` that disagrees with the buffer is an error, never repaired.
  - An unrecognised `command_id` keeps the body as `fields["body"]` and parses no TLVs.
- `frame`: `PduFramer::push(&mut self, &[u8]) -> Result<Vec<Vec<u8>>, PduDecodeError>`; `pending(&self) -> Pending { buffered: usize, declared: Option<u32> }`; `reset(&mut self)`; `MAX_PDU_LENGTH = 1_048_576`.
  - **The error carries the PDUs already framed from the same chunk** (`framed`). Losing them is the "never lose traffic" anti-goal failing.

- [ ] **Step 1: Write the failing framing tests**

Port all of `tests/smpp/frame.test.ts`, including: one PDU per chunk; nothing until complete; reassembly across three chunks; two PDUs in one chunk; a tail that ends mid-PDU; a boundary inside the 16-byte header; `command_length` below 16; above `MAX_PDU_LENGTH`; a lying length with no allocation; `pending` reporting; reset.

- [ ] **Step 2: Write the failing decode tests**

Port all of `tests/smpp/decode.test.ts`: the header; `submit_sm` fields in wire order; `short_message` read via `sm_length`; trailing TLVs; `submit_sm_resp`; an unknown `command_id` keeping its body; `raw` preserved exactly; an unterminated C-string carrying `raw`; an `sm_length` past the body; a `command_length` disagreeing with the buffer.

- [ ] **Step 3: Run them to watch them fail**

Run: `cd src-tauri && cargo test smpp::`
Expected: FAIL.

- [ ] **Step 4: Implement `defs.rs`**

- [ ] **Step 5: Implement `frame.rs`**

Validate `command_length` against both bounds **before** using it as a size.

- [ ] **Step 6: Implement `decode.rs`**

- [ ] **Step 7: Run the framing tests** *(Review Focus 1)*

Run: `cd src-tauri && cargo test smpp::frame`
Expected: PASS, including the split-chunk, two-in-one-chunk and survives-a-bad-neighbour cases.

- [ ] **Step 8: Run the decode tests**

Run: `cd src-tauri && cargo test smpp::decode`
Expected: PASS.

- [ ] **Step 9: Add the lying-length and stalled-length tests** *(Review Focus 2)*

```rust
fn rejects_a_lying_command_length_without_allocating()
fn rejects_command_length_8()
fn reports_held_bytes_when_a_pdu_declares_more_than_it_sends()
```

Run: `cd src-tauri && cargo test smpp::frame`
Expected: PASS.

- [ ] **Step 10: Commit**

---

### Task 5: `smpp::encode`

**Files:** Create `src-tauri/src/smpp/encode.rs`
**Ports:** `src/smpp/encode.ts`.

**Interfaces:**
- `PduInput { command_id, command_status, sequence_number, fields, tlvs }`; `encode_pdu(&PduInput) -> Vec<u8>`; `encode_raw(&[u8]) -> Vec<u8>` (identity).
- No validation, no clamping, no truncation. The only computed field is `command_length`; a length or count field the caller supplied always wins over the derived one.

- [ ] **Step 1: Write the failing tests**

Port `tests/smpp/encode.test.ts` in full, including: round-trip; `command_length` matches; an over-long `service_type` verbatim; `data_coding` 0xff verbatim; derived `sm_length`; a caller-supplied `sm_length` that disagrees, going on the wire as supplied; zero values for omitted fields; TLVs appended; `bind_transceiver`; `submit_multi` with a destination list; an unknown command id from a raw body; `encode_raw` leaving a `0xdeadbeef` length untouched.

- [ ] **Step 2: Run them to watch them fail**

Run: `cd src-tauri && cargo test smpp::encode`
Expected: FAIL.

- [ ] **Step 3: Implement `encode.rs`**

- [ ] **Step 4: Run them to watch them pass**

Expected: PASS.

- [ ] **Step 5: Commit**

---

### Task 6: `smpp::text`

**Files:** Create `src-tauri/src/smpp/text.rs`
**Ports:** `src/smpp/text.ts`.

**Interfaces:**
- `Gsm7Packing { Unpacked, Packed }` — **unpacked is the default**, because that is what SMSCs expect over SMPP.
- `SegmentationMode { None, Udh, Sar }`.
- `encode_text(&str, data_coding: u8, packing) -> Encoded { bytes: Vec<u8>, unmappable: Vec<String> }`; `decode_text(&[u8], u8, packing) -> String`.
- `septets_for(&str) -> Vec<u8>`, `split_septets(&[u8], limit) -> Vec<Vec<u8>>` (never between an ESC and its pair), `split_by_code_units(&str, limit) -> Vec<String>` (never inside a surrogate pair).
- `measure(&str, u8, SegmentationMode, Gsm7Packing) -> Measurement { chars, bits, bytes, parts, unmappable }`.
- Limits: `GSM7_SINGLE 160`, `GSM7_CONCAT 153`, `UCS2_SINGLE_UNITS 70`, `UCS2_CONCAT_UNITS 67`, `OCTET_SINGLE 140`, `OCTET_CONCAT 134`.
- `SAMPLES` — the four presets.
- GSM 03.38 basic and extension tables by hand; Latin-1, Cyrillic, Hebrew and Shift-JIS through `encoding_rs`.

- [ ] **Step 1: Write the failing tests**

Port `tests/smpp/text.test.ts` in full: septet packing (160 chars → 140 bytes packed, 160 unpacked); unpacked is the default; round-trips both ways; an escape character costs two septets; every escape-table character encodes; UCS2 big-endian; octet passthrough; Latin-1, Cyrillic, Hebrew, JIS; part counts at 160/161 and 70/71; one part in `None` mode; the four presets measuring as their names claim.

- [ ] **Step 2: Run them to watch them fail**

Run: `cd src-tauri && cargo test smpp::text`
Expected: FAIL.

- [ ] **Step 3: Implement the GSM 03.38 tables and packing**

- [ ] **Step 4: Implement the other codings through `encoding_rs`**

- [ ] **Step 5: Implement `measure` and the splitters**

- [ ] **Step 6: Run them to watch them pass**

Expected: PASS.

- [ ] **Step 7: Add the unmappable-character tests** *(Review Focus 4)*

```rust
fn reports_emoji_as_unmappable_under_gsm7_and_does_not_drop_them()
fn substitutes_a_question_mark_for_an_unmappable_character()
fn reports_each_distinct_unmappable_character_once()
fn reports_unmappable_characters_from_measure_too()
```

Expected: PASS.

- [ ] **Step 8: Commit**

---

### Task 7: `smpp::segment`

**Files:** Create `src-tauri/src/smpp/segment.rs`
**Ports:** `src/smpp/segment.ts`.

**Interfaces:**
- `BodyPlacement { ShortMessage, MessagePayload }`.
- `segment(SegmentOptions) -> Vec<Segment>` where `Segment { fields, tlvs, unmappable }`.
- Rules, all of which the TypeScript learned the hard way: `None` returns exactly one segment however long the text is; a body within the **single**-segment limit is one segment even in a concatenating mode; `Udh` emits an 8-bit-reference concatenation IE and sets UDHI into the caller's `esm_class`; `Sar` emits `sar_*` TLVs and leaves `esm_class` alone; a caller-supplied UDH turns automatic concatenation off and wins.

- [ ] **Step 1: Write the failing tests**

Port `tests/smpp/segment.test.ts` in full, and these two in particular:

```rust
fn sends_a_full_single_segment_as_one_pdu_not_two()  // 160/70/140 boundaries
fn agrees_with_the_live_counter_about_how_many_parts()  // measure().parts == segment().len()
```

- [ ] **Step 2: Run them to watch them fail**

Expected: FAIL.

- [ ] **Step 3: Implement `segment.rs`**

- [ ] **Step 4: Run them to watch them pass**

Expected: PASS.

- [ ] **Step 5: Run the counter-agreement test at every boundary** *(Review Focus 4)*

Run: `cd src-tauri && cargo test smpp::segment::agrees_with_the_live_counter`
Expected: PASS. This is the defect that split a plain 160-character message in two.

- [ ] **Step 6: Commit**

---

### Task 8: `smpp::receipt`, `smpp::summary` and `app::dlr`

**Files:** Create `src-tauri/src/smpp/receipt.rs`, `src-tauri/src/smpp/summary.rs`, `src-tauri/src/app/mod.rs`, `src-tauri/src/app/dlr.rs`
**Ports:** `src/smpp/receipt.ts`, `src/smpp/summary.ts`, `src/main/dlr.ts`.

**Interfaces:**
- `Receipt { message_id: Option<String>, source: ReceiptSource, state, error_code, submit_date, done_date }`; `parse_receipt(&Pdu) -> Option<Receipt>`, accepting `deliver_sm` and `data_sm`, and treating a PDU as a receipt when esm_class says so, or it carries `receipted_message_id`, or its body reads as receipt text.
- `summarise(&Pdu) -> String` and `describe(&Pdu) -> Vec<Field { name, value }>`.
- `CorrelationConfig { prefer_tlv, parse_text, normalise_radix, strip_leading_zeros }`; `normalise_id(&str, &CorrelationConfig) -> Vec<String>`; `DlrCorrelator::{register, match_receipt, unmatched, pending, reset}`. A matched registration is retired so a duplicate cannot steal another row's match.

- [ ] **Step 1: Write the failing tests**

Port `tests/smpp/receipt.test.ts`, `tests/smpp/summary.test.ts` and `tests/main/dlr.test.ts` in full, including the hex↔decimal and leading-zero cases and "does not let a second receipt steal the first row's match".

- [ ] **Step 2: Run them to watch them fail**

Expected: FAIL.

- [ ] **Step 3: Implement `receipt.rs`**

- [ ] **Step 4: Implement `summary.rs`**

- [ ] **Step 5: Implement `dlr.rs`**

- [ ] **Step 6: Run them to watch them pass**

Expected: PASS.

- [ ] **Step 7: Commit**

---

### Task 9: the fake SMSC test fixture

**Files:** Create `src-tauri/src/testing/fake_smsc.rs` (behind `#[cfg(any(test, feature = "testing"))]`)
**Ports:** `tests/fixtures/smsc-server.ts`.

**Interfaces:**
- `FakeSmsc::new()`, `with_tls(TlsMaterial)`, `listen() -> u16`, `close()`, `received() -> Vec<Pdu>`, `on(command, handler)` where a handler returns `Option<Vec<PduInput>>` and `None` means answer nothing, `send(PduInput)`, `send_raw(&[u8])`, `wait_for_connection()`, `connection_count()`, `last_peer_certificate_subject()`, `last_servername()`.
- Defaults: bind answers `ESME_ROK`; `submit_sm` answers an incrementing `message_id`; `enquire_link` answers `ESME_ROK`; a response from the client is never answered.
- Reuses the committed test certificates in `tests/fixtures/tls`.

- [ ] **Step 1: Write the failing tests**

Port `tests/fixtures/smsc-server.test.ts`: accepts a bind; records what it received; incrementing message ids; silence when the handler says so; a handler choosing the status; an unsolicited `deliver_sm`; malformed bytes via `send_raw`; a TLS connection; a requested client certificate.

- [ ] **Step 2: Run them to watch them fail**

Expected: FAIL.

- [ ] **Step 3: Implement `fake_smsc.rs`** on tokio.

- [ ] **Step 4: Run them to watch them pass**

Expected: PASS.

- [ ] **Step 5: Commit**

---

### Task 10: `app::transport`

**Files:** Create `src-tauri/src/app/transport.rs`
**Ports:** `src/main/transport.ts`.

**Interfaces:**
- `TlsOptions { enabled, ca: Option<Vec<u8>>, reject_unauthorized, servername, client_cert, client_key, client_key_passphrase }`; `TransportOptions { host, port, tls, connect_timeout }`.
- `Transport::connect(opts) -> Result<Transport, TransportError>`; `write(&[u8])`; `shutdown()`; `pending() -> Pending`; an event stream carrying `Pdu bytes`, `DecodeError`, `Closed(Option<String>)`, `BytesIn`, `BytesOut`.
- **It never reconnects.** The PDUs already framed before a decode error are delivered before the error.
- rustls, with a custom verifier for `reject_unauthorized: false`, a custom root store for `ca`, and client certificates for mutual TLS.

- [ ] **Step 1: Write the failing tests**

Port `tests/main/transport.test.ts` in full: plain TCP framing; two PDUs in one read; the good PDUs that shared a read with a malformed one; `pending` reporting; decode error without closing; close without reconnect; close exactly once; byte counters; a refused port; a timeout against 192.0.2.1; TLS refusing an untrusted certificate; accepting it when verification is off; accepting it with the CA; SNI; a client certificate.

- [ ] **Step 2: Run them to watch them fail**

Expected: FAIL.

- [ ] **Step 3: Implement `transport.rs`**

- [ ] **Step 4: Run them to watch them pass**

Expected: PASS.

- [ ] **Step 5: Commit**

---

### Task 11: `app::session`

**Files:** Create `src-tauri/src/app/session.rs`
**Ports:** `src/main/session.ts`.

**Interfaces:**
- `BindMode { Transceiver, Transmitter, Receiver }`; `BindState { Unbound, Binding, Bound, Unbinding, Closed }`; `BindOptions`.
- `Session::new(transport, SessionOptions { window_size, response_timeout })`; `bind(opts) -> Result<Pdu, SessionError>`; `unbind()`; `send(PduInput) -> Result<Pdu, SessionError>`; `send_raw(&[u8])`; `state()`; `mode()`; `pending()`.
- Events: `PduIn`, `PduOut`, `State { state, mode }`, `Orphan(Pdu)`, `Timeout { sequence_number }`, `DecodeError`, `RawOut`.
- Answers `deliver_sm`, `data_sm` and `enquire_link` automatically; answers an inbound `unbind` and closes. Anything else is an orphan and gets **no** reply.
- `send` resolves on a non-zero `command_status`; only a timeout or a lost connection fails. `bind` checks the status itself.
- Sequence numbers from 1, wrapping at `0x7fffffff`.
- **A send queued for a window slot is failed when the connection drops** — see Review Focus 5. Mark the session closing before settling anything, so releasing a slot cannot wake a send onto a dead socket.

- [ ] **Step 1: Write the failing tests**

Port `tests/main/session.test.ts` in full.

- [ ] **Step 2: Run them to watch them fail**

Expected: FAIL.

- [ ] **Step 3: Implement bind, unbind and state**

- [ ] **Step 4: Implement sequence allocation, the window and response matching**

- [ ] **Step 5: Implement the automatic answers**

- [ ] **Step 6: Run them to watch them pass**

Expected: PASS.

- [ ] **Step 7: Verify no reconnect**

Run: `cd src-tauri && cargo test session::does_not_re_bind`
Expected: PASS. This pins a Global Constraint; do not weaken it later for convenience.

- [ ] **Step 8: Add the orphan and unknown-command tests** *(Review Focus 3)*

Expected: PASS, with the session still bound afterwards.

- [ ] **Step 9: Add the response-timeout tests** *(Review Focus 5)*

A request the SMSC never answers fails, emits `Timeout`, and frees its window slot.

- [ ] **Step 10: Add the queued-send test** *(Review Focus 5)*

```rust
fn rejects_sends_still_queued_for_a_window_slot_when_the_connection_drops()
```

With `window_size` smaller than the queue depth. Without this the burst hangs forever — the defect the Electron review caught.

- [ ] **Step 11: Commit**

---

### Task 12: `app::trace`, `app::export`

**Files:** Create `src-tauri/src/app/trace.rs`, `src-tauri/src/app/export.rs`
**Ports:** `src/main/trace.ts`, `src/main/export.ts`.

**Interfaces:**
- `TraceEntry { seq, at, direction, command_name, command_status, sequence_number, summary, raw, decode_error }`; `TraceStore::{add, entries(from, limit), size, dropped, counters, set_cap, clear}`; a ring capped at 10,000 by default, counters never reduced by eviction; `TraceFileSink`.
- `to_csv(rows, columns) -> String` (RFC 4180) and `trace_to_text(&[TraceEntry]) -> String`.

- [ ] **Step 1: Write the failing tests**

Port `tests/main/trace.test.ts` and `tests/main/export.test.ts` in full.

- [ ] **Step 2: Run them to watch them fail**

Expected: FAIL.

- [ ] **Step 3: Implement both modules**

- [ ] **Step 4: Run them to watch them pass**

Expected: PASS.

- [ ] **Step 5: Commit**

---

### Task 13: `app::recipients`, `app::burst`

**Files:** Create `src-tauri/src/app/recipients.rs`, `src-tauri/src/app/burst.rs`
**Ports:** `src/main/recipients.ts`, `src/main/burst.ts`.

**Interfaces:**
- `RecipientSource { Repeat { number, count }, Range { from, count }, Csv { text, column } }`; `resolve_recipients(&RecipientSource) -> Result<Resolved { numbers, warnings }, String>`; `MAX_BURST 50_000`, `MAX_RATE 500`. Header detection: naming a column means there is a header; otherwise row one is a header when its cell is a known header word or holds no digits while a later row's does.
- `BurstConfig { recipients, rate_per_sec, template, text, data_coding, segmentation, placement, packing, backoff_on_throttle, await_dlr }`; `BurstRow`; `BurstStats`; `BurstRunner::{start, stop, rows, stats, current_rate_per_sec}`; events `Row`, `Progress`, `Done { reason, stats, warnings }`.
- Behaviour the TypeScript learned: hold the requested rate on `ESME_RTHROTTLED` unless backoff is on; stop and report on a lost bind; refuse over the ceilings before sending anything; one row per message with `pdus` counted separately; the first non-zero status is the row's verdict; **the achieved rate is measured over the sending, not the wait for receipts**; **DLR latency is measured from the row's own submit**; **a rate below one message per pacing tick is honoured, not rounded up**; recipient warnings reach `Done`.

- [ ] **Step 1: Write the failing recipient tests**

Port `tests/main/recipients.test.ts` in full.

- [ ] **Step 2: Write the failing burst tests**

Port `tests/main/burst.test.ts` in full, including the four defect tests named in the spec's "Porting the tests".

- [ ] **Step 3: Run them to watch them fail**

Expected: FAIL.

- [ ] **Step 4: Implement `recipients.rs`**

- [ ] **Step 5: Implement `burst.rs`**

Pace on a tokio interval; above one message per tick send a batch each tick, below it send one and wait the whole gap.

- [ ] **Step 6: Run them to watch them pass**

Expected: PASS.

- [ ] **Step 7: Commit**

---

### Task 14: `app::profiles`

**Files:** Create `src-tauri/src/app/profiles.rs`
**Ports:** `src/main/profiles.ts`.

**Interfaces:**
- `Profile` carrying no secret fields, with CA and certificate **paths** rather than contents; `SecretStore` trait with a `keyring`-backed implementation and a fake for tests; `ProfileStore::{list, save, password, key_passphrase, remove}`.
- Secrets are written before the profile record; saving without a password keeps the stored one; a store that cannot encrypt refuses rather than writing plaintext; a corrupt profile file starts empty.
- **Additionally** (a deferred Minor from the Electron review, worth fixing while porting): `save` whitelists the fields it writes, so the "no secret reaches the profile file" guarantee is structural rather than resting on the caller's shape.

- [ ] **Step 1: Write the failing tests**

Port `tests/main/profiles.test.ts` in full, plus:

```rust
fn writes_only_known_fields_even_if_handed_extra_ones()
```

- [ ] **Step 2: Run them to watch them fail**

Expected: FAIL.

- [ ] **Step 3: Implement `profiles.rs`**

- [ ] **Step 4: Run them to watch them pass**

Expected: PASS.

- [ ] **Step 5: Commit**

---

### Task 15: the Tauri bridge

**Files:** Create `src-tauri/src/commands.rs`, `src-tauri/src/app/controller.rs`, `src-tauri/src/app/events.rs`
**Ports:** `src/main/controller.ts`, `src/main/ipc.ts`.

**Interfaces:**
- `Controller` owning the transport, session, trace, burst runner and profile store behind a `tokio::sync::Mutex`, held in Tauri state.
- One `#[tauri::command]` per name in the TypeScript `COMMAND_NAMES`, with the same semantics: `connect`, `disconnect`, `bind`, `unbind`, `send_pdu`, `send_raw`, `start_burst`, `stop_burst`, `trace`, `pdu_fields`, `pdu_hex`, `clear_trace`, `set_trace_cap`, `export_trace`, `export_burst`, `burst_rows`, `list_profiles`, `save_profile`, `remove_profile`, `profile_secrets`, `measure`, `pick_file`, `status`. Plus two the renderer now needs because it no longer builds PDUs: `compose_submit` (form values → the PDUs that would be sent, with warnings) and `preview_pdu` (hex → decoded summary, fields and `sendable`).
- File dialogs come from `tauri-plugin-dialog`, injected behind a trait so the controller's tests do not need a window — the same seam the TypeScript `Dialogs` interface provided.
- Events emitted to the webview: `state`, `pdu` (batched), `counters` (batched, latest wins), `burst-progress`, `burst-row` (batched), `burst-done`, `error`. A ~100 ms batching tick; delay is allowed, dropping and reordering are not.

- [ ] **Step 1: Write the failing tests**

```rust
fn every_command_the_renderer_calls_is_registered()  // against a list mirroring the TS COMMAND_NAMES
fn batches_pdu_events_instead_of_one_per_pdu()
fn preserves_pdu_order_inside_a_batch()
fn never_drops_a_pdu_when_batching()                 // 2000 in, 2000 out
fn batches_counters_latest_snapshot_wins()
fn a_failing_command_returns_an_error_not_a_panic()
fn compose_submit_warns_without_withholding_the_pdus()
```

- [ ] **Step 2: Run them to watch them fail**

Expected: FAIL.

- [ ] **Step 3: Implement `events.rs` (the batcher)**

- [ ] **Step 4: Implement `controller.rs`**

- [ ] **Step 5: Implement `commands.rs`**

- [ ] **Step 6: Run them to watch them pass**

Expected: PASS.

- [ ] **Step 7: Commit**

---

### Task 16: the renderer talks to Tauri

**Files:**
- Modify: `src/ui/source/class/smppclient/Api.js`, `SubmitForm.js`, `HexComposer.js`, `BurstPanel.js`
- Modify: `src/shared/ui/submit.ts`, `src/shared/ui/hex.ts`, `src/shared/ui/index.ts`, `src/shared/api.ts`
- Delete: `src/shared/ui/buffer-shim.ts`
- Modify: `package.json` (drop the `buffer` dependency and the esbuild inject)
- Test: `tests/ui/*` adjusted

**Interfaces:**
- `smppclient.Api` calls `window.__TAURI__.core.invoke(command, args)` and `window.__TAURI__.event.listen(name, cb)`, keeping its existing `call`/`on`/`commandError` surface so no other widget changes.
- `src/shared/ui/submit.ts` keeps the form-values shape and the option lists but no longer segments or encodes: `buildSubmitInput` becomes the argument builder for `compose_submit`.
- `src/shared/ui/hex.ts` keeps `parseHex`/`formatHex`; `previewPdu` becomes a call to `preview_pdu`.

- [ ] **Step 1: Write the failing tests**

```ts
it("builds the compose_submit arguments from the form, including TLV rows")
it("keeps every option list the widgets read")
it("no longer exports a PDU builder")            // the renderer must not compose PDUs
it("parses and formats hex without Buffer")      // the bundle test, now with no polyfill
```

- [ ] **Step 2: Run them to watch them fail**

Run: `npx vitest run tests/ui`
Expected: FAIL.

- [ ] **Step 3: Rework `submit.ts` and `hex.ts`**

- [ ] **Step 4: Point `Api.js` at Tauri**

- [ ] **Step 5: Drop the Buffer shim and the `buffer` dependency**

- [ ] **Step 6: Run the renderer tests**

Run: `npx vitest run`
Expected: PASS, with no module importing from `src/main`.

- [ ] **Step 7: Commit**

---

### Task 17: parity verification and packaging

**Files:**
- Modify: `src-tauri/tauri.conf.json` (bundle targets)
- Delete: `src/main/**`, `src/preload/**`, `electron-builder.yml`, the `electron` and `electron-builder` dependencies
- Test: `src-tauri/tests/packaging.rs`

**Interfaces:** `npm run tauri:build` produces a macOS `.app` and `.dmg`.

- [ ] **Step 1: Write the failing test**

```rust
fn bundles_for_macos_without_signing()
fn does_not_package_the_test_certificates()
```

- [ ] **Step 2: Run it to watch it fail**

Expected: FAIL.

- [ ] **Step 3: Configure the bundle**

- [ ] **Step 4: Run the test**

Expected: PASS.

- [ ] **Step 5: Verify parity against a live SMSC**

Against the fake SMSC, through the real application, confirm every one of these — the same list the Electron build was held to:
- bind reaches "Bound (TRX)" and the counters move
- a three-part preset sends three PDUs with a shared concatenation reference and `sm_length` under 255
- a delivery receipt is decoded and correlated
- a malformed inbound PDU appears as a decode error and the good PDUs sharing its read survive
- clicking a trace row fills the hex and field panes
- the flow diagram draws one arrow per PDU with both offset columns
- a burst of 20 against an SMSC throttling one in five reports 16 accepted, 4 throttled, 16 receipts, 0 unmatched
- a hand-composed PDU with `command_length` `0xdeadbeef` arrives at the socket byte for byte

- [ ] **Step 6: Build and launch the packaged app**

Run: `npm run tauri:build`, then launch the produced `.app` and bind it to a fake SMSC.
Expected: it starts, renders, and binds. Record the bundle size against Electron's 242 MB.

- [ ] **Step 7: Remove Electron**

- [ ] **Step 8: Run everything**

Run: `npx vitest run && cd src-tauri && cargo test && cargo clippy -- -D warnings && cargo fmt --check`
Expected: all green.

- [ ] **Step 9: Commit**
