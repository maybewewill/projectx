"""Unicorn checks of the channel lifecycle and the Blizzard channel duration of Game.dll 1.26a (S35).

ORIGINAL code of the user's Game.dll, run for real (docs/spec/abilities-human-heroes.md, section 14):
  1. Channel duration of Blizzard: the ability method `0x6F1B9E90` (the point-form parameter hand-over) ends by
     storing a real in the property at ability `+0x90` (the "channel time" the cast core arms the `0xD01BD` timer
     with). Rows: level fields cast, DataA -> the stored value (bits).
  2. The two property compares of the cast tail (`0x6F063760`): `0x6F022270` (property differs from a constant, with
     the 0.001 epsilon, false on NaN) and `0x6F022340` (the same without the NaN test). Rows: property bits,
     constant bits -> result.

Usage: GAME_DLL=/path/Game.dll py tools/emu/human_heroes3_emu.py [--dump OUT_DIR] [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
import human_heroes2_emu as H2  # noqa: E402

mu = emu.mu
wr, rd, f2u = A.wr, A.rd, A.f2u
DATA = emu.DATA

F_ARGS = 0x6F1B9E90
F_SETUP = 0x6F1B9BB0
F_UNIT_FIN = 0x6F077630
F_PROP_NE = 0x6F022270
F_PROP_NE2 = 0x6F022340
FLOATMINI_VT = 0x6F876D38
VT_BLIZZARD = 0x6F910CAC
CONST = DATA + 0x6300


def rbits(r, small=False):
    k = r.random()
    if k < 0.08:
        return r.choice([0, 0x80000000, 0x7F800000, 0x7FC00000, 0x3F800000, 0xBF800000, 1, 0x7F7FFFFF])
    if k < 0.85:
        return f2u(r.choice([r.uniform(0, 12), float(r.randint(0, 12)), r.uniform(-3, 3), r.uniform(0, 1), 1.0]))
    return r.getrandbits(32)


def install():
    H2.install()
    # the 20-argument effect setup is not under test here: return at once, cleaning its stack arguments
    A.patch_jump(F_SETUP, A.stub_ret(0x50, 0))
    A.patch_jump(F_UNIT_FIN, A.stub_ret(0, 0))
    A.patch_jump(0x6F0419C0, A.stub_ret(4, 0))   # agent handle assignment (needs the agent world)


def check_channel_time(cases, r, rows):
    for _ in range(cases):
        cast, data_a = rbits(r), rbits(r)
        A.make_ability(VT_BLIZZARD, 0x41486262, 75)
        L = A.LVL
        wr(L + 0x04, cast)
        wr(L + 0x20, data_a)
        wr(A.OBJ + 0x9C, 0)
        wr(A.OBJ + 0x90, FLOATMINI_VT)
        wr(A.OBJ + 0x94, 0xDEADBEEF)
        mu.mem_write(H2.OBJ2, b"\0" * 0x400)
        wr(H2.BUFFS, 0x1111)
        wr(L + 0x4C, 2)
        wr(L + 0x50, H2.BUFFS)
        wr(H2.EF, 0x3333)
        wr(L + 0x5C, 1)
        wr(L + 0x60, H2.EF)
        emu.call(F_ARGS, ecx=A.OBJ, stack_args=(H2.OBJ2, f2u(1.0), f2u(2.0)))
        rows.append((cast, data_a, rd(A.OBJ + 0x94)))


def check_prop_compare(cases, r, rows):
    obj = DATA + 0x6400
    consts = [0, 0x7F7FFFFF, 0x3F800000]
    for _ in range(cases):
        prop = rbits(r)
        if r.random() < 0.3:
            prop = r.choice([0, 0x7F7FFFFF, f2u(0.0005), f2u(0.001), f2u(0.0011), 0x80000000])
        c = r.choice(consts) if r.random() < 0.7 else rbits(r)
        mu.mem_write(obj, b"\0" * 0x10)
        wr(obj + 4, prop)
        wr(CONST, c)
        a = emu.call(F_PROP_NE, ecx=obj, edx=CONST) & 0xFF
        b = emu.call(F_PROP_NE2, ecx=obj, edx=CONST) & 0xFF
        rows.append((prop, c, a, b))


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 35
    r = random.Random(seed)
    install()
    ch, pc = [], []
    check_channel_time(cases, r, ch)
    check_prop_compare(cases, r, pc)
    print("channel time rows %d, property compare rows %d" % (len(ch), len(pc)))
    for row in ch[:8]:
        print(" ".join("%08x" % x for x in row))
    if dump:
        os.makedirs(dump, exist_ok=True)
        with open(os.path.join(dump, "channel_time.txt"), "w") as fh:
            for row in ch:
                fh.write(" ".join("%08x" % x for x in row) + "\n")
        with open(os.path.join(dump, "prop_compare.txt"), "w") as fh:
            for row in pc:
                fh.write("%08x %08x %d %d\n" % row)


if __name__ == "__main__":
    main()
