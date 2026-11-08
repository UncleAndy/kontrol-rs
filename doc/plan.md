# Plan: pages, knobs and plugin screens

Ordered work, not a wish list: each item says what changes, why, where in the code, and
when it counts as done. Item 1 is first because the owner asked for it, and because doing
it before the frame renderer means the renderer is not written twice.

## 1. Drop the size cycling

**What.** Remove `Size` (Small / Medium / Large) as a user-visible setting. Every page has
one fixed layout from now on.

**Why.** The layouts in `doc/layouts/screens/` are fixed 128x32 compositions: a header rule
at y=10, a big value at scale 2, a bar along the bottom. A second and third text size cannot
be laid over that, and SHIFT + a page button currently cycles sizes, which would break the
picture on exactly the pages that matter.

**Where.** `src/studio.rs` (`Size` and its `next`/`previous`/`line_*`), `src/live.rs`
(`Paging.size`, the SHIFT branch at ~125-131, `page_and_size` and the `-S page-track:large`
form), `src/pages.rs` (`render`, `lines_at`, `line_characters`, `line_row` — about 96 uses of
`Size::` across the tree), `src/panel.rs` (`Job.size`), tests that assert per-size rows.

**Done when.** `Size` is gone from the model; `-S` takes a page and nothing else; the pages
render at one fixed layout; the page tests assert the frame, not row counts.

## 2. A frame renderer for pages

**What.** Render pages as absolute-coordinate frames, the way `doc/layouts/render.py` composes
the mockups, instead of the current left/right text lines.

**Why.** The mockups need a header rule, right-aligned tags, inverted badges, a bipolar bar
centred at 64 and an eight-band grid at 15 px pitch. The line model cannot express any of it.

**Where.** `src/pages.rs` — `render` already hands `Page::General` to `mixer()` (a hand-drawn
frame), so the mechanism exists; new pages follow that shape. `src/screen.rs` already has
`draw_text`, `draw_text_scaled`, `draw_bar`, `fill_rect` and raw frame access.

**Missing pieces.** Inverted text (filled badge with the glyphs cut out, for SOLO / MUTE),
small transport icons (triangle, two bars, circle — buildable from rectangles at 7x7), and a
bipolar bar variant of `draw_bar`.

**Done when.** Page 1 is drawn from the mockup coordinates and pixel-compared against
`01_main_play.png` (the render script asserts 128x32 one-bit, so the comparison is exact).

## 3. Page 1 in mockup form

**What.** The main page as `01_main_*` shows it: track name at (2,1), state tag and transport
icon top right, timecode at scale 2, SOLO / MUTE badges, and the variants `02_volume`,
`03_master`, `04_pan` for whichever control the knobs are on.

**Where.** `src/pages.rs`, from `Studio` fields already present (`track_name`, `position`,
`transport`, `volume_fraction`, `bank[].pan`, `master_fader`).

**Done when.** Each state has a frame matching its mockup, and the tests check the distinguishing
pixels (rule line, badge fill, bar extent).

## 4. The EQ page: eight knobs, eight bands

**What.** Page 2 shows `05_eq8` and puts the eight knobs on the eight band gains. Entry sends the
MCU `EQ` button bang (note `0x2C`), exit sends `PAN` (`0x2A`), so the DAW does not stay in plugin
mode while the main page is showing.

**Why the DAW's own mode.** MCU has no separate "EQ protocol": the eight V-Pots are always
`CC 16..23` relative, and the assignment buttons change what the DAW does with them. Our
`pan_relative_message` already emits exactly those controllers, so the send path is reused,
not duplicated.

**Where.** `src/mcu_buttons.rs` (add `ASSIGN_PAN`, `ASSIGN_PLUGIN`, `ASSIGN_EQ`, `ASSIGN_DYN`
to the name table), `src/live.rs` (the knob branch at ~354-410 becomes page-aware:
`KnobAction` gains an `EqBand(n)` case), `src/studio.rs` (a V-Pot mode field).

**Requirement on the plugin.** The first eight parameters must be the eight band gains, in
band order. ReaEQ cannot do this (its parameters come in Freq / Gain / Q triplets), which is
why item 8 exists.

**Done when.** Turning knob N moves band N's gain in the DAW, and on leaving the page the
main page's knobs control pan again.

## 5. Parameter feedback, done properly

**What.** Two separate caches instead of one: track names and plugin parameter names/values.
Parse the LCD *values* row, which is currently thrown away.

