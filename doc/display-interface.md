# The display interface

**Status:** design, agreed with the owner. Everything marked *measured* comes from `doc/Captured_Codes.md`; everything marked *open* is a question that a capture or a run has to answer before code depends on it.

**Steps 1 and 2 are in place:** the state and the four pages are drawn (`src/studio.rs` for the state, `src/pages.rs` for the layout), and the MCU half of the state is filled by `src/mcu.rs` from a real capture - checked against the DAW's own bytes by `tests/mcu_capture.rs` and the fixture it reads. The pages can be looked at on the panel with `komplement -S page-track:large` and its three siblings, at any of the three sizes, with no DAW involved at all. Nothing opens a port yet: the daemon owns that, in step 4, and the FIFO half is step 3.

## Why the DAW cannot simply write text

The panel of an A-series keyboard is a mono 128x32 display, one bit per pixel, and it has **no text mode**: the text reports described in `doc/mini-screen.txt` were measured on hardware and do nothing. So the DAW cannot put text on the screen - nobody can except this project, which renders the pixels itself (frames, fonts, bars; see `references/a49-display-protocol.md` in the `komplementary-kontrol` skill).

The interface therefore carries **data and messages, never pixels**. Layout, fonts, pages and the redraw policy stay here; the DAW says *what* is true, not *what to draw*. That division is what makes several sources possible at once and what keeps the screen useful when the DAW is not the thing being used.

## The layers

```
  MCU (states)  ---\
                    >---  Studio (state)  +  message queue  --->  page  --->  frame  --->  two HID reports
  FIFO (messages) -/
```

A page is a function of the state and the chosen glyph size; the screen is a page plus an optional message overlay. Nothing above the renderer knows about pixels, and nothing in the renderer knows where the state came from.

## Source 1: MCU (states)

Reaper's *Mackie Control Universal* control surface is switched on in Preferences -> Control/OSC/Web -> **Add -> Mackie Control Universal**, pointed at a MIDI output, and then it feeds that port by itself, with no script and no ReaScript:

- track names in the LCD SysEx blocks: seven characters per channel, eight channels per bank;
- the transport state, and the loop and click LEDs, as note messages - the same ones that light a real MCU's LEDs - and the channel rows of arm, solo, mute and select the same way;
- fader positions as **pitch bend**, one message per channel, 14 bits, the ninth being the master, and the meter levels as channel aftertouch;
- and **not** the position, and **not** pan: the capture has no position SysEx at all and no pan controller anywhere, so a pan display has no source and the time is still a gap. The twelve-character display on controllers 64..75 is the only candidate for a time, and it is not decoded yet.

This is one-way listening: we never answer and never send, so we cannot disturb Reaper's handling of the surface.

Why it is the right first channel: nothing has to be installed in the DAW and nothing is tied to a ReaScript API version; the data arrives by itself, which matters because a separate process could not poll Reaper for it any other way; it is a de-facto standard, so Ardour, Mixbus, Bitwig, Cubase and others will drive the same screen; and it carries exactly the kind of "state" the pages want to show.

Its limits, stated plainly: MCU carries MCU's own vocabulary and no arbitrary text (that is what the FIFO is for); names arrive eight tracks at a time, so a list of more tracks needs us to send bank-up/bank-down notes back; **tempo is not in MCU at all**, so a tempo field would need OSC or a script - moot, since it is not wanted on the screen; and SysEx has to actually pass through the port, which on Linux is not a given and is to be verified before anything relies on track names.

The opcodes are no longer a guess: they were measured from a capture of Reaper's own output (`tools/capture-mcu.sh`, the log kept in `kk-mcu-20261017-032907`, the facts written up in `doc/Captured_Codes.md`), and `src/mcu.rs` decodes exactly what that capture contains. What the capture did *not* contain matters as much as what it did - the position and pan above are the two things missing from it.

## Source 2: FIFO (messages)

A named pipe, by default `$XDG_RUNTIME_DIR/kontroller.screen` (overridable), read line by line by the daemon:

```sh
echo "Render done" > "$XDG_RUNTIME_DIR/kontroller.screen"
```

One line is one message, shown over the current page for a few seconds. A leading `!` makes it **sticky**: it stays until it is replaced by another message or dismissed, which is what an error or a warning wants. Anything else is plain text; a line with an unknown prefix is still just text, so the format cannot be broken by accident.

This is the channel for the things MCU cannot say: the result of an action, a warning, a countdown from a script, a line from a shell. It needs no configuration at all, works from bash, from a script, and from ReaScript (`io.open` and `write`). The daemon holds the FIFO open for reading, so a writer never blocks while it runs; when the daemon is **not** running a writer blocks on open, so scripts that must not hang should write with a timeout.

