# Implementation notes

Clean-room implementation of `g435-host-protocol-v3.2`.

## Files created

- `src/lib.rs`: crate root, `#![forbid(unsafe_code)]`, public re-exports.
- `src/error.rs`: `Error` and `Result`.
- `src/protocol.rs`: HID report encoding, PA request encoding, PI reply
  parsing, OTA reply predicate, receive assembler, feature-fragment
  extraction, storage chunk plan.
- `src/transport.rs`: `Transport` trait and hidapi-backed `HidTransport`.
- `src/device.rs`: `G435`, mode/status/RSSI/image types, bootloader bridge
  scoping (all PA commands), remote-mode retry, storage-read state machine.
- `src/cli.rs`: clap definition, numeric/MAC parsers, and pure human/JSON
  formatting helpers.
- `src/bin/g435ctl.rs`: thin CLI dispatch using the formatting helpers.
- `tests/protocol.rs`: encoding, parsing, malformed frames, length bounds,
  resynchronization, buffer bounds, chunk planning, category-`0x01` OTA reply
  and predicate coverage.
- `tests/device.rs`: mock transport: bridge-wrapped link commands,
  single-session remote-mode + OTA enforcement, chunk assembly, MORE_FRAME,
  non-matching reply sequences, empty-ACK OTA reads, buffered-frame retention,
  category-`0x01` OTA replies, ignored `01` bridge-control fragments, stale
  ACK/event filtering, exit ACK handling/timeout, error-path bridge exit,
  timeouts.
- `tests/cli.rs`: CLI parsing, numeric and MAC validation, conflicts.
- `tests/cli_output.rs`: pure human/JSON formatting and SHA-256 text.
- `.github/workflows/ci.yml`: format, clippy, and test on stable Rust.
- `IMPLEMENTATION-NOTES.md`: this file.

No existing source files were modified; `Cargo.toml`, `Cargo.lock`, `SPEC.md`,
`LICENSE`, `.gitignore`, and `AGENT-BRIEF.md` are untouched.

## Commands run

Build artifacts under `/tmp` hit the container's per-directory disk quota
(`Disk quota exceeded`, then a `rustc` SIGSEGV from the same condition). The
build directory was therefore redirected outside the work area; source
location and dependencies are unchanged. `CARGO_PROFILE_DEV_DEBUG=0` keeps the
cache small.

| Command | Exit |
| --- | ---: |
| `cargo fmt --check` | 0 |
| `CARGO_TARGET_DIR=~/.cache/g435-cleanroom-target CARGO_PROFILE_DEV_DEBUG=0 RUSTFLAGS="-D warnings" cargo clippy --offline --all-targets` | 0 |
| `CARGO_TARGET_DIR=~/.cache/g435-cleanroom-target CARGO_PROFILE_DEV_DEBUG=0 cargo test --offline` | 0 |

Test totals: 77 passing (12 CLI parsing, 8 CLI output, 39 device, 18
protocol); 0 failed.

## Design decisions and spec ambiguities

- **Bridge scope.** The bridge is entered for every PA command: OTA reads
  (battery, RSSI, GRS, image version, storage) and link/radio commands
  (status, mode, scan, pair). Link command enter/exit is scoped per call,
  including the calls made by `ensure_remote`.
- **Reply correlation (v2.1).** OTA waits accept a PI frame whose category is
  `0x01` or `0x06` and whose payload starts with `0x06`; link waits accept only
  category `0x0e`. The request sequence is still emitted, but replies are not
  matched by sequence byte. Input reports are drained before each request. The
  persistent receive buffer is retained across `MORE_FRAME` and across frames
  of one exchange; it is cleared only when the bridge is entered for a fresh
  top-level operation.
- **Bridge control fragments.** A feature fragment starting with `0x01` is
  not parsed as OTA data. If the same fragment also contains a `50 49` PI
  frame after the control prefix, only the PI suffix is assembled; the
  control prefix is discarded. This mixed layout caused intermittent 512-byte
  read timeouts until the suffix was recovered.
- **One bridge session per OTA operation.** `ensure_remote` (GET_MODE /
  SET_MODE) and the OTA query run inside a single bridge enter/exit scope.
  Splitting them into separate sessions caused the receiver to answer only
  with a `01` bridge-control frame and never emit the OTA PI data frame.
- **Exit acknowledgement.** `exit_bridge` waits for the bootloader ACK
  (input or feature) bounded by the acknowledgement timeout, then drains
  stale input. On timeout it returns an error and keeps the maybe-open state
  so `Drop` retries the exit write.
- **`CREATE_CONNECT_EX` profile byte.** The spec shows `<profile>` but does not
  give its value; `0x00` is used. The BD_ADDR is sent least-significant byte
  first as specified.
- **Link setters (v3.1).** `BT_INQUIRY_SCAN` and `CREATE_CONNECT_EX` are
  fire-and-forget: write the request, drain, return. `SET_MODE` is written and
  then confirmed by reading `GET_MODE` in the same bridge session, up to five
  confirmation retries with 100 ms between reads. `ensure_remote_locked`
  continues to read `GET_MODE` before and after `SET_MODE`. No setter waits
  for a setter event.
- **Bounded waits.** Reply waits use 2 s and bootloader ACK waits use 1 s, with
  the existing frame and retry limits.
