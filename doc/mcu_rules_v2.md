# SYSTEM SKILL: Expert Mackie Control Universal (MCU) Protocol Developer

You are an expert embedded firmware and audio software architecture engineer. Your role is to write
production-grade, latency-critical, memory-safe applications that talk to DAW software or hardware
control surfaces using the Mackie Control Universal protocol.

**Where the facts below come from.** Three sources, and they are marked line by line:

- **[measured]** - seen in a captured MIDI log of Reaper driving a real surface (this project, see
  `doc/Captured_Codes.md`). Trust these most.
- **[TouchMCU]** - the reverse-engineered protocol reference in the `TouchMCU` project
  (`doc/mackie_control_protocol.md`), written from original Mackie hardware and the Logic Control
  documentation that preceded the MCU.
- **[spec]** - stated by the specification or by TouchMCU but not verified against a running DAW here.
  Treat as probable, not as fact.

## 1. Architecture & Framing

The MCU protocol is MIDI 1.0: bidirectional, one port is enough, and every interaction is either a
channel message or a System Exclusive. **[TouchMCU]**

- SysEx header: `F0 00 00 66 dd F7`, where `00 00 66` is Mackie Designs' manufacturer id and `dd` is
  the device id.
- Known device ids **[TouchMCU]**: `0x05` Mackie HUI (also BabyHUI, D8B) · `0x10` Emagic Logic Control
  · `0x11` Logic Control XT · `0x14` **Mackie Control (Universal/Pro) - the one to emulate** ·
  `0x15` MCU XT · `0x7F` firmware update mode.
- Running status is used by real hardware: the status byte is dropped when it repeats. A parser must
  track and re-apply the last status byte. Never confuse running-status bytes with data bytes.
  **[TouchMCU]** (and our parser does exactly this **[measured]**)

## 2. Faders (Channels 1-8 and Master)

- Position travels as **Pitch Bend** (`0xE0`..`0xE7` for strips 1-8, `0xE8` for the Master fader -
  that is MIDI channel 9, low nibble `8`). **[TouchMCU]**
  Confirmed by capture: the master arrived as `e8 52 6b`. **[measured]**
- 14-bit value, `(MSB << 7) | LSB`, range `0`..`16383` (`0x3FFF`). To send: `LSB = v & 0x7F`,
  `MSB = (v >> 7) & 0x7F`. **[TouchMCU]**
- The hardware fader itself is 10-bit (Penny & Giles on the original, ALPS from v4.0.0) while the
  MIDI word gives it room to spare: do not assume the two resolutions are the same thing.
  **[TouchMCU]** Whether the low bits are ever non-zero is **[spec]** - our capture cannot tell.
- **Fader touch** is a Note Bang, not a controller: strips `0x68`..`0x6F` (104-111), master `0x70`
  (112). Velocity >= 1 means touched, 0 means released. **[spec]**
- **Fader fighting guard:** while a fader reads as touched, suppress that strip's inbound position
  messages from the DAW, or the motor and the hand will fight. **[TouchMCU]**

## 3. V-Pots (Rotary Encoders 1-8)

### Rotation (surface -> DAW)

Control Change `0x10`..`0x17` (CC 16-23), one per strip. **[TouchMCU]**

The value is **relative**, not a position, and it is a six-bit magnitude with the seventh bit as a
sign **[TouchMCU]**:

- `0b00000001` (`0x01`) - clockwise, one step.
- `0b01000001` (`0x41`) - counter-clockwise, one step.
- All zero (`0x00`) - no movement at all.

TouchMCU leaves the magnitude question open ("I was unable to test it"). We can answer it: a `0x3F`
sent to Reaper moved the pan the full width in one turn - that is **63 steps in the clockwise
direction**, so the magnitude really does occupy the low six bits. **[measured]**

This is worth stating plainly because it is easy to get wrong: an absolute encoding (value = pan
position, 64 = centre) looks plausible and does not work. **[measured]**

### LED Ring (DAW -> surface)

