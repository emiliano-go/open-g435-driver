# Clean-room process log

## Purpose

This log records how the G435 host driver was produced, which inputs each role
had, and what was verified. It is a process record, not a legal guarantee.
Nothing here is legal advice.

The public branch contains one squashed clean-room commit. The earlier
pre-clean-room integration history is not present on the public branch.

## Roles and identities

| Role | Identity | Date | Responsibilities |
| --- | --- | --- | --- |
| Analysis | `analysis-agent-v2` | 2026-11-03 | device observation, protocol specification, specification defects, test review, this log |
| Implementation | `implementation-agent-v2`, session `ses_e5b8b8517ffe9YwrVwjMsXKzHQ` | 2026-11-03 | all Rust driver and test code |
| Committer | project owner | 2026-11-03 | repository operations and release |

## Frozen specification

- Path: `docs/PROTOCOL.md`
- Spec ID: `g435-host-protocol-v2.5`
- SHA-256:
  `aad7df22640e062fcc5aff551f6b404a8dbff1fbd96ee2a45073c323c1e8fd92`
- Status: frozen before the final implementation and squashed commit.

The specification contains protocol facts only. It contains no vendor source,
firmware bytes, decompiled vendor code, or vendor binaries.

## Access restrictions

Allowed to the implementation agent:

- `docs/PROTOCOL.md` (the frozen specification)
- `Cargo.toml`, `Cargo.lock`, `LICENSE`, `.gitignore`
- the Rust toolchain and the dependencies listed in `Cargo.toml`
- the physical receiver, for short read-only checks explicitly requested by
  the analysis agent

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
4. The final source, tests, and specification were squashed into one public
   commit and released as v0.2.0.

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

## Verification

Code checks:

- `cargo fmt --check`: pass
- `RUSTFLAGS="-D warnings" cargo clippy --all-targets`: pass
- `cargo test`: 60 passed, 0 failed
  - 9 CLI parsing
  - 8 CLI output
  - 25 device state-machine tests with a mock transport
  - 18 protocol and frame tests
  - no hardware tests in CI

Live checks on `046d:0acb`:

| Check | Result |
| --- | --- |
| `battery` | `3825 mV` |
| `status` | `link status: 0x01 (bits 0b00000001)` |
| `mode` | `remote` |
| `rssi` | `AP L/R: -66 / -66 dBm, PTP L/R: -128 / -128 dBm` |
| `info` | image version `020208011e01`; GRS `0x00`; battery `3825 mV` |
| `read 0 48` | SHA-256 `0c6b048bd814d2f6ecd25f365562257dd453afb9b40591f5af42f6fe9b9553f3` |
| `read 0 96` | SHA-256 `fda8055fd1b6803ef5069e7ce8aaf1ded463eac42915717a104c7d0833e1be59` |
| `read 0 512` | SHA-256 `aa401fc348b5fcc8c85ad8922839d364c171f8eba5e7427d534634b264f0e7a6` |

The 48, 96, and 512 byte reads matched the reference values. The 512 byte read
was repeated five times with the same digest before the final bound was added.
The 48 byte data begins `ff ff cf 25 57 49 58 50 ...`; the `f0 00 00 00`
`READ_MAX_LEN` acknowledgement is not included.

Audio note: bootloader mode mutes the receiver. Live checks were short and the
bridge was exited on success and on error.

## Legal review

Counsel review of this log, the specification hash, and the reimplementation
plan is pending. The review packet is kept in the private firmware repository.
This log does not claim that counsel has approved it.

## Limitations

- This is a documented process separation, not a legal guarantee.
- The driver is read-only. It does not implement OTA writes, erase, reboot,
  handover, or update.
- The mask ROM, RF/PHY stack, and full live flash image are outside this
  driver.
- Only the listed live checks were performed. Inquiry scan and create-connect
  remain specification-only until tested with a suitable headset.
