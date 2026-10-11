"""Unicorn vectors for the quoted branch of the hotkey text parser 0x6F4CE050 (docs/spec/natives-lane-f.md, section 4).

A text that starts with a single quote is read as a raw code: up to four characters, stopping at the closing quote or
the end of the text, folded big-endian. Anything else goes to the C runtime `strtol(text, 0, 0)` (imported from
MSVCR80, not part of Game.dll, so it is not run here). One case per line: the text as hex bytes, then the result.

  GAME_DLL=/path/to/Game.dll python3 tools/emu/hotkey_text_emu.py OUT.txt [N_CASES] [SEED]
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402

TEXT = emu.DATA + 0x6000


def main():
    out_path = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 300
    rng = random.Random(int(sys.argv[3]) if len(sys.argv) > 3 else 1)
    alphabet = b"abcdefQWERTYXYZ0123456789 _-"
    lines = []
    for _ in range(n):
        body = bytes(rng.choice(alphabet) for _ in range(rng.randrange(0, 8)))
        if rng.random() < 0.5:
            cut = rng.randrange(0, len(body) + 1)
            body = body[:cut] + b"'" + body[cut:]
        text = b"'" + body
        emu.mu.mem_write(TEXT, text + b"\0")
        r = emu.call(0x6F4CE050, ecx=0, edx=TEXT) & 0xFFFFFFFF
        lines.append(f"{text.hex()} {r:08x}")
    with open(out_path, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print(f"{n} cases -> {out_path}")


if __name__ == "__main__":
    main()
