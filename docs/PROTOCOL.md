# Logitech G435 host protocol

Recovered from the vendor SDK `AWToolLIB2` 1.0.3.3 (embedded in
`G435Update_3.1.1.14.exe`) and the dongle's USB descriptors. This is the
host↔dongle control plane. Audio is standard USB Audio Class and is handled by
the OS.

## 1. USB interface

`046d:0acb` (HDT647 receiver). Four interfaces:

- Audio control + two isochronous streaming interfaces (48 kHz, 16-bit;
  stereo out, mono mic), UAC 1.0.
- HID interface (class 3) with vendor collection usage page `0xFFC0`,
  report ID 6:
  - output report: 514 bytes
  - feature report: 63 bytes
  - input report: 7 bytes

## 2. RTK HID transport

The vendor `AW_HID_RTK` path uses **output reports** for commands and
**feature reports** for replies. `OutputReportLength` must be 515
(report ID + 514 bytes).

### Enter / exit bootloader (ALC4021 bridge)

Output report 6, 515 bytes:

```
06 01 03 00 ...     enter
06 08 03 00 ...     exit
```

Ack: input report `06 5a`, or feature report with `buf[1] == 0x5a`.

### Send a PA command

Output report 6, 515 bytes:

```
06 04 03 <len_lo> <len_hi> <PA frame...> 00 ...
```

`len` is the PA frame length (LE16). Entering bootloader mutes audio for the
duration; always exit afterwards.

### Read a reply

Feature report 6, 64 bytes:

```
buf[0] = 0x06
buf[1..1+N] = fragment          (N = buf[63], the last byte)
```

Concatenate fragments until a complete PI frame is present:

```
50 49 <type> <seq> ... <len_lo> <len_hi> <payload...>
                          ^ offset 8
```

Total frame = `10 + len`. The payload for OTA replies is
`06 <seq> 00 <data...>`; the data starts at `frame[13]`.

## 3. PA frames

Request (host → dongle):

```
50 41 <category> <seq> <len> <command...>
```

Reply (dongle → host):

```
50 49 <category-ish> <seq> ... <len16> <payload>
```

- OTA (category `0x06`): payload `06 <seq|0x80> <status> <data...>`, data at
  offset 13 of the frame; status `0` = OK.
- Link/radio (category `0x0e`): the event reply `50 49 0e ...`; data starts at
  frame offset 11.
- Boot-mode (separate `PX`/`px` layer): request `50 58 <cmd> ...`, response
  `70 78 <cmd> <len> <data...>`; used by `BootMode_Parser`. The OTA layer above
  is what the G435 RTK bridge uses.

## 4. OTA command table (category 0x06)

From `OTA_Commands.cs`:

| ID | Name | Notes |
| ---: | --- | --- |
| `0x22` | STORAGE_WRITE | |
| `0x23` | STORAGE_READ | `00 23 01 addr_le32 len_le32` |
| `0x24` | MORE_FRAME | fetch next data frame |
| `0x25` | IOCTL | sub-op in the next bytes |
| `0x30` | OTA_MODE | `00 30 <1/0>` |
| `0x31` | GET_BATTERY | reply `06 seq 00 mv_lo mv_hi` |
| `0x32` | GET_RSSI | four signed bytes: L/R AP, L/R PTP |
| `0x33` | GET_GRS_STATUS | status bits |
| `0x34` | GET_IMAGE_VERSION | |
| `0x35` | ERASE_LOAD_REGION | destructive |
| `0x36` | SET_LOAD_INFO | |
| `0x37` | LOAD_MORE | |
| `0x38` | CALC_LOAD_REGION_CRC | |
| `0x39` | UPDATE | destructive |
| `0x3a` | HANDOVER | |
| `0x3b` | GET_FILESYSTEM_TABLE | returns the WIXP partition header |
| `0x3c` | REBOOT | |
| `0x42` | REBOOT_AND_POWERON | |
| `0x43` | READ_MP_DATA_BY_ID | |
| `0x44` | WRITE_MP_DATA_BY_ID | |
| `0x45` | GET_ATB_VALUE | |
| `0x46` | READ_NVDS_DATA_BY_ID | |
| `0x49` | GET_FILESYSTEM_INDEX | |
| `0x40` | READ_CUST_INFO | |
| `0x3d` | CALC_LOAD_REGION_CRC32 | |
| `0xfe` | ECHO | |

