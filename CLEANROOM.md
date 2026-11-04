# Clean-room process log

## Roles

| Role | Who | Responsibilities |
| --- | --- | --- |
| Analysis | analysis agent | device observation, firmware analysis, frozen `docs/PROTOCOL.md` |
| Implementation | separate implementation agent | all Rust implementation code in this repository |

## Inputs available to the implementation agent

- `docs/PROTOCOL.md` (frozen functional specification).
- The repository skeleton and public interface requirements.
- The real device (`046d:0acb`) for observation and tests.

## Explicitly denied to the implementation agent

- The decompiled vendor SDK (`AWToolLIB2` / `/tmp/g435/awtool_src`).
- The decompiled firmware (`analysis/` in the private firmware repository).
- Vendor firmware images, the vendor updater, and any vendor source.

## Review and corrections

The analysis agent reviewed the implementation for build/API correctness and
protocol conformance and corrected defects. Corrections were limited to
defects and interface consistency; no vendor code was copied into this
repository. Corrections are logged below as they occur.

## Correction log

- (pending implementation)

## Sources

- The frozen protocol specification.
- The physical device and its public USB/HID behavior.
- Public prior work, used for background only (endes G435 write-up,
  `deddroid/logitech-g435-battery`, `leag/razer-barracuda`).

## Limitations

This is a documented process separation, not a legal guarantee. Nothing here
is legal advice.
