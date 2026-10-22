# Decision record — SMPP client

Every deviation from the plan, why it was made, and what it costs if it was the
wrong call. Written as the work happened, so it is a log rather than a summary.
Each `Ruling:` line is a decision taken without asking; 45 of them in total.

Plan: docs/plans/2026-10-21-smpp-client.md

Spec: docs/design/2026-10-21-smpp-client-design.md (read)
Executor: inline (the implementation workflow), native execution chosen by user
MERGE_BASE: b2878148f521822259d1d7551cec86088bd0378f (main)
Branch: feat/smpp-client

Setup: Ruling: no worktree — the directory was not a git repository at all, so there was
no existing branch or work to isolate from. `git init -b main` + empty initial commit +
`git checkout -b feat/smpp-client`, which also gives the final review a real MERGE_BASE.
Deviates from Task 1 Step 1 ("git init") only by naming the branch and adding the empty
commit. Cost if wrong: trivial — rename the branch.

## Pre-flight scan (shared interfaces)

| Produces | Consumes | Checked | Result |
|---|---|---|---|
| T2 CommandId/commandName/Tlv/decodeTlvs | T3, T4, T16 | names + shapes | clean |
| T3 PDU_FIELDS, Pdu, PduDecodeError, PduFramer | T4, T9, T18, T20 | names + shapes | clean |
| T4 PduInput, encodePdu, encodeRaw | T6, T10, T12, T20 | names + shapes | clean |
| T5 SegmentationMode, encodeText, measure, SAMPLES | T6, T16 | `SegmentationMode` moved to T5 during plan self-review | clean |
| T6 BodyPlacement, segment() | T12, T16 | names + shapes | clean |
| T7 Receipt, CorrelationConfig, DlrCorrelator | T12, T13, T18 | names + shapes | clean |
| T8 FakeSmsc | T9, T10, T12 | names + shapes | clean |
| T9 TlsOptions, Transport | T10, T13, T15 | names + shapes | clean |
| T10 BindMode, BindState, BindOptions, Session | T12, T13, T14, T15 | names + shapes | clean |
| T11 TraceEntry, TraceStore, toCsv | T14, T18 | `TraceEntry.seq` (row ordinal) vs `.sequenceNumber` (SMPP) — both needed, kept | clean |
| T12 BurstConfig/Row/Stats, MAX_BURST, MAX_RATE | T14, T19 | names + shapes | **conflict, see ruling below** |
| T14 Commands, Events, window.smpp | T15–T20 | every command has a producer | clean |

Pre-flight: Ruling: T19 must omit the DLR columns under a transmitter-only bind, but T14's
`"state"` event was specified as carrying only a `BindState`, which does not say which bind
mode is live. Decided: the `"state"` event payload is `{ state: BindState, mode: BindMode | null }`,
`mode` being null when unbound. Smallest change that satisfies the spec's session model.
Cost if wrong: one field on one event.

## Tasks

Task 1: Ruling: installed the toolchain (package.json, tsconfigs, vitest.config, npm install)
before Step 3 instead of after — `npx vitest` cannot run before vitest is installed, so the
plan's step order was impossible. The test was still written before any product code and was
still watched failing. Cost if wrong: none; ordering only.
Task 1: Ruling: `npm install` failed with EACCES because ~/.npm/_cacache holds root-owned
files (needs `sudo chown -R 501:20 ~/.npm`, which I cannot run). Installed with
`--cache <scratchpad>/npm-cache`. Cost if wrong: future installs need the same flag until the
user fixes the cache ownership. Flagged to the user.
Task 1: Ruling: the plan listed `compile.json` at the repo root, but `qx create` puts it in the
app root it generates (src/ui/compile.json). Kept the CLI's layout: build target outputPath is
`../../compiled/build` and `build:ui` is `cd src/ui && qx compile --target=build`, so the bundle
still lands at repo-root compiled/build as the plan's Interfaces block requires.
Cost if wrong: one path in one script.
Task 1: Ruling: Step 7 ("verify the window opens") verified headlessly — a scratchpad Electron
harness loaded compiled/build/index.html with the real security flags and read back
document.body.innerText = "SMPP Client", with no did-fail-load. Stronger evidence than
eyeballing a window, and it proves the bundle's relative paths work under file://.
Cost if wrong: none.
Task 1: note for final review: qooxdoo's generated boot HTML ships no Content-Security-Policy,
so Electron logs an "Insecure Content-Security-Policy / unsafe-eval" warning in dev. The
renderer loads only local files. Not fixed — out of the plan's scope for this task.
Task 1: complete (commits b287814..ffd4c52, tests: npx vitest run →    Duration  223ms (transform 21ms, setup 0ms, collect 15ms, tests 1ms, environment 0ms, prepare 55ms))

