# Codes measured on a Komplete Kontrol A49 (17cc:1740)

Measured with `tools/capture-a49.sh` (empty mapping, HID `-d` dump plus `amidi`
side by side), capture `kk-capture-20261016-193833`: 21 markers, 58 HID reports,
896 MIDI messages. Packet is 30 bytes; the button field is

    key_value = buf[1] | buf[2]<<8 | buf[3]<<16 | buf[4]<<24        (bits 0..31)

and bit *N* is button number *N* in the name table of `button_names.c`
(bit 0 = Shift). The reading loop stops at `TOTAL_HID_BUTTONS == 21`, so only
buttons 1..20 and the 4D dial (byte 28) are handled today.

## Rotary knobs: touch

The touch sensors are ordinary bits in the same mask, one packet per press and
one per release. Measured in the order the knobs were touched, 1..8:

| button | bit | byte | value |
|---|---|---|---|
| Rotary1 | 25 | buf[4] | 0x02 |
| Rotary2 | 26 | buf[4] | 0x04 |
| Rotary3 | 27 | buf[4] | 0x08 |
| Rotary4 | 28 | buf[4] | 0x10 |
| Rotary5 | 29 | buf[4] | 0x20 |
| Rotary6 | 30 | buf[4] | 0x40 |
| Rotary7 | 31 | buf[4] | 0x80 |
| Rotary8 | 32 | buf[5] | 0x01 |

`buf[5]` is not part of `key_value` at the moment, so Rotary8's touch is
invisible even after the loop bound is raised.

## 4D dial: directions and button

Directions are bits too (they are *not* the byte 28 position, which stayed 0x07
throughout the capture):

| button | bit | byte | value |
|---|---|---|---|
| 4D Up | 21 | buf[3] | 0x20 |
| 4D Right | 23 | buf[3] | 0x80 |
| 4D Left | 22 | buf[3] | 0x40 |
| 4D Down | 24 | buf[4] | 0x01 |

The bit numbers come from the order in which the directions were pressed
(up, right, left, down) and were then confirmed with a second run that pressed
one direction at a time: right reports 0x80 (bit 23) and left 0x40 (bit 22),
while the name table in `button_names.c` had the two the other way round. Up
(bit 21) and down (bit 24) matched from the start; the table has been corrected.

4D Button (bit 33, `buf[5]` 0x02) *did* appear once, in a single packet: it is
a normal button bit and is currently masked out together with the rest of
`buf[5]`. Easing the eight encoders in produced no packets at all - on the A49
the encoder push does not seem to be reported (in MIDI mode at least).

## The two buttons beside the display

Measured with `tools/capture-screen-buttons.sh` (HID `-d` and `amidi` side by
side), capture `kk-screen-buttons-20261017-021840`, first pass (display still in
MIDI mode): four prompts, 16 HID reports, and not one MIDI byte.

| button | bit | mask | byte | value | name in the table |
|---|---|---|---|---|---|
| left | 14 | 0x0000004000 | buf[2] | 0x40 | "Mute" |
| right | 15 | 0x0000008000 | buf[2] | 0x80 | "Solo" |

One packet per press and one per release, three presses per button, no repeats
and no other bit moving, so the two are told apart by a single bit each - which
is all that is needed to page a screen. The log follows the prompts exactly:
three presses of bit 14, three of bit 15, then bit 14 and bit 15 once each, then
nothing at all during the quiet step. **Nothing arrives over MIDI**, so these are
HID-only, like most of the keyboard's buttons; only knob *rotation* is MIDI.

The names are inherited, not measured: `button_names.c` (and this port after it)
calls bits 14 and 15 Mute and Solo, but no capture had ever pressed either on an
A49 - the earlier capture only pressed a known button as a control. Asked
directly, the owner confirms this keyboard **has no Mute or Solo buttons at all**,
so those two bits are the display's page buttons and the inherited names were
simply wrong for it. They are now called **Page Left** (bit 14) and **Page Right**
(bit 15), with `Mute`/`Solo` kept as aliases so mapping files written against the
old names still parse (`doc/Supported_Keys.md`), and the shipped mappings
(`mappings/reaper.map`, `mappings/rosegarden.map`) spell them the new way.

