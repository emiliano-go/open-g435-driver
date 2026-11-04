# Logitech G435 host protocol, frozen specification v3.2

Status: frozen for clean-room implementation.
Date: 2026-11-03
Spec ID: `g435-host-protocol-v3.2`

Revision history:

- v2: initial frozen specification.
- v2.1: corrected the OTA reply category. The bridge replies with category
  `0x01`, not the request category `0x06`. Bridge control fragments starting
  with `0x01` are not PI frames and must be ignored. Added an explicit bound
  on device waits.
- v2.2: the remote-mode check and the following OTA read must share one
  bootloader session. Exiting and re-entering between them can make the device
  return only an `01` control frame and no OTA data.
- v2.3: after `READ_MAX_LEN`, wait for the optional acknowledgement before
  sending the first `STORAGE_READ`. Sending the first read immediately can
  make the device return only an empty acknowledgement and no data. Link
  setters may also complete with a bridge-control acknowledgement carrying a
  `0x0e` payload instead of a PI link event.
- v2.4: `GET_IMAGE_VERSION` uses image signature `MC` (`4d 43`) after the
  opcode, not `00 00`. A bridge-control fragment may be followed by a complete
  PI frame in the same feature report; discard only the control prefix and
  process the `50 49` suffix. The optional `READ_MAX_LEN` wait ignores input
  ACKs and waits for the feature-report acknowledgement.
- v2.5: storage reads are bounded to the 1 MiB external flash. Reject lengths
  above 1 MiB and any address plus length beyond 1 MiB before device access.
- v3.0: full command inventory, typed safe read-only commands, and a public
  raw tunnel for every PA category. Destructive commands are reachable only
  through the raw tunnel with explicit unsafe confirmation. No destructive
  live test is required or performed on the only receiver.
- v3.1: link setters are fire-and-forget. `SET_MODE` is confirmed by a
  subsequent `GET_MODE`, not by waiting for a setter event.
  `BT_INQUIRY_SCAN` and `CREATE_CONNECT_EX` return after the write and drain.
  Live testing showed the receiver may not emit a link event for a setter.
- v3.2: exact link-query payloads and reply layouts for LPC, quality, DEVM,
  and AFH map. These commands may time out on a receiver that does not emit
  the link event; the typed command must report the timeout, not guess.

This document is the only functional specification for the clean-room host
driver. It contains observed protocol facts, not vendor source. It is written
for an implementation agent that has no access to vendor code, vendor binaries,
firmware images, decompiled vendor output, or the previous implementation.

## 1. Scope

The receiver is `046d:0acb` (HDT647). The driver implements the vendor HID
control plane only:

- battery voltage
- link/connection status
- link mode
- RSSI
- GRS status
- image version
- Bluetooth inquiry scan
- create-connection request
- read-only storage access

Audio is standard USB Audio Class and is handled by the OS. The driver must not
implement OTA writes, erase, reboot, handover, or any other destructive
command.

The host protocol is a request/reply tunnel over HID output and feature
reports. The receiver has a bootloader bridge that must be entered for PA
commands and exited promptly. Bootloader mode mutes audio.

## 2. USB interface

Four interfaces:

- Audio control and two isochronous streaming interfaces (48 kHz, 16-bit;
  stereo out, mono mic), UAC 1.0.
- HID interface (class 3) with vendor collection usage page `0xFFC0` and
  report ID 6.

Report sizes:

| Report | Size including report ID | Direction |
| --- | ---: | --- |
| Output report 6 | 515 bytes | host to device |
| Feature report 6 | 64 bytes | device to host |
| Input report 6 | 7 bytes | device to host |

The driver may enumerate by VID/PID and open the first matching HID device.
Multiple devices are not required.

## 3. Bootloader bridge

Enter:

```
output report 6, 515 bytes: 06 01 03 00 ... 00
```

Exit:

```
output report 6, 515 bytes: 06 08 03 00 ... 00
```

Acknowledge:

- input report starting `06 5a`, or
- feature report with `buf[1] == 0x5a`.

The bridge is stateful. Enter and exit are idempotent from the caller's point
of view, but the implementation must track whether it believes the bridge is
open. Enter only when a command needs the PA tunnel, and exit as soon as the
operation finishes, including error paths. Opening the device must not enter
the bridge. Bootloader mode mutes audio; a long-lived open handle must not hold
the bridge open.

