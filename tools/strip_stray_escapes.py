"""Removes the stray OSC escape that this environment's file writers leave behind.

The sequence is `ESC ] 0 ; BEL` - a terminal title request, five bytes. It is never
part of a source file or a document, and it sits at the end of a file after a
patch was written, which is what makes `rustc` complain about an unknown token.

Run from the project root: `python3 tools/strip_stray_escapes.py`.
"""

import pathlib
import re

STRAY = re.compile("\x1b\\][0-9;]*\x07")
SUFFIXES = (".rs", ".md", ".map", ".py", ".toml", ".lock", ".nix", ".txt", ".awk", ".sh", ".json")

fixed = []
for path in sorted(pathlib.Path(".").rglob("*")):
    text_path = str(path)
    if ".git/" in text_path or "target/" in text_path or not path.is_file():
        continue
    if path.suffix not in SUFFIXES:
        continue
    try:
        text = path.read_text(errors="surrogateescape")
    except PermissionError:
        # Somebody else's build directory, most likely: not ours to touch.
        continue
    cleaned = STRAY.sub("", text)
    if cleaned != text:
        path.write_text(cleaned)
        fixed.append((text_path, len(text) - len(cleaned)))

for name, removed in fixed:
    print(f"{name}: {removed} bytes of escape removed")
print(f"files fixed: {len(fixed)}")
