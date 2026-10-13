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


F_WAVE_UNIT = 0x6F1AF950
EFFECT = DATA + 0x9000      # fake CEffectBlizzard
EVT = DATA + 0x9400         # its vtable (slots 0x80, 0x84, 0x8C are code stubs)
TARGET = DATA + 0x9800      # fake target unit
BUFFOBJ = DATA + 0x9C00     # fake existing BHbz buff (property at +0xCC, value at +0xD0)
BVT = DATA + 0xA000         # its vtable (slot 0x350 stub)
CAPT = DATA + 0xA400        # capture cells
DPTR = DATA + 0xA480        # the damage cell passed by pointer
CODE = DATA + 0xA800


def asm_stubs():
    """Machine code for the three effect slots and the buff's set-duration slot (records its arguments)."""
    def a32(x):
        return struct.pack("<I", x)
    # slot 0x8C: mov eax,[CAPT+0x20]; ret
    neg = bytes([0xA1]) + a32(CAPT + 0x20) + b"\xC3"
    # slot 0x84 (unit, &interval, &damage): record unit, damage, interval, bump the call counter; return the buff
    s84 = (b"\x8B\x44\x24\x04" + b"\xA3" + a32(CAPT + 0x00)
           + b"\x8B\x44\x24\x0C\x8B\x00" + b"\xA3" + a32(CAPT + 0x04)
           + b"\x8B\x44\x24\x08\x8B\x00" + b"\xA3" + a32(CAPT + 0x08)
           + b"\xA1" + a32(CAPT + 0x0C) + b"\x40" + b"\xA3" + a32(CAPT + 0x0C)
           + b"\xA1" + a32(CAPT + 0x24) + b"\xC2\x0C\x00")
    # slot 0x80: ret 0xC (the optional debuff; the numeric test keeps its durations at 0)
    s80 = b"\xC2\x0C\x00"
    # buff slot 0x350 (set duration, two arguments): mov [CAPT+0x10],1; ret 8
    s350 = b"\xC7\x05" + a32(CAPT + 0x10) + a32(1) + b"\xC2\x08\x00"
    # the smart-pointer assignment 0x6F038450 does a run-time type check through the agent registry; replace it by
    # its effect (store the pointer, add one reference), `ret 4`
    sp = b"\x8B\x44\x24\x04\x89\x01\x85\xC0\x74\x03\xFF\x40\x04\x8B\xC1\xC2\x04\x00"
    mu.mem_write(CODE + 0x200, sp)
    A.patch_jump(0x6F038450, CODE + 0x200)
    mu.mem_write(CODE + 0x240, sp[:-3] + b"\xC2\x08\x00")
    A.patch_jump(0x6F042B20, CODE + 0x240)   # the same for the buff reference (two arguments)
    mu.mem_write(CODE, neg)
    mu.mem_write(CODE + 0x40, s84)
    mu.mem_write(CODE + 0x100, s80)
    mu.mem_write(CODE + 0x140, s350)
    mu.mem_write(EVT, b"\0" * 0x400)
    wr(EVT + 0x8C, CODE)
    wr(EVT + 0x84, CODE + 0x40)
    wr(EVT + 0x80, CODE + 0x100)
    mu.mem_write(BVT, b"\0" * 0x400)
    wr(BVT + 0x350, CODE + 0x140)


def check_wave_unit(cases, r, rows):
    asm_stubs()
    for _ in range(cases):
        n = r.choice([1, 1, 2, 3, 4, 5, 8, 12, r.randint(1, 60)])
        cap = r.choice([0, f2u(0.0), f2u(r.uniform(1, 300)), f2u(r.uniform(-5, 5)), f2u(float(r.randint(1, 200)))]) \
            if r.random() < 0.9 else rbits(r)
        d = rbits(r) if r.random() < 0.15 else f2u(r.choice([r.uniform(1, 80), float(r.randint(1, 60))]))
        building = rbits(r) if r.random() < 0.2 else f2u(r.choice([0.5, 0.25, 1.0, r.uniform(0, 1.5)]))
        structure = r.random() < 0.3
        ward = r.random() < 0.15
        negate = int(r.random() < 0.2)
        has_buff = r.random() < 0.5
        old = rbits(r) if r.random() < 0.15 else f2u(r.choice([r.uniform(0, 80), float(r.randint(0, 60))]))
        mu.mem_write(EFFECT, b"\0" * 0x400)
        wr(EFFECT, EVT)
        wr(EFFECT + 0x80, f2u(1.0))
        wr(EFFECT + 0x94, cap)
        wr(EFFECT + 0xAC, building)
        for off in (0x35C, 0x364):             # the optional debuff durations: properties with value 0.0
            wr(EFFECT + off, FLOATMINI_VT)
            wr(EFFECT + off + 4, 0)
        mu.mem_write(TARGET, b"\0" * 0x400)
        wr(TARGET + 4, 100)                    # reference count
        mu.mem_write(TARGET + 0x33, b"A")      # a hero id: the hero branch needs no helper
        wr(TARGET + 0x5C, 0x10000 if structure else 0)
        wr(TARGET + 0x20, 0x200000 if ward else 0)
        mu.mem_write(BUFFOBJ, b"\0" * 0x200)
        wr(BUFFOBJ, BVT)
        wr(BUFFOBJ + 4, 100)
        wr(BUFFOBJ + 0xCC, FLOATMINI_VT)
        wr(BUFFOBJ + 0xD0, old)
        mu.mem_write(CAPT, b"\0" * 0x40)
        wr(CAPT + 0x20, negate)
        wr(CAPT + 0x24, BUFFOBJ if has_buff else 0)
        dptr = DPTR
        wr(dptr, d)
        emu.call(F_WAVE_UNIT, ecx=EFFECT, stack_args=(TARGET, n, dptr))
        out = [rd(CAPT + 0x04), rd(CAPT + 0x00), rd(CAPT + 0x0C), rd(BUFFOBJ + 0xD0), rd(CAPT + 0x10), rd(CAPT + 0x08)]
        rows.append([n, cap, d, building, int(structure), int(ward), negate, int(has_buff), old] + out)


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 35
    r = random.Random(seed)
    install()
    ch, pc, wv = [], [], []
    check_channel_time(cases, r, ch)
    check_prop_compare(cases, r, pc)
    check_wave_unit(cases, r, wv)
    print("channel time rows %d, property compare rows %d, wave unit rows %d" % (len(ch), len(pc), len(wv)))
    for row in wv[:10]:
        print(" ".join("%08x" % x for x in row))
    if dump:
        os.makedirs(dump, exist_ok=True)
        with open(os.path.join(dump, "channel_time.txt"), "w") as fh:
            for row in ch:
                fh.write(" ".join("%08x" % x for x in row) + "\n")
        with open(os.path.join(dump, "prop_compare.txt"), "w") as fh:
            for row in pc:
                fh.write("%08x %08x %d %d\n" % row)
        with open(os.path.join(dump, "wave_unit.txt"), "w") as fh:
            for row in wv:
                fh.write(" ".join("%08x" % x for x in row) + "\n")


if __name__ == "__main__":
    main()
