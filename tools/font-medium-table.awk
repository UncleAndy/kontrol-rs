# Turns the 7x9 art in /home/hermes/kfont/glyphs.txt into (a) a printable review
# sheet on stdout and (b) a Rust table in /home/hermes/kfont/font_medium_table.rs.
/^@/ {
    character = substr($0, 2)
    order[++glyphs] = character
    row[character] = 0
    current = character
    next
}
{
    if (current == "") next
    row[current]++
    if (row[current] > 9) { print "TOO MANY ROWS: " current > "/dev/stderr"; next }
    art[current, row[current]] = $0
}

END {
    bad = 0
    for (i = 1; i <= glyphs; i++) {
        character = order[i]
        if (row[character] != 9) {
            print "WRONG ROW COUNT: " character " = " row[character] > "/dev/stderr"
            bad = 1
        }
        for (r = 1; r <= 9; r++) {
            if (length(art[character, r]) != 7) {
                print "WRONG WIDTH: " character " row " r " = [" art[character, r] "]" > "/dev/stderr"
                bad = 1
            }
        }
    }
    if (bad) exit 1

    print "glyphs: " glyphs

    for (start = 1; start <= glyphs; start += 16) {
        line = ""
        for (i = start; i < start + 16 && i <= glyphs; i++) line = line sprintf("%-10s", order[i])
        print line
        for (r = 1; r <= 9; r++) {
            line = ""
            for (i = start; i < start + 16 && i <= glyphs; i++) line = line art[order[i], r] "   "
            print line
        }
        print ""
    }

    table = "/home/hermes/kfont/font_medium_table.rs"
    printf "pub const GLYPHS: [(char, [u8; GLYPH_HEIGHT]); %d] = [\n", glyphs > table
    for (i = 1; i <= glyphs; i++) {
        character = order[i]
        printf "    ('%s', [", character >> table
        for (r = 1; r <= 9; r++) {
            value = 0
            for (k = 1; k <= 7; k++) {
                if (substr(art[character, r], k, 1) == "#") value += 2 ^ (k - 1)
            }
            printf "%s0b%s", (r == 1 ? "" : ", "), bits(value) >> table
        }
        printf "]),\n" >> table
    }
    printf "];\n" >> table
    print "table written to " table
}

function bits(value,   text, bit, out) {
    out = ""
    for (bit = 6; bit >= 0; bit--) {
        if (value >= 2 ^ bit) { value -= 2 ^ bit; out = out "1" } else out = out "0"
    }
    return out
}
