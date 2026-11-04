# open-g435-driver

Independent, clean-room Rust driver for the Logitech G435 LIGHTSPEED USB
receiver (`046d:0acb`).

**Status: v0.3.0.** Most safe control and read-only commands are live-verified
on `046d:0acb`. The typed LPC, quality, DEVM, and AFH-map commands are
implemented from the specification, but this receiver emits no matching link
event and they return a timeout. The public raw tunnel can send every known
OTA and link command, including destructive ones, but it requires explicit
unsafe confirmation and was not live-tested destructively.

This is an unofficial interoperability project. It is not affiliated with,
endorsed by, or sponsored by Logitech.

## Scope

Safe typed commands:

- battery, link status, link mode, RSSI, GRS status, image version
- filesystem table and index
- customer info, MP data, NVDS data, ATB value
- storage read
- link quality, LPC, DEVM, AFH map
- inquiry scan
- create-connect request

Public raw commands:

- `raw-ota`
- `raw-link`
- `raw-pa`

The raw tunnel is intentionally unsafe. It can write flash, erase, reboot, or
change device state. Every raw command requires:

```sh
--unsafe --confirm I-UNDERSTAND
```

No destructive raw command was live-tested; the project has one receiver and
no spare.

Audio remains standard USB Audio Class and is handled by the OS
(ALSA/PipeWire); this driver does not replace the audio class driver.

## Build

```sh
cargo build --release
cargo test
RUSTFLAGS="-D warnings" cargo clippy --all-targets
cargo fmt --check
```

## Usage

Safe commands:

```sh
g435ctl battery
g435ctl status
g435ctl mode
g435ctl mode --set remote
g435ctl rssi
g435ctl info
g435ctl fs-table
g435ctl fs-index 0
g435ctl cust-info 15
g435ctl mp-read 1
g435ctl nvds-read 0
g435ctl atb 0
g435ctl echo 00010203
g435ctl lpc
g435ctl quality
g435ctl devm
g435ctl afh-map
g435ctl scan
g435ctl scan --off
g435ctl scan --duration 10
g435ctl pair AA:BB:CC:DD:EE:FF
g435ctl read 0 48
g435ctl read 0 512 --output image.bin
```

Raw commands:

```sh
g435ctl raw-ota --unsafe --confirm I-UNDERSTAND 0x31
g435ctl raw-link --unsafe --confirm I-UNDERSTAND 2
g435ctl raw-pa --unsafe --confirm I-UNDERSTAND 0x0e 02
```

Add `--json` for machine-readable output. `read` accepts decimal or
`0x`-prefixed hexadecimal addresses and lengths that are multiples of four.
Storage reads are limited to the 1 MiB external flash and use 48-byte chunks.

Example:

```text
$ g435ctl battery
4146 mV

$ g435ctl status
link status: 0x01 (bits 0b00000001)

$ g435ctl info
image version: 020208011e01
grs status: 0x00 (bits 0b00000000)
battery: 4146 mV
```

## Hardware notes

- The receiver's bootloader bridge mutes audio while it is open. The driver
  enters it only for the duration of a command and exits on success and error.
  A short query can cause a brief audio gap.
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

Counsel review of the process, the spec hash, and the public raw destructive
surface is pending; the packet is kept in the private firmware repository.

## Legal

This repository contains no vendor code, firmware, or binaries. No
authentication or encryption was broken. The project is provided "as is",
without warranty. Nothing here is legal advice.

The raw tunnel can modify or damage the receiver. Use it at your own risk.

"Logitech" and "G435" are trademarks of Logitech, used only to describe
compatibility.

## License

MIT.