Command bytes:

```
GET_BATTERY      50 41 06 SS 01 00 31
GET_RSSI         50 41 06 SS 01 00 32
GET_GRS_STATUS   50 41 06 SS 01 00 33
GET_IMAGE_VERSION 50 41 06 SS 04 00 34 <op> <sig0> <sig1>
READ_MAX_LEN     50 41 06 SS 0d 00 25 34 12 5a 5a 01 00 00 00 f0 00 00 00
STORAGE_READ     50 41 06 SS 0a 00 23 01 <addr LE32> <len LE32>
MORE_FRAME       50 41 06 SS 01 00 24
```

`READ_MAX_LEN` sets the per-request maximum (the vendor uses 240). On this
bridge a `MORE_FRAME` reply can deliver ~111 bytes; loop until the requested
length is collected. Address and length must be multiples of 4.

### Storage read algorithm

1. Enter bootloader.
2. `READ_MAX_LEN(240)`.
3. `STORAGE_READ(addr, len)`; wait for its ack PI.
4. Loop:
   - `MORE_FRAME`.
   - Read the PI frame; data = `frame[13:]`.
   - Append; stop at `len`.
5. Exit bootloader.

The PI length is the full requested length; the bridge may fragment the data
across several feature reports and several `MORE_FRAME` calls.

## 5. Link / radio commands (category 0x0e)

From `send_LQ_GET`, `send_REMOTE_*`, `send_TX_DG_*`:

| Command | Bytes | Reply |
| --- | --- | --- |
| GET_CONNECT_STATUS | `50 41 0e SS 01 e3` | event data at frame[11], bitmask |
| GET_MODE | `50 41 0e SS 01 e0` | `1` = remote, else local |
| SET_MODE | `50 41 0e SS 02 e1 <0/1>` | ack |
| BT_INQUIRY_SCAN | `50 41 0e SS 02 f0 <0/1>` | ack |
| CREATE_CONNECT_EX | `50 41 0e SS 04 e5 <mode> <profile> [bd_addr...]` | ack |
| DISCONNECT | `50 41 0e SS 02 e4 <flag>` | ack |
| GET_QUALITY | `50 41 0e SS 01 03` | link-quality event |
| GET_DEVM | `50 41 0e SS 01 05` | |
| GET_AFH_MAP | `50 41 0e SS 01 02` | |
| SET_AFH_MAP | `50 41 0e SS 01 01 ...` | |
| MONITOR_ENABLE | `50 41 0e SS 02 00 <0/1>` | |

`CREATE_CONNECT_EX` with `mode=0` takes a 6-byte BD_ADDR; `mode=1` (remote)
uses the current link.

Dongle audio control (`AUDIO_CTRL_SET_ACTION = 0x12`):

| Action | Value |
| --- | ---: |
| PAIR | 1 |
| EXIT_PAIR | 2 |
| CREATE_SERVICE_CONN | 3 |
| DISC_ALL_SERVICE_CONN | 4 |
| POWER_OFF | 5 |
| ENTER_DFU_MODE | 6 |
| ENTER_DUT_MODE | 7 |

## 6. Firmware containers

```
WIXP at 0x4   partition table (offsets)
0x1000        AWDC0 + IMGDATA / XRIMGDATA
0x2000        PXAT "NORMAL" or "patch"
0x6000        PXAT "patch"       (some variants)
0x51000       PXAT "NORMAL"      (some variants)
0x76000       IMGDATA            (some variants)
```

PXAT header: magic at 0, label at `0x10`, load address at `0x24` (LE32),
code offset at `0x28`, code size at `0x2c`; the code starts 0x30 bytes into
the declared offset (TLV metadata first).

## 7. Safety

`STORAGE_WRITE`, `ERASE_LOAD_REGION`, `UPDATE`, `REBOOT`, `HANDOVER`,
`WRITE_MP_DATA_BY_ID` and `WRITE_NVDS` are destructive. The reference driver
implements reads and control only.

## 8. Provenance

Decompiled with ILSpy from `AWToolLIB2` (SHA-256
`e109aac33d28de2d04004221826218144a5dd657091aef42abcde1ddee78ebdf`).
Firmware/build details in `docs/G435-FIRMWARE.md`.
