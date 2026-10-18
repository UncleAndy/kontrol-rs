# Changelog

## 0.1.0

First release: the Rust port of the original C tools, plus what the port adds on
top of them.

### The port

- `komplement`, the resident daemon: reads HID reports from the keyboard, turns
  its non-MIDI buttons into keystrokes through `/dev/uinput`, sends MMC over an
  ALSA sequencer port, and lights the LEDs over HID.
- `konfigure`, the offline generator for the SysEx that configures the
  keyboard's own knobs and buttons.
- The original's command line options, its `.map` mapping and `.pst` preset
  formats, its LED behaviour and its exit codes. A mapping written for the C
  tool works unchanged.
- The decision logic ("what does this HID report mean?") sits behind a `Sink`
  trait, so the tests feed it synthetic reports and check exactly which keys,
  MMC messages and LED updates come out - no device and no kernel module
  involved.

### Beyond the original

- **MCU mode**: the keyboard can appear to a DAW as a Mackie Control surface, so
  its transport, its buttons and its display are the DAW's own. `mcu = self`
  makes the tool a peer of the DAW's control-surface port instead of hiding
  behind `Midi Through`.
- **Knobs that move the DAW**: eight knobs, each bound with `knobN` to `volume`
  (the current track's level), `pan` or `master`, with `knobN_step` and
  `knobN_step_shift` for how far one detent goes. Volume and master travel as
  pitch bend; pan is sent to Reaper the way a V-Pot sends it, relative, with the
  direction in bit `0x40`.
- **The keyboard's own screen** carries a panel - track pages, their size, and
  the time in one of the usual formats.
- **A settings file**, `~/.config/komplement/config`, holding what used to be
  command line options. A value that cannot be used is reported and ignored
  rather than obeyed.

### Packaging and services

- A systemd unit for the whole machine and one for a single session, with udev
  rules that grant access to the keyboard and to `/dev/uinput`.
- `.deb` and `.rpm` from one description, a portable single file that unpacks
  and runs, and an AppImage, all built from the same flake output.
- NixOS and home-manager modules: the first installs the binaries, the udev
  rules and can run the system service; the second writes the user's config and
  runs the user service.

### Notes

- The original C tools are by @hvangalen; this is a port of them, and the MIT
  licence of the original is kept in `LICENSE`.
- Reaper's control surface behaviour in this release was measured against a
  running Reaper, not inferred from the MCU specification: the faders on channels
  one to eight, the master on the ninth, pan as a relative V-Pot value, the ring
  of twelve dots coming back, the display digits as CC 64..73 and the meters as
  channel pressure.
