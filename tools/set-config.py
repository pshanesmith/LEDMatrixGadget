#!/usr/bin/env python3
"""Set one value in a TOML config file safely: idempotent, repairs duplicate sections.

  sudo python3 tools/set-config.py /etc/matrix/config.toml display pwm_bits 10

- Creates the [section] if missing, otherwise changes (or adds) just that key.
- Merges accidentally duplicated [sections] (later values win), keeping other lines and comments.
- Validates the result with a TOML parser before touching the file; keeps a .bak copy.
"""
import re
import shutil
import sys
import tomllib
from pathlib import Path

HEADER = re.compile(r"^\s*\[([^\[\]]+)\]\s*(#.*)?$")
KEY = re.compile(r"^\s*([A-Za-z0-9_.-]+)\s*=")


def format_value(raw: str) -> str:
    """Keep numbers and booleans bare, quote anything else."""
    if re.fullmatch(r"-?\d+(\.\d+)?|true|false", raw):
        return raw
    if raw.startswith(('"', "'", "[")):
        return raw
    return '"' + raw.replace('"', '\\"') + '"'


def update(text: str, section: str, key: str, value: str) -> str:
    preamble: list[str] = []
    sections: dict[str, list[str]] = {}
    current = None
    for line in text.splitlines():
        m = HEADER.match(line)
        if m:
            current = m.group(1).strip()
            sections.setdefault(current, [])
            continue
        (preamble if current is None else sections[current]).append(line)
    # merge: within a section a later "key = ..." replaces an earlier one (keep the first position)
    for name, lines in sections.items():
        seen: dict[str, int] = {}
        out: list[str] = []
        for line in lines:
            m = KEY.match(line)
            if m and m.group(1) in seen:
                out[seen[m.group(1)]] = line
            else:
                if m:
                    seen[m.group(1)] = len(out)
                out.append(line)
        sections[name] = out
    lines = sections.setdefault(section, [])
    new_line = f"{key} = {format_value(value)}"
    for i, line in enumerate(lines):
        m = KEY.match(line)
        if m and m.group(1) == key:
            lines[i] = new_line
            break
    else:
        # insert after the last non-blank line of the section
        pos = len(lines)
        while pos and not lines[pos - 1].strip():
            pos -= 1
        lines.insert(pos, new_line)
    out = list(preamble)
    for name, body in sections.items():
        while out and not out[-1].strip():
            out.pop()
        out += ["", f"[{name}]"] + [l for l in body]
    return "\n".join(out).strip("\n") + "\n"


def main(argv: list[str]) -> int:
    if len(argv) != 5:
        print(__doc__, file=sys.stderr)
        return 2
    path, section, key, value = Path(argv[1]), argv[2], argv[3], argv[4]
    new_text = update(path.read_text(encoding="utf-8"), section, key, value)
    tomllib.loads(new_text)                      # raises (and aborts) if the result is invalid
    shutil.copy2(path, path.with_suffix(path.suffix + ".bak"))
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(new_text, encoding="utf-8")
    shutil.copymode(path, tmp)
    tmp.replace(path)
    print(f"set [{section}] {key} = {format_value(value)} in {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
