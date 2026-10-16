"""Unicorn checks of the V8 verification wave (docs/spec/abilities-unit-spells.md section 14).

The ORIGINAL per-unit effect / buff routines run on fake objects (the pattern of spell_buff_emu.py / v4_spells_emu.py);
only world-dependent callees are stubs. Rows are dumped as hex words and re-computed by crates/sim/tests/v8_spell_vectors.rs.

Usage: GAME_DLL=/path/Game.dll py tools/emu/v8_spells_emu.py [--dump DIR] --only NAME [CASES] [SEED]
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

VT_ROAR_ABIL, ABIL_ROAR = 0x6F8A21BC, 0x41726F61
VT_ROAR_BUFF = 0x6F8A1E54


def check_roar_unit(cases, r, rows):
    """Roar's per-unit effect slot 0x3EC (0x6F1FB1F0) -> buff creator 0x6F1FAF90 -> the CBuffRoar apply slot 0x354 (shared with
    Inner Fire, 0x6F12CA40) -> the attack/armor/regeneration helpers. Rows: A B C D | duration argument | weapons (8 words) |
    slot-0x320 arg1 arg2 | counter deltas | stored amounts +0xE0/+0xE4 | armor value, calls | life regen value, calls |
    mana regen value, calls."""
    S.install()
    s320 = S.slot_capture2()
    s324 = A.stub_ret(4, 0)
    A.patch_jump(0x6F1F97F0, A.stub_ret(0, BUFF))
    durbox = S.M + 0x6000
    for _ in range(cases):
        da, db, dc, dd = (rbits(r) for _ in range(4))
        if r.random() < 0.5:
            da = f2u(r.choice([0.1, 0.05, 0.15, 0.25, r.uniform(-0.5, 0.5), 1.0, r.uniform(0, 2)]))
        if r.random() < 0.15:
            dc = 0
        if r.random() < 0.15:
            dd = 0
        dur = rbits(r)
        weap = S.random_weapons(r)
        A.make_ability(VT_ROAR_ABIL, ABIL_ROAR, 75)
        level(A.LVL, 0, 0, [da, db, dc, dd])
        S.make_target(False)
        base0, base1 = r.randint(0, 5), r.randint(0, 5)
        S.make_attack(weap, base0, base1)
        S.make_buff(VT_ROAR_BUFF, (0xCC, 0xD4, 0xE8, 0xF0, 0xF8), (), {0x320: s320, 0x324: s324})
        wr(durbox, dur)
        wr(durbox + 8, 0x1234)
        S.clear_cap()
        S.call(0x6F1FB1F0, ecx=A.OBJ, stack_args=(TGT, durbox + 8, durbox))
        rows.append([da, db, dc, dd, dur] + [x for w in weap for x in w]
                    + [rd(CAP + 0x80), rd(CAP + 0x84), u32(rd(ATK + 0xAC) - base0), u32(rd(ATK + 0xB0) - base1),
                       rd(S.BUFF + 0xE0), rd(S.BUFF + 0xE4), rd(CAP + 0x30), rd(CAP + 0x34), rd(CAP + 0x40), rd(CAP + 0x44),
                       rd(CAP + 0x50), rd(CAP + 0x54)])


CHECKS = {
    "roar_unit": check_roar_unit,
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
    seed = int(args[1]) if len(args) > 1 else 81
    for name, fn in CHECKS.items():
        if only is None or name not in only:
            continue
        r = random.Random(seed)
        rows = []
        fn(cases, r, rows)
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
