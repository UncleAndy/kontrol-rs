#!/usr/bin/env bash
#
# Guided capture of what a Komplete Kontrol A-series actually sends for the
# controls that komplement does not handle yet: the eight rotary encoders
# (rotation AND touch), the 4D directions and the rotary push buttons.
#
# Run it from the dev shell:
#
#   nix develop -c bash tools/capture-a49.sh              # product id 1740
#   nix develop -c bash tools/capture-a49.sh 1730 mydir   # A25, own output dir
#   nix develop -c bash tools/capture-a49.sh 1740 mydir hw:1,0,0   # own MIDI port
#
# You will be asked to do one thing at a time and press Enter in between, so
# every packet in the log belongs to exactly one control. Three files are
# written:
#
#   <out>/hid.txt      HID reports, only the ones that changed (-d), with the
#                      button mask and 4D dial decoded
#   <out>/midi.txt     the MIDI bytes the device sends (amidi): the encoders
#                      may well report rotation/touch over MIDI instead of HID
#   <out>/markers.txt  one line per step, stamped on the same clock as the
#                      dump (@ms), so the steps can be mapped onto the log
#
# Nothing is sent to any application while capturing: the empty mapping
# (mappings/none.map) is used on purpose.
#
set -u

PID="${1:-1740}"
OUT="${2:-kk-capture-$(date +%Y%m%d-%H%M%S)}"
MIDI_PORT_ARG="${3:-}"
KOMPLEMENT_PID=""
MIDI_PID=""

CLEANING=0
cleanup() {
    # SIGINT/SIGTERM can arrive here more than once, and one of them may be
    # ignored in the process tree this script was started from (SIG_IGN is
    # inherited and cannot be undone by kill), so: run once, ignore further
    # signals, and stop the children with SIGTERM - komplement handles SIGTERM
    # gracefully - escalating to SIGKILL instead of blocking in `wait`.
    [ "$CLEANING" = 1 ] && return
    CLEANING=1
    trap '' INT TERM

    stop() {
        [ -n "$1" ] || return 0
        kill -0 "$1" 2>/dev/null || return 0
        kill -TERM "$1" 2>/dev/null
        for _ in 1 2 3 4 5 6 7 8 9 10; do
            kill -0 "$1" 2>/dev/null || return 0
            sleep 0.2
        done
        kill -KILL "$1" 2>/dev/null
        return 0
    }

    echo
    echo "== stopping the capture..."
    stop "$KOMPLEMENT_PID"
    stop "$MIDI_PID"
    echo "== capture finished, files in $OUT:"
    ls -l "$OUT" 2>/dev/null || true
    echo
    echo "== now send hid.txt, midi.txt and markers.txt (or just the parts"
    echo "   between the markers) so the codes can be worked out"
    exit 0
}
trap cleanup INT TERM

marker() {
    # CLOCK_MONOTONIC (what -d stamps its lines with) is the uptime clock,
    # minus time spent suspended - good enough to line the two up.
    local ms
    ms="$(awk '{ printf "%d", ($1 * 1000) }' /proc/uptime)"
    printf '[marker @%sms] %s\n' "$ms" "$1" | tee -a "$OUT/markers.txt"
}

if [ -x ./komplement ]; then
    KOMPLEMENT=./komplement
elif command -v komplement >/dev/null 2>&1; then
    KOMPLEMENT=komplement
else
    echo "!! komplement not found; build it first from the dev shell: make all" >&2
    exit 1
fi

MAP=mappings/none.map
[ -f "$MAP" ] || { echo "!! $MAP missing (run this from the repository root)" >&2; exit 1; }

if ! command -v amidi >/dev/null 2>&1; then
    echo "!! amidi not found; start this script from the dev shell (nix develop)" >&2
    exit 1
fi

lsusb | grep -i 17cc || echo "!! no 17cc device on the USB bus - is the keyboard plugged in?"

mkdir -p "$OUT"

echo "== MIDI ports ALSA knows about:"
amidi -l

PORT="$MIDI_PORT_ARG"
if [ -z "$PORT" ]; then
    PORT="$(amidi -l | awk 'tolower($0) ~ /kontrol|a-series|a25|a49|a61/ { print $2; exit }')"
fi

if [ -n "$PORT" ]; then
    echo "== capturing MIDI from $PORT -> $OUT/midi.txt"
    amidi -d -p "$PORT" > "$OUT/midi.txt" 2>&1 &
    MIDI_PID=$!
else
    echo "!! could not recognise the keyboard's MIDI port, capturing HID only."
    echo "   For the MIDI part either pass it as the third argument:"
    echo "     bash tools/capture-a49.sh $PID $OUT hw:1,0,0"
    echo "   or start this in another terminal while the script runs:"
    echo "     amidi -d -p hw:1,0,0 | tee $OUT/midi.txt"
fi

echo
echo "== starting the HID capture ($KOMPLEMENT -m $MAP -p $PID -n -d)"
"$KOMPLEMENT" -m "$MAP" -p "$PID" -n -d > "$OUT/hid.txt" 2>&1 &
KOMPLEMENT_PID=$!
sleep 2

if ! kill -0 "$KOMPLEMENT_PID" 2>/dev/null; then
    echo "!! komplement exited straight away - last lines of its output:"
    tail -5 "$OUT/hid.txt"
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
echo " Now do exactly what is asked, one control at a time, pressing Enter"
echo " after each. Do not touch anything else in between."
echo "======================================================================"

for knob in 1 2 3 4 5 6 7 8; do
    ask "TOUCH encoder $knob: put a finger on it (the sensor, do not turn), hold ~1s, let go" \
        "touch encoder $knob"
done

for knob in 1 2 3 4 5 6 7 8; do
    ask "TURN encoder $knob: ~5 clicks clockwise, pause, ~5 clicks counter-clockwise" \
        "rotate encoder $knob"
done

ask "PRESS the four 4D directions: up, right, left, down" "4D directions"
ask "PUSH the eight encoders in, one after the other (they click)" "encoder push"
ask "PRESS one of the known-good buttons, e.g. Play" "known button (control)"
ask "Now do nothing at all for ~3 seconds" "quiet period"

cleanup
