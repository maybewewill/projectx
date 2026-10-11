"""Unicorn check of the hero intelligence getter (Game.dll 1.26a, docs/spec/heroes.md section 6).

The core `0x6F0DB3A0` (__fastcall, ECX = hero sub-object at unit+0x1F0) computes
    c1 + trunc(gain * c2)
where gain is the real at hero+0xDC, c1 is the counter reached through the reference at hero+0x7C
(the intelligence base) and c2 the counter reached through the reference at hero+0x6C (the level counter).
The counter read `0x6F473170` (ECX = reference address) is hooked and answers from a per-reference table, so
only the arithmetic and the reference choice of the original run. The reads are checked to use ECX = hero+0x7C
for the base and hero+0x6C for the level (the hook records the ECX values it sees).

Output: one line per case `base_counter level_counter gain_bits result`, compared with
`sim::heroes::attribute(base, gain, level_counter)` by crates/sim/tests/hero_int_vectors.rs.

Run:  GAME_DLL=/path/to/Game.dll python3 tools/emu/hero_int_emu.py OUT.txt [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_EAX, UC_X86_REG_ECX  # noqa: E402

COUNTER = 0x6F473170   # the counter read: ECX = reference object, returns the counter
INT_CORE = 0x6F0DB3A0  # __fastcall(hero), returns c1 + trunc(gain * c2)
HERO = emu.DATA + 0x2000
REF_BASE = HERO + 0x7C   # intelligence base reference (c1)
REF_LEVEL = HERO + 0x6C  # level reference (c2)
counters = {}
seen = set()


def rd32(a):
    return struct.unpack("<I", bytes(emu.mu.mem_read(a, 4)))[0]


def wr32(a, v):
    emu.mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def on_counter(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    ref = uc.reg_read(UC_X86_REG_ECX)
    seen.add(ref)
    uc.reg_write(UC_X86_REG_EAX, counters.get(ref, 0) & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_ESP, esp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    out_path = sys.argv[1]
    cases = int(sys.argv[2]) if len(sys.argv) > 2 else 300
    rng = random.Random(int(sys.argv[3]) if len(sys.argv) > 3 else 1)
    emu.mu.hook_add(UC_HOOK_CODE, on_counter, begin=COUNTER, end=COUNTER)
    gains = [0.0, 1.0, 2.5, 0.5, 1.75, 3.3333333, 0.1, -1.25, 2.2, 1000.0, 7.9]
    lines = []
    for _ in range(cases):
        base = rng.randint(-40, 400)
        level = rng.randint(0, 25)
        gain = rng.choice(gains) if rng.random() < 0.6 else rng.uniform(-5.0, 20.0)
        gain_bits = struct.unpack("<I", struct.pack("<f", gain))[0]
        counters.clear()
        seen.clear()
        counters[REF_BASE] = base
        counters[REF_LEVEL] = level
        wr32(HERO + 0xDC, gain_bits)
        result = emu.call(INT_CORE, ecx=HERO)
        # The original must read exactly the two references and nothing else.
        assert seen == {REF_BASE, REF_LEVEL}, "unexpected counter references %r" % seen
        res = struct.unpack("<i", struct.pack("<I", result & 0xFFFFFFFF))[0]
        lines.append("%d %d %08x %d" % (base, level, gain_bits, res))
    with open(out_path, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("intelligence cases written: %d -> %s" % (len(lines), out_path))


if __name__ == "__main__":
    main()
