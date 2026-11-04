# Clean-room process log

## Purpose

This log records how the G435 host driver was produced, which inputs each role
had, and what was verified. It is a process record, not a legal guarantee.
Nothing here is legal advice.

The v0.2.0 baseline is one squashed clean-room commit. v0.3.0 is a follow-up
commit produced with the same role separation and the same access rules. The
earlier pre-clean-room integration history is not present on the public branch.

## Roles and identities

| Role | Identity | Dates | Responsibilities |
| --- | --- | --- | --- |
| Analysis | `analysis-agent-v2` | 2026-11-03 to 2026-11-04 | device observation, protocol specification, specification defects, test review, this log |
| Implementation | `implementation-agent-v2`, session `ses_e5b8b8517ffe9YwrVwjMsXKzHQ` | 2026-11-03 to 2026-11-04 | all Rust driver and test code |
| Committer | project owner | 2026-11-03 to 2026-11-04 | repository operations and releases |

## Frozen specification

- Path: `docs/PROTOCOL.md`
- Spec ID: `g435-host-protocol-v3.2`
- SHA-256:
  `bc4eb4caef60f0b36c6737eeabf14f5a93a2a45942351ad1cee2906a656e4071`
- Status: frozen for v0.3.0.

The specification contains protocol facts only. It contains no vendor source,
firmware bytes, decompiled vendor code, or vendor binaries.

## Access restrictions

Allowed to the implementation agent:

- `docs/PROTOCOL.md` (the frozen specification)
- `Cargo.toml`, `Cargo.lock`, `LICENSE`, `.gitignore`
- the Rust toolchain and the dependencies listed in `Cargo.toml`
- the physical receiver, for short checks explicitly requested by the analysis
  agent

Denied to the implementation agent:

- the previous driver implementation
- the vendor SDK and updater
- vendor firmware images and WIP images
- the private firmware analysis repository
- decompiled vendor code, captures, and analysis outputs

The work area was isolated at `/tmp/cleanroom-g435`. The implementation agent
confirmed that no denied input was read.

## Process

1. The analysis agent recovered protocol facts and wrote the specification.
2. The implementation agent implemented the crate, CLI, and tests from the
   specification only.
3. Short live checks exposed specification gaps. The implementation agent
   reported observations; the analysis agent validated them and issued
   specification revisions. The implementation agent applied each revision.
   The analysis agent did not write or edit the driver source in the clean
   work area.
4. v0.2.0 was squashed into one public commit.
5. v0.3.0 added safe metadata commands and the public raw tunnel, again through
   specification revisions applied by the implementation agent.
6. No destructive write, erase, update, reboot, or handover command was sent to
   the receiver. Only one receiver is available, so destructive paths are
   implemented and mock-tested but not live-tested.

The analysis agent's vendor-derived firmware notes remain in the private
firmware repository and are not part of this repository.

## Specification revisions

- v2: initial frozen specification.
- v2.1: OTA reply frames use category `0x01`, not the request category
  `0x06`; bridge-control fragments starting with `0x01` are not PI frames.
- v2.2: the remote-mode check and the following OTA read must share one
  bootloader session.
- v2.3: wait for the optional `READ_MAX_LEN` feature acknowledgement before
  the first `STORAGE_READ`; link setters may complete with a bridge-control
  acknowledgement carrying `0x0e`.
- v2.4: `GET_IMAGE_VERSION` uses signature `MC` (`4d 43`); recover a PI frame
  that follows a bridge-control prefix in the same feature report; ignore
  input ACKs during the optional `READ_MAX_LEN` wait.
- v2.5: bound storage reads to the 1 MiB external flash and use checked
  address arithmetic.
- v3.0: full OTA command inventory, typed safe read-only commands, and a public
  raw tunnel for every PA category. Destructive commands are reachable only
  through the raw tunnel with explicit confirmation.
- v3.1: link setters are fire-and-forget. `SET_MODE` is confirmed by
  `GET_MODE`; the receiver may not emit a setter event.
- v3.2: exact typed link-query payloads and reply layouts for LPC, quality,
  DEVM, and AFH map. The receiver emits no matching link event, so these
  commands return a timeout instead of synthesized data.

## Verification

Code checks:

- `cargo fmt --check`: pass
- `RUSTFLAGS="-D warnings" cargo clippy --all-targets`: pass
- `cargo test`: 77 passed, 0 failed
  - 12 CLI parsing
  - 8 CLI output
  - 39 device state-machine tests with a mock transport
  - 18 protocol and frame tests
  - no hardware tests in CI

Live checks on `046d:0acb`, v0.3.0:

| Check | Result |
| --- | --- |
| `status` | `link status: 0x01` |
| `battery` | `4146 mV` |
| `info` | image version `020208011e01`; GRS `0x00`; battery `4146 mV` |
| `fs-table` | 140-byte version-1 filesystem table |
| `cust-info 15` | 91 bytes |
| `mp-read 1` | 6 bytes: `a45a14995840` |
| `mp-read 2` | 5 bytes: `46377ca005` |
| `atb 0` | `0x00000000` |
| `echo 00010203` | echoed `00010203` |
| `read 0 48` | SHA-256 `0c6b048bd814d2f6ecd25f365562257dd453afb9b40591f5af42f6fe9b9553f3` |
| `mode --set local` then `mode` | `local` |
| `mode --set remote` then `mode` | `remote` |
| `raw-ota 0x31` with confirmation | OTA status 0, data `32100000` |

Known device limitations found during safe testing:

- `fs-index 1` and `fs-index 2` return status `0xff`; use `fs-table` for the
  mapping.
- `nvds-read 1`, `2`, and `15` return status `0xff`; NVDS is readable as the
  storage range `0xFE000..0x100000`.
- `lpc`, `quality`, `devm`, and `afh-map` time out waiting for a link event.
- `raw-link` times out for IDs `2` through `5`.
- `raw-pa 0x0e 02` returns only the link acknowledgement, data `0e8000`.

Audio note: bootloader mode mutes the receiver. Live checks were short and the
bridge was exited on success and on error.

## Destructive commands

The public raw tunnel can send any OTA or link command, including destructive
ones. Every raw command requires:

```
--unsafe --confirm I-UNDERSTAND
```

No destructive raw command was live-tested. The project has one receiver and
no spare, so a failed write could leave it unusable until physical recovery.

## Legal review

Counsel review of this log, the v3.1 specification hash, the public raw
destructive surface, and the reimplementation plan is pending. The review
packet is kept in the private firmware repository. This log does not claim that
counsel has approved it.

## Limitations

- This is a documented process separation, not a legal guarantee.
- The default typed commands are read-only. The raw tunnel is explicitly
  unsafe and can write, erase, reboot, or hand over the receiver.
- The mask ROM and RF/PHY stack remain outside this driver.
- Only the listed live checks were performed. Inquiry scan, create-connect,
  link-quality, and NVDS commands remain unverified on this receiver.
