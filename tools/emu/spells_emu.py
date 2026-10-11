"""Unicorn checks of the spell cost pieces of Game.dll 1.26a (docs/spec/abilities-5.md).

ORIGINAL code of the user's Game.dll, run for real with the helpers it calls:
  1. The unit-side mana check `0x6F283C70` (ECX = unit, args: pointer to the cost float, flag).
     The agile mana getter `0x6F4773A0` is replaced by a stub that writes a chosen mana value (the agile
     object needs the world clock). Result: 1 iff mana >= cost as an x87 compare, see the rows.
  2. The mana payment `0x6F283DB0` (ECX = unit, args: out pointer, pointer to cost float, flag 0).
     Mana getter and the property setter `0x6F477350` are stubs (the setter records the float it is handed).
     The soft sub/add/neg/min logic around them runs for real.
  3. The hero test `0x6F1D2660` (Dur versus HeroDur choice of the target-effect slots): the unit type id's
     first byte at +0x33 in 'A'..'Z', or one of two helper results (stubbed).

Usage (the user's own Game.dll):
  GAME_DLL=/path/Game.dll python3 tools/emu/spells_emu.py [--dump OUT_DIR] [CASES] [SEED]

Prints one summary line per check; with --dump writes rows for the Rust vector tests (derived results only).
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402

mu = emu.mu
DATA = emu.DATA
wr, rd = A.wr, A.rd

UNIT = DATA + 0x2000
COSTP = DATA + 0xD000
OUTP = DATA + 0xD010
MANA_IN = DATA + 0xD020
MANA_OUT = DATA + 0xD030
HERO1 = DATA + 0xD040
HERO2 = DATA + 0xD044

F_UNIT_CHECK = 0x6F283C70
F_PAY = 0x6F283DB0
F_HERO = 0x6F1D2660
F_AGILE_GET = 0x6F4773A0
F_AGILE_SET = 0x6F477350
F_HERO_A = 0x6F1D2630
F_HERO_B = 0x6F0766A0


def install():
    A.setup_stubs()
    # agile getter: eax = [esp+4]; [eax] = MANA_IN; ret 4
    code = (b"\x8b\x44\x24\x04" b"\x8b\x15" + struct.pack("<I", MANA_IN) + b"\x89\x10" b"\xc2\x04\x00")
    a = A._stub_next[0]
    mu.mem_write(a, code)
    A._stub_next[0] += 0x20
    A.patch_jump(F_AGILE_GET, a)
    # agile setter: MANA_OUT = [[esp+4]]; ret 4
    code = (b"\x8b\x44\x24\x04" b"\x8b\x10" b"\x89\x15" + struct.pack("<I", MANA_OUT) + b"\xc2\x04\x00")
    a = A._stub_next[0]
    mu.mem_write(a, code)
    A._stub_next[0] += 0x20
    A.patch_jump(F_AGILE_SET, a)
    # hero helpers: eax = [HERO1] / [HERO2]
    for fn, cell, n in ((F_HERO_A, HERO1, 0), (F_HERO_B, HERO2, 0)):
        code = b"\xa1" + struct.pack("<I", cell) + (b"\xc2" + struct.pack("<H", n) if n else b"\xc3")
        a = A._stub_next[0]
        mu.mem_write(a, code)
        A._stub_next[0] += 0x20
        A.patch_jump(fn, a)


def fresh_unit(flags=0):
    mu.mem_write(UNIT, b"\0" * 0x400)
    wr(UNIT + 0x5C, flags)
    wr(UNIT + 0x1DC, 0xFFFFFFFF)
    wr(UNIT + 0x1E0, 0xFFFFFFFF)


SPECIAL = [0x00000000, 0x80000000, 0x7F800000, 0xFF800000, 0x7FC00000, 0xFFC00001, 0x00000001, 0x007FFFFF,
           0x3F800000, 0xBF800000, 0x42960000, 0x43480000]


def rnd_bits(r):
    k = r.random()
    if k < 0.25:
        return r.choice(SPECIAL)
    if k < 0.55:
        v = float(r.randint(0, 400))
        return struct.unpack("<I", struct.pack("<f", v))[0]
    return r.getrandbits(32)


def check_unit(cases, r, rows):
    bad = 0
    for _ in range(cases):
        mana = rnd_bits(r)
        cost = mana if r.random() < 0.1 else rnd_bits(r)
        flag = r.randint(0, 1)
        fresh_unit(0)
        wr(MANA_IN, mana)
        wr(COSTP, cost)
        got = emu.call(F_UNIT_CHECK, ecx=UNIT, stack_args=(COSTP, flag)) & 0xFF
        rows.append((mana, cost, flag, got))
    return bad


def check_pay(cases, r, rows):
    for _ in range(cases):
        mana = rnd_bits(r)
        cost = rnd_bits(r)
        fresh_unit(0)
        wr(MANA_IN, mana)
        wr(MANA_OUT, 0xDEADBEEF)
        wr(COSTP, cost)
        wr(OUTP, 0xCAFEF00D)
        emu.call(F_PAY, ecx=UNIT, stack_args=(OUTP, COSTP, 0))
        rows.append((mana, cost, rd(OUTP), rd(MANA_OUT)))


def check_hero(cases, r, rows):
    for _ in range(cases):
        first = r.choice([r.randint(0, 255), r.randint(60, 95), r.randint(97, 125)])
        h1 = r.randint(0, 1)
        h2 = r.randint(0, 1)
        fresh_unit(0)
        mu.mem_write(UNIT + 0x33, bytes([first]))
        wr(HERO1, h1)
        wr(HERO2, h2)
        got = emu.call(F_HERO, ecx=UNIT) & 0xFF
        want = 1 if ((first - 65) & 0xFF) <= 25 or h1 or h2 else 0
        want = 1 if (65 <= first <= 90) or h1 or h2 else 0
        rows.append((first, h1, h2, got))
        if got != want:
            rows.append(("MISMATCH", first, h1, h2, got, want))


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump = args[1]
        args = args[2:]
    cases = int(args[0]) if args else 2000
    seed = int(args[1]) if len(args) > 1 else 5
    r = random.Random(seed)
    install()
    ur, pr, hr = [], [], []
    check_unit(cases, r, ur)
    check_pay(cases, r, pr)
    check_hero(cases, r, hr)
    hero_bad = [x for x in hr if x[0] == "MISMATCH"]
    print("unit check cases=%d" % len(ur))
    print("payment cases=%d" % len(pr))
    print("hero test cases=%d mismatches=%d" % (len(hr) - len(hero_bad), len(hero_bad)))
    if dump:
        os.makedirs(dump, exist_ok=True)
        for name, rows in (("unit_check", ur), ("payment", pr), ("hero", [x for x in hr if x[0] != "MISMATCH"])):
            with open(os.path.join(dump, name + ".txt"), "w") as f:
                for row in rows:
                    f.write(" ".join("%08x" % v for v in row) + "\n")
    return 1 if hero_bad else 0


if __name__ == "__main__":
    sys.exit(main())
