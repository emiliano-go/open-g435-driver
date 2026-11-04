# Implementation notes

Clean-room implementation of `g435-host-protocol-v2.5`.

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

No existing source files were modified; `Cargo.toml`, `Cargo.lock`, `docs/PROTOCOL.md`,
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

Test totals: 60 passing (9 CLI parsing, 8 CLI output, 25 device, 18
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
- **Link setters consume their event.** `SET_MODE`, `BT_INQUIRY_SCAN`, and
  `CREATE_CONNECT_EX` wait for a category `0x0e` PI event and consume it so it
  cannot satisfy a later query. The device does not always emit a PI event;
  it may answer with a `01` bridge-control ACK carrying an `0x0e` payload.
  That control ACK is accepted as completion and remains unavailable to later
  PI queries.
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
- **OTA write/erase/reboot/handover commands** are not implemented.

## Live device results

Receiver present: `046d:0acb` (Logitech G435 Wireless Gaming Headset).
Short checks only; no storage read or dump was performed.

Live checks after the v2.4 changes:

| Command | Exact output | Exit |
| --- | --- | ---: |
| `g435ctl status` | `link status: 0x01 (bits 0b00000001)` | 0 |
| `g435ctl mode` | `remote` | 0 |
| `g435ctl battery` | `3825 mV` | 0 |
| `g435ctl rssi` | `AP L/R: -66 / -66 dBm, PTP L/R: -128 / -128 dBm` | 0 |
| `g435ctl info` | `image version: 020208011e01`, `grs status: 0x00 (bits 0b00000000)`, `battery: 3825 mV` | 0 |
| `g435ctl --json info` | `{"battery":3816,"grs_status":0,"image_version":"020208011e01"}` | 0 |
| `g435ctl read 0 48` | `sha256: 0c6b048bd814d2f6ecd25f365562257dd453afb9b40591f5af42f6fe9b9553f3` | 0 |
| `g435ctl read 0 96` | `sha256: fda8055fd1b6803ef5069e7ce8aaf1ded463eac42915717a104c7d0833e1be59` | 0 |
| `g435ctl read 0 512` | `sha256: aa401fc348b5fcc8c85ad8922839d364c171f8eba5e7427d534634b264f0e7a6` | 0 |

The 48-byte data starts `ff ff cf 25 57 49 58 50 ...`; the `f0 00 00 00`
READ_MAX_LEN acknowledgement is no longer included. The 48/96/512 hashes match
the previously known values, and the smaller hex is a prefix of the larger.
`info` now succeeds with image version `020208011e01`; the `MC` signature
removed the previous status `0xff`. After adding the 1 MiB bound, `read 0 48`
was rechecked and returned the same known SHA-256
`0c6b048bd814d2f6ecd25f365562257dd453afb9b40591f5af42f6fe9b9553f3`. No
larger storage read and no dump was performed. The mode was left set to
remote.

## Forbidden input

No file from `/home/eclipse/Documents/GitHub/open-g435-driver`, `/tmp/g435`,
any vendor SDK, firmware image, decompiled output, or capture was read,
copied, searched, or inspected. Inputs used were `SPEC.md`, `Cargo.toml`,
`Cargo.lock`, `LICENSE`, `.gitignore`, `AGENT-BRIEF.md`, the Rust toolchain,
the local Cargo registry, and the physical `046d:0acb` device for the short
checks above. No network access was used beyond offline Cargo
resolution; commands used `--offline`.
