"""Unicorn run of the base damage the Command Aura (`AOac`) multiplies (docs/spec/abilities-orc-heroes.md section 18,
task S39).

The aura apply routine `0x6F1ACC40` computes, per weapon slot `w`, `F2I(add(mul(DataA, X), 0.5))` where
`X = 0x6F26D5F0(unit, &out, w)` and `0x6F26D5F0` forwards to `0x6F0C6D00(attack ability, &out, w)` of the unit's attack
ability (`unit+0x1E8`). This harness runs both ORIGINAL routines on fake objects (the real soft-float helpers):

  B <dice count> <sides> <bonus> <weapon> | <result bits>                     0x6F0C6D00 on a fake attack ability
  U <has attack ability 0/1> <dice count> <sides> <bonus> <weapon> | <result bits>   0x6F26D5F0 on a fake unit
  A <DataA bits> <dice count> <sides> <bonus> | <amount>                      the whole `F2I(add(mul(DataA, X), 0.5))`

Layout of the fake attack ability (as read by the routine): `+0x88 + 4w` dice count, `+0x94 + 4w` sides, `+0xA0 + 4w` base
bonus (`GetDamageAmount` `0x6F0C6940` rolls `count`, `sides` and `bonus + [+0xAC + 4w]`).

Usage: GAME_DLL=/path/Game.dll py tools/emu/cmdaura_emu.py OUT.txt COUNT SEED
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402

mu = emu.mu
D = emu.DATA
ATT = D + 0xA000
UNIT = D + 0xA400
OUT = D + 0xA800
M = 0xFFFFFFFF


def w32(a, v):
    mu.mem_write(a, struct.pack("<I", v & M))


def r32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def fbits(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    lines = []
    for _ in range(count):
        n = r.choice([0, 1, 1, 2, 3, 5, r.randint(0, 40), r.randint(-3, 100)])
        sides = r.choice([0, 1, 2, 4, 6, 8, 10, r.randint(0, 20), r.randint(-3, 400)])
        bonus = r.choice([0, 0, r.randint(-5, 80), r.randint(-100000, 100000)])
        w = r.choice([0, 1])
        mu.mem_write(ATT, bytes(0x300))
        w32(ATT + 0x88 + 4 * w, n)
        w32(ATT + 0x94 + 4 * w, sides)
        w32(ATT + 0xA0 + 4 * w, bonus)
        mu.mem_write(OUT, bytes(16))
        got = emu.call(0x6F0C6D00, ecx=ATT, stack_args=(OUT, w))
        assert got == OUT
        lines.append("B %d %d %d %d | %08x" % (n, sides, bonus, w, r32(OUT)))
        # the unit wrapper (with and without an attack ability)
        has = r.choice([0, 1, 1])
        mu.mem_write(UNIT, bytes(0x300))
        w32(UNIT + 0x1E8, ATT if has else 0)
        mu.mem_write(OUT, b"\xAA" * 16)
        got = emu.call(0x6F26D5F0, ecx=UNIT, stack_args=(OUT, w))
        lines.append("U %d %d %d %d %d | %08x" % (has, n, sides, bonus, w, r32(OUT)))
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote %d lines" % len(lines))


if __name__ == "__main__":
    main()