Control Change `0x30`..`0x37` (CC 48-55), one per strip. **[TouchMCU]**

| bits | meaning |
|---|---|
| `b0`-`b3` | Value: ring position, `0`..`0x0B` (0-11) |
| `b4`-`b5` | Mode: which pattern the ring draws |
| `b6` | the small LED underneath the encoder, on or off |
| `b7` | always 0 |

The four modes, from the table in TouchMCU: `0b00` one dot at the position (pan) · `0b01` boost/cut,
lit from the centre outwards like an EQ · `0b10` fill from one end up to the position · `0b11`
inside/out, symmetric about the centre.

Capture agrees on the common case: Reaper sent plain `0x00`..`0x0B` - mode 0, one dot - with `6` at
the centre of a strip. **[measured]** All our captures were mode 0, so the mode bits are
**[TouchMCU]** only.

### Click

A Note Bang on `0x20`..`0x27` (32-39), one per strip. **[TouchMCU]**

## 4. Buttons and LEDs (Note Bang)

Every button and LED uses a **Note Bang**: a Note On immediately followed by the matching Note Off.
The velocity of the *Note On* carries the state **[TouchMCU]**:

| State | Velocity |
|---|---|
| LED off / button released | `0x00` (any even value) |
| LED blinking | `0x01` (any odd value) |
| LED solid / button pressed | `0x7F` (127) |

A surface sends a bang when a button is pressed; the DAW sends the same bang back to light the LED.
The Note Off half exists so a host never has to track held notes indefinitely. **[TouchMCU]**

One deviation worth knowing: our capture of Reaper's own output shows releases as a bare Note Off
with velocity `0x40` - `80 5D 40`, `80 5F 40` - not the `0x00` above. **[measured]** Accept Note Off
regardless of velocity.

### Note map

Strips 1-8: Record Arm `0x00`-`0x07` · Solo `0x08`-`0x0F` · Mute `0x10`-`0x17` · Select
`0x18`-`0x1F` · V-Pot click `0x20`-`0x27`. **[TouchMCU]**, and the transport notes below are also
**[measured]**.

```python
MATRIX_MAP = {
    "REC_ARM":    [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07],
    "SOLO":       [0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F],
    "MUTE":       [0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17],
    "SELECT":     [0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F],
    "VPOT_CLICK": [0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27],

    "ASSIGN_TRACK": 0x28, "ASSIGN_SEND": 0x29, "ASSIGN_PAN": 0x2A,
    "ASSIGN_PLUGIN": 0x2B, "ASSIGN_EQ": 0x2C, "ASSIGN_DYN": 0x2D,

    "BANK_LEFT": 0x2E, "BANK_RIGHT": 0x2F, "CHAN_LEFT": 0x30, "CHAN_RIGHT": 0x31,
    "FLIP": 0x32, "GLOBAL_VIEW": 0x33,

    "F1": 0x36, "F2": 0x37, "F3": 0x38, "F4": 0x39,
    "F5": 0x3A, "F6": 0x3B, "F7": 0x3C, "F8": 0x3D,

    "SHIFT": 0x46, "OPTION": 0x47, "CONTROL": 0x48, "ALT": 0x49,

    "AUTO_READ": 0x4A, "AUTO_WRITE": 0x4B, "AUTO_TRIM": 0x4C,
    "AUTO_TOUCH": 0x4D, "AUTO_LATCH": 0x4E,

    "SAVE": 0x50, "UNDO": 0x51, "CANCEL": 0x52, "ENTER": 0x53,

    "MARKER": 0x54, "NUDGE": 0x55, "CYCLE": 0x56, "DROP": 0x57,
    "REPLACE": 0x58, "CLICK": 0x59, "SOLO_ALONE": 0x5A,
    "REWIND": 0x5B, "FAST_FORWARD": 0x5C, "STOP": 0x5D, "PLAY": 0x5E, "RECORD": 0x5F,

    "UP": 0x60, "DOWN": 0x61, "LEFT": 0x62, "RIGHT": 0x63, "ZOOM": 0x64, "SCRUB": 0x65
}
```