The second pass - the same sequence with the display taken over (`a0 03 04`) -
first produced no data at all, for a reason in the tool rather than in the
keyboard: `-x` was written into the usage text but left out of the parser's
option list, so `komplement` printed `Unknown option` and exited at once. Fixed,
and a test now walks the usage text and asks the parser about every option it
mentions (and the other way round); that test immediately found a second, older
divergence of the same kind - `-s` was documented but rejected by the parser,
while `-h` was accepted but never documented. With that fixed the pass ran and
came out **identical** to the first one: bits 14 and 15, one packet per press and
per release, nothing during the quiet step, nothing over MIDI. So who owns the
display does not change what these buttons report, and paging can be driven from
the HID path alone.

## Rotary knobs: rotation - MIDI, not HID

Rotation never shows up in the HID report: the eight knobs send absolute
Control Change messages on channel 1, one controller each, and the ranges walk
up and down while a knob is turned (0..74 observed, single steps):

| knob | MIDI |
|---|---|
| Rotary1 | CC 14 (0x0E) |
| Rotary2 | CC 15 (0x0F) |
| Rotary3 | CC 16 (0x10) |
| Rotary4 | CC 17 (0x11) |
| Rotary5 | CC 18 (0x12) |
| Rotary6 | CC 19 (0x13) |
| Rotary7 | CC 20 (0x14) |
| Rotary8 | CC 21 (0x15) |

Each knob appeared as one contiguous block, in knob order, which is what ties a
CC number to a physical knob. The values are absolute (0..127, one step per
detent), not relative, so a DAW can MIDI-learn them directly. Note that the
values are the knob's own position: turning a knob sends its new position, so
the parameter jumps to that position unless the DAW is told to take the value
relatively.

## What is quiet

Nothing at all was reported during the four second silence at the end of the
capture, and nothing came through for Button34..Button39 (bits 34..40): they
did not answer on this keyboard, consistent with the remark in
`doc/Supported_Keys.md`.

## What the tool does with this now

* `key_value` includes the payload's fifth byte, so bits 32..39 are visible;
* the reading loop walks all 40 button bits (`MAX_BUTTONS`) instead of 21; the
  LED report keeps its fixed 22 bytes, which is why the LED constant was left
  alone;
* `button_names.c` has 4D Left/Right the right way round;
* `-d` now prints the names of the buttons it sees, so a capture can be read
  without the bit table at hand;
