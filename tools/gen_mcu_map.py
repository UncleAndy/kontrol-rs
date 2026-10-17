"""Makes mappings/reaper-mcu.map out of mappings/reaper.map.

The buttons the DAW's own Mackie Control surface understands are sent as MCU
messages instead of keystrokes; every other line is copied as it is, so the
SHIFT variants and everything the MCU has no word for keep working exactly as in
the original. Run from the project root: `python3 tools/gen_mcu_map.py`.
"""

import pathlib
import re

SOURCE = pathlib.Path("mappings/reaper.map")
TARGET = pathlib.Path("mappings/reaper-mcu.map")

# Button -> the MCU name, as `crate::mcu_buttons` spells them. Only the plain
# layer: MCU has no shifted transport, so `Shift+...` stays a keystroke.
REPLACEMENTS = {
    "Play": "mcu:play",
    "Stop": "mcu:stop",
    "Record": "mcu:record",
    "Loop": "mcu:loop",
    "Metro": "mcu:click",
    "Track": "mcu:track_right",
}

HEADER = """# The MCU variant of reaper.map: the buttons the DAW's own surface understands
# are sent as MCU messages instead of keystrokes, so the DAW does not have to
# agree with us about a keyboard shortcut, and so the button can be lit from the
# state the DAW sends back.
#
# It needs the MCU port: run with `-X self` (or `-X auto`). Without one those
# buttons have nowhere to go and do nothing at all; everything else here is the
# same as in reaper.map."""

lines = SOURCE.read_text().splitlines()
out = []
changed = []
for line in lines:
    # A button name is everything before the '='; `Shift+Play` is a different
    # button and must not be mistaken for `Play`.
    match = re.match(r"^([A-Za-z][A-Za-z0-9 _-]*)=", line)
    if match and match.group(1) in REPLACEMENTS:
        new = f"{match.group(1)}={REPLACEMENTS[match.group(1)]}"
        out.append(new)
        changed.append((line, new))
    else:
        out.append(line)

TARGET.write_text(HEADER + "\n" + "\n".join(out) + "\n")
print(f"changed lines: {len(changed)}")
for old, new in changed:
    print(f"  {old}  ->  {new}")
print(f"lines: {len(lines)} -> {len(HEADER.splitlines()) + len(out)}")
