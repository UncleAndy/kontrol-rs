# Turns a capture-mcu.sh log into raw MIDI bytes: one message per line, as hex.
# The decoder is tested against these bytes, not against the text a tool printed,
# so the fixture is the stream itself - exactly what the daemon will have to read.
#
# Only the lines whose timestamp is inside [from, to] (milliseconds, as the log
# writes them) are kept, which is how a few interesting seconds are cut out of a
# long capture.
#
#   awk -v from=55151200 -v to=55159000 -f tools/mcu-slice.awk mcu.txt >> fixture
#
# Line shapes taken from the log, verbatim:
#   14:0   Note on                 0, note 16, velocity 127
#   14:0   Note off                0, note 50
#   14:0   Control change          0, controller 75, value 48
#   14:0   Channel aftertouch      0, value 15
#   14:0   Pitch bend              8, value 5586
#   14:0   System exclusive           F0 00 00 66 14 12 ... F7
#
# A note off loses its release velocity on the way through aseqdump, so the
# fixture writes the usual 0x40 back in; the decoder ignores it either way.

function hex(n) { return sprintf("%02X", n) }

# Index of `word` among the first `count` tokens, or 0.
function at(tokens, count, word,   i) {
    for (i = 1; i <= count; i++) if (tokens[i] == word) return i
    return 0
}

# The number following `word`, or 0 when `word` is not there.
function after(tokens, count, word,   i) {
    i = at(tokens, count, word)
    if (i == 0 || i == count) return 0
    return tokens[i + 1] + 0
}

{
    ms = 0
    if (match($0, /@[0-9]+ms/)) ms = substr($0, RSTART + 1, RLENGTH - 3) + 0
    if (ms == 0 || ms < from || ms > to) next

    count = 0
    for (i = 1; i <= NF; i++) { count++; tokens[count] = $i }

    # The channel is the first field written as "N," - aseqdump prints the two
    # words of the event name, then the channel, then "note"/"value"/whatever.
    channel = 0
    for (i = 1; i <= count; i++) {
        if (tokens[i] ~ /^[0-9]+,$/) { channel = tokens[i] + 0; break }
    }
}

/Note on/ {
    printf "%s %s %s\n", hex(0x90 + channel), hex(after(tokens, count, "note")),
                         hex(after(tokens, count, "velocity"))
    next
}

/Note off/ {
    printf "%s %s %s\n", hex(0x80 + channel), hex(after(tokens, count, "note")), hex(0x40)
    next
}

/Control change/ {
    printf "%s %s %s\n", hex(0xB0 + channel), hex(after(tokens, count, "controller")),
                         hex(after(tokens, count, "value"))
    next
}

/Channel aftertouch/ {
    printf "%s %s\n", hex(0xD0 + channel), hex(after(tokens, count, "value"))
    next
}

/Pitch bend/ {
    value = after(tokens, count, "value") + 8192
    printf "%s %s %s\n", hex(0xE0 + channel), hex(value % 128), hex(int(value / 128))
    next
}

/System exclusive/ {
    start = at(tokens, count, "F0")
    if (start == 0) next
    line = ""
    for (i = start; i <= count; i++) line = line tokens[i] " "
    sub(/ +$/, "", line)
    print line
    next
}
