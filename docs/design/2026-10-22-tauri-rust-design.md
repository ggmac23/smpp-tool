# SMPP Client on Tauri and Rust — design delta

Date: 2026-10-22

This amends `2026-10-21-smpp-client-design.md`. **Everything in that document
still holds** — the intent, the goals, the non-goals and above all the four
anti-goals. What changes is only how it is built.

Read that spec first. This one records what moves, what stays, and the calls
made along the way.

## Why

The Electron build works and is verified to the socket, so this is not a
rescue. The reasons to move:

- The packaged app is **242 MB**. A Tauri build of the same application is
  roughly 10-20 MB, because it uses the operating system's own webview instead
  of shipping a copy of Chromium.
- Lower memory and a faster start, for a tool that is opened to answer one
  question and closed again.
- Byte-level protocol work is what Rust is for. The codec is the heart of this
  application and it spends its life slicing buffers and bounds-checking
  lengths.

The cost, stated plainly: the half of the application being rewritten is the
half that is currently proven. The nine defects found during the Electron build
(see `decisions-2026-10-21-smpp-client.md`) have to be re-prevented, not
inherited. The test suite is the mechanism for that — see "Porting the tests".

## What moves to Rust

Everything that touches the network, the bytes or the disk:

| TypeScript today | Rust module |
|---|---|
| `src/smpp/constants.ts` | `smpp::constants` |
| `src/smpp/tlv.ts` | `smpp::tlv` |
| `src/smpp/pdu-defs.ts` | `smpp::defs` |
| `src/smpp/decode.ts` | `smpp::decode` |
| `src/smpp/encode.ts` | `smpp::encode` |
| `src/smpp/frame.ts` | `smpp::frame` |
| `src/smpp/text.ts` | `smpp::text` |
| `src/smpp/segment.ts` | `smpp::segment` |
| `src/smpp/receipt.ts` | `smpp::receipt` |
| `src/smpp/summary.ts` | `smpp::summary` |
| `src/main/transport.ts` | `app::transport` |
| `src/main/session.ts` | `app::session` |
| `src/main/dlr.ts` | `app::dlr` |
| `src/main/burst.ts` | `app::burst` |
| `src/main/recipients.ts` | `app::recipients` |
| `src/main/trace.ts` | `app::trace` |
| `src/main/export.ts` | `app::export` |
| `src/main/profiles.ts` | `app::profiles` |
| `src/main/controller.ts` | `app::controller` |
| `tests/fixtures/smsc-server.ts` | `tests::fake_smsc` |

Roughly 3,700 lines of TypeScript and 3,900 lines of its tests.

## What stays

The whole qooxdoo renderer — `src/ui/` — and the presentation logic in
`src/shared/ui/`, about 4,000 lines plus 1,400 lines of tests. It is a web
application; Tauri serves it to a webview exactly as Electron did.

Three things in the renderer change:

1. **The bridge.** `smppclient.Api` currently calls `window.smpp.<command>()`
   and `window.smpp.on(event)`. It will call Tauri's `invoke(command, args)`
   and `listen(event)`. Contained to `Api.js` and the preload, which goes away.
2. **No more Buffer in the renderer.** See the next section.
3. **Type declarations.** `src/shared/ui/*` imports types from the main-process
   modules today. Those types will live in Rust, so the TypeScript side gets
   its own hand-written declarations in `src/shared/api.ts`, generated from
   nothing and kept honest by the tests that use them.

## The renderer stops building PDUs

A correction, not a port. The Electron build drifted from its own spec: the
spec says "the renderer never parses or builds PDUs; it sends intents", but
`src/shared/ui/submit.ts` segments the message, encodes GSM 03.38 and hands
main a list of hex strings. That is why the renderer needs a `Buffer`
polyfill at all.

In the Tauri build the renderer sends what the user chose — the text, the
`data_coding`, the segmentation mode, the TLV rows as typed — and Rust
composes the PDUs. Consequences:

- `segment.ts` and `text.ts` have no renderer counterpart; the live size
  counter asks Rust through a `measure` command, as it already does for the
  burst path.
- The `buffer` dependency and the esbuild `--inject` shim go.
- `previewPdu` in the hex composer becomes a `preview_pdu` command.
- `src/shared/ui/` keeps only what it should always have been: option lists,
  label-to-value mapping, and formatting (`stats`, `hexdump`, `flow`,
  `themes`, `connection`, `fields`).

## Constraints

Everything in the original spec's Global Constraints still applies, plus:

- **Tauri v2** and the **stable** Rust toolchain.
- The renderer is still built by the `qx` compiler to `compiled/build`, which
  Tauri serves as its frontend dist. No bundler is introduced for the qooxdoo
  side.
- **macOS uses WKWebView, not Chromium.** qooxdoo 7 supports Safari, but this
  is an assumption that must be tested before the port is committed to, not
  after. See the plan's first task.
- Async in Rust via **tokio**, because the session needs timers, sockets and
  concurrent request windows.
- Serialisation across the bridge via **serde**. Bytes cross as lowercase hex
  strings, exactly as they do today, so the renderer never handles binary.
- Errors never cross as panics. A command returns `Result<T, String>` and the
  bridge surfaces the message, matching today's `{_ok, value}` envelope
  behaviour.

## Porting the tests

The tests are the only thing that stops this being a fresh set of the same
nine bugs. Every backend test in the TypeScript suite is ported to a Rust
`#[test]` or `#[tokio::test]` **before or alongside** the code it covers, and
the ones that encode a defect we already fixed are ported first. Named
explicitly, because these are the ones that cost real time to find:

- a send queued behind a full window is rejected when the connection drops
- `measure().parts` equals `segment().len()` at every boundary
- the PDUs framed before a malformed one in the same read are not lost
- a rate below one message per pacing tick is honoured
- DLR latency is measured from the row's own submit
- `sm_length` wrapping is warned about
- a hand-written UDH turns concatenation off, and the counter agrees
- `command_length` that lies is rejected without allocating
- an orphan response and an unknown `command_id` leave the session bound

## Non-goals for this work

- No behaviour changes. If the Tauri build does something the Electron build
  did not, that is a defect unless it is listed here.
- No new features. The flow diagram, theme picker and compact forms added
  during the Electron build are carried across as they are.
- Electron is not kept alongside. Once the Tauri build passes the same
  verification, the Electron main process and its packaging go.
- Cross-compilation and signing remain out of scope, as in the original spec.

## Left to the builder

- Crate layout (one crate or a workspace), module boundaries, error types.
- Whether to use a Rust SMPP crate: the survey found none suitable —
  [`smpp`](https://docs.rs/crate/smpp/latest) is early-stage and roughly three
  years stale, [`rusmpp`](https://docs.rs/rusmpp) is a low-level v5 library —
  and the original spec's reasoning against a library that abstracts the bytes
  applies with equal force here. The expectation is a hand-written codec, as
  in the TypeScript build.
- How the burst engine paces in tokio, and how backpressure maps onto the
  session window.
- How trace events are batched to the webview (the same "batching may delay,
  never drop or reorder" rule applies).
- Test fixture design for the fake SMSC.
