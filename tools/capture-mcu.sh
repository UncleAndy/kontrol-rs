#!/usr/bin/env bash
#
# Guided capture of what a DAW's Mackie Control Universal surface sends, so the
# MCU decoder that will fill the screen's state is built on measured opcodes
# rather than on documentation. Nothing here touches the keyboard.
#
# Run it from the dev shell:
#
#   nix develop -c bash tools/capture-mcu.sh                # asks for the port
#   nix develop -c bash tools/capture-mcu.sh mydir          # own output dir
#   nix develop -c bash tools/capture-mcu.sh mydir 128:0    # sequencer port
#   nix develop -c bash tools/capture-mcu.sh mydir hw:2,0,0 # rawmidi device
#
# Two files are written:
#
#   <out>/mcu.txt      every byte the surface sent, one event per line, each
#                      stamped with the monotonic clock in milliseconds
#   <out>/markers.txt  one line per step, stamped on the same clock, so every
#                      event belongs to exactly one action in the DAW
#
# Why aseqdump and not amidi: a DAW on Linux writes MIDI to a *sequencer* port,
# which `amidi` cannot see at all - it only reads rawmidi hardware nodes. A
# `hw:N,0,0` port (a `snd-virmidi` device, say) works with either; a `client:port`
# needs aseqdump.
#
# Setting up the DAW side first:
#
#   1. Reaper -> Preferences -> MIDI Devices: make sure the output you want to
#      use is enabled (right-click -> Enable output).
#   2. Reaper -> Preferences -> Control/OSC/Web -> Add -> "Mackie Control
#      Universal", MIDI output = that device, MIDI input = none (nothing will be
#      sent back). Reaper then feeds that port by itself, with no script.
#   3. Find the port this script should listen to:
#        aconnect -l          # sequencer ports, e.g. "client 128: 'Reaper'"
#        amidi -l             # rawmidi hardware, e.g. "hw:2,0,0"
#      A Reaper output is a sequencer port, so it is the first list that matters.
#
set -u

OUT_ARG=""
PORT_ARG=""
while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help) sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) if [ -z "$OUT_ARG" ]; then OUT_ARG="$1"; else PORT_ARG="$1"; fi ;;
    esac
    shift
done

OUT="${OUT_ARG:-kk-mcu-$(date +%Y%m%d-%H%M%S)}"
READER_PID=""

# The monotonic clock in milliseconds: what `-d` stamps its HID lines with, and
# what the markers use, so the two line up.
uptime_ms() {
    awk '{ printf "%d", ($1 * 1000) }' /proc/uptime
}

CLEANING=0
cleanup() {
    # A signal can arrive more than once (and SIGINT may be inherited as
    # SIG_IGN, which cannot be undone from inside), so: run once, ignore further
    # signals, stop the reader with SIGTERM and do not block in `wait`.
    [ "$CLEANING" = 1 ] && return
    CLEANING=1
    trap '' INT TERM

    if [ -n "$READER_PID" ] && kill -0 "$READER_PID" 2>/dev/null; then
        kill -TERM "$READER_PID" 2>/dev/null
        for _ in 1 2 3 4 5 6 7 8 9 10; do
            kill -0 "$READER_PID" 2>/dev/null || break
            sleep 0.2
        done
        kill -KILL "$READER_PID" 2>/dev/null
    fi

    echo
    echo "== capture finished, files in $OUT:"
    ls -l "$OUT" 2>/dev/null || true
    if [ -f "$OUT/mcu.txt" ]; then
        echo
        echo "== what arrived (counted by event type):"
        awk '
            /Control change/ { cc++ }
            /Note on/        { on++ }
            /Note off/       { off++ }
            /System exclusive/ { sysex++ }
            /Program change/ { pc++ }
            END {
                printf "   control change: %d\n", cc + 0
                printf "   note on:        %d\n", on + 0
                printf "   note off:       %d\n", off + 0
                printf "   program change: %d\n", pc + 0
                printf "   system exclusive: %d\n", sysex + 0
                if (sysex + 0 == 0)
                    print "   !! no SysEx at all: track names and the time display travel in SysEx," \
                          "\n      so check that the output the surface uses is the one you listened to," \
                          "\n      and that nothing in the way filters SysEx."
            }
        ' "$OUT/mcu.txt"
    fi
    echo
    echo "== send mcu.txt and markers.txt (or the parts between the markers) so"
    echo "   the opcodes can be worked out"
    exit 0
}
trap cleanup INT TERM

