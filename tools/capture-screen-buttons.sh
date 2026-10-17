#!/usr/bin/env bash
#
# Guided capture of the two buttons beside the display of a Komplete Kontrol
# A-series keyboard. It answers three questions at once:
#
#   * are those buttons a HID report bit, a MIDI controller, or silent?
#   * do they differ from each other (so left and right can be told apart)?
#   * does what they send change while the display is ours rather than Native
#     Instruments' - which is the state the screen daemon will be in?
#
# Run it from the dev shell, in the repository root:
#
#   nix develop -c bash tools/capture-screen-buttons.sh              # A49
#   nix develop -c bash tools/capture-screen-buttons.sh 1730 mydir   # A25, own dir
#   nix develop -c bash tools/capture-screen-buttons.sh 1740 out taken-over
#                                                                    # only the
#                                                                    # second pass
#
# The same sequence is asked twice, once per display owner. Files written:
#
#   <out>/hid-midi-mode.txt    HID reports (-d), display still NI's (`a0 07 00`)
#   <out>/hid-taken-over.txt   HID reports (-d), display ours (`a0 03 04`, `-x`)
#   <out>/midi.txt             everything the keyboard sends over MIDI (amidi)
#   <out>/markers.txt          one line per step, stamped on the same clock
#
# If the binary is missing it is built with cargo; inside the dev shell that
# works. Nothing is sent to uinput during the capture: the empty mapping
# `mappings/none.map` is used on purpose, and the keyboard is left in MIDI mode.
#
set -u

PID="${1:-1740}"
OUT="${2:-kk-screen-buttons-$(date +%Y%m%d-%H%M%S)}"
ONLY="${3:-}"
BINARY="${KOMPLEMENT:-/tmp/krs-target/debug/komplement}"
MIDI_PORT="${MIDI_PORT:-}"
HID_PID=""
MIDI_PID=""

CLEANING=0
cleanup() {
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
    echo "== stopping the capture (the display is handed back to the keyboard)"
    stop "$HID_PID"
    stop "$MIDI_PID"
    echo "== files in $OUT:"
    ls -l "$OUT" 2>/dev/null || true
    echo
    echo "== now send hid-midi-mode.txt, hid-taken-over.txt, midi.txt and"
    echo "   markers.txt (the parts between the markers are enough) so the"
    echo "   buttons can be worked out."
    exit 0
}
trap cleanup INT TERM

marker() {
    # The HID log is stamped with CLOCK_MONOTONIC, which is the uptime clock
    # minus suspended time - good enough to line the two logs up.
    local ms
    ms="$(awk '{ printf "%d", ($1 * 1000) }' /proc/uptime)"
    printf '[marker @%sms] %s\n' "$ms" "$1" | tee -a "$OUT/markers.txt"
}

if [ ! -f mappings/none.map ]; then
    echo "!! run this from the repository root (mappings/none.map is missing)" >&2
    exit 1
fi

if ! command -v amidi >/dev/null 2>&1; then
    echo "!! amidi not found; start this from the dev shell (nix develop)" >&2
    exit 1
fi

if [ -x "$BINARY" ] && command -v cargo >/dev/null 2>&1; then
    # Always rebuild: a stale binary silently swallows options the source has
    # (that is exactly how `-x` was lost on the first run of this script).
    echo "== building komplement (cargo build)"
    CARGO_TARGET_DIR="$(dirname "$(dirname "$BINARY")")" cargo build || exit 1
elif [ ! -x "$BINARY" ]; then
    if ! command -v cargo >/dev/null 2>&1; then
        echo "!! $BINARY is missing and cargo is not in PATH;" >&2
        echo "   build it from the dev shell first:" >&2
        echo "   CARGO_TARGET_DIR=/tmp/krs-target nix develop --command cargo build" >&2
        exit 1
    fi
    echo "== building komplement into $(dirname "$(dirname "$BINARY")")"
    CARGO_TARGET_DIR="$(dirname "$(dirname "$BINARY")")" cargo build || exit 1
fi

lsusb | grep -i 17cc || echo "!! no 17cc device on the USB bus - is the keyboard plugged in?"

mkdir -p "$OUT"

echo "== MIDI ports ALSA knows about:"
amidi -l

if [ -z "$MIDI_PORT" ]; then
    MIDI_PORT="$(amidi -l | awk 'tolower($0) ~ /kontrol|a-series|a25|a49|a61/ { print $2; exit }')"
fi

if [ -n "$MIDI_PORT" ]; then
    echo "== capturing MIDI from $MIDI_PORT -> $OUT/midi.txt"
    amidi -d -p "$MIDI_PORT" > "$OUT/midi.txt" 2>&1 &
    MIDI_PID=$!
else
    echo "!! could not recognise the keyboard's MIDI port, capturing HID only."
    echo "   pass it as an environment variable to include it:"
    echo "     MIDI_PORT=hw:1,0,0 bash tools/capture-screen-buttons.sh $PID $OUT"
fi

ask() {
    echo
    echo "-- $1"
    printf "   press Enter when done> "
    read -r _ || true
    marker "$2"
}

# One pass over the two buttons with a given display owner. `$3` decides whether
# the pass also asks about separate Mute/Solo buttons (only asked once: which
# physical button a bit belongs to cannot depend on who owns the display).
pass() {
    local file="$1" label="$2" ask_mute_solo="$3"
    shift 3

    echo
    echo "======================================================================"
    echo " Pass: $label"
    echo "======================================================================"

    "$BINARY" -m mappings/none.map -p "$PID" -n -d "$@" > "$OUT/$file" 2>&1 &
    HID_PID=$!
    sleep 2

    if ! kill -0 "$HID_PID" 2>/dev/null; then
        echo "!! komplement exited straight away - last lines of $OUT/$file:"
        tail -5 "$OUT/$file"
        cleanup
    fi

    marker "start: $label"

    ask "PRESS the LEFT of the two buttons beside the display, slowly, three times" \
        "left button x3 ($label)"
    ask "PRESS the RIGHT of the two buttons beside the display, slowly, three times" \
        "right button x3 ($label)"
    ask "PRESS LEFT, then RIGHT, once each, about two seconds apart" \
        "left then right ($label)"

    if [ "$ask_mute_solo" = yes ]; then
        ask "PRESS the keyboard's Mute button, then its Solo button, once each - if those are buttons of their own and not the two beside the display. If the keyboard has no such pair, just press Enter" \
            "mute, solo (if they exist) ($label)"
    fi

    ask "Do nothing at all for ~3 seconds" "quiet ($label)"

    kill -TERM "$HID_PID" 2>/dev/null
    wait "$HID_PID" 2>/dev/null
    HID_PID=""
}

echo
echo "======================================================================"
echo " The keyboard's two display buttons: which control are they?"
echo " Do exactly what is asked, one step at a time, pressing Enter after each."
echo " Do not touch anything else in between, and mind which of the two is"
echo " LEFT and which is RIGHT - the whole point is telling them apart."
echo "======================================================================"

# The display is still Native Instruments' here: the tool only sends `a0 07 00`.
if [ -z "$ONLY" ] || [ "$ONLY" = midi-mode ]; then
    pass "hid-midi-mode.txt" "display in MIDI mode" yes
fi

# The display is ours here: `-x` sends `a0 03 04` and gives it back on exit.
if [ -z "$ONLY" ] || [ "$ONLY" = taken-over ]; then
    pass "hid-taken-over.txt" "display taken over (-x)" no -x
fi

cleanup