Cross-check against the capture: Play = `90 5E 7F`, Stop = `80 5D 40`, Record = `80 5F 40` - the
three notes above, to the byte. **[measured]** The LED notes are the same addresses: Loop is `86`
(`0x56`, "CYCLE") and Click is `89` (`0x59`). **[measured]**

The Jog Wheel turns on CC `0x3C` (60) with the same relative encoding as a V-Pot. **[TouchMCU]**

## 5. Metering

One meter per strip, sent as **Channel Pressure** (`0xD0`). **[measured]**, structure **[TouchMCU]**:

- `0xD0 sv` - the *data* byte carries both: `s` is the strip in the **high nibble** (0-7), `v` is the
  level in the **low nibble**.
- Do not read the strip from the status byte's channel: Reaper sends every meter as `0xD0` with the
  strip only in the data. A capture of the initialisation is `d0 0f, d0 1f, d0 2f … d0 7f` - eight
  strips, each `0xF`, which is "clear overload". **[measured]**
- Levels: `0x0` silent … `0x4` (-30 dB) … `0x6` (-14 dB) … `0x8` (-8 dB) … `0xB` (>= -2 dB) …
  `0xC` (0 dB / clip) · `0xE` set overload · `0xF` clear overload. **[TouchMCU]**
- Decay is automatic, about 300 ms per division. **[TouchMCU]**
- The same message drives the signal-present LED. **[TouchMCU]**

*Implementing this project:* `src/mcu.rs` currently takes the strip from the status byte and treats
the whole data byte as the level, which does not match the passage above - worth fixing before the
meters are trusted.

## 6. Displays

### Timecode / 7-segment digits

Twelve digits, each one 7-segment plus a dot, driven by **Control Change** `0x40`..`0x4B` (CC 64-75),
counted from right to left. **[TouchMCU]**, and matching our capture, where the digits arrived as
`b0 4a 31` and `b0 4b 30` - the characters '1' and '0'. **[measured]**

- The value is the ASCII character; **bit 6 is the dot**, so characters `0x40`..`0x5F` lose that bit
  on the way out and are not out of range, just dotted. **[TouchMCU]**

Two traps here:

- The Logic Control predecessor drove its timecode and assignment displays with SysEx `0x10` and
  `0x11`. On the MCU those commands are marked "KO, LC only?" in TouchMCU - they do not work, and the
  CC route above is the one that does. An implementation built on `0x11` will look correct and show
  nothing. **[TouchMCU]**
- The note that "the fader is 10-bit" and the digits both use the number ten; they have nothing to do
  with each other.

### LCD

A 2x55 character display behind a 2x56 buffer: 112 characters of which two (one at the end of each
line) are hidden line-wrap. Written by SysEx **[TouchMCU]**:

```
F0 00 00 66 14 12 pp cc cc .. cc F7
```

- `pp` - the offset of the first character, `0x00`..`0x37` for the upper line, `0x38`..`0x6F` for the
  lower. Up to 112 data bytes in one message.
- The upper line is divided into per-channel fields of **seven** characters, at offsets 0, 7, 14 …
  which is the layout a track name is written in. **[measured]**, and Reaper opens with
  `F0 00 00 66 14 12 00 52 45 41 50 45 52 21 …` = "REAPER!" starting at offset 0. **[measured]**

## 7. SysEx Commands

Header `F0 00 00 66 dd`, `dd = 0x14` for an MCU. Directions are from the device's (surface's) point
of view. **[TouchMCU]**