* `tests/hid_dump_test.c` covers the fifth byte, short reports and the names
  (24 checks, run by the flake's checkPhase);
* `mappings/reaper.map` maps the 4D directions, 4D Button and the touch
  sensors; those entries used to be commented out.

Still open: the knobs' rotation. It is MIDI only, so either MIDI-learn the
parameters in the DAW from the keyboard's MIDI input, or teach the tool to read
MIDI input (it currently only creates an output port) and translate CC 14..21.

## What Reaper sends over MCU (measured)

`tools/capture-mcu.sh` recorded a Mackie Control Universal surface in Reaper
writing into `Midi Through`: 33920 events in 70 seconds, kept in
`kk-mcu-20261017-032907/` and summarised by `tools/mcu-report.awk`. The DAW's
output had to be listened to with `aseqdump` (a sequencer port - `amidi` sees
rawmidi hardware only); the events arrive with source `14:0`, i.e. out of the
Through port, as they should.

* **Track names are LCD writes**: `F0 00 00 66 14 12 <offset> <7 ASCII> F7`.
  The offsets are `0x00, 0x07, 0x0E, 0x15, 0x1C, 0x23, 0x2A, 0x31` - eight
  fields of exactly seven characters, one per channel of the bank. The capture
  contains `0 Lead`, `1 Back`, `2 Drum`, `3 Bass`, `4 Guit`, `5 Keyb`, `6 Perc`:
  the number Reaper prepends, then the name, padded with spaces.
* **A longer write at offset 0x00** (119 bytes, 17 times) carries Reaper's own
  status line - `REAPER! Initializing... Please wait...` - so the same command
  does a whole line and a single field, and the length says which.
* **Channel LEDs are notes**, one per channel, in four blocks: 0..7 record-arm,
  8..15 solo, 16..23 mute, 24..31 select. Measured as transitions: selecting the
  first track turns note 24 on, muting turns note 16 or 17 on, and so on; lit is
  note on with velocity 127, dark is note off.
* **Meters are channel aftertouch** (0xD0), values 0..127, one stream per
  channel and by far the most traffic in the capture (9800 events in the busiest
  step). A level display is built from these.
* **Transport and utility LEDs** are notes from 90..101 and 113..115 (94 = play,
  93 = stop, 95 = record appeared in the transitions), plus 74..78 and 86/89.
* **SysEx command 0x20** arrives eight times in a row as `00 03` .. `07 03`: one
  per channel, the same value every time. Not yet identified.
* **Controllers 48..55** (0x30..0x37) appear 19 times each with constant values -
  V-Pot ring modes, refreshed rather than changed.
* **Controllers 64..75 are the DAW's own display**, and it is a time display. The
  protocol says twelve seven-segment digits, **numbered from the right**: controller
  64 is the rightmost character, 65 the one left of it, and so on; the ten rightmost
  are the timecode and the two left of them are the assignment digits, which is why
  the capture always shows `01` before the time. Bit 6 (0x40) of the value is the
  dot of the segment, not part of the character, and a zero means the DAW turned
  that digit off - which is why a reading taken from the capture has holes in it
  (`01 25 1  000`). Measured behaviour, all of it consistent with a position rather
  than a clock face: nothing arrives while the DAW is stopped; while it played, the
  three rightmost digits moved fastest (they are collected in bursts, the counter's
  fine part) and one digit walked 1, 2, 3 - the beat; and the "position jump" step
  moved the coarse field from 11 to 25, which is what a jump of the playhead does
  and what no clock does. The old note here - "not a clock, because it changed four
  times in seventy seconds" - was reading a display that only *refreshes* when its
  digits change, which for a stopped DAW is never.
* **Faders are pitch bend**, which is why they are easy to miss: one message per
  channel - midi channels 1..8 for the eight strip faders and channel 9 for the
  master - with a signed 14-bit value (`(value + 8192) / 128` is the familiar
  0..127 position). All nine usually arrive together in a burst, and that burst
  is what the "Pitch bend x9" in every step of the report is; moving a fader
  loosens a smooth run instead (channel 4 walked 917, 1081, 1261, 1458 ... 4243
  while the third track's volume was moved with the mouse). So a fader position
  *is* available - it just is not a controller message.
* **Pan does not come through**: no CC 16..23 (the V-Pots) appear anywhere, so a
  pan display would have to be left out or asked for separately.
* **Nothing at all for eight seconds while the DAW was idle** (the quiet step),
  so there is no periodic heartbeat to design around: the screen has to redraw
  when something arrives, and can sit still otherwise.

### What the decoder does with it

`src/mcu.rs` reads this stream and fills the screen's state message by message;
`tests/mcu_capture.rs` feeds it a slice of the capture itself
(`tests/fixtures/mcu-reaper.txt`, cut out of the log with `tools/mcu-slice.awk`),
so the measurements above are what the tests assert rather than a comment about
them. Three questions were settled on the way:

* **The bank is eight channels.** The bank-change step writes new names - `8 Synt`,
  `9 Othe`, `Vocal` - into the cells that held `0 Lead`, `1 Back`, `2 Drum`, and
  clears the rest, which is what a bank with three tracks left looks like. Nothing
  in MCU says which project tracks the bank covers: the digits in those names are
  the owner's own text, not positions. `7 Strings` is a track whose name happens to
  begin with a digit - and the track it names is not the seventh of the project
  either, as the owner put it. So the eight is the *step* the surface takes when a
  bank changes, and a page may print the bank's own numbering, not a track number.
* **The LCD cells line up with the strip LEDs.** The names arrived at offsets
  7, 14 ... 49 - field *k* at offset 7*k* - and offset 0 carried Reaper's own
  announcement (112 bytes, 18 times) and once a name, `7 Stri`, which the owner
  identified as the start of the track name `7 Strings`. So the first cell is not
  special: Reaper writes the first strip's name into it, and its announcement takes
  the same place when it needs the row. The strip LEDs number the first channel
  zero - selecting the first three tracks lit notes 24, 25 and 26 - which is the
  same first channel. No off-by-one: the bank simply began at a track whose name
  reads like a number.
* **The display is the position**, and it needed the protocol to be read at all:
  twelve digits numbered from the right, the ten rightmost the timecode, the dot in
  bit 6 and a zero meaning "this digit is dark". What made it look unreadable in the
  capture was the reading, not the data - a change trace of a row the DAW updates one
  digit at a time looks like a random number, and the row is *refreshed* only when a
  digit changes, so a stopped DAW leaves it alone entirely. `Studio::position` holds
  the timecode and `Studio::display` the whole row, and both are asserted against the
  fixture.