**Why.** Today every seven-character field at a multiple of seven becomes
`Message::Name { offset, text }` and is written into `bank[index].name` (`src/mcu.rs:457-461`).
In plugin mode the DAW writes *parameter* names into exactly those fields, so switching to the
EQ page would overwrite the track names and page 1 would show "LowGain" where "03 BASS" was.

Also `Message::Status` is dropped on the floor (`src/mcu.rs:464`), and that is where the
values live: without them the detail view has no number to show.

**Where.** `src/mcu.rs` (`decode_sysex` at 273-308 and the `apply` arms), `src/studio.rs`
(new state for the current mode and the eight parameter names/values).

**Details to settle on hardware, with `KOMPLEMENT_MCU_LOG=1`.**
- Whether values arrive as eight seven-character fields (one per parameter) or as one wide
  field for the last touched parameter.
- How a value is truncated at the edges of its range ("-12.0 dB" is eight characters).
- Whether the DAW refreshes values on page entry, or only when something changes; if not,
  whether flipping the parameter bank and back forces a redraw.

**Done when.** The EQ page's overview bars follow the rings (12 steps, 2 dB each in a
-12..+12 range) and the detail view shows the value text; page 1's track names survive a
visit to page 2.

## 6. Band detail view (mockups 06 and 07)

**What.** While a knob is touched or has just moved, the page switches from the eight-band
overview to one band: number, frequency, gain at scale 2, and the wide bipolar bar. A second
of stillness returns to the overview.

**Why.** This is where the exact number is needed, and it is the only place: on the overview
the bars are orientation, as the owner put it.

**Where.** `src/pages.rs` (a second frame per page, chosen by a timer in `Studio` or `Paging`),
`src/live.rs` (the knob branch sets "which band is being touched" and when it was last moved).

**Note.** Frequencies are fixed by the helper plugin (item 8), so the detail view can print
them from our own table and needs no feedback for them.

**Done when.** Touching knob 3 shows "EQ 3 / 1.2k / +3.0 dB" driven by real values, and the
frame matches `06_eq_boost.png`.

## 7. The compressor page

**What.** Page 3 shows `12_compressor` and puts the knobs on the compressor's parameters.
Entry sends `DYN` (`0x2D`).

**Open.** The GR block in the mockup is a *meter*, and MCU carries no plugin metering — only
per-channel levels on `0xD0`. Either the GR block goes, or it is fed by a parameter that the
helper plugin updates itself (see item 8). Decided after a hardware test of what Reaper
reports for `EQ` / `DYN` on a track that has such a plugin at all.

**Done when.** Knobs move the compressor parameters and the frame matches the mockup minus
whatever the GR decision removes.

## 8. A helper JSFX for Reaper

**What.** A small plugin, `komplement-eq` (and later `komplement-comp`), written for this
keyboard: eight gain parameters first (-12..+12 dB, 0.1 dB step), named by frequency, then a
second eight for the SHIFT page.

**Why.** It is the only way to guarantee that parameter *order* is the band order, that the
range is symmetric so ring position 6 is 0 dB, that names fit seven characters, and that the
value text is a number the tool can parse. It can also update its own sliders, which is the
one possible route to a GR readout over MCU.

**Cost.** JSFX is Reaper-only. The tool keeps working elsewhere as a plain MCU surface with
the generic pages; the pretty pages are a Reaper feature.

**Done when.** A track with the plugin shows eight bands on the EQ page, and the value text
for a band parses to a number within 0.1 dB.

## 9. "No EQ on this track"

**What.** If nothing arrives in the parameter zone within 300-500 ms of sending `ASSIGN_EQ`,
draw the hint; clear it on the first sign of activity.

**Why.** MCU has no "list the plugins" query and no "insert a plugin" command, so absence can
only be inferred from silence. Reaper on a track without an FX simply does nothing.

**Where.** `src/live.rs` (a deadline after the page change), `src/pages.rs` (the hint line).

**Done when.** A track without an EQ plugin shows the hint, a track with one does not, and the
hint disappears by itself when the plugin appears.

## 10. Carried over: a dedicated reader thread

Unchanged from the earlier note (`references/kontrol-rs-reader-thread.md` in the skill): the
reader must own the ALSA client, so the sends and the DAW-restart handling move with it. Wanted
because a stalled drain lets the sequencer pool fill, and then it is the DAW's own writes that
block — the 2-3 s freeze.

## 11. Optional: accept the Logic Control device ids

The SysEx header is matched as `F0 00 00 66 14` (MCU). Accepting `0x10` / `0x11` / `0x15` as
well would let the tool answer Logic Control and an MCU XT. One constant, but needs a decision
about which display commands those units actually honour.
