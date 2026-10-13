"""Unicorn check of the Critical Strike (`AOcr`) chance machinery of Game.dll 1.26a (docs/spec/abilities-orc-heroes.md).

Original code that runs:
  0x6F1D21F0   the pseudo-random-distribution (PRD) table lookup: ECX = result pointer, EDX = pointer to the nominal
               chance c; result = table[clamp(f2i(add(mul(c, K1), K2)), 0, 20)]; the 21-entry table is built by the same
               function on its first call (static guard at 0x6FAB314C)
  0x6F1A3470   the effective chance of `CAbilityCriticalStrike`: ECX = ability, stack = result pointer;
               c = DataA of the current level (0x6F02E9E0), in PERCENT units (15.0 = 15 %), n = the ability's hit counter (+0x84),
               result = mul(I2R(n), 0x6F1D21F0(c))

Output (text, one case per line, derived numbers only):
  T <chance bits> <result bits>                     the table lookup
  E <chance bits> <count> <result bits>              the effective chance (level record DataA = chance bits)
  K <K1 bits> <K2 bits>                              the two constants used by the index formula (runtime values)
  W <index> <entry bits>                             the 21 table entries

Usage: GAME_DLL=/path/Game.dll py tools/emu/crit_emu.py OUT.txt COUNT SEED
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
FS_SCRATCH = D + 0xC000
F_TABLE = 0x6F1D21F0
F_CHANCE = 0x6F1A3470

OUT = D + 0x1000
IN = D + 0x1010
ABIL = D + 0x2000
DEF = D + 0x3000
LVL = D + 0x3200


def w32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def r32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def table_lookup(bits):
    w32(IN, bits)
    emu.call(F_TABLE, ecx=OUT, edx=IN)
    return r32(OUT)


def effective(chance_bits, count):
    mu.mem_write(ABIL, bytes(0x100))
    mu.mem_write(DEF, bytes(0x100))
    mu.mem_write(LVL, bytes(0x400))
    w32(ABIL + 0x50, 0)          # level index 0
    w32(ABIL + 0x54, DEF)        # definition
    w32(ABIL + 0x84, count)      # hit counter
    w32(DEF + 0x2C, 1)           # has level data
    w32(DEF + 0x50, 3)           # level count
    w32(DEF + 0x54, LVL)         # level records
    w32(LVL + 0x20, chance_bits)  # DataA of level 0
    emu.call(F_CHANCE, ecx=ABIL, stack_args=(OUT,))
    return r32(OUT)


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    rng = random.Random(seed)
    mu.msr_write(0xC0000100, FS_SCRATCH)
    lines = []
    lines.append(f"K {r32(0x6FAAE4B0):08x} {r32(0x6FAAE4D0):08x}")
    special = [0x00000000, 0x80000000, 0x3F800000, 0x3F000000, 0x7FC00000, 0xFFC00000, 0x7F800000, 0xFF800000,
               0xBF800000, 0x3D4CCCCD, 0x3DCCCCCD, 0x3E4CCCCD, 0x3E99999A, 0x3F4CCCCD, 0x3F666666, 0x3F7FFFFF, 0x41700000, 0x41A00000, 0x41200000, 0x42C80000, 0x41F00000, 0x42480000]
    w32(0x6FAB314C, 0)
    # prime the table (first call builds it), then dump the 21 entries by probing c = 5*i percent
    table_lookup(0)
    for i in range(21):
        bits = struct.unpack("<I", struct.pack("<f", i * 5.0))[0]
        lines.append(f"W {i} {table_lookup(bits):08x}")
    for n in range(count):
        mode = rng.random()
        if mode < 0.15:
            bits = rng.choice(special)
        elif mode < 0.7:
            bits = struct.unpack("<I", struct.pack("<f", rng.random() * 110.0 - 5.0))[0]
        else:
            bits = rng.getrandbits(32)
        lines.append(f"T {bits:08x} {table_lookup(bits):08x}")
        cnt = rng.choice([1, 1, 2, 3, 5, 9, 20, 100, 0, -1, rng.randrange(0, 1 << 20)])
        lines.append(f"E {bits:08x} {cnt & 0xFFFFFFFF:08x} {effective(bits, cnt):08x}")
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print(f"wrote {len(lines)} lines")


if __name__ == "__main__":
    main()
