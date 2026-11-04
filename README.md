# open-g435-driver

Independent, clean-room Rust driver for the Logitech G435 LIGHTSPEED USB
receiver (`046d:0acb`).

**Status:** work in progress. The implementation is being written by a
separate implementation agent from a frozen protocol specification; the
analysis agent reviews and corrects defects.

## Scope

- Control plane over the receiver's vendor HID interface: battery, connection
  status, mode, pairing/scan, RSSI, read-only storage access.
- Audio remains standard USB Audio Class and is handled by the OS
  (ALSA/PipeWire); this driver does not replace the audio class driver.
- Read-only by default. OTA/flash writes and destructive commands are not
  implemented.

## Build

```sh
cargo build --release
cargo test
RUSTFLAGS="-D warnings" cargo clippy --all-targets
cargo fmt --check
```

## Usage

The CLI is `g435ctl`. Planned commands:

```sh
g435ctl battery
g435ctl status
g435ctl mode
g435ctl rssi
g435ctl scan
g435ctl pair <mac>
```

The exact command set will be finalized with the implementation.

## Protocol

The host protocol summary is in [`docs/PROTOCOL.md`](docs/PROTOCOL.md).

## Legal and clean-room notice

This is an independent, unofficial interoperability project. It is not
affiliated with, endorsed by, or sponsored by Logitech. "Logitech" and "G435"
are trademarks of Logitech, used only to describe compatibility.

This repository contains no vendor code, firmware, or binaries. The
implementation was written by a separate implementation agent from a written
interface specification produced by a separate analysis effort. The
implementation agent had no access to vendor source code, vendor binaries, or
firmware. The analysis agent reviewed and corrected defects in the
implementation; no vendor code was copied. See `CLEANROOM.md`.

No authentication or encryption was broken. Provided "as is", without
warranty. Nothing here is legal advice.

## License

MIT.