If the enter write succeeds but the acknowledgement times out, the device may
still have entered the bridge. Mark the bridge as possibly open and attempt an
exit before returning the error. A `Drop` fallback must attempt an exit if a
session is still open.

## 4. Command output report

Every PA request is wrapped in output report 6:

```
06 04 03 <len_lo> <len_hi> <PA frame...> 00 ...
```

- `len` is the length of the PA frame in bytes, little-endian 16-bit.
- The report is always 515 bytes including the report ID.
- The PA frame must fit in the 510 bytes after the five-byte header. Reject
  longer frames instead of truncating.
- The report is zero-padded.

## 5. PA request frames

Format:

```
50 41 <category> <seq> <len> <payload...>
```

- `50 41` is the request magic.
- `category` is `0x06` for OTA and `0x0e` for link/radio.
- `seq` is the caller's sequence byte; wrap at 256.
- `len` at offset 4 is a single byte. It is not 16-bit.
- For OTA (`0x06`): `len = payload.len() - 1`; the payload starts with `00`.
- For link (`0x0e`): `len = payload.len()`; the payload starts with the
  command ID.

The total request length is `5 + payload.len()`.

## 6. PI reply frames

Format:

```
50 49 <category> <seq> <four bytes> <len_lo> <len_hi> <payload...>
```

- `50 49` is the reply magic.
- The declared payload length is a little-endian 16-bit value at offset 8.
- The total frame length is `10 + payload_len`.
- Extra bytes after the declared total are not part of the frame.
- Reject frames shorter than 10 bytes, with bad magic, or truncated relative
  to the declared length.

Payload views:

- OTA reply: payload starts `06 <seq> <status> <data...>`. The status is the
  third payload byte; `0` means OK. Data starts at frame offset 13.
- On this bridge the OTA reply frame's category byte at offset 2 is `0x01`,
  not the request category `0x06`. The request category and the reply category
  are different. Accept an OTA reply when its category is `0x01` or `0x06`
  and its payload starts with `0x06`. Do not require the reply category to
  equal the request category.
- Link/radio reply: payload starts `<command-ish> <data...>`. Event data starts
  at frame offset 11. At least two payload bytes are required to read the first
  data byte.

## 7. Reply transport

Replies arrive as feature report 6:

```
buf[0] = 0x06
buf[1..1+n] = fragment
n = buf[63]
```

- `n` is the last byte of the 64-byte feature report.
- `n == 0` or `n > 63` means no usable fragment.
- Concatenate fragments into a persistent receive buffer until one complete PI
  frame is present.
- If the buffer does not start with `50 49`, resynchronize by discarding bytes
  until the next `50 49`.
- A fragment that starts with `0x01` is a bridge control or acknowledgement
  frame, not a PI frame. Ignore it and continue reading; do not parse it as
  OTA data. Its layout is `01 <4-byte token> <length> 00 <opaque bytes>` and
  is not needed by the driver.
- One feature report can contain a bridge control prefix followed by a complete
  PI frame (`50 49 ...`). Discard the control prefix and process the PI suffix;
  do not discard the whole report.
- A bridge control acknowledgement can carry a link completion at byte 8
  equal to `0x0e`. A link setter may use that as its completion when no PI
  `0x0e` event follows. Do not let such an acknowledgement satisfy a later
  link query.
- If a complete-looking frame fails validation, discard one byte and continue
  looking; do not stall forever.
- After a reply is returned, drain stale input reports. Do not let an ACK or an
  old event be mistaken for the next command's reply.

## 8. Reply ordering

- A link query may receive a bootloader ACK or an unrelated frame before the
  category `0x0e` event. A link setter may emit no event at all; do not block
  waiting for one.
- An OTA read may receive an ACK frame before the data frame. The ACK payload
  is `06 <seq> <status>` with no data.
- OTA data filters must ignore empty OTA data payloads and wait for a non-empty
  data frame. Accept category `0x01` or `0x06` with payload starting `0x06`.
- Link filters must accept only category `0x0e`.
- Use a bounded number of frames and a timeout; fail with a protocol error
  rather than looping forever.

## 9. OTA commands (category 0x06)

Full observed command inventory. `R` means read-only or non-destructive.
`D` means destructive or state-changing.