Task 2: Ruling: the spec and plan list `broadcast_multi` as a PDU, but SMPP defines no such
command. The v5.0 broadcast family is broadcast_sm (0x111), query_broadcast_sm (0x112) and
cancel_broadcast_sm (0x113); "BroadcastMulti" on the reference page has no protocol counterpart.
Implemented the three real commands and dropped broadcast_multi rather than fabricate a command
id that no SMSC could parse — which the spec's own "never silently repair / never fabricate"
stance requires. **Task 17 must use this set**, not the plan's `broadcast_multi`.
Cost if wrong: if Melrose really exposes a vendor-specific broadcast_multi, it is one more entry
in REQUESTS plus one form; nothing else changes.
Task 2: note: added `outbind` and `alert_notification` as one-way commands (the protocol defines
them with no response) so the decoder can name them when an SMSC sends one. Not in the plan's
list; they cost two table entries and prevent an "unknown(0x…)" row for a legitimate PDU.
Task 2: complete (commits ffd4c52..b55b55f, tests: npx vitest run →    Duration  267ms (transform 47ms, setup 0ms, collect 57ms, tests 6ms, environment 0ms, prepare 111ms))

Task 3: Ruling: the plan gave FieldDef four types (cstring/u8/u32/octets), but submit_multi
carries a repeating dest_address list and submit_multi_resp a repeating unsuccess_sme list,
which those types cannot express — and submit_multi is in the spec's PDU list. Added `u16`,
`dest_list` and `unsuccess_list`, plus `DestEntry`/`UnsuccessEntry`, and widened
`Pdu.fields` to `Record<string, FieldValue>` where FieldValue adds those arrays.
**Tasks 4 and 17 use FieldValue, not the plan's narrower union.**
Cost if wrong: two field types and one exported type alias.
Task 3: Ruling: `decodePdu` rejects a PDU whose command_length disagrees with the buffer it was
handed, rather than trusting one over the other. The framer already guarantees they agree for
framed input, so this only fires for hand-composed bytes — where the hex composer (Task 20)
wants the error shown in its preview, not silently fixed. Cost if wrong: the preview would need
a lenient mode; sending is unaffected because sendRaw never decodes.
Task 3: Ruling: my own `reset` test asserted that pushing the tail of a half-sent PDU after
reset yields no PDUs. Wrong premise — those bytes start mid-header and read as command_length 0,
so throwing is correct. Rewrote the test to assert `buffered` goes to 0 and the next whole PDU
frames cleanly. Production code unchanged. Cost if wrong: none.
Task 3: complete (commits b55b55f..a0b298f, tests: npx vitest run →    Duration  290ms (transform 104ms, setup 0ms, collect 188ms, tests 16ms, environment 1ms, prepare 203ms))

Task 4: no rulings. Length/count fields are derived only when the caller omits them; a supplied
sm_length that contradicts short_message goes on the wire as supplied, which the test pins.
Task 4: complete (commits a0b298f..12f39e8, tests: npx vitest run →    Duration  287ms (transform 138ms, setup 0ms, collect 246ms, tests 25ms, environment 1ms, prepare 263ms))

Task 5: Ruling: the brief's test demanded packed GSM 03.38 septets (160 chars -> 140 bytes), but
over SMPP `data_coding 0` is conventionally sent one septet per octet, because sm_length counts
octets — packed 7-bit is the GSM air-interface layout and most SMSCs reject it. Shipping the
plan's packing as the only behaviour would have made the tool fail against real SMSCs. Made it a
`Gsm7Packing` parameter ("unpacked" default, "packed" available), kept the plan's 140-byte test
for the packed path and added the 160-byte test for the default. **Tasks 6, 16 and 19 pass this
through; the submit form needs a packing control.** Cost if wrong: one default to flip.
Task 5: Ruling: encoding for Cyrillic, Hebrew, Latin-1 and JIS is derived from the platform's
own TextDecoder by decoding every byte (and, for Shift-JIS, every two-byte pair) once and
inverting the result, instead of hand-written tables. Built lazily and cached. Cost if wrong: a
coding whose decoder the platform lacks degrades to octets, which the unmappable list reports.
Task 5: Ruling: exported `septetsFor`, `splitSeptets` and `splitByCodeUnits` beyond the brief's
interface list, so the live counter and Task 6's segmentation cannot disagree about where a
boundary falls. Not in the plan's Produces block. Cost if wrong: three extra exports.
Task 5: Ruling: two of my own tests were wrong — the escape-split case did not exceed 160
septets (so one part was correct), and the ucs2_3 preset was long enough for four parts. Fixed
the test's premise and shortened the preset. Production code unchanged by the first; the preset
is product content. Cost if wrong: none.
Task 5: complete (commits 12f39e8..ff5d2ff, tests: npx vitest run →    Duration  302ms (transform 250ms, setup 0ms, collect 402ms, tests 38ms, environment 1ms, prepare 335ms))