- **`READ_MAX_LEN` acknowledgement.** After sending it, the driver waits up
  to 250 ms for one feature-report frame (PI or bridge control) and discards
  it. Input ACKs (`06 5a`) do not end this wait. The feature ACK carries
  `f0 00 00 00`; it must never be appended to storage data. If no feature
  frame arrives, the wait returns and the read continues. The first
  `STORAGE_READ` is not sent until this wait completes.
- **Reply data aggregation.** Battery, RSSI, GRS, and image-version reads keep
  reading category `0x06` frames until one has non-empty data and status `0`,
  bounded by the frame limit. They never send `MORE_FRAME`. Storage reads are
  the only path that sends `MORE_FRAME`, and they retain buffered frames
  between requests.
- **OTA reply category (v2.1).** `PaFrame::ota_status` no longer requires the
  frame category to be `0x06`; it requires at least three payload bytes and
  `payload[0] == 0x06`. Storage reads use the same OTA predicate, including
  category `0x01` replies.
- **GET_IMAGE_VERSION signature (v2.4).** The request payload is
  `00 34 00 4d 43` (opcode `0`, signature `MC`), length byte 4. The previous
  `00 00` signature returned device status `0xff`.
- **1 MiB storage bound.** `protocol::MAX_STORAGE_READ` is 1 MiB. Both
  `G435::read_storage` and `protocol::storage_chunks` reject a length above
  the bound, or an `addr + len` above it, using checked arithmetic, before any
  device access. A full 1 MiB plan is accepted but was not executed.
- **`01` bridge-control frames are ignored.** They are opaque and are not a
  data source, per `SPEC.md` section 7; an embedded PI frame is still parsed.
- **CLI JSON for `read --output`.** The spec marks JSON as n/a for this form;
  the human "wrote N bytes ..." line is printed even when `--json` is given.
  `info` JSON keys are `image_version`, `grs_status`, and `battery`.
- **Frame ceiling.** A 4096-byte declared PI payload ceiling and a 16 KiB
  receive-buffer ceiling are added as hardening. The bridge only produces
  small frames; oversized declarations are treated as malformed and
  resynchronized.
- **Zero-length storage read.** Alignment is still validated; `len == 0` then
  returns an empty vector without device access.
- **v3.0 typed safe commands.** `fs-table`, `fs-index`, `cust-info`,
  `mp-read`, `nvds-read`, `atb`, and `echo` use the OTA payloads from section
  9. Replies are returned as raw hex with decoded fields where specified.
- **Typed link queries (v3.2).** `lpc`, `quality`, `devm`, and `afh-map` use
  link payloads `04`, `03`, `05`, and `02`. Replies are parsed as specified:
  LPC count + 8 PLC bytes; quality handle/count/RSSI/quality; DEVM
  handle/count + 80 bytes; AFH handle + 10 channel bytes. Short replies are
  protocol errors. These queries return the link timeout if the receiver emits
  no event; data is never synthesized.
- **Raw tunnel.** `raw-ota`, `raw-link`, and `raw-pa` exist behind
  `--unsafe --confirm I-UNDERSTAND`; absent or wrong confirmation is rejected
  by clap before device access. Raw commands emit a stderr warning and do not
  retry writes. The typed path remains read-only; the raw tunnel is the only
  destructive path. No raw command was run live.
- **OTA write/erase/reboot/handover commands** are not implemented as typed
  commands.

## Live device results

Receiver present: `046d:0acb` (Logitech G435 Wireless Gaming Headset).
Short checks only; no storage read or dump was performed.

Live checks after the v3.2 changes (SPEC v3.2 SHA-256 verified):
`bc4eb4caef60f0b36c6737eeabf14f5a93a2a45942351ad1cee2906a656e4071`.

| Command | Exact output | Exit |
| --- | --- | ---: |
| `lpc` | `error: timeout: waiting for category 0x0e reply` | 1 |
| `quality` | `error: timeout: waiting for category 0x0e reply` | 1 |
| `devm` | `error: timeout: waiting for category 0x0e reply` | 1 |
| `afh-map` | `error: timeout: waiting for category 0x0e reply` | 1 |
| `status` | `link status: 0x01 (bits 0b00000001)` | 0 |
| `battery` | `4146 mV` | 0 |
| `fs-table` | `filesystem table (140 bytes)` plus 140-byte hex | 0 |
| `mp-read 1` | `mp-read[1]: 6 bytes`, `a45a14995840` | 0 |
| `read 0 48` | `sha256: 0c6b048bd814d2f6ecd25f365562257dd453afb9b40591f5af42f6fe9b9553f3` | 0 |

The four typed link queries time out on this receiver, as anticipated by
v3.2: it emits no matching link event. No data was synthesized. No raw command
or destructive command was run. No 4 KiB or larger storage read and no dump was
performed. The mode was left remote.

## Forbidden input

No file from `/home/eclipse/Documents/GitHub/open-g435-driver`, `/tmp/g435`,
any vendor SDK, firmware image, decompiled output, or capture was read,
copied, searched, or inspected. Inputs used were `SPEC.md`, `Cargo.toml`,
`Cargo.lock`, `LICENSE`, `.gitignore`, `AGENT-BRIEF.md`, the Rust toolchain,
the local Cargo registry, and the physical `046d:0acb` device for the short
checks above. No network access was used beyond offline Cargo
resolution; commands used `--offline`.