| ID | Name | Class | Notes |
| ---: | --- | --- | --- |
| `0x22` | STORAGE_WRITE | D | flash write |
| `0x23` | STORAGE_READ | R | `00 23 01 <addr LE32> <len LE32>` |
| `0x24` | MORE_FRAME | R | fetch next storage data frame |
| `0x25` | IOCTL | D | sub-operation in arguments |
| `0x30` | OTA_MODE | D | enter or leave OTA mode |
| `0x31` | GET_BATTERY | R | LE16 millivolts |
| `0x32` | GET_RSSI | R | four signed bytes |
| `0x33` | GET_GRS_STATUS | R | status byte |
| `0x34` | GET_IMAGE_VERSION | R | `00 34 00 4d 43` |
| `0x35` | ERASE_LOAD_REGION | D | erase |
| `0x36` | SET_LOAD_INFO | D | load metadata |
| `0x37` | LOAD_MORE | D | load data |
| `0x38` | CALC_LOAD_REGION_CRC | R | calculate CRC |
| `0x39` | UPDATE | D | commit update |
| `0x3A` | HANDOVER | D | handover |
| `0x3B` | GET_FILESYSTEM_TABLE | R | 140-byte filesystem table |
| `0x3C` | REBOOT | D | reboot |
| `0x3D` | CALC_LOAD_REGION_CRC32 | R | calculate CRC32 |
| `0x40` | READ_CUST_INFO | R | `00 40 <id>` |
| `0x42` | REBOOT_AND_POWERON | D | reboot and power on |
| `0x43` | READ_MP_DATA_BY_ID | R | `00 43 <id>` |
| `0x44` | WRITE_MP_DATA_BY_ID | D | `00 44 <id> <data>` |
| `0x45` | GET_ATB_VALUE | R | `00 45 <gpio>` |
| `0x46` | READ_NVDS_DATA_BY_ID | R | `00 46 <id>` |
| `0x48` | ENABLE_ADVERTISING | D | advertising control |
| `0x49` | GET_FILESYSTEM_INDEX | R | `00 49 <index>` |
| `0x60` | SET_BLE | D | BLE control |
| `0x61` | TEST_MODE | D | test mode |
| `0x62` | SET_LED | D | LED control |
| `0x63` | GET_IO_LEVEL | R | read GPIO |
| `0x64` | SET_LED_CURRENT | D | LED current |
| `0x65` | SET_LED_PWM | D | LED PWM |
| `0x66` | SET_IO_LEVEL | D | write GPIO |
| `0x67` | WRITE_I2C | D | I2C write |
| `0x68` | READ_I2C | R | I2C read |
| `0x70` | SIDETONE_ENABLE | D | sidetone |
| `0x71` | SET_SIDETONE_GAIN | D | sidetone gain |
| `0x72` | GET_SIDETONE_GAIN | R | sidetone gain |
| `0x7F` | SET_OTA_STATUS | D | OTA status |
| `0xFE` | ECHO | R | `00 FE <data...>` |

Safe typed payloads:

- `GET_FILESYSTEM_TABLE`: `00 3B`, length 1. Reply is 140 bytes.
- `GET_FILESYSTEM_INDEX`: `00 49 <index>`, length 2. Reply is 8 bytes:
  offset LE32, size LE32.
- `READ_CUST_INFO`: `00 40 <id>`, length 2.
- `READ_MP_DATA_BY_ID`: `00 43 <id>`, length 2.
- `READ_NVDS_DATA_BY_ID`: `00 46 <id>`, length 2.
- `GET_ATB_VALUE`: `00 45 <gpio>`, length 2. Reply is 4 bytes.
- `ECHO`: `00 FE <data...>`, length `1 + data.len()`.

The raw tunnel can send any OTA command ID with arbitrary arguments. It
returns the OTA status and data bytes. It is dangerous and must require an
explicit unsafe confirmation flag. It must not be the default path for typed
safe commands.

## 10. Storage read algorithm

1. Validate: `addr % 4 == 0`, `len % 4 == 0`, `len != 0` (zero returns an empty
   vector without touching the device). Reject `len > 1 MiB` and any
   `addr + len > 1 MiB` with an argument error. Use checked arithmetic.
