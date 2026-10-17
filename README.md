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

The port also has modules the C tool has no counterpart for, all of them for the keyboard's
display: `src/screen.rs` (frames, reports and the drawing primitives), `src/font.rs` (the 5x7
font), `src/font_medium.rs` (the 7x9 font), `src/studio.rs` (the state the screen shows and the
pages it has), `src/pages.rs` (those pages), `src/mcu.rs` (the decoder that fills that state from a
DAW's *Mackie Control Universal* output), `src/mcu_port.rs` (the ALSA sequencer port that decoder
reads from) and `src/nav.rs` (the earlier drafts of the same
screens, kept as `-S nav` states until the pages have been judged on hardware). `src/live.rs` puts the
two together - listen, decode, draw - as `komplement -X <port>`, and `doc/display-interface.md`
works out how a DAW is meant to feed them.

Two more that are not about the display: `src/settings.rs` (the tool's own settings file:
`vid`, `pid`, `uinput`, `mapping`, `mcu`) and `src/presence.rs` (finding the keyboard and the
DAW's port instead of being told, and saying so when that step is a guess).

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
`-o <uinput path>`, `-c <settings file>`, `-a` (no ALSA port), `-n` (no LED animation), `-q`
(quiet), `-d` (dump HID reports), `-l` (list the HID devices the system exposes and exit), `-s`
(probe the keyboard's display), `-x` (take the display over for a capture), `-S <state>` (draw one
named state), `-X <port>` (draw what a DAW reports over Mackie Control, live; `-X auto` searches
for the port), `-T <format>` (how to read the DAW's time display in `-X`), `-h`.
`konfigure` options: `-p <preset>`, `-o <file>`, `-m <alsa port>`, `-q`, `-h`.

`komplement -l` needs neither a mapping file nor the keyboard: it prints every HID device the
system exposes (the keyboard's vendor id first, the one matching `-v`/`-p` marked) with its USB
ids, interfaces and paths. Use it to find out which ids and how many interfaces the keyboard has
- on Linux hidapi does not fill in the interface number, so the path's last component is the one
to look at (`...:0003:0001:00` is interface 0).

`komplement -s -p 1740` probes the keyboard's display. It disables the keyboard's own display
logic (`a0 03 04`, as in `doc/kontrol_usb_re.cpp`) and then shows coarse states only - the whole
screen bright, the whole screen dark, upper half, lower half, left half, right half, each followed
by its opposite - asking a yes/no question after each one. Single pixels and thin lines are
invisible on this screen, so the layout is measured with states big enough to see; the last step
tries the text report guessed at in `doc/mini-screen.txt` on a cleared screen, checks that the
keys still arrive, and puts the device back into MIDI mode (`a0 07 00`). Without `-s` the display
is never touched.

The frame is 128x32 at one bit per pixel: two reports (`e0 00 00 <page> 00 80 00 02 00` + 256 bytes,
with `<page>` 0 and 2), one byte per column of eight pixels, bit 0 the top row of its page. The panel
is **inverted** - a set bit is a dark pixel, which is why all-zero payloads light the whole screen -
so `screen::report` complements the payload and frames in the code keep the natural convention (a set
bit is a lit pixel). Only those two offsets are meaningful: other values can leave the panel stuck
fully lit, and replugging the keyboard is what clears that.

`komplement -S <state> -p 1740` shows one named state and waits (Enter, or 10 s without a terminal),
so a state can be looked at or photographed: `on, off, top, bottom, left, right, vstripes, hstripes,
border, message, nav, navmed, navmid, navbig, page-transport, page-track, page-tracks, page-general,
glyphs, text60`. `message` draws four lines with the built-in 5x7 font (`src/font.rs`, 21 characters
per line, lines at pixel rows 0, 8, 16 and 24) and is the end-to-end check of the renderer; `nav`,
`navmed`, `navmid` and `navbig` draw the earlier drafts of the navigation screen for a sample track
(laid out in `src/nav.rs`); `glyphs` shows the whole medium font on the panel; `text60` is the text
protocol guessed at in `doc/mini-screen.txt` - it was measured on hardware and does nothing, so the
port does not depend on it.

The four `page-` states are the real screen pages (`src/studio.rs` for the state, `src/pages.rs` for
the layout), drawn from a sample `Studio` so they can be judged without a DAW: `page-transport`,
`page-track`, `page-tracks` and `page-general`. Each takes a glyph size after a colon -
`page-track:large`, one of `small` (four 5x7 lines), `medium` (three 7x9 lines) or `large` (two lines
at double scale) - because the size is what SHIFT plus a page button steps through on the panel. A
page says what it wants on the screen as a few lines, most important first, and the size decides how
many of them fit; the general page is the exception, drawn as a mixer of eight channels (label, mute
and solo marks, a fader filled from the bottom) and ignoring the size. How a DAW is meant to feed all
this - MCU for states, a FIFO for messages - is worked out in `doc/display-interface.md`.

The decoder can be seen on the panel before any daemon exists: `komplement -X <port>` listens to the
port a DAW's control surface writes to and draws one page of the state it reports, live. `<port>` is
an address (`14:0`) or a piece of the port's name (`"Midi Through"`, quoted) as `aconnect -l` shows it,
with the list printed when nothing matches. `-S page-track:large` picks the page and the glyph size
(default: the track page at medium). It takes the display over for the run and gives it back on the way
out, including on Ctrl+C; a frame the panel already shows is never sent twice, and nothing is ever sent
back to the DAW. The whole queue is drained into the state before anything is drawn, and the panel is
written at most twenty times a second: a DAW sends in bursts (nine pitch bends at once, hundreds of
aftertouch events a second while playing) and drawing per message runs the sequencer's queue over -
`ENOSPC`, which is where the first hardware run ended. It prints the last state - transport, position,
display, track - when the display goes back, and `-q` keeps even that quiet. All four pages are
reachable while it runs: the two page buttons turn the page and SHIFT with them changes the glyph
size, so the small and the large layout can be judged with the DAW playing. The FIFO, the message
overlay and the daemon itself are still to come.

The position on that screen is the DAW's *own* display, decoded from its twelve seven-segment digits
(controllers 64..75, numbered from the right, dot in bit 6): `Studio::position` is the ten-digit
timecode and `Studio::display` the whole row, holes included. **The two leftmost of those digits are
the number of the bank's first track** (`Studio::assignment`): measured on hardware, with the surface
on tracks 9..16 they read `09`, and nothing else in MCU carries an absolute track number at all - which
is why the tracks page can keep counting past the eighth track. Its meaning depends on the format the DAW
was told to send, and the row does not carry that format anywhere - so `-T h:m:s:f` names it
(`src/timecode.rs`; measured against the DAW's own transport, where `000324 00` is `00:03:24:00`), and
the default `-T written` draws the digits exactly as they arrived. Leading zeros are left out, so the
panel reads `3:24:00`, and a size with no room for the whole time gets its short form - the seconds,
with the minutes in front - so the large size still says `3:24`. So the track page leads with position and
transport, which is what has to survive the small size; the mute, solo and arm marks ride on the name
line instead of taking a line of their own.

The medium font (`src/font_medium.rs`, 7x9, sixteen characters per line) is authored as art in
`tools/font-medium-glyphs.txt` and generated into Rust by `tools/font-medium-table.awk`, which also
prints a sheet of the whole font as the panel will show it.

Controls that are not handled yet are found by capture rather than by guessing:
`tools/capture-a49.sh` walks the encoders, the 4D dial and the pushes, and
`tools/capture-screen-buttons.sh` covers the two buttons beside the display (a HID bit, a MIDI
controller, or silent), twice - once with the display in MIDI mode and once with it taken over by
`komplement -x`, which is the state a screen daemon runs in. Both write HID and MIDI logs with
matching timestamps; `doc/Captured_Codes.md` holds what they have measured so far, and
`doc/Supported_Keys.md` what is therefore mappable.

`tools/capture-mcu.sh` is the same idea for the other side of the interface: it listens to the port a
DAW's *Mackie Control Universal* surface writes to (with `aseqdump`, because a DAW's output is an ALSA
sequencer port and `amidi` only reads rawmidi hardware) and walks through play/stop, track selection,
mute/solo/arm, volume, click and loop, bank changes, a rename, a tempo change and a quiet period,
stamping every event on the same clock as the step markers.

One such log has been taken, and `tools/mcu-report.awk` files every event of it under the guided step
it happened in. `tools/mcu-slice.awk` cuts a few interesting seconds out of it as raw MIDI bytes; that
is what `tests/fixtures/mcu-reaper.txt` holds and what `tests/mcu_capture.rs` feeds to `src/mcu.rs`, so
the decoder is checked against the DAW's own stream. Every opcode in the decoder was measured this way -
none of them came from documentation.

The listening path itself is checked too, where it can be: `tests/mcu_port_roundtrip.rs` starts a
writer port and the listening port in one process and asserts that a mute LED, a fader and a name
field arrive at the decoder through ALSA unchanged. It needs `/dev/snd/seq`, so it does its work on a
machine with a sequencer and says so instead of failing on one without.

The keyboard needs read/write permission for the user running `komplement`, and `/dev/uinput`
needs write permission for sending keystrokes. There is a NixOS module for that (below); the C
project's `README.md` and `udev/55-komplete-kontrol.rules` describe the manual route.

Do it with **group rights, not ACLs**: `komplement` opens `/dev/uinput` read-write, so an ACL
entry left by `setfacl -m u:<user>:w` (write only, no read) makes the open fail with
`Permission denied` even though `ls -l` shows `crw-rw---- root audio`. Check with
`getfacl /dev/uinput`, or better by trying the access itself
(`[ -r /dev/uinput ] && [ -w /dev/uinput ]` - with an ACL present the mode's group field shows the
mask, not the group's rights, so `ls -l` is misleading). Clear leftovers with
`sudo setfacl -b /dev/uinput && sudo chmod 660 /dev/uinput`; the udev rule
(`GROUP="audio"`, `MODE="0660"`) gives the group everything that is needed.

## Settings file

`komplement` reads one settings file, and what it says is used where the command line said
nothing - a flag always beats the file, never the other way round:

```sh
komplement -m mappings/reaper.map     # everything else is found by itself
komplement -c /etc/komplement/config  # or name the file yourself
```

Without `-c` the file is `$XDG_CONFIG_HOME/komplement/config`, usually
`~/.config/komplement/config`. It being absent is normal (a fresh install runs on the defaults);
a path given with `-c` that is not there is an error, because that is a typo and not a default.
One `key = value` per line, `#` starts a comment, and a value may be quoted to hold a space:

```text
# the keyboard: the A25 is 1730, the A49 is 1740
pid = 1740
# the port the DAW's control surface writes to (see -l and -X)
# mcu = Midi Through
# mapping = /home/andy/projects/RUST/kontrol-rs/mappings/reaper.map
# uinput = /dev/uinput
```

A key that is not known, or a value that does not parse, is *reported and skipped* and the
default stays in place: a service that quietly ignores half its settings is worse than one that
refuses to start.

The keyboard and the DAW's port are both *found*, not passed in:

- **the keyboard** - `-v`/`-p` (either of them pins the pair), else the settings file, else the
  only device of the family this tool knows (`1730`, `1740`). One keyboard shows up as several HID
  interfaces, and several *interfaces* are not several *keyboards*; if several models really are
  attached, the first is used **and that is said out loud**, because a guess nobody can see is a
  guess nobody can correct. Nothing attached is an error that lists what was seen and how to name
  a model this list does not know yet (`pid = ...`).
- **the DAW's port** (`-X`) - the argument first (an address like `14:0`, or a piece of a name);
  if that finds nothing, or with `-X auto`, the *only* port that can be the surface is taken:
  never our own port, never ALSA's own clients, never the keyboard's own MIDI port. If several
  could be it, the settings file decides (`mcu = ...`), and failing that the ports are listed
  together with the line that pins the answer.

This is the *finding* half of the service that is planned; the service itself - one program with
keystrokes and panel, the panel in a task of its own that is restarted when it fails - is the
next step, and `doc/display-interface.md` keeps that plan.

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
* Diagnosing a failure is deliberately different: the C tool prints "Opening the HID device
  ... failed" both when the device cannot be opened *and* when it opens but LED writes are
  refused, which hides the cause. Here the two are separate messages, the reason hidapi gave
  is included, and a failed open also lists the devices the HID layer can see with that vendor
  id - so "not present / wrong `-p` mode" is told apart from "present but held by something
  else".
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