The alternative that was considered and rejected (kept here for the record because it may come back): a Unix datagram socket, which never blocks a writer at all. It was rejected in favour of the simpler, more familiar pipe.

## The state (`Studio`)

Plain data, filled field by field by whichever source knows that field, so a missing source leaves the rest intact:

- the selected track: number, name, volume, pan, mute, solo, record-arm;
- transport: position, playing/stopped, recording, loop and click states;
- the current MCU bank: its eight tracks with their fader and pan positions and their mute/solo states;
- per-field freshness: which source wrote it and when, so the screen can show "as of when" and a dead source is visible rather than silently stale. Not in the state yet: it arrives with the MCU decoder (step 2), because with no sources there is nothing to date.

**Answered by the capture:** meters do arrive, as channel aftertouch, one per channel, and the decoder keeps them (`Decoder::levels`) though no page reads them yet; the bank is eight channels - a bank change rewrites all eight names, so eight is the step the surface takes - and which project tracks a bank covers is not in MCU at all, since the digits in the names are the owner's own text, not positions (`7 Strings` is a name whose track is not the seventh); SysEx does pass through on this host, because the names arrived; and **the position is in the stream**, in the DAW's own twelve-digit display on controllers 64..75, which the decoder now keeps (`Studio::position`, the ten rightmost digits, and `Studio::display` for the whole row). The protocol numbers those digits from the right and puts the dot in bit 6; the capture's fixture reads `01 25 1  000` - bar 25, beat 1, tick 000 - and the position-jump step is what moved it. Reading a digit row that only refreshes when a digit changes is what made it look dead the first time: a stopped DAW never touches it.