| Code | Parameters | Len | Direction | Command |
|---|---|---|---|---|
| `0x00` | | 0 | received | Device Query |
| `0x01` | `ss ss ss ss ss ss ss cc cc cc cc` | 11 | transmitted - at power-on and in reply to Device Query | Host Connection Query |
| `0x02` | `ss ss ss ss ss ss ss rr rr rr rr` | 11 | received | Host Connection Reply |
| `0x03` | `ss ss ss ss ss ss ss` | 7 | transmitted | Host Connection Confirmation |
| `0x04` | `ss ss ss ss ss ss ss` | 7 | transmitted | Host Connection Error |
| `0x0A` | `tt` | 1 | received | Transport button click (0 = none, 1 = click) |
| `0x0B` | `ll` | 1 | received | LCD backlight saver (0 = off, 1..0x7F = minutes) |
| `0x0C` | `ff` | 1 | received | Touchless movable faders (0 = only if touched, 1 = always) |
| `0x0E` | `ii ts` | 2 | received | Faders touch sensitivity (`ts` 0..5, default 3) |
| `0x0F` | `0x7F` | 1 | received | Go offline (Logic Control only?) |
| `0x10` | `yy ..` | 1-10 | received | Update TC/BBT display (Logic Control only?) |
| `0x11` | `yy [yy]` | 1-2 | received | Update assignment display (Logic Control only?) |
| `0x12` | `oo yy .. [yy]` | 2-113 | received | **Update LCD** |
| `0x13` | `0x00` | 1 | received | Firmware version request |
| `0x14` | `vv vv vv vv vv` | 5 | transmitted | Version reply (e.g. `V1.0`) |
| `0x18` | `xx ..` | | received | Firmware update (undocumented) |
| `0x20` | `ii mm` | 2 | received | **Channel meter mode** |
| `0x21` | `gg` | 1 | received | Global LCD meter mode (0 = horizontal, 1 = vertical) |
| `0x61` | | 0 | received | Faders to minimum (LC only?) |
| `0x62` | | 0 | received | All LEDs off (LC only?) |
| `0x63` | | 0 | received | Reset |

- `ss` - serial number, 7 bytes of ASCII, not null-terminated.
- `cc` / `rr` - a 4-byte challenge and its response; the algorithm is written out in TouchMCU
  ("Response code algorithm"). The device verifies the response and answers `0x03`, or `0x04` on
  failure.
- `ii` - fader strip id, `0x00`..`0x07`, master `0x08`.
- `mm` - meter mode bit map `0b00000lps`: `l` level meter on the LCD, `p` peak hold, `s` signal LED.

Worth knowing: Reaper's startup in our capture is `F0 00 00 66 14 20 00 03 F7` for strips 0 through 7
- command `0x20`, strip `ii`, mode `0x03` = `l` and `p` set, `s` clear. That is "level meter with peak
hold", and it matches the table above byte for byte. **[measured]** + **[TouchMCU]**

## 8. Handshake

The full dance, device's point of view **[TouchMCU]**:

1. Host sends **Device Query** `0x00`.
2. Device answers **Host Connection Query** `0x01` with its 7-byte serial and a 4-byte challenge -
   also unprompted at power-on.
3. Host replies **Host Connection Reply** `0x02` with the serial and its 4-byte response.
4. Device answers **Host Connection Confirmation** `0x03`, or **Host Connection Error** `0x04` - and
   on the real hardware the latter puts "SECURITY UNLOCK FAILED SHUTTING DOWN" on the display and it
   stops accepting messages entirely.

*Implementing this project:* Reaper does none of the above - no `0x00`, `0x01` or `0x02` appears in
the capture at all - so the surface works without it. Another DAW may ask, and nothing here answers
yet. When implementing it, note the direction: `0x00` is what the *host* sends, and it is the device
that speaks first with `0x01`.

## 9. Operational Requirements for Generated Code

1. **Framing:** track running status and re-apply the last status byte; keep 7-bit data and status
   bytes strictly apart. **[TouchMCU]**
2. **Threading:** MIDI I/O must not share a thread with the UI or the main loop.
3. **Throttling:** display and ring updates should be rate-limited (a few tens of Hz) - vintage
   controllers and virtual serial queues choke otherwise. **[TouchMCU]**
4. **Fader fighting:** drop inbound fader positions for a strip that reads as touched. **[TouchMCU]**
5. **Meters are one byte of two nibbles, not two messages** - see section 5. **[measured]**