Task 6: Ruling: `sar` mode splits at the same 153 septets / 67 UCS2 units as `udh` mode, even
though a sar body carries no UDH and could hold 160/70. Reason: the live counter (Task 5
`measure`) and segmentation must agree about where a boundary falls, and they share
`splitSeptets`. The spec does not specify sar segment sizing. Cost if wrong: a sar message of
154-160 septets takes two segments where one would do; one constant to change.
Task 6: Ruling: when placement is `message_payload` and the mode is `udh`, the concatenation UDH
goes inside message_payload and UDHI is still set, rather than refusing the combination. The
combination is contradictory in practice, but the spec's anti-goal forbids blocking what the user
asked for — so the bytes are exactly what those two choices mean. Cost if wrong: the form could
warn instead; no code change needed to warn.
Task 6: `segment()` also returns `unmappable` per segment (not in the plan's Segment shape) so
the submit form can surface it without re-encoding. `esmClass` is an input so UDHI is OR-ed into
the form's messaging mode instead of overwriting it.
Task 6: complete (commits ff5d2ff..fbe9f2a, tests: npx vitest run →    Duration  302ms (transform 264ms, setup 0ms, collect 453ms, tests 48ms, environment 1ms, prepare 385ms))

Task 7: Ruling: `parseReceipt` treats a PDU as a receipt when esm_class says so, OR it carries a
receipted_message_id TLV, OR its body reads as receipt text (an `id:` plus a `stat:`). The brief
only mentioned the esm_class bits, but real SMSCs send receipt text with esm_class 0, and this is
an interop-testing tool. A plain inbound message still returns null, which the brief's test pins.
Cost if wrong: an odd inbound message with `id:`+`stat:` in its text would show as a receipt.
Task 7: Ruling: `parseReceipt` also accepts data_sm, not just deliver_sm — some SMSCs deliver
receipts that way. Cost if wrong: one command id in one condition.
Task 7: Ruling: a matched registration is retired, so a duplicate receipt cannot steal another
row's match, and colliding registrations are served earliest-first. The brief named the rule but
not the retirement; without it a repeated receipt silently reassigns a row. Cost if wrong: a
legitimate duplicate receipt lands in `unmatched`, where it is visible rather than lost.
Task 7: complete (commits fbe9f2a..c81a4f8, tests: npx vitest run →    Duration  457ms (transform 483ms, setup 0ms, collect 1.01s, tests 81ms, environment 2ms, prepare 1.15s))

Task 8: Ruling: used committed test certificates (tests/fixtures/tls, CA + server + client,
expiring 2046, documented as test-only with the private keys in the clear) instead of the brief's
`generateSelfSignedCert()`. The brief allowed this explicitly, and it also gives the Task 9 tests
a real CA to trust and a real client cert for mutual TLS, which runtime self-signing would not.
Exported as `testCerts`. Cost if wrong: regenerate with the openssl commands in this commit.
Task 8: added `waitForConnection()`, `connectionCount` and `lastPeerCertificateSubject` beyond the
brief's API — the first two so a test can push a PDU only once a client is attached, the third so
the mTLS test can assert the certificate actually arrived.
Task 8: complete (commits c81a4f8..2ce5f53, tests: npx vitest run →    Duration  592ms (transform 422ms, setup 0ms, collect 849ms, tests 322ms, environment 2ms, prepare 672ms))

Task 9: Ruling: the brief had one dead-port test for connectTimeoutMs, but a closed local port
is refused immediately rather than timing out, so that test would never have exercised the
timeout. Split it: one test asserts rejection on a refused port, one asserts the timeout against
192.0.2.1 (TEST-NET-1, never routed) with a 200ms budget. Cost if wrong: the second test is the
only one that depends on an address staying unroutable.
Task 9: a decode error resets the framer and keeps the socket open, which the brief specified;
the test also pins that a well-formed PDU arriving afterwards is still delivered.
Task 9: added `lastServername` to the FakeSmsc fixture so the SNI test can assert what the
server actually received.
Task 9: complete (commits 2ce5f53..af446fb, tests: npx vitest run →    Duration  1.02s (transform 592ms, setup 0ms, collect 1.21s, tests 908ms, environment 2ms, prepare 1.01s))

Task 10: Ruling: an inbound request the client does not answer (an unknown command_id, or any
request other than deliver_sm / data_sm / enquire_link / unbind) is emitted as `orphan` and
nothing is sent back. The protocol would have the peer answered with generic_nack, but a test
client inventing traffic the user did not ask for is worse here: the user sees the orphan in the
trace and can answer by hand from the hex composer. Cost if wrong: an SMSC waiting on a response
could stall; the fix is one `reply()` call behind a preference.
Task 10: Ruling: `send()` resolves on a non-zero command_status instead of rejecting, so the
burst engine can record errors by status; only a timeout or a lost connection rejects. `bind()`
checks the status itself and rejects on refusal, which the brief's test pins.
Task 10: Ruling: the `state` event payload is `{ state, mode }` as decided in the pre-flight
scan, and `Session.mode` exposes the live bind mode for Task 19's DLR columns.
Task 10: added beyond the brief: `pending` (requests in flight), `decode-error` (a well-framed
PDU whose body will not parse — the trace needs it), `raw-out` (bytes from sendRaw that do not
decode), and `setNextSequenceNumberForTest` so the 0x7fffffff wrap is testable without 2^31 sends.
Also: every pending request rejects with "connection closed" when the socket drops, so nothing
leaks.
Task 10: complete (commits af446fb..e02aaee, tests: npx vitest run →    Duration  1.22s (transform 445ms, setup 0ms, collect 921ms, tests 1.76s, environment 1ms, prepare 640ms))

Task 11: no rulings. Default cap 10,000 entries. `setCap` lowered trims and counts the trim in
`dropped`, so the UI never silently shrinks the history. Added `size` and `formatTraceLine`
beyond the brief (the file sink and the text export share one line format).
Task 11: complete (commits e02aaee..8698165, tests: npx vitest run →    Duration  1.43s (transform 789ms, setup 0ms, collect 1.66s, tests 1.97s, environment 2ms, prepare 1.31s))

Task 12: Ruling: CSV header detection does not use "does this cell look like an address",
because alphanumeric sender and destination addresses are legitimate and so letters cannot mark a
header. Final rule: naming a column means the file has a header row (whether or not the name is
found); with no column named, row 1 is a header when its cell is a known header word (msisdn,
number, destination, to, ...) or when it holds no digits while a later row's cell does. A file of
purely alphanumeric addresses therefore keeps every row. Three of my tests failed on the first
heuristic and drove this. Cost if wrong: a file whose first data row happens to be a header word
loses one row; the user sees the count before starting.
Task 12: Ruling: `BurstConfig` carries `text`, `dataCoding` and optional `packing` alongside
`template`, which the plan's shape did not. The runner has to segment the body itself to report
`pdus` per row, and segmentation needs the coding and the packing. **Task 19 passes these.**
Cost if wrong: three fields on one interface.
Task 12: Ruling: a row is marked accepted only once every one of its parts came back ESME_ROK,
and the first non-zero status is the row's verdict and is never overwritten — so a 3-part message
whose second part fails reads as failed, which the brief's test pins. A timeout or a lost
connection is recorded under TIMEOUT / CONNECTION_LOST in errorsByStatus and the row's status is
-1; nothing is retried behind the user's back.
Task 12: `currentRatePerSec` is exposed so a test (and the UI) can see that backoff actually
happened; without it "backs off" is unobservable.
Task 12: complete (commits 8698165..8ebb267, tests: npx vitest run →    Duration  7.74s (transform 724ms, setup 0ms, collect 1.60s, tests 9.29s, environment 3ms, prepare 1.10s))

Task 13: Ruling: `Profile` holds CA, client-certificate and client-key **paths**, not contents,
and the files are read at connect time. The plan's shape omitted the cert fields from `tls`
without saying where they go. Paths keep profiles.json small and mean a rotated certificate is
picked up without re-saving the profile. Cost if wrong: a moved file breaks the connection with a
read error, which the user sees.
Task 13: Ruling: secrets are written before the profile record, so a failure to encrypt cannot
leave a profile pointing at a password that was never stored; and saving a profile again without
a password keeps the stored one rather than clearing it (pinned by a test), so editing a host
does not silently drop the credential.
Task 13: a corrupt or unreadable profiles.json starts empty rather than refusing to open the app.
`electronSecretStore()` is the thin `safeStorage` adapter; it is deliberately not unit-tested
(it needs a live Electron app) and holds no logic.
Task 13: complete (commits 8ebb267..4eda2f3, tests: npx vitest run →    Duration  7.67s (transform 554ms, setup 0ms, collect 1.24s, tests 9.16s, environment 2ms, prepare 989ms))

Task 14: DISCIPLINE SLIP, corrected: I wrote src/main/controller.ts before its test, then watched
the test pass first time — which proves nothing. Deleted controller.ts, re-ran the test to watch
it fail, and reimplemented it from the test. Recorded here because the fix is only visible in the
commit history otherwise.
Task 14: Ruling: a real bug my first envelope had. `registerIpc` returned `{...value, _ok:true}`,
which turns an array result into an object with numeric keys — `trace()`, `listProfiles()`,
`burstRows()` and `pduFields()` all return arrays, so every one of them was broken. Wrote the
failing test ("returns an array result as an array"), watched it fail, then changed the envelope
to `{_ok, value}` (api.ts `Result<T>`, ipc.ts, preload all together). Cost if wrong: none; this
is strictly more correct.
Task 14: Ruling: `registerIpc(target, io, opts)` takes an `IpcIo` seam (handle/send) rather than
the brief's `BrowserWindow`, with `electronIpcIo(ipcMain, webContents)` for production. The
brief's own test said "stub BrowserWindow.webContents.send and ipcMain.handle"; a seam is the
clean way to do that and keeps Electron out of the unit tests entirely. The handle also exposes
pushPdu/pushBurstRow/pushState/pushEvent/flush/dispose, which the brief did not name but the
controller needs. Cost if wrong: one adapter function.
Task 14: Ruling: `summarise`/`describe` live in src/smpp/summary.ts (main side), not in the UI as
the plan put them in Task 18. The trace store is in the main process and its entries carry the
summary, so the UI cannot be the only place that can produce one. **Task 18 consumes
src/smpp/summary.ts and does not define summary.js.** Cost if wrong: none.
Task 14: Ruling: added src/main/controller.ts, which the plan never names but Task 14's "deps"
implies — it is the object that owns transport+session+trace+burst+profiles and the thing every
command dispatches to. Tested against the FakeSmsc fixture (17 tests). Without it the UI tasks
have nothing to talk to.
Task 14: Ruling: the preload is bundled with esbuild (new devDependency) instead of being emitted
by tsc. A sandboxed preload cannot `require` a relative module, so `import "../shared/api"` left
`window.smpp` undefined — verified by launching the real app and finding `hasBridge: false`. The
alternative was `sandbox: false`, which would have broken a Global Constraint. After bundling:
24 commands exposed, arrays intact, errors rejecting. Also verified from the page that `process`,
`Buffer`, `module` and `ipcRenderer` are undefined and that no Node module can be required.
Cost if wrong: one build step.
Task 14: complete (commits 4eda2f3..77d46ea, tests: npx vitest run →    Duration  7.93s (transform 1.30s, setup 0ms, collect 2.73s, tests 9.94s, environment 4ms, prepare 1.88s))

Task 15: Ruling: the UI's pure logic lives in TypeScript under src/shared/ui/, bundled by esbuild
into compiled/build/forms.js as the global `SmppForms`, and the qooxdoo boot template loads it
before the app. Reason: vitest cannot import a `qx.Class.define` file (it needs the qx runtime),
so the plan's "keep the mapping functions callable without a DOM" is only achievable if they are
not qooxdoo classes at all. The same functions are therefore unit-tested and shipped.
**Tasks 16-20 add their module to src/shared/ui/index.ts.** Cost if wrong: one build step and one
script tag; the widgets would otherwise be untestable.
Task 15: Ruling: `bind()`, `unbind()`, `enquireLink()`, `setFormValues()`, `getStateLabel()` and
`getCountersText()` are public on ConnectionPanel rather than private. It made the panel drivable
for verification, and `__loadSelectedProfile` now reuses `setFormValues` instead of repeating
every field assignment.
Task 15: fixed a real bug while refactoring: after moving profile loading onto `setFormValues`,
the profileSecrets callback still referenced a now-out-of-scope `f`, so loading a profile's
password would have thrown. The qooxdoo compiler's "unresolved symbol" warnings caught it; the
build is warning-free now.
Task 15: Step 5 verified for real, not by eye: a harness stood up a minimal SMSC, loaded the
compiled main process, called panel.setFormValues + panel.bind(), and read back
stateBefore "Unbound" -> stateAfter "Bound (TRX)", counters "bind_transceiver sent 1 / recv 0,
bind_transceiver_resp sent 0 / recv 1", and the SMSC receiving bind_transceiver with
system_id "esme" and interface_version 0x34.
Task 15: complete (commits 77d46ea..4a15bc0, tests: npx vitest run →    Duration  7.83s (transform 882ms, setup 0ms, collect 2.04s, tests 9.72s, environment 9ms, prepare 1.74s))

Task 16: Ruling: a real bug the plan could not have anticipated. The shared form logic is written
against Node's `Buffer`, which the renderer does not have, so `forms.js` threw "Buffer is not
defined" the moment a widget built a PDU. Wrote tests/ui/forms-bundle.test.ts, which loads the
real bundle in a vm context with no Node globals and calls the functions the widgets call,
watched three of them fail, then fixed it by injecting the `buffer` package into the browser
bundle (`--inject:src/shared/ui/buffer-shim.ts`, new devDependency). The bundle is 93kb instead
of 26kb. Cost if wrong: size only; the alternative was rewriting the codec against Uint8Array.
Task 16: Ruling: the schedule and validity options are functions, not constants, because
"Historic submit time" depends on the moment it is read. `relativeTime`/`absoluteTime` produce the
16-character SMPP forms (trailing `R` relative, `000+` absolute UTC) and are tested directly.
Task 16: Ruling: a multi-part send stops at the first part that fails rather than firing the rest,
and says which part it was. A half-delivered concatenation is worth knowing about; carrying on
would hide it.
Task 16: Step 6 verified: bound to a fake SMSC, loaded the gsm3 preset, picked UDH concatenation,
pressed Send. Three submit_sm arrived with concatenation UDHs sharing reference f1 and sequences
01/02/03, esm_class 64 on all three, three submit_sm_resp back, and the live counter read
"3 parts · 431 chars · 3024 bits · 432 bytes".
Task 16: complete (commits 4a15bc0..560445f, tests: npx vitest run →    Duration  7.84s (transform 1.17s, setup 0ms, collect 2.32s, tests 10.02s, environment 4ms, prepare 1.70s))

Task 17: Ruling: `SendPduRequest` gained `destList`, because its `fields` is
`Record<string, string | number>` and submit_multi's dest_address is a repeating structure that
cannot travel as either. The main process derives `number_of_dests` from it. Added a controller
test that drives a destList through to the wire. Cost if wrong: one field on one interface.
Task 17: Ruling: the broadcast family is one form with a command selector covering broadcast_sm,
query_broadcast_sm and cancel_broadcast_sm — the Task 2 ruling about there being no
`broadcast_multi` in SMPP, carried through to the UI as promised.
Task 17: Ruling: data_sm's body always goes in message_payload whatever the placement selector
says, because the command has no short_message field. The form says so on screen.
Task 17: process fix. The first verification run showed submit_multi arriving with zero
destinations, which looked like a serious IPC bug. It was not: I had changed controller.ts and run
`tsc --noEmit`, so `dist/` was stale and the harness was driving a build without destList support.
Found it by instrumenting the compiled file rather than guessing. From here on every harness run
is preceded by `npm run build` in the same command. Worth flagging to the reviewer: a harness
that can silently test a stale build is a hazard, not a one-off mistake.
Task 17: verified: all four tabs present, and data_sm / query_sm / replace_sm / submit_multi all
reached the SMSC, with submit_multi carrying three addresses plus one distribution list and
number_of_dests 4.
Task 17: complete (commits 560445f..b8bf8ce, tests: npx vitest run →    Duration  7.77s (transform 1.03s, setup 0ms, collect 2.03s, tests 9.71s, environment 4ms, prepare 1.67s))

Task 18: Ruling: `summarise`/`describe` were already built in Task 14 (src/smpp/summary.ts), so
this task added the hex dump, the row rendering and the panel. The trace table uses qooxdoo's
Simple model rather than the plan's remote paging model: the table widget already virtualises its
rendering, and the trace is capped at 10,000 rows in the main process, so paging would add a
round trip per scroll for no gain. Cost if wrong: swapping the model is contained to one class.
Task 18: fixed two real bugs the verification found. (1) The trace row ordinal was smuggled as a
sixth element of each table row; the model keeps only as many columns as it was given, so it was
silently dropped and the hex pane stayed empty for every selection. Replaced with a parallel
`__seqByRow` index. (2) A programmatic `setSelectionInterval` does not raise the table's
changeSelection event, so `selectRow` now refreshes the detail panes itself; a user's click still
goes the same way.
Task 18 verified: 7 trace rows covering both directions, a delivery receipt decoded as
"receipt id=A7F2 DELIVRD", a deliberately malformed PDU shown as "decode error: service_type:
unterminated C-string at offset 0", and selecting the submit_sm row showing its real bytes
(offset/hex/ASCII) and decoded field list.

User request mid-task: theme and layout.
- Theme: asked for "modern", then for "window". qooxdoo has no theme called window, and the ask
  was ambiguous between an MDI window layout and the Windows-looking Classic theme, so I asked.
  The user chose Classic. The four theme parts now extend qx.theme.classic.*.
- Layout pass: the connection column scrolls on its own (with the TLS group open it is taller
  than a short window), its sections are group boxes, and bind state reads as a coloured dot plus
  a label. The status line is a proper bar. The vertical split favours the forms 3:2 with minimum
  heights on both. Form fields are capped at 760px so they stay readable on a wide window, and
  the trace's PDU column takes the spare width instead of the Status column.
- Cleaned up: my verification runs had written test profiles into the real
  ~/Library/Application Support/smppclient; removed. Harnesses should use a scratch userData dir.
Task 18: complete (commits b8bf8ce..e078e45, tests: npx vitest run →    Duration  7.79s (transform 1.22s, setup 0ms, collect 2.26s, tests 9.76s, environment 4ms, prepare 1.73s))

User requests mid-task (beyond the plan): flow diagram, compact form, theme.
- Flow diagram: the user showed a 5gVision "SIP/H323/SMPP flow" ladder view and asked for the
  same behind a button. Built as `buildFlowSvg` in src/shared/ui/flow.ts (19 tests, TDD) so the
  drawing is unit-tested without a DOM, embedded by smppclient.FlowDiagram, opened from a
  "Flow diagram" button in the trace toolbar. Columns: offset from the first packet and from the
  previous one; arrows coloured by family; the far lifeline is labelled with host:port.
  Verified against a live trace: 7 arrows for 7 PDUs, both offset columns, the decode-error row
  drawn in red.
- Bug found and fixed there: FlowDiagram's method was called `show(rows)`, which overrides
  qx.ui.window.Window#show; open() calls show(), so it recursed until the stack gave out
  ("Maximum call stack size exceeded"). Renamed to `display(rows)` with a comment saying why.
- Compact form: the user asked for source/destination inline so the form fits without scrolling.
  Added Widgets.compactGrid/pair/wide/addressCell; submit_sm now puts the TON/NPI selector and
  the address on one line and pairs the remaining fields two per row, roughly halving its height.
- Theme: asked for modern, then window->Classic, then indigo, then modern again. Rather than
  rebuild each time, every theme is now in the bundle (compile.json include) and the status bar
  carries a live theme picker that remembers the choice in localStorage. Default Modern.
  `resolveTheme` falls back to the default for a theme the build does not carry.
- Discipline note: I wrote src/shared/ui/themes.ts before its test. Deleted it, watched the test
  fail, reimplemented. Second time this happened (see Task 14); worth the reviewer's attention as
  a pattern, not a one-off.

Task 19: fixed two real bugs the verification found, neither visible to the unit tests.
(1) The count and rate spinners were bounded by MAX_BURST and MAX_RATE, so asking for a million
messages silently became fifty thousand and the refusal path never fired. Silent clamping is
exactly the "degrades without saying so" the Global Constraints forbid. The spinners now accept
values above the ceilings and `checkBurstLimits` refuses out loud with the limit named, Start
disabled. Verified: "1000000 messages is above the 50000 this build holds in memory."
(2) `achievedRatePerSec` was measured to the end of the run, which includes the wait for delivery
receipts — a 20-message burst at 50/sec with a 1.5s DLR wait reported 10.2/sec. Wrote the failing
test ("measures the achieved rate over the sending, not the wait for receipts"), watched it fail
at 9.0/sec, added `sendingEndedAt` and measured to there. Same burst now reports 45.0/sec against
a requested 50. Cost if wrong: the figure answers "how fast did we send", which is the question
the burst exists to ask.
Task 19 verified end to end: 20 rows against an SMSC throttling one in five — 16 accepted,
4 ESME_RTHROTTLED (matching the SMSC's own count), 16 receipts correlated, 0 unmatched, and the
DLR columns present under a transceiver bind.
Task 19: complete (commits e078e45..9013906, tests: npx vitest run →    Duration  8.87s (transform 1.03s, setup 0ms, collect 2.11s, tests 10.84s, environment 6ms, prepare 1.83s))

Task 20: Ruling: `previewPdu` reports `sendable` independently of `ok`. A PDU that will not decode
is exactly what a tester often wants to send, so the preview is informational and the Send button
is gated only on there being bytes at all. Added a "Tidy" button that re-spaces the hex without
changing a byte.
Task 20 verified — the spec's central anti-goal, end to end and at the socket. Took a real
submit_sm out of the trace with "Edit as hex", replaced its command_length with deadbeef, and
captured the raw TCP bytes at the far end: the preview read "will not decode: command_length
3735928559 disagrees with the 62 byte(s) present", Send stayed enabled, and the byte sequence
arrived at the SMSC exactly as composed. Nothing in the path repaired it.
Task 20: complete (commits 9013906..3aab6a8, tests: npx vitest run →    Duration  239.84s (transform 1.11s, setup 0ms, collect 2.03s, tests 241.83s, environment 5ms, prepare 1.64s))

Investigation: suite "slowed from 9s to 850s with an intermittent failure".
Followed the implementation workflow rather than guessing.
- Evidence: the failure was always in tests/main/burst.test.ts ("keeps every row for 1000
  messages", "emits progress while it runs"), taking 75-180s against a 30s timeout; the file
  alone ran in 9s. A bisect suggested any predecessor file made it worse (session+burst: 1045s).
- Instrumented instead of theorising: active handles and real timer latency inside the burst
  file after the session file ran. Timers were healthy (20ms nominal -> 21ms actual), and a
  40-message burst measured 420ms at 95/sec. So neither the event loop nor the engine was at
  fault, which killed the leak hypothesis.
- Root cause: my own overlapping background vitest runs. Each run that outlived the tool's
  600s timeout kept going, and I launched more on top; the CPU contention stretched the burst
  tests, which pace on wall-clock timers, past their limits. Not a product bug and not a test
  bug — an artefact of how I was running them.
- Verified: with nothing else running, three consecutive full runs gave 462 passed in 9.3s each.
- Standing finding for the reviewer: the burst tests are wall-clock-paced with a 30s cap, so they
  are load-sensitive by construction and could flake on a busy CI machine. I did not raise the
  timeout, because that would mask the sensitivity rather than address it; noted as a known
  characteristic instead.

Task 21: Ruling: `npm run dist` is `npm run build && electron-builder`, not electron-builder
alone as the brief implied. Packaging a stale dist/ is exactly the trap that cost time in Task 17,
and the packaging test pins that the script builds first.
Task 21: the `files` list excludes tests/, src/, docs/ and source maps explicitly. Verified
against the produced asar: dist/main/index.js, dist/preload/index.js, compiled/build/index.html,
compiled/build/forms.js and package.json are all present, and zero files matching tests/, src/,
*.key or *.map leaked — which matters because tests/fixtures/tls holds private keys in the clear.
Task 21: Step 5 verified on the PACKAGED app, not the dev build. Launched
"release/mac-arm64/SMPP Client.app" with a remote debugging port, drove its renderer over CDP:
the UI rendered, window.smpp was present (so the bundled preload survives packaging), SmppForms
was present (so forms.js loads from inside the asar), all six tabs were there, and a bind against
a real socket reached "Bound (TRX)" with the SMSC receiving bind_transceiver from system_id
"packaged".
Task 21: complete (commits 3aab6a8..454b11e, tests: npx vitest run →    Duration  8.91s (transform 1.08s, setup 0ms, collect 2.28s, tests 10.92s, environment 5ms, prepare 1.99s))

## Final review and fix pass

Reviewer: fresh context, Opus, whole-branch diff (22 commits, 115 files), given the spec, the
plan, the Review Focus list and this ledger. Verdict: "Ready to merge? No — not until C1."
Re-graded by effect; all three Criticals stood, and all six Importants were real.

Fixed, each with a test watched failing first:
- C1 session.ts: `acquireWindowSlot` parked bare resolve callbacks and `onClose` discarded them,
  so every send queued behind a full window never settled and a burst hung forever —
  `Promise.all(inflight)` never resolved, no `done` event, no IPC reply, UI stuck on "Burst
  started". Waiters now carry a reject handle and are drained first, with a `closing` flag so
  releasing a slot cannot wake a send onto a dead socket. Confirmed red: the new test timed out
  at 20s without the fix.
- C2 segment.ts: `chunk` used the concatenation limits (153/67/134) where `measure` used the
  single-segment ones (160/70/140), so a plain 160-septet message — the textbook single SMS —
  was split into two UDH parts while the counter said one. Now both use the single-segment limit
  first, with a test asserting `measure().parts === segment().length` at every boundary.
- C3 frame.ts + transport.ts: a malformed PDU discarded every good PDU framed from the same TCP
  read. `PduDecodeError` now carries `framed`, and the transport emits those before reporting the
  error. This was the "never lose traffic" anti-goal failing in exactly the case the tool exists
  for.
- I1 burst.ts: DLR latency was measured from the burst start, so a row submitted 90s in reported
  ~90000ms. Now stamped per row from its own first send.
- I2 ipc.ts + controller.ts: counters were pushed one IPC message per PDU, bypassing the batcher
  and contradicting the spec's architecture note. `pushCounters` now batches, latest snapshot wins.
- I3 burst.ts: `resolveRecipients` warnings were dropped on the floor. They now ride the `done`
  event and the burst panel shows them.
- I4 frame.ts + controller.ts + TracePanel: a `command_length` larger than the data that ever
  arrives used to stall silently. `PduFramer.pending` reports bytes held and the length claimed;
  `status()` carries it and the trace shows "N of L byte(s) held".
- I5 submit.ts: a body over 254 octets in `short_message` wrapped `sm_length` with no warning
  (300 bytes declared as 44). Now warned, still sent.
- I6 submit.ts: the counter ignored a hand-written UDH, reading "3 parts" while one PDU went out.

Found while fixing, not in the review: `perTick = Math.max(1, ...)` meant the pacer could not go
below one message per 20ms tick, so a request for 5/sec silently went out at 50/sec. Below one per
tick it now sends one message and waits the whole gap. Test pinned.

Minors: deferred, listed to the user, not fixed.

Two user-reported bugs fixed in the same pass:
- Clicking a trace row showed no bytes. The listener was on the table; a click is announced by the
  table's *selection model*, so every mouse selection left the hex and field panes empty. My own
  verification had only ever selected rows programmatically, which is a different path — that is
  why it was missed. Verified afterwards with a real `sendInputEvent` mouse click.
- Picking a "3 parts" sample left the segmentation on "one PDU", so the body overflowed sm_length
  and the SMSC rejected it. Presets now carry the segmentation they imply (`SAMPLE_PRESETS`),
  verified on the wire: 3 PDUs with sm_length 159/159/132 and esm_class 64.
- UI: Bind and the Send buttons are now primary actions — accent colour, bold, Send alone and
  right-aligned. The theme's button decorator paints over a background colour, so it is dropped
  on those buttons; the colour is set explicitly because the theme is swappable at runtime.
