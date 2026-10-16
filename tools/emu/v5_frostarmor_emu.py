"""V5 Unicorn run of Frost Armor (`AUfa`, `CAbilityFrostArmor` vtable 0x6F8AFD64, unit-target slot 0x424 = 0x6F0BA520) of
Game.dll 1.26a (docs/spec/abilities-undead-ne-heroes.md, V5).

ORIGINAL code: the effect `0x6F0BA520` (real level getters DataA `0x6F02E9E0`, DataB `0x6F02EA50`, Dur `0x6F02E760`, HeroDur
`0x6F02E7D0`, the hero test `0x6F1D2660`, soft add `0x6F6EF000`) and the buff creator tail `0x6F0BA2E0` run on a fake ability and
a fake target. Stubbed: the info builder `0x6F042B20`, the buff-id getters, the existing-buff lookup `0x6F0787D0` (none), the buff
allocator `0x6F0B5BD0` (returns a fake buff whose vtable slots 0x324 / 0x354 record their arguments), the registration
`0x6F079CA0`. The recorded apply arguments are the numbers the real `CBuffFrostArmor` apply `0x6F0A1280` stores
(holder +0xCC <- arg2, holder +0xD4 <- arg3, +0xDC <- arg4, the duration reaches the base apply `0x320`).

Row:  F <DataA> <DataB> <Dur> <HeroDur> <+0x9C> <hero> | <apply calls> <arg values: a1 a2 a3 a4 a5 as pointee-or-raw words>
Usage: GAME_DLL=/path/Game.dll py tools/emu/v5_frostarmor_emu.py [--dump FILE] [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
import spell_buff_emu as S  # noqa: E402

mu = emu.mu
wr, rd, f2u = A.wr, A.rd, A.f2u

VT_FROST_ARMOR = 0x6F8AFD64
VT_BUFF = 0x6F8A10A4
F_EFFECT = 0x6F0BA520
F_ALLOC = 0x6F0B5BD0
F_LOOKUP = 0x6F0787D0
F_REG = 0x6F079CA0
F_ID = 0x6F082080

LOG = S.M + 0x9000


def install():
    S.install()
    A.patch_jump(F_LOOKUP, A.stub_ret(0x14, 0))
    A.patch_jump(F_REG, A.stub_ret(4, 0))
    A.patch_jump(F_ID, A.stub_ret(0, 0x42554661))
    mu.mem_write(S.FVT, bytes(mu.mem_read(VT_BUFF, 0x400)))
    # slot 0x324 (init, 1 arg): ret 4 ; slot 0x354: records 5 stack words and the pointees of words 2..5 (value or 0)
    wr(S.FVT + 0x324, A.stub_ret(4, 0))
    rec = []
    for i in range(5):
        rec.append(b"\x8B\x44\x24" + bytes([4 + 4 * i]) + b"\xA3" + S.a32(LOG + 4 * i))      # mov eax,[esp+N]; mov [LOG+4i],eax
        if 1 <= i <= 3:
            rec.append(b"\x8B\x00\xA3" + S.a32(LOG + 0x20 + 4 * i))                           # mov eax,[eax]; mov [LOG+0x20+4i],eax
    code = b"".join(rec) + b"\xFF\x05" + S.a32(LOG + 0x40) + b"\xC2\x14\x00"
    wr(S.FVT + 0x354, S.new_code(code))
    wr(S.BUFF, S.FVT)
    A.patch_jump(F_ALLOC, A.stub_ret(0, S.BUFF))


def run(cases, r):
    install()
    rows = []
    for _ in range(cases):
        da, db, dur, hdur = S.rbits(r), S.rbits(r), S.rbits(r), S.rbits(r)
        extra = S.rbits(r) if r.random() < 0.3 else 0
        hero = r.random() < 0.5
        A.make_ability(VT_FROST_ARMOR, 0x41556661, 75)
        S.level(A.LVL, dur, hdur, [da, db])
        wr(A.OBJ + 0x9C, extra)
        S.make_target(hero)
        mu.mem_write(LOG, b"\0" * 0x80)
        S.call(F_EFFECT, ecx=A.OBJ, stack_args=(S.TGT,))
        rows.append((da, db, dur, hdur, extra, int(hero), rd(LOG + 0x40), [rd(LOG + 4 * i) for i in range(5)],
                     [0] + [rd(LOG + 0x20 + 4 * i) for i in range(1, 5)]))
    return rows


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 5
    rows = run(cases, random.Random(seed))
    for da, db, dur, hdur, extra, hero, n, raw, deref in rows[:6]:
        print("A=%08x B=%08x dur=%08x hdur=%08x ex=%08x hero=%d | calls=%d raw=%s deref=%s" % (
            da, db, dur, hdur, extra, hero, n, " ".join("%08x" % x for x in raw), " ".join("%08x" % x for x in deref)))
    if dump:
        with open(dump, "w") as fh:
            for da, db, dur, hdur, extra, hero, n, raw, deref in rows:
                fh.write("F %08x %08x %08x %08x %08x %d | %d %s | %s\n" % (
                    da, db, dur, hdur, extra, hero, n, " ".join("%08x" % x for x in raw),
                    " ".join("%08x" % x for x in deref[1:])))
    print("cases", len(rows))


if __name__ == "__main__":
    main()