2. Enter the bootloader bridge.
3. Send `READ_MAX_LEN(240)`. Wait up to 250 ms for one optional
   feature-report acknowledgement frame and discard it. Ignore input ACKs
   (`06 5a`) for this wait; they do not carry the feature acknowledgement.
   If no feature frame arrives, continue. Do not send the first
   `STORAGE_READ` immediately after `READ_MAX_LEN`; the device can otherwise
   return only an empty acknowledgement and no data. The acknowledgement is a
   PI frame with payload `06 <seq> 00 f0 00 00 00`; it is not storage data.
4. For each chunk:
   - Chunk size is at most 48 bytes. The bridge is reliable with one
     feature-report frame per chunk; larger sizes wedge or truncate.
   - Send `STORAGE_READ(chunk_addr, chunk_len)`.
   - Read PI frames until a non-empty OTA data frame is accepted.
   - If the accepted frame has no data, send `MORE_FRAME` and read again.
   - Append at most `chunk_len` bytes and stop at the requested total.
   - Drain stale input before the next chunk. Do not send the next
     `STORAGE_READ` until the previous chunk is complete.
5. Exit the bootloader bridge in all paths, including errors.
6. Return exactly `len` bytes.

Data starts at frame offset 13. The bridge may fragment a reply across several
feature reports and several `MORE_FRAME` calls. Do not assume one frame equals
one chunk.

## 11. Link commands (category 0x0e)

| Command | Payload | Effect |
| --- | --- | --- |
| GET_CONNECT_STATUS | `e3` | event data at frame[11], bitmask |
| GET_MODE | `e0` | event data at frame[11]; `1` = remote, else local |
| SET_MODE | `e1 <0/1>` | set local/remote |
| BT_INQUIRY_SCAN | `f0 <0/1>` | enable/disable inquiry scan |
| CREATE_CONNECT_EX | `e5 <mode> <profile> [bd_addr]` | request a connection |

`CREATE_CONNECT_EX` with `mode = 0` takes a six-byte BD_ADDR sent
least-significant byte first. `mode != 0` uses the current link and carries no
address.

Link-query payloads and reply layouts:

- `GET_AFH_MAP`: payload `02`; reply `conn_handle LE16` followed by 10
  channel bytes.
- `GET_QUALITY`: payload `03`; reply `conn_handle LE16`, `count u8`,
  `rssi i8`, `quality u8`.
- `GET_LPC`: payload `04`; reply `count u8` followed by 8 PLC bytes.
- `GET_DEVM`: payload `05`; reply `conn_handle LE16`, `count u8`, then 80
  DEVM bytes.

These are link category `0x0e` requests. A receiver that does not emit the
link event must produce a timeout error; do not synthesize data.

Before an OTA read of battery, RSSI, GRS, or image version, ensure the receiver
is in remote mode:

1. Read `GET_MODE`.
2. If it is remote, continue.
3. Otherwise send `SET_MODE(1)`, wait 100 ms, and read `GET_MODE` again.
4. Retry at most five times. If the mode does not become remote, return an
   error instead of sending the OTA read.

`SET_MODE` is written and then confirmed by reading `GET_MODE` again. Do not
wait for a setter event. `BT_INQUIRY_SCAN` and `CREATE_CONNECT_EX` are
fire-and-forget after the write and drain.

The remote-mode check and the following OTA read must run inside one
bootloader session. Do not exit the bridge between the last mode check and the
OTA request. Exiting and re-entering at that point can cause the device to
return only an `01` control frame and no OTA data.

## 12. Public library contract

The crate is `open_g435`; the binary is `g435ctl`. The following public surface
must exist and behave as described. Internals are free.

- `G435::open() -> Result<G435>`
- `G435::open_with(vid: u16, pid: u16) -> Result<G435>`
- `G435::battery() -> Result<u16>`
- `G435::link_status() -> Result<LinkStatus>`
- `G435::mode() -> Result<Mode>`
- `G435::set_mode(Mode) -> Result<()>`
- `G435::set_inquiry_scan(bool) -> Result<()>`
- `G435::create_connect([u8; 6]) -> Result<()>`
- `G435::rssi() -> Result<Rssi>`
- `G435::grs_status() -> Result<GrsStatus>`
- `G435::image_version() -> Result<ImageVersion>`
- `G435::read_storage(addr: u32, len: u32) -> Result<Vec<u8>>`

Types:

