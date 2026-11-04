# Implementation agent brief

You are the implementation agent for a clean-room reimplementation.

## Work area

Work only in `/tmp/cleanroom-g435`.

## Allowed inputs

- The frozen functional specification, published as `docs/PROTOCOL.md` and copied to the work area as `SPEC.md`
- `Cargo.toml` and `Cargo.lock` (build configuration)
- `LICENSE`, `.gitignore`
- The public Rust toolchain and crates already listed in `Cargo.toml`
- The physical device, only if a short manual check is explicitly requested

## Forbidden inputs

Do not read, copy, inspect, or search any of these:

- `/home/eclipse/Documents/GitHub/open-g435-driver` (previous implementation)
- `/tmp/g435` (vendor SDK, firmware, decompiled output, captures)
- Any file outside `/tmp/cleanroom-g435` except the Rust toolchain and Cargo
  registry
- The vendor updater, vendor binaries, firmware images, decompiled C# or C

If you believe the specification depends on forbidden material, stop and write
the concern to `SPEC-DEFECTS.md` instead.

## Task

Implement the complete Rust crate and CLI from the frozen specification:

- library crate `open_g435`
- binary `g435ctl`
- public API and CLI behavior in sections 12 and 13
- unit tests in `tests/` covering frame encoding, parsing, malformed input,
  length bounds, chunk assembly, and CLI parsing
- no hardware tests

Do not implement OTA writes, erase, reboot, handover, or any destructive
command.

## Hardening

- `#![forbid(unsafe_code)]`
- no panics on malformed device input
- no silent truncation
- bounded timeouts and retries
- bootloader enter/exit scoped to each operation, released on error
- stale ACK and event filtering
- read chunk size 48, address and length multiples of 4

## Verification

Run and report:

```sh
cargo fmt --check
RUSTFLAGS="-D warnings" cargo clippy --all-targets
cargo test
```

Do not use the network except for Cargo dependency resolution from the local
cache or registry.

## Deliverable

Leave the source, tests, and a short `IMPLEMENTATION-NOTES.md` in
`/tmp/cleanroom-g435`. The notes must list:

- files created
- commands run and their exit status
- known limitations or spec ambiguities
- confirmation that no forbidden input was read

Do not commit or push anything.
