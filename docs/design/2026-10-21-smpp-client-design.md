# SMPP Client — Design

Date: 2026-10-21

## Intent and why

A desktop SMPP client for testing SMSCs and SMS aggregators. The reference
is the Melrose Labs browser-based SMPP Client Proxy
(<https://melroselabs.com/tools/smppclientproxy/>): the app must cover
everything that tool does, and then go beyond it in two ways the browser
cannot.

The browser tool speaks WebSocket to a Melrose-operated proxy, because a
browser cannot open a TCP socket. Two consequences the user wants gone:

1. **Reachability.** The proxy must be able to reach the SMSC. Private
   networks, VPN-only hosts, lab SMSCs and internal CA certificates are out
   of reach. An Electron app opens the TCP/TLS connection from the main
   process, so anything the user's own machine can reach works.
2. **Byte fidelity.** A test tool for aggregators needs to show the exact
   bytes on the wire and to send arbitrary bytes — unknown TLVs, raw UDH,
   deliberately odd field values. A hosted proxy mediates that; a local
   socket does not.

The primary job is interop testing: "does this aggregator accept what I
send, what does it answer, and do the delivery receipts come back?"

## Goals

- Full feature parity with the Melrose SMPP Client Proxy (enumerated below).
- Direct TCP and TLS connections from the user's machine, no intermediary.
- A faithful PDU trace: decoded view plus the original hex bytes, both
  directions, every PDU.
- Burst sending with a per-message result table, aggregate statistics and
  CSV export, so the user can push throughput at an aggregator and see how
  it holds up and whether every DLR comes back.
- Saved connection profiles, with passwords encrypted at rest.
- A raw hex PDU composer, so the user can send byte sequences no form could
  produce — wrong `command_length`, truncated PDUs, unknown `command_id`,
  trailing garbage.
- TLS configuration deep enough for private and lab SMSCs: custom CA,
  self-signed acceptance, client certificates, explicit SNI.

## Non-goals

- Acting as an SMSC (server-side SMPP, ESME simulation for someone else to
  bind to). Client side only.
- Acting as a proxy for other clients.
- Being a production SMS-sending platform: no campaign management, no
  contact lists, no scheduling of real traffic, no billing.
- Any other protocol (SMPP only — no HTTP SMS APIs, no SS7, no CIMD/UCP).
- Mobile or web deployment.
- Distribution to other people. This is the user's own tool. No macOS code
  signing or notarization, no Windows signing, no auto-update, no crash
  reporting, no three-platform CI. `electron-builder` is run by hand when a
  build is wanted.
- Deep links and CLI parameter presets (the reference tool's query-string
  feature). Saved profiles cover the need; revisit after v1.

## Anti-goals — what would make this a failure even if it works

- **Hiding the bytes.** If the user cannot see the exact octets sent and
  received, or cannot send an octet sequence of their choosing, the tool has
  failed at its purpose, however pretty it is.
- **Refusing to send something "invalid."** The app must let the user send
  field values and TLVs that the app itself considers wrong. Validation
  warns; it never blocks. Testing an SMSC means testing how it reacts to bad
  input.
- **Losing traffic under load.** During a burst the trace and the result
  table must stay correct. Dropping or mis-correlating messages to keep the
  UI responsive is worse than a slow UI. If the trace has to be capped, the
  cap is explicit and visible, and the burst result table is never capped.
- **A silent reconnect or re-bind.** The connection state shown must be the
  real one. Never paper over a dropped bind.

## Constraints

- **Electron** for the shell. (User's choice.)
- **qooxdoo** for the UI. (User's choice.) Stable release is 7.9.3; 8.0 is
  still beta, so 7.9.3 it is.
- **SMPP versions** 3.3, 3.4 and 5.0, plus a "default (0x00)" interface
  version for binds.
- Primary platform macOS; builds for Windows and Linux as well.
- UI language: English. Every label in this domain is a spec term
  (`esm_class`, `registered_delivery`), and mixing languages around them
  reads badly. Proposed and accepted.
- Sizing target: up to 50,000 messages per burst at up to 500 messages/sec.
  Results may be held in memory at that scale. Proposed and accepted.
  Beyond that ceiling the result store would have to move to disk, so the
  burst UI should degrade honestly (refuse or warn) rather than silently
  struggle if asked for much more.

## Decisions the user made

### Window layout — "faithful to the web tool"

Shown three wireframes, the user picked the one closest to the Melrose
layout:

- **Left column:** connection panel (host, port, TLS, credentials, version,
  bind mode, status indicator) with the per-PDU sent/response counters
  below it.
- **Centre:** the PDU forms, as tabs — one tab per command.
- **Bottom, full width:** the PDU trace table, with a hex view of the
  selected PDU.

One session per window.

### Burst sending

The user confirmed they need to "spray" traffic: N messages at a given
rate. Confirmed contents of the burst result view:

- Per-message row: recipient, the text/body sent, the `message_id` returned
  in `submit_sm_resp`, the correlated delivery receipt with its final
  state, and latency.
- Aggregates: submitted, accepted, errors broken down by SMPP command
  status, actual achieved throughput, latency percentiles.
- CSV export.

### Burst recipient sources — all three

The user asked for all of: a CSV file they load, a generated number range,
and the same number repeated N times.

### Protocol implementation — own codec

Discussed the npm `smpp` package (at `0.6.0-rc.4`) versus writing the PDU
codec in-house. The user chose in-house: encode/decode in TypeScript in the
main process, all commands, v3.3/3.4/5.0, arbitrary and unknown TLVs, raw
UDH, and the original buffer retained for the trace. The reason recorded in
the conversation: a library that abstracts the bytes fights the anti-goals
above.

### Raw hex PDU composer — in scope

Forms cannot produce a wrong `command_length`, a truncated PDU, a bogus
`command_id` or trailing garbage, which is exactly the malformed input an
SMSC test tool exists to send. So there is a hex compose-and-send mode, and
a "take this PDU from the trace and edit it as hex" path from any traced
PDU. The codec therefore needs a send-raw-bytes entry point alongside
encode-from-model, and must never silently repair what it is handed.

### Burst behaviour on throttling and on connection loss

On `ESME_RTHROTTLED` the default is to **record the error and hold the
requested rate** — the point is to measure where the SMSC actually breaks.
Automatic backoff is available as a checkbox, not the default.

If the bind drops mid-burst the burst **stops and reports**. No silent
reconnect, no silent re-bind (see anti-goals).

### TLS configuration

Beyond on/off: custom CA bundle, accept self-signed, client certificate for
mutual TLS with its key passphrase in `safeStorage`, and explicit SNI.
Private and lab SMSCs are a founding reason for the desktop app, so
"TLS on/off" would undercut the intent.

### Session model

One bind per window. DLR-backed bursts are therefore **transceiver only**
in v1. A TX+RX bind pair in one window (two sockets, two sequence spaces,
correlation across them) was offered and deferred.

### Storage

- Connection profiles in a JSON file under Electron's `userData`.
- Passwords encrypted with Electron's `safeStorage` (system keychain),
  never in plaintext on disk and never in a query string.
- Trace held in memory with a configurable cap; export to CSV/JSON/text;
  optional append-to-file for long sessions.

## Feature inventory — parity with the reference tool

Everything in this section was read off the Melrose tool and is in scope.

### Connection and bind

| Field | Values |
|---|---|
| Host, Port | free |
| TLS | on/off |
| `system_id`, `password`, `system_type` | free (`system_type` ≤ 5 chars in the reference) |
| `interface_version` | default `0x00`, v3.3 `0x33`, v3.4 `0x34`, v5.0 `0x50` |
| Bind mode | transceiver, transmitter, receiver |

Plus `unbind`, `enquire_link`, and a bind-state indicator that reads
"Unbound" when not bound.

The reference tool accepts URL query-string presets (`host`, `port`,
`systemtype`, `systemid`, `password`, `submitsm.*`). The desktop equivalent
is saved profiles; see "Left to the builder" for whether a deep link is
also wanted.

### PDUs

Outbound: `submit_sm`, `data_sm`, `query_sm`, `cancel_sm`, `replace_sm`,
`submit_multi`, `broadcast_sm`, `broadcast_multi`, `enquire_link`,
`bind_transmitter`, `bind_receiver`, `bind_transceiver`, `unbind`.

Inbound: the `_resp` for each of the above, plus `deliver_sm` (answered
automatically with `deliver_sm_resp`), `enquire_link` from the peer,
`unbind` from the peer, and `generic_nack`.

### `submit_sm` fields

- **Source and destination address** with TON/NPI selectors: Auto, Intl,
  Nat, Alpha, E164. (The reference requires at least 8 characters for an
  address; see "Left to the builder".)
- **`service_type`** — up to 5 characters.
- **`esm_class`** — Default, Datagram, Forward/Transaction, Store and
  Forward, each with and without UDHI; i.e. values 0–3 and 64–67.
- **`protocol_id`**.
- **`priority_flag`** — 0–4.
- **`registered_delivery`** — 0–3 and 16–19 (receipt and intermediate
  notification combinations).
- **`schedule_delivery_time`** — immediate, +1/+2/+5/+15 minutes, and a
  historic submit time.
- **`validity_period`** — default, 1 min, 30 min, 1 h, 4 h, 1 day.
- **`data_coding`** — 0–8: GSM default, IA5/ASCII, octet (8-bit), Latin-1,
  JIS, Cyrillic, Latin/Hebrew, UCS2.
- **Message body.** Note the reference sends the text as the
  `message_payload` TLV rather than `short_message`. The app must be able
  to do **either**, selectable, because which one an aggregator accepts is
  exactly the kind of thing being tested.
- **TLVs** — arbitrary tag/length/value entries, including tags the app
  does not know.
- **UDH** — raw information-element bytes.
- **Live size counter** — SMS parts, characters used, bits used, total
  message size, for the selected `data_coding`.
- **Sample content presets** — GSM 1-part, UCS2 1-part, GSM 3-part, UCS2
  3-part, and Clear.
- A "show all" style toggle between a short form and the full field set.

### Message-referencing commands

`query_sm`, `cancel_sm`, `replace_sm` take a `message_id` of up to 64
characters, plus source address and `service_type`.

### Counters and trace

- Per-PDU sent/response counters for bind, submit, query, cancel, replace,
  `data_sm` and `enquire_link`.
- Trace table columns: time, sequence number, direction, decoded PDU.
- Hex view of the selected PDU.
- View controls: jump to top, compact, expand, clear.

## Architecture

Two processes, one boundary.

**Main process (Node, TypeScript)** owns everything that touches the
network and the disk:

- the TCP/TLS socket and its lifecycle;
- the PDU codec (encode, decode, and the byte buffer of each PDU);
- sequence-number allocation, request/response matching, window management;
- the burst engine and its rate pacing;
- DLR correlation;
- profile storage and `safeStorage`.

**Renderer (qooxdoo)** is presentation and input only. It never parses or
builds PDUs; it sends intents ("bind with this profile", "submit this form")
and receives events (PDU in, PDU out, state change, counter update, burst
progress).

`contextIsolation` on, `nodeIntegration` off, a preload script exposing a
narrow typed IPC surface. The qooxdoo bundle is built by the `qx` CLI and
loaded into the renderer.

Events from main to renderer should be batched for the renderer's sake
during a burst — the renderer must not get one IPC message per PDU at
high rates. The anti-goal stands: batching may delay the trace, never lose
or reorder it.

## Left to the builder

- Project layout, build pipeline, how the `qx` compile is wired into the
  Electron packaging, and the electron-builder configuration.
- The exact IPC message schema.
- Internal structure of the codec (how PDU definitions are declared, how
  mandatory vs optional parameters are modelled).
- How the trace is stored in memory and capped, and how the renderer's
  table virtualises large traces.
- How the burst engine paces (token bucket, interval timer, something else)
  and how the SMPP window size interacts with the requested rate.
- Test strategy. A loopback SMPP server used only by the tests is the
  obvious way to test the client end-to-end; building one is a test
  fixture, not a product feature, and does not conflict with the non-goal
  above.
- Whether the "short form / show all" split reproduces the reference's
  exact field grouping or is reorganised; the field set is fixed, the
  grouping is not.
- Error presentation: how a non-zero `command_status` surfaces beyond the
  trace.
- Visual design within the chosen layout.

## My calls

Decided by me, not by the user. Worth a second look while reading.

- **DLR correlation** is configurable per profile, because aggregators get
  this wrong constantly. The chain: prefer the `receipted_message_id` TLV;
  otherwise parse the `id:` field out of the receipt text; normalise
  hex↔decimal; strip leading zeros. Delivery receipts that still match
  nothing go into their own visible "unmatched" bucket rather than being
  dropped or guessed at.
- **Segmentation** is done by the app, with the method selectable: UDH
  concatenation, `sar_*` TLVs, or none (send one PDU exactly as composed).
  A user-supplied raw UDH switches automatic concatenation off — the user's
  bytes win. Burst aggregates count messages and PDUs as separate figures,
  so a 3-part message that fails on part 2 is legible.
- **Address length** is validated as a warning, never a block, per the
  anti-goal. Same principle for every other field validation.
- **Parity** is judged against the feature inventory in this document, not
  against the live Melrose tool. The inventory is authoritative; nothing the
  reading of that page may have missed is in scope.

## Open questions

None. Everything above was either decided by the user or is marked as my
call.
