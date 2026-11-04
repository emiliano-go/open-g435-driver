# open-g435-driver

Independent, clean-room Rust driver for the Logitech G435 LIGHTSPEED USB
receiver (`046d:0acb`).

**Status: v0.2.0, control plane live-verified on `046d:0acb`.** Verified:
battery, link status, link mode, RSSI, GRS status, image version, inquiry scan
command construction, and read-only storage up to 512 bytes. Inquiry scan and
create-connect are implemented from the specification but were not exercised
against the headset.

This is an unofficial interoperability project. It is not affiliated with,
endorsed by, or sponsored by Logitech.

## Scope

- Control plane over the receiver's vendor HID interface: battery, connection
  status, mode, RSSI, GRS status, image version, inquiry scan, connection
  request, and read-only storage access.
- Audio remains standard USB Audio Class and is handled by the OS
  (ALSA/PipeWire); this driver does not replace the audio class driver.
- Read-only. OTA/flash writes, erase, reboot, handover, and update commands are
  not implemented.
- Storage reads are limited to the 1 MiB external flash and use 48-byte
  chunks.

## Build

```sh
cargo build --release
cargo test
RUSTFLAGS="-D warnings" cargo clippy --all-targets
cargo fmt --check
```

## Usage

The CLI is `g435ctl`:

```sh
g435ctl battery
g435ctl status
g435ctl mode
g435ctl mode --set remote
g435ctl rssi
g435ctl info
g435ctl scan
g435ctl scan --off
g435ctl scan --duration 10
g435ctl pair AA:BB:CC:DD:EE:FF
g435ctl read 0 48
g435ctl read 0 512 --output image.bin
```

Add `--json` for machine-readable output. `read` accepts decimal or
`0x`-prefixed hexadecimal addresses and lengths that are multiples of four.

Example:

```text
$ g435ctl battery
3825 mV

$ g435ctl status
link status: 0x01 (bits 0b00000001)

$ g435ctl rssi
AP L/R: -66 / -66 dBm, PTP L/R: -128 / -128 dBm

$ g435ctl info
image version: 020208011e01
grs status: 0x00 (bits 0b00000000)
battery: 3825 mV
```

## Hardware notes

- The receiver's bootloader bridge mutes audio while it is open. The driver
  enters it only for the duration of a command and exits on success and error.
  A short battery or status query can therefore cause a brief audio gap.
- The vendor HID node is often `root:root 0600`. Install a udev rule or adjust
  permissions for the intended user.
- If a process is killed while the bridge is open, the receiver can remain
  muted or wedged. Unplug and replug the USB-A receiver to reset it. A soft
  USB unbind/bind does not reliably clear this state.
- Do not run long storage reads while listening to audio; the bridge mutes the
  output for the duration.

## Protocol

The frozen host protocol is in [`docs/PROTOCOL.md`](docs/PROTOCOL.md). It is a
functional specification of observed protocol facts. It contains no vendor
source or firmware.

## Clean room

The implementation was written by a separate implementation agent from the
frozen specification only. The implementation agent had no access to vendor
source, vendor binaries, firmware images, decompiled vendor output, or the
previous implementation. The analysis agent wrote the specification, filed
specification defects, and reviewed test observations; it did not edit the
driver source. See [`CLEANROOM.md`](CLEANROOM.md),
[`docs/CLEANROOM-BRIEF.md`](docs/CLEANROOM-BRIEF.md), and
[`IMPLEMENTATION-NOTES.md`](IMPLEMENTATION-NOTES.md).

Counsel review of the process and log is pending; the packet is kept in the
private firmware repository.

## Legal

This repository contains no vendor code, firmware, or binaries. No
authentication or encryption was broken. The project is provided "as is",
without warranty. Nothing here is legal advice.

"Logitech" and "G435" are trademarks of Logitech, used only to describe
compatibility.

## License

MIT.