- `Mode` is `Local` or `Remote`; display as `local` or `remote`.
- `Rssi` has `left_ap`, `right_ap`, `left_ptp`, `right_ptp` as `i8`.
- `LinkStatus` has `bits: u8`.
- `GrsStatus` has `bits: u8`.
- `ImageVersion` exposes the raw bytes and a lowercase hex representation.
- `Error` and `Result` cover transport, protocol, invalid argument, and device
  status errors.

Modules `protocol` and `transport` may expose the types and constants needed by
the tests and the binary. The exact names beyond the items above are free.

## 13. CLI contract

Global option: `--json`.

| Command | Human output | JSON |
| --- | --- | --- |
| `battery` | `<mv> mV` | `{"millivolts": <mv>}` |
| `status` | `link status: 0xXX (bits 0b........)` | `{"bits": <u8>}` |
| `mode` | `local` or `remote` | `{"mode": "..."}` |
| `mode --set local|remote` | `mode set to ...` | `{"set": "..."}` |
| `rssi` | `AP L/R: L / R dBm, PTP L/R: L / R dBm` | four signed fields |
| `scan` | `inquiry scan enabled (use --off to disable)` | `{"inquiry_scan": true}` |
| `scan --off` | `inquiry scan disabled` | `{"inquiry_scan": false}` |
| `scan --duration N` | enable, sleep N seconds, disable | `{"inquiry_scan": false}` |
| `pair <mac>` | `connection requested for <mac>` | `{"paired": "<mac>"}` |
| `info` | image version, GRS status, battery | three fields |
| `read <addr> <len>` | hex bytes, then `sha256: <hex>` | addr, len, sha256, data |
| `read <addr> <len> --output PATH` | `wrote N bytes to PATH sha256 <hex>` | n/a |

- `addr` and `len` accept decimal or `0x`-prefixed hexadecimal.
- `mac` is six colon-separated octets.
- Read prints the SHA-256 of the returned bytes.
- With `--output`, write raw bytes and do not print the hex.
- Errors go to stderr and exit non-zero.
- Logging goes to stderr; default level `warn`.

### Raw and unsafe commands

The public CLI must provide a raw tunnel for every PA command:

- `g435ctl raw-ota <command-id> [args-hex]`
- `g435ctl raw-link <command-id> [args-hex]`
- `g435ctl raw-pa <category> <payload-hex>`

Each raw command must require:

```
--unsafe --confirm I-UNDERSTAND
```

The token is exactly `I-UNDERSTAND`. Raw commands must print a warning to
stderr. They must return the status byte, the raw reply data as hex, and
the raw frame bytes. They must not silently retry a write.

Typed safe commands must exist for:

```
g435ctl fs-table
g435ctl fs-index <index>
g435ctl cust-info <id>
g435ctl mp-read <id>
g435ctl nvds-read <id>
g435ctl atb <gpio>
g435ctl echo <hex>
g435ctl lpc
g435ctl quality
g435ctl devm
g435ctl afh-map
```

The typed safe commands must be usable without unsafe flags. They must not
send write, erase, update, reboot, or handover commands.

## 14. Hardening requirements

- The library uses `#![forbid(unsafe_code)]`.
- No panics on malformed HID data. All parsing and indexing must be bounds
  checked; public parsing returns `Result`.
- No silent truncation. Reject overlong PA frames and reports.
- No unbounded loops. Every device wait has a timeout and bounded retries.
- `open` does not change device state. Bootloader enter/exit is scoped to each
  operation and is released on error.
- `read_storage` exits the bridge on every path.
- Storage reads are bounded to 1 MiB and use checked address arithmetic.
- Stale ACKs and events are drained; a reply from a previous command must not
  satisfy the next command.
- Only read-only commands are implemented. No write, erase, reboot, update,
  handover, or OTA mode commands.
- CLI `--output` writes only the bytes returned by the read.
- Unit tests cover frame encoding, parsing, malformed input, length bounds,
  chunk assembly, and CLI parsing. Hardware tests are optional and must not run
  in CI.

## 15. Provenance

This specification was produced by an analysis effort from the public device
behavior and the vendor updater's managed SDK. It contains no vendor source
code, no firmware bytes, and no decompiled vendor code. Vendor binaries and
firmware are not redistributed. The updater SHA-256 and image hashes are in the
private firmware repository.
