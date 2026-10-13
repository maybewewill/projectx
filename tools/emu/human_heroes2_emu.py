"""Unicorn checks of Blizzard and Mass Teleport of Game.dll 1.26a (docs/spec/abilities-human-heroes.md, sections 7, 11, 12).

ORIGINAL code of the user's Game.dll, run for real:
  1. Blizzard parameter mapping: the ability method `0x6F1B9E90` (ECX = a fake CAbilityBlizzard with a real level record) hands
     20 arguments to the effect setup `0x6F1B9BB0`; the run stops there and the arguments (and the reals they point to)
     are read. Rows: level fields (cast, dur, herodur, A..F, area) -> waves, interval, damage, cap, shards, building factor,
     DataE, Dur sum, HeroDur, area.
  2. Blizzard shard placement `0x6F1AF6E0` (ECX = a fake effect with real FloatMini property objects): the global
     generator shuffle (`0x6F4AEE00`) and the stream-4 draws; the shard launcher `0x6F1A1000` is stubbed. Rows: states,
     count, area -> permutation and the (x, y) offset property of every shard.
  3. Mass Teleport sort comparator `0x6F026120` on two 20-byte records.
  4. Order ids: vtable slot `0x308` of the human hero abilities.

Usage: GAME_DLL=/path/Game.dll py tools/emu/human_heroes2_emu.py [--dump OUT_DIR] [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ECX  # noqa: E402

mu = emu.mu
DATA = emu.DATA
wr, rd = A.wr, A.rd
f2u = A.f2u

VT_BLIZZARD = 0x6F910CAC
F_ARGS = 0x6F1B9E90
F_SETUP = 0x6F1B9BB0
F_PLACE = 0x6F1AF6E0
F_LAUNCH = 0x6F1A1000
F_CMP = 0x6F026120
FLOATMINI_VT = 0x6F876D38
STREAM_BASE = DATA + 0xE000
GADDR = emu.GADDR
OBJ2 = DATA + 0x5000
BUFFS = DATA + 0x3400
EF = DATA + 0x3440


def rbits(r):
    k = r.random()
    if k < 0.1:
        return r.choice([0, 0x80000000, 0x7F800000, 0x7FC00000, 0x3F800000, 0xBF800000, 1])
    if k < 0.8:
        return f2u(r.choice([r.uniform(0, 300), float(r.randint(0, 12)), r.uniform(-5, 5), r.uniform(0, 1)]))
    return r.getrandbits(32)


def install():
    A.setup_stubs()
    wr(A.TIB_INNER + 0xC, STREAM_BASE)
    A.patch_jump(F_LAUNCH, A.stub_ret(0, 0))


def check_args(cases, r, rows):
    for _ in range(cases):
        v = [rbits(r) for _ in range(10)]   # cast dur herodur A B C D E F area
        A.make_ability(VT_BLIZZARD, 0x41486262, 75)
        L = A.LVL
        for off, val in zip((0x04, 0x08, 0x0C, 0x20, 0x24, 0x28, 0x2C, 0x30, 0x34, 0x18), v):
            wr(L + off, val)
        wr(BUFFS, 0x1111)
        wr(BUFFS + 4, 0x2222)
        wr(L + 0x4C, 2)
        wr(L + 0x50, BUFFS)
        wr(EF, 0x3333)
        wr(L + 0x5C, 1)
        wr(L + 0x60, EF)
        wr(A.OBJ + 0x9C, 0)
        mu.mem_write(OBJ2, b"\0" * 0x400)
        hit = {}

        def cb(uc, addr, size, user):
            esp = uc.reg_read(UC_X86_REG_ESP)
            args = [struct.unpack("<I", bytes(uc.mem_read(esp + 4 + 4 * i, 4)))[0] for i in range(20)]
            hit["a"] = args
            hit["d"] = {i: struct.unpack("<I", bytes(uc.mem_read(a, 4)))[0] for i, a in enumerate(args)
                        if 0x100000 <= a < 0x110000}
            uc.emu_stop()

        h = mu.hook_add(UC_HOOK_CODE, cb, begin=F_SETUP, end=F_SETUP)
        try:
            emu.call(F_ARGS, ecx=A.OBJ, stack_args=(OBJ2, f2u(1.0), f2u(2.0)))
        except RuntimeError:
            pass
        mu.hook_del(h)
        a, d = hit["a"], hit["d"]
        rows.append(tuple(v) + (d[3], a[4], d[5], d[6], d[7], a[9], d[11], d[15], d[16], d[17]))


def check_place(cases, r, rows):
    for _ in range(cases):
        n = r.randint(1, 32)
        area = rbits(r) if r.random() < 0.3 else f2u(r.uniform(50, 600))
        ga, gb, sa, sb = (r.getrandbits(32) for _ in range(4))
        mu.mem_write(OBJ2, b"\0" * 0x400)
        for i in range(33):
            wr(OBJ2 + 0x150 + 8 * i, FLOATMINI_VT)
            wr(OBJ2 + 0x254 + 8 * i, FLOATMINI_VT)
        wr(OBJ2 + 0x98, n)
        wr(OBJ2 + 0x74, area)
        mu.mem_write(GADDR, struct.pack("<II", ga, gb))
        mu.mem_write(STREAM_BASE + 4 + 8 * 4, struct.pack("<II", sa, sb))
        emu.call(F_PLACE, ecx=OBJ2)
        perm = [rd(OBJ2 + 0xCC + 4 * i) for i in range(n)]
        xs = [rd(OBJ2 + 0x154 + 8 * i) for i in range(n)]
        ys = [rd(OBJ2 + 0x258 + 8 * i) for i in range(n)]
        g2 = struct.unpack("<II", bytes(mu.mem_read(GADDR, 8)))
        s2 = struct.unpack("<II", bytes(mu.mem_read(STREAM_BASE + 4 + 8 * 4, 8)))
        rows.append([ga, gb, sa, sb, n, area] + list(g2) + list(s2) + perm + xs + ys)


def check_cmp(cases, r, rows):
    ua, ub = DATA + 0x6000, DATA + 0x6100
    ra, rb = DATA + 0x6200, DATA + 0x6300
    for _ in range(cases):
        def rec():
            small = lambda: r.choice([0, 1, 1, 2, r.randint(0, 5)])
            return [r.randint(0, 3) if r.random() < 0.5 else r.randint(0, 900000),
                    small() if r.random() < 0.6 else r.randint(0, 100), small() & 1, small() & 1, r.randint(0, 4)]
        a, b = rec(), rec()
        for u, rr, x in ((ua, ra, a), (ub, rb, b)):
            mu.mem_write(u, b"\0" * 0x20)
            wr(u + 0xC, x[4])
            wr(rr, u)
            wr(rr + 4, x[0])
            wr(rr + 8, x[1])
            wr(rr + 0xC, x[2])
            wr(rr + 0x10, x[3])
        res = emu.call(F_CMP, stack_args=(ra, rb))
        res = res - (1 << 32) if res >= 1 << 31 else res
        rows.append(tuple(a) + tuple(b) + (1 if res > 0 else (-1 if res < 0 else 0),))


ORDER_CLASSES = [
    ("Blizzard", 0x6F910CAC), ("MassTeleport", 0x6F88BEFC), ("ThunderClap", 0x6F8F67C4), ("Avatar", 0x6F90998C),
    ("DivineShield", 0x6F894AEC), ("Resurrection", 0x6F91C1CC), ("WaterElemental", 0x6F89BF84),
    ("HolyBolt", 0x6F8B1B4C), ("ThunderBolt", 0x6F8F56DC),
]


def check_orders():
    for name, vt in ORDER_CLASSES:
        fn = rd(vt + 0x308)
        code = bytes(mu.mem_read(fn, 6))
        val = struct.unpack("<I", code[1:5])[0] if code[0] == 0xB8 else None
        print("order id %-15s slot 0x308 -> %s (%s)" % (name, hex(val) if val is not None else "?", val))


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 30
    r = random.Random(seed)
    install()
    a_rows, p_rows, c_rows = [], [], []
    check_args(cases, r, a_rows)
    check_place(cases, r, p_rows)
    check_cmp(cases, r, c_rows)
    check_orders()
    print("rows: args %d, placement %d, compare %d" % (len(a_rows), len(p_rows), len(c_rows)))
    if dump:
        os.makedirs(dump, exist_ok=True)
        for name, rows in (("blizzard_args.txt", a_rows), ("blizzard_place.txt", p_rows)):
            with open(os.path.join(dump, name), "w") as fh:
                for row in rows:
                    fh.write(" ".join("%08x" % (x & 0xFFFFFFFF) for x in row) + "\n")
        with open(os.path.join(dump, "mt_compare.txt"), "w") as fh:
            for row in c_rows:
                fh.write(" ".join(str(x) for x in row) + "\n")


if __name__ == "__main__":
    main()