marker() {
    printf '[marker @%sms] %s\n' "$(uptime_ms)" "$1" | tee -a "$OUT/markers.txt"
}

for tool in aseqdump aconnect amidi; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "!! $tool not found; start this script from the dev shell (nix develop)" >&2
        exit 1
    }
done

mkdir -p "$OUT"

echo "== sequencer ports ALSA knows about (this is where a DAW's output shows up):"
aconnect -l 2>&1 | sed -n '1,40p'
echo
echo "== rawmidi hardware (amidi); a 'Virtual MIDI' device here works with either tool:"
amidi -l 2>&1 | sed -n '1,12p'
echo

PORT="$PORT_ARG"
if [ -z "$PORT" ]; then
    echo "-- which port should I listen to?"
    echo "   A DAW output is a sequencer port: pass it as client:port, e.g. 128:0."
    echo "   A rawmidi node instead: pass it as hw:N,0,0."
    echo "   (In Reaper: Preferences -> Control/OSC/Web -> Add -> Mackie Control"
    echo "    Universal, and pick that port as its MIDI output.)"
    printf "   port> "
    read -r PORT || true
fi

case "$PORT" in
    hw:*|plughw:*)
        READER_CMD=(amidi -d -p "$PORT")
        ;;
    *:*)
        # Line-buffer the reader, or the stamps of a whole burst arrive in one
        # lump: aseqdump writes to a pipe, which is block-buffered by default.
        if command -v stdbuf >/dev/null 2>&1; then
            READER_CMD=(stdbuf -oL aseqdump -p "$PORT")
        else
            READER_CMD=(aseqdump -p "$PORT")
        fi
        ;;
    *)
        echo "!! '$PORT' is neither client:port nor hw:N,0,0" >&2
        exit 1
        ;;
esac

echo
echo "== listening on $PORT (${READER_CMD[*]}) -> $OUT/mcu.txt"
{ "${READER_CMD[@]}" 2>&1 | while IFS= read -r line; do
      printf '@%sms %s\n' "$(uptime_ms)" "$line"
  done > "$OUT/mcu.txt"; } &
READER_PID=$!
sleep 2

if ! kill -0 "$READER_PID" 2>/dev/null; then
    echo "!! the reader stopped straight away - last lines of its output:"
    tail -5 "$OUT/mcu.txt" 2>/dev/null
    echo "   (a port that does not exist, or a client the tool cannot subscribe to)"
    cleanup
fi

marker "capture start"

ask() {
    echo
    echo "-- $1"
    printf "   press Enter when done> "
    read -r _ || true
    marker "$2"
}

echo
echo "======================================================================"
echo " Now do exactly this in the DAW, one step at a time, pressing Enter"
echo " after each. Do not touch anything else in between: every packet is"
echo " then attributable to one action."
echo "======================================================================"

ask "press PLAY, let it run ~3 seconds, then STOP" "play then stop"
ask "click somewhere else in the timeline, so the position jumps" "position jump"
ask "select track 1, then track 2, then track 3, one at a time" "select tracks 1-3"
ask "with track 2 selected: MUTE it, then unmute it" "mute and unmute"
ask "with track 3 selected: SOLO it, then unsolo it" "solo and unsolo"
ask "arm track 4 for recording, then disarm it" "record arm"
ask "move the selected track's volume up and down (with the mouse is fine)" "volume change"
ask "turn the metronome on and off, then the loop on and off" "click and loop"
ask "if the project has more than 8 tracks: select track 9, then track 1" "bank change"
ask "change the tempo (120 -> 121), then change it back" "tempo change"
ask "rename the selected track (add a letter to its name), then undo it" "rename track"
ask "now do NOTHING AT ALL for ~5 seconds" "quiet period"

cleanup