**Still open:** whether pan is worth having, since no source knows it; the record LED (95 is assumed: nothing in the capture engaged record); which of notes 86 and 89 is the loop and which the click (both lit in the step where the two were toggled, and only 86 follows MCU's own LED map); and whether the daemon has to answer MCU's device-query (some DAWs want a reply before they consider a surface alive; Reaper usually does not). The LCD cells themselves are settled: field *k* sits at offset 7*k*, the first cell is the first strip's name, and Reaper's announcement line takes that same first cell when it needs the row. The two assignment digits left of the timecode are still unread: they read `01` throughout this capture, which is not enough to say what they mean.

## Messages

A message is text plus two switches: sticky (yes/no) and a lifetime (default a few seconds). Rules:

- a message takes the whole screen while it shows, and the page underneath is kept as it was;
- when it expires the page comes back unchanged - the pages are recomputed, not restored;
- **pressing a page button dismisses the message** and pages from there (the 4D dial and its button stay untouched, by the owner's decision, so paging is the dismiss gesture);
- there is no message history. A message that arrives while nobody is looking at the keyboard is lost; that is a decision, not an oversight - the fourth page was wanted for general state instead.

## The pages

Four, in a ring, with the state each one reads:

| # | page | shows | notes |
|---|---|---|---|
| 1 | transport / time | position, playing/stopped/recording, loop, click | no tempo: the owner does not want it on the screen, and MCU does not carry it anyway. The position is the DAW's own twelve-digit display, decoded from controllers 64..75 |
| 2 | selected track | the position and the transport first, then number and name with the mute/solo/arm marks, then volume and pan | the two lines the owner asked for while the DAW plays come first, because the size decides how many lines fit |
| 3 | tracks | the bank's tracks with their flags, the selected one marked, as text | the size decides how many of the eight are shown, which is the whole point of the page/size split |
| 4 | general info | the bank as a mixer: one column per channel with a two-character label, mute and solo marks and a fader filled from the bottom, plus a bottom line with the transport and the bank's numbering | "general information that Reaper gives over MCU", as the owner put it; the only page that is drawn rather than written, and the only one that ignores the size |

The four navigate with the two buttons beside the display: **Page Left is bit 14 (0x40 in `buf[2]`) and Page Right is bit 15 (0x80)**, both measured, both HID-only, one packet per press and one per release, and identical whether the display is Native Instruments' or ours. They do not auto-repeat, so accelerated paging would need hold detection here; not wanted yet.

**SHIFT (bit 0) + a page button cycles the glyph size**: small 5x7 (four lines, 21 characters), medium 7x9 (three lines, 16 characters), large (two double-size lines, 10 characters). The mixed "medium value, small sides" layout stays available as a per-page choice if it turns out to be the one he prefers on the panel.

## Who owns the panel

While the daemon runs, it owns the display: `a0 03 04` after opening the device, `a0 07 00` on every exit path, including SIGINT and SIGTERM, so the keyboard's own logic comes back. If the device cannot be opened the daemon says why and exits; if it cannot open `/dev/uinput` it does not need it - the daemon never sends keystrokes.

`komplement` (keystrokes) and the daemon can read the same HID interface at the same time. The one collision there was is settled: `mappings/reaper.map` no longer maps **Page Left** or **Page Right** at all, so the two buttons belong to the daemon alone and one press cannot both page the screen and toggle mute in the DAW. (They used to be mapped as Alt+M and Alt+Shift+S under the inherited names Mute and Solo, which this keyboard does not have.)

## Redraw policy

- a frame is 512 bytes in two 265-byte reports;
- at most 20 frames per second, and only when the picture actually changed - an unchanged frame is compared byte for byte and not sent;
- messages may bypass that limit, but not more often than about ten per second;
- the two reports of one frame go out back to back, never interleaved with another frame.

## Implementation order

Each step is verifiable on the panel before the next one starts:

1. **done** - the model and the four pages, rendered from a canned state: the screen, the sizes and the paging can be judged with no DAW involved at all (`src/studio.rs`, `src/pages.rs`, the `page-*` states of `-S`);
2. **done** - the capture and the decoder that fills `Studio` from it, opcodes measured rather than guessed: `src/mcu.rs` turns the DAW's bytes into the state, `tests/mcu_capture.rs` feeds it a slice of the real stream (`tests/fixtures/mcu-reaper.txt`, cut out with `tools/mcu-slice.awk`), and opening the port itself is left to the daemon in step 4, so nothing here needs hardware to be verified. The capture is `tools/capture-mcu.sh` (listen to the surface's port with `aseqdump`, walk play/stop, track selection, mute/solo/arm, volume, click/loop, a bank change, a rename, a tempo change and a quiet period, each step marked on the monotonic clock); its log is the only source for the decoder, exactly as the keyboard's own protocol was measured before it was coded;
3. the FIFO reader and the message overlay;
4. the daemon: ownership, paging, sizes, the redraw policy, run by hand at first, as a systemd unit later. Until it exists, `komplement -X <port>` already draws the decoder's state on the panel, live: one page, one size, no messages (see the section above).

## Seeing the decoder before the daemon exists

Steps 3 and 4 are still ahead, so there is a way to put the decoder on the panel now, with a DAW attached and no daemon at all:

- `komplement -X <port>` listens to the port the control surface writes to - an address like `14:0`, or a piece of its name as `aconnect -l` shows it - and draws one page of the state it reports. With nothing matching it prints the ports to choose from. `-S page-track:large` chooses the page and the glyph size (default: the track page at medium).
- It takes the display over for the run and gives it back on the way out, including on Ctrl+C; **nothing is ever sent back to the DAW**.
- It already keeps the two rules below that the daemon will keep: a frame that the panel is already showing is not sent again, and the panel is written at most twenty times a second. That is not decoration. The first run on hardware ended with `ENOSPC` from the sequencer: the DAW sends in bursts (nine pitch bends together, channel aftertouch while playing - the capture measured some five hundred events a second), and drawing once per message meant the queue filled up while the keyboard was being written to. So the whole queue is drained into the state first, and only then is a frame drawn - waiting happens after the drain, where it costs nothing but frames.
- A full queue is reported and survived rather than fatal: a few values are older than they could have been, the rest of the picture is the DAW's, and the count is printed on the way out. If the sequencer complains about a full queue without a single message arriving behind it for five seconds, that is not a lost burst but nothing being delivered at all, and it says so and stops.
- This is deliberately not the daemon: no FIFO, no messages, no paging by the two buttons.

What makes it checkable off the hardware is that the parts are separable: the stream is tested against the capture (`tests/mcu_capture.rs`), the port choosing and the picture rule are unit-tested, and `tests/mcu_port_roundtrip.rs` puts a writer port and the listening port in one process and checks the whole path through ALSA - that last one does nothing on a machine without `/dev/snd/seq`, and runs for real where the live view runs.

## What is deliberately absent

- No pixels, no fonts, no text protocol over the wire: the DAW never draws.
- No ReaScript, no Lua, no per-project installation.
- No OSC in the first version (it is the fallback if tempo or meters turn out to be needed).
- No message history, no logging to disk, no configuration file beyond the FIFO path.
- Nothing in this interface depends on the fourth page's contents, on meters, or on tempo, so those can be decided later without redoing the rest.
