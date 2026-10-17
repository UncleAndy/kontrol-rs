# Summarises a capture-mcu.sh log: what each step of the guided capture produced.
# Reads two files: first the markers, then mcu.txt.
#
#   awk -f tools/mcu-report.awk <dir>/markers.txt <dir>/mcu.txt
#
# The point is attribution: every event is filed under the step it happened in, so
# "which message does a mute produce" is answered by the capture instead of by
# documentation.

FNR == NR {
    if ($0 ~ /^\[marker @[0-9]+ms\]/) {
        ms = $0; sub(/^\[marker @/, "", ms); sub(/ms\].*$/, "", ms)
        label = $0; sub(/^\[marker @[0-9]+ms\][[:space:]]*/, "", label)
        ms_at[++n] = ms + 0
        step_name[n] = label
    }
    next
}

{
    ms = 0
    if (match($0, /@[0-9]+ms/)) ms = substr($0, RSTART + 1, RLENGTH - 3) + 0

    # A marker is written when its step *finishes*, so the window between two
    # markers belongs to the step the later marker names - not the earlier one.
    # Events after the last marker are the tail, once the guide was done.
    step = 0
    for (i = 1; i <= n; i++) if (ms <= ms_at[i]) { step = i; break }
    if (step == 0) step = n + 1

    if (/System exclusive/) {
        cnt[step, "sysex"]++
        for (i = 1; i <= NF; i++) {
            if ($i == "F0") {
                cmd = $(i + 5)
                if (cmd != "") sysex_cmds[step, cmd] = 1
                break
            }
        }
        # How long the message is: the byte count says whether it is a track name
        # block, a display write or a one-byte query.
        length_of_message = 0
        for (i = 1; i <= NF; i++) if ($i == "F0") { length_of_message = NF - i }
        sysex_lengths[step, length_of_message] = 1
        next
    }

    if (/Control change/) {
        cnt[step, "cc"]++
        for (i = 1; i <= NF; i++) {
            if ($i == "controller") {
                ctrl = $(i + 1); sub(/,.*$/, "", ctrl)
                if (ctrl != "") ccs[step, ctrl] = 1
            }
            if ($i == "value") {
                value = $(i + 1)
                values[step, "cc"] = values[step, "cc"] " " value
            }
        }
        next
    }

    if (/Note on/)  {
        cnt[step, "noteon"]++
        for (i = 1; i <= NF; i++) if ($i == "note") { note = $(i + 1); sub(/,.*$/, "", note); notes[step, note] = 1 }
        next
    }
    if (/Note off/) {
        cnt[step, "noteoff"]++
        for (i = 1; i <= NF; i++) if ($i == "note") { note = $(i + 1); sub(/,.*$/, "", note); notes[step, note] = 1 }
        next
    }

    if (/Program change/) { cnt[step, "program"]++; next }

    # Everything else, named: aseqdump prints the event name in the third field,
    # and knowing whether the rest is clock, active sensing or pitch bend decides
    # whether the decoder has to care about it at all.
    {
        name = $3 " " $4
        if ($3 == "") name = $2
        others[step, name]++
        cnt[step, "other"]++
    }
}

function keys_of(arr, step,    k, parts, out) {
    for (k in arr) {
        split(k, parts, SUBSEP)
        if (parts[1] + 0 == step) out = out " " parts[2]
    }
    return out
}

# Prints the distinct values a step used, in ascending numeric order: the values
# are small integers (notes, controllers, command bytes), so sorting them here is
# cheaper and more reliable than piping to `sort`, which loses the output.
function print_sorted(label, arr, step,    k, parts, max, v) {
    max = -1
    for (k in arr) {
        split(k, parts, SUBSEP)
        if (parts[1] + 0 != step) continue
        v = parts[2] + 0
        if (v > max) max = v
    }
    if (max < 0) return
    printf "%s", label
    for (v = 0; v <= max; v++) if ((step SUBSEP v) in arr) printf " %d", v
    printf "\n"
}

END {
    for (s = 0; s <= n + 1; s++) {
        if (s == 0) printf "\n== [before the first marker]\n"
        else if (s == n + 1) printf "\n== [after the last marker - the tail]\n"
        else printf "\n== [%s]\n", step_name[s]

        total = cnt[s, "cc"] + cnt[s, "noteon"] + cnt[s, "noteoff"] + cnt[s, "sysex"] + cnt[s, "program"] + cnt[s, "other"]
        if (total == 0) { print "   (nothing at all)"; continue }

        printf "   events: %d  (cc=%d noteon=%d noteoff=%d sysex=%d program=%d other=%d)\n",
            total, cnt[s, "cc"], cnt[s, "noteon"], cnt[s, "noteoff"], cnt[s, "sysex"], cnt[s, "program"], cnt[s, "other"]

        print_sorted("   CC controllers:", ccs, s)
        print_sorted("   notes:", notes, s)
        print_sorted("   sysex commands:", sysex_cmds, s)
        print_sorted("   sysex byte lengths:", sysex_lengths, s)

        for (k in others) {
            split(k, parts, SUBSEP)
            if (parts[1] + 0 == s) printf "   other: %s x%d\n", parts[2], others[k]
        }
    }
}
