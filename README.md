# kontrol-rs

Rust port of [komplementary-kontrol](../komplementary-kontrol) - the utilities that make the
non-MIDI controls of a Komplete Kontrol A-series keyboard (A25/A49/A61) usable under Linux.

Both tools of the original are here, with the same command line options, the same mapping
(`.map`) and preset (`.pst`) file formats, the same HID protocol handling, the same LED
behaviour and the same exit codes:

| binary | what it does |
|---|---|
| `komplement` | resident daemon: reads HID reports from the keyboard, turns buttons into keystrokes through `/dev/uinput`, sends MMC over an ALSA sequencer port, lights the LEDs over HID |
| `konfigure` | offline generator for the SysEx that configures the keyboard's knobs and buttons |

A mapping file written for the C tool works unchanged with this one.

## Module map

The port keeps the C file structure, so the two implementations can be read side by side:

| Rust | C |
|---|---|
| `src/buttons.rs` | `button_names.c` |
| `src/keys.rs` | `uinput_stuff.c` (the key table, generated from it) |
| `src/mmc.rs` | `mmc_stuff.c` |
| `src/mapping.rs` | `mapping.c` |
| `src/config.rs` | `config.c` |
| `src/hid.rs` | `hid.c` |
| `src/hid_dump.rs` | `hid_dump.c` |
| `src/leds.rs` | `button_leds.c` |
| `src/uinput.rs` | `uinput_stuff.c` (the virtual device) |
| `src/alsa_seq.rs` | `alsa.c` |
| `src/preset.rs` | `konfigure_parser.c` + the SysEx layout of `konfigure.c` |
| `src/engine.rs` | the main loop of `komplement.c` |
| `src/bin/komplement.rs` | `komplement.c` |
| `src/bin/konfigure.rs` | `konfigure.c` |

`src/engine.rs` is the one structural difference: the decision logic ("what does this HID
report mean?") is separated from the device I/O behind a `Sink` trait, so the tests feed it
synthetic reports and check exactly which keys, MMC messages and LED updates come out - no
keyboard required. `komplement`'s main loop is a thin wrapper around it.

## Building

With NixOS (or any machine with Nix):

```sh
nix develop --command cargo build --release   # rustc, cargo, gcc, ALSA/hidapi/libusb on PKG_CONFIG_PATH
nix build                                     # ./result/bin/{komplement,konfigure} + share/kontrol-rs
```

Plain `cargo build` also works where `pkg-config`, `alsa-lib`, `hidapi` and `libusb-1.0` are
installed.

## Running

```sh
# Komplete Kontrol A49 (USB 17cc:1740), Reaper mapping:
disown -h  # in a shell you close afterwards
komplement -m mappings/reaper.map -p 1740 -o /dev/uinput

# the mapping file is read once at start-up, so restart the tool after editing it
konfigure -q -p presets/basic_cc.pst > my_template.syx   # send it to the keyboard as documented
```

`komplement` options: `-m <mapping>` (required to be useful), `-p <productId>`, `-v <vendorId>`,
`-o <uinput path>`, `-a` (no ALSA port), `-n` (no LED animation), `-q` (quiet), `-d` (dump HID
reports), `-h`.
`konfigure` options: `-p <preset>`, `-o <file>`, `-m <alsa port>`, `-q`, `-h`.

The keyboard needs read/write permission for the user running `komplement`, and `/dev/uinput`
needs write permission for sending keystrokes. There is a NixOS module for that (below); the C
project's `README.md` and `udev/55-komplete-kontrol.rules` describe the manual route.

## Mapping files

One line per control, `#` starts a comment, `Shift+` selects the second layer:

```
Play=Space                       # a keystroke
Shift+Play=MMC_Play              # an MMC message (needs the ALSA port)
4D CW=LeftCtrl,LeftShift,Right   # up to four keys, sent in this order
```

Button and key names are matched case-insensitively; button names may be abbreviated
(`Rotary` matches `Rotary1`), which the C tool allows too.

Without SHIFT the LEDs light up for every button that has a mapping in the normal layer
(plus SHIFT and the octave buttons, which are always on). Holding SHIFT repaints them for the
shifted layer, with SHIFT itself bright.

## Verification

Everything in `komplement` beyond the device I/O is covered by tests, and the parts that share
an interface with the C tool were checked against it with its own data files:

```sh
nix develop --command cargo test        # 22 tests: parser, SysEx, HID state machine, HID dump
```

* parsing `mappings/reaper.map` prints the same 64 lines (62 mapped rows) as the C tool's
  `komplement -m mappings/reaper.map` - `diff` is empty;
* `konfigure -q -p presets/basic_cc.pst` produces a byte-identical 536 byte SysEx stream;
* the LED report, button bits, 4D dial stepping (including the 0/15 wrap), the SHIFT layer
  switch and the release-on-SHIFT-release behaviour follow the C code, including the cases
  where it is quirky (see the comments in `src/engine.rs`).

What is *not* covered by tests: actual USB traffic and LED behaviour on hardware (no keyboard
was attached while porting).

## Differences from the C tool

* `-o <path>` is accepted but has no effect: the virtual keyboard is created with the `evdev`
  crate, which always uses `/dev/uinput`. Symlink or bind-mount that path if your system puts
  it elsewhere.
* A row with more than four keys is capped and reported (`more than 4 keys, ignoring ...`)
  instead of writing past the fixed-size array the C tool has.
* Report/usage text is the same, except that the wording of a missing mapping file says "nor
  in the mappings folder" only in the nix build, where that folder exists.

## NixOS module

```nix
{
  inputs.kontrol-rs.url = "path:/path/to/kontrol-rs";

  # in your configuration:
  imports = [ inputs.kontrol-rs.nixosModules.default ];
  services.kontrol-rs.enable = true;   # package, udev rules for 17cc:1730/1740, uinput module
  users.users.<you>.extraGroups = [ "audio" ];
}
```

Options: `enable`, `package`, `group` (default `audio`), `productIds` (default `1730`, `1740`),
`installUdevRules`, `loadUinputModule`.

## License

`LICENSE` in this repository is the original MIT one (© Andrey Velikoredchanin). Note that the
code is a port of
[komplementary-kontrol](https://github.com/fsphil/komplementary-kontrol), which is
GPL-3.0-or-later, so redistribution of the derived work has to respect GPL-3 as well - decide
which of the two should apply before publishing.
