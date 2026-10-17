"""Unicorn checks of the V9 verification wave (docs/spec/abilities-v9.md).

The ORIGINAL per-unit effect / buff routines run on fake objects (the pattern of spell_buff_emu.py / v8_spells_emu.py);
only world-dependent callees are stubs. Rows are dumped as hex words and re-computed by crates/sim/tests/v9_vectors.rs.

Usage: GAME_DLL=/path/Game.dll py tools/emu/v9_emu.py [--dump DIR] --only NAME [CASES] [SEED]
(each check patches functions of the image for good, so run one check per process.)
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import spell_buff_emu as S  # noqa: E402
import v4_spells_emu as V4  # noqa: E402
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402

mu = emu.mu
wr, rd, f2u = A.wr, A.rd, A.f2u
a32, new_code, level, rbits, u32 = S.a32, S.new_code, S.level, S.rbits, S.u32
CAP, BUFF, TGT, ATK = S.CAP, S.BUFF, S.TGT, S.ATK
VARS = V4.VARS
recorder = V4.recorder

VT_FRENZY_ABIL, ABIL_FRENZY = 0x6F90F2CC, 0x4166 << 16 | 0x7A79      # 'Afzy'
VT_FRENZY_BUFF = 0x6F90363C


def check_frenzy(cases, r, rows):
    """Frenzy (CAbilityFrenzy < Bloodlust, self cast): the no-target effect slot 0x428 (0x6F1CB1D0) -> creator 0x6F1CB150 ->
    CBuffFrenzy apply slot 0x35C (0x6F1C2870) -> the shared sink routines. Rows: dur herodur DataA DataB DataC extra hero |
    slot-0x320 arg1 arg2 | movement sink value, calls | attack speed sink value, calls."""
    S.install()
    ret0 = A.stub_ret(0, 0)
    s320 = S.slot_capture2()
    s324 = A.stub_ret(4, 0)
    A.patch_jump(0x6F1C8870, A.stub_ret(0, BUFF))
    for _ in range(cases):
        dur, hdur, da, db, dc = rbits(r), rbits(r), rbits(r), rbits(r), rbits(r)
        extra = rbits(r) if r.random() < 0.3 else 0
        hero = r.random() < 0.5
        A.make_ability(VT_FRENZY_ABIL, ABIL_FRENZY, 75)
        level(A.LVL, dur, hdur, [da, db, dc])
        wr(A.OBJ + 0x9C, extra)
        S.make_target(hero)
        wr(A.OBJ + 0x30, TGT)
        S.make_buff(VT_FRENZY_BUFF, (0xCC, 0xD4, 0xFC, 0x104), (), {0x320: s320, 0x324: s324, 0x360: ret0})
        S.clear_cap()
        S.call(0x6F1CB1D0, ecx=A.OBJ)
        rows.append([dur, hdur, da, db, dc, extra, int(hero), rd(CAP + 0x80), rd(CAP + 0x84),
                     rd(CAP + 0x10), rd(CAP + 0x14), rd(CAP + 0x20), rd(CAP + 0x24)])


CHECKS = {
    "frenzy": check_frenzy,
}


def main():
    args = sys.argv[1:]
    dump = None
    only = None
    while args and args[0] in ("--dump", "--only"):
        if args[0] == "--dump":
            dump, args = args[1], args[2:]
        else:
            only, args = args[1].split(","), args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 91
    for name, fn in CHECKS.items():
        if only is None or name not in only:
            continue
        r = random.Random(seed)
        rows = []
        ring = V4._trace_install() if os.environ.get("V4_TRACE") else None
        try:
            fn(cases, r, rows)
        except Exception:
            if ring:
                sys.stderr.write("last: %s\n" % " ".join("%x" % a for a in ring))
            raise
        print("%s rows %d" % (name, len(rows)))
        for row in rows[:5]:
            print(" ".join("%08x" % x for x in row))
        if dump:
            os.makedirs(dump, exist_ok=True)
            with open(os.path.join(dump, name + ".txt"), "w") as fh:
                for row in rows:
                    fh.write(" ".join("%08x" % x for x in row) + "\n")


if __name__ == "__main__":
    main()
