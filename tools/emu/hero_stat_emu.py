"""Unicorn check of the hero strength and agility getters (Game.dll 1.26a, docs/spec/heroes.md section 5).

`0x6F2777B0` (strength) and `0x6F2778B0` (agility) read the base at hero+0x94 / hero+0xA8 and the per-level gain at
hero+0xD4 / hero+0xE4 (reals), and return base + trunc(gain * counter) with the counter read by `0x6F473170`. That
counter read is replaced by a hook that returns the synthetic counter, so only the arithmetic of the original runs.
The test calls each getter with includeBonus = 1 (the item bonus is then not subtracted).

The output is one line per case: `KIND base gain_bits counter result` (KIND is STR or AGI; numbers are decimal,
gain_bits in hex). Compare with `sim::heroes::attribute` (crates/sim/tests/hero_stat_vectors.rs).

Run:  GAME_DLL=/path/to/Game.dll python3 tools/emu/hero_stat_emu.py OUT.txt [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_EAX  # noqa: E402

COUNTER = 0x6F473170   # returns the level counter of the object reached through ECX (stubbed)
STR_GETTER = 0x6F2777B0  # __thiscall(unit), [esp+4] = includeBonus, ret 4
AGI_GETTER = 0x6F2778B0
UNIT = emu.DATA + 0x1000
HERO = emu.DATA + 0x2000
state = {"counter": 0}


def rd32(a):
    return struct.unpack("<I", bytes(emu.mu.mem_read(a, 4)))[0]


def wr32(a, v):
    emu.mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def on_counter(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    uc.reg_write(UC_X86_REG_EAX, state["counter"] & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_ESP, esp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    out_path = sys.argv[1]
    cases = int(sys.argv[2]) if len(sys.argv) > 2 else 400
    rng = random.Random(int(sys.argv[3]) if len(sys.argv) > 3 else 1)
    emu.mu.hook_add(UC_HOOK_CODE, on_counter, begin=COUNTER, end=COUNTER)
    gains = [0.0, 1.0, 2.5, 0.5, 1.75, 3.3333333, 0.1, -1.25, 2.2, 1000.0, 7.9]
    lines = []
    for _ in range(cases):
        for kind, base_off, gain_off, getter in (("STR", 0x94, 0xD4, STR_GETTER),
                                                 ("AGI", 0xA8, 0xE4, AGI_GETTER)):
            base = rng.randint(-40, 400)
            gain = rng.choice(gains) if rng.random() < 0.6 else rng.uniform(-5.0, 20.0)
            gain_bits = struct.unpack("<I", struct.pack("<f", gain))[0]
            counter = rng.randint(0, 25)
            wr32(UNIT + 0x1F0, HERO)
            wr32(HERO + base_off, base)
            wr32(HERO + gain_off, gain_bits)
            state["counter"] = counter
            result = emu.call(getter, ecx=UNIT, stack_args=(1,))
            lines.append("%s %d %08x %d %d" % (kind, base, gain_bits, counter, struct.unpack("<i", struct.pack("<I", result & 0xFFFFFFFF))[0]))
    with open(out_path, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("stat cases written: %d -> %s" % (len(lines), out_path))


if __name__ == "__main__":
    main()
