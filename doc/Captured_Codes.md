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
