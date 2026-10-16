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


VT_UF_BUFF = 0x6F8EA55C
OWNER = S.M + 0x7800      # the owner object handed to the buff (vtable slot 0xEC = player index)
OWNVT = S.M + 0x7900
recorder = V4.recorder


def check_unholy_buff(cases, r, rows):
    """CBuffUnholyFrenzy new-buff apply slot 0x378 (0x6F177340) -> CBuffDoT apply 0x6F0D5390 / 0x6F0D5250 and the attack speed
    refresh 0x6F1707B0 (all real). Stubs: timers (record the delay), smart reference assign, the attack speed sink 0x6F0C9090
    (records value and flag), the Misc bool reader. Rows: dur DataA DataB player illusion misc_flag has_attack pre_flags |
    stored amount (+0xEC), stored attack speed (+0x100), player (+0x104), buff flags (+0x20) | slot 0x320 arg1 arg2 |
    timer delay, calls | sink value, flag, calls."""
    S.install()
    A.patch_jump(0x6F0419C0, A.stub_ret(4, 0))
    A.patch_jump(0x6F477D30, A.stub_ret(0, 0))
    A.patch_jump(0x6F4778F0, A.stub_ret(0x14, 0))
    A.patch_jump(0x6F477A80, recorder(0x90, [(0, True), (1, False)], 0x14, counter=0x1D0))
    A.patch_jump(0x6F0C9090, recorder(0x98, [(0, True), (1, False)], 8, counter=0x1D4))
    V4.misc_int_stub(0x6F009F90, 0xC)
    mu.mem_write(OWNVT, b"\0" * 0x200)
    wr(OWNVT + 0xEC, new_code(b"\xA1" + a32(VARS + 0x10) + b"\xC3"))
    s320 = S.slot_capture2()
    durbox = S.M + 0x6000
    for _ in range(cases):
        dur, da, db = rbits(r), rbits(r), rbits(r)
        player = r.randint(0, 15)
        illusion = r.random() < 0.3
        misc = r.random() < 0.5
        has_attack = r.random() < 0.8
        pre = r.choice([0, 0, 0x200, 0x100, 0x8000000])
        S.make_target(False)
        wr(TGT + 0x5C, 0x40000000 if illusion else 0)
        if not has_attack:
            wr(TGT + 0x1E8, 0)
        S.make_buff(VT_UF_BUFF, (0xE8, 0xFC), (), {0x320: s320})
        wr(BUFF + 0x20, pre)
        mu.mem_write(OWNER, b"\0" * 0x20)
        wr(OWNER, OWNVT)
        wr(durbox, dur)
        wr(durbox + 4, da)
        wr(durbox + 8, db)
        wr(VARS + 0xC, int(misc))
        wr(VARS + 0x10, player)
        S.clear_cap()
        S.call(0x6F177340, ecx=BUFF, stack_args=(TGT, durbox, OWNER, durbox + 8, durbox + 4))
        rows.append([dur, da, db, player, int(illusion), int(misc), int(has_attack), pre,
                     rd(BUFF + 0xEC), rd(BUFF + 0x100), rd(BUFF + 0x104), rd(BUFF + 0x20), rd(CAP + 0x80), rd(CAP + 0x84),
                     rd(CAP + 0x90), rd(CAP + 0x1D0), rd(CAP + 0x98), rd(CAP + 0x9C), rd(CAP + 0x1D4)])


def check_dot_tick(cases, r, rows):
    """CBuffDoT tick 0x6F0D54B0 (real; buff = CBuffUnholyFrenzy, slot 0x360 real = the stored amount at +0xEC). Stubs: the buff
    lookup (BUsl), the class word slot 0x374 (random), slot 0x368 (ethereal flag), the Misc readers, the creator smart
    reference, the damage record builder 0x6F2556E0 (recorded), the target's damage slot 0x120 and life reader slot 0x12C,
    the life adder 0x6F277C20, the post slot 0x370. Rows: amount class misc_flag imm_magic imm_phys busl s368 bonus buff_flags
    f4 f0 life0 life1 | damage calls, builder (ecx edx amount class source flags zero), flag 0x2000000 seen at the damage call,
    slot 0x120 arg1 arg2, heal calls, heal amount | post calls, post delta."""
    S.install()
    unit_t = V4.VTT
    mu.mem_write(unit_t, b"\0" * 0x200)
    V4.misc_int_stub(0x6F009F90, 0xC)
    V4.misc_real_stub(0x6F009EE0, 0x14)
    V4.ret_var(0x6F0787D0, 0x14, 0x18)          # buff lookup on the target: BUsl present?
    A.patch_jump(0x6F4786B0, A.stub_ret(0, 0x5150))       # smart reference -> the creator unit
    A.patch_jump(0x6F2556E0, recorder(0xB0, [(0, True), (1, False), (2, False), (3, False), (4, False)], 0x14, regs=True,
                                      counter=0x1C0))
    rec120 = recorder(0xE0, [(0, False), (1, False)], 0x10, counter=0x1C4)
    flagcheck = new_code(b"\xA1" + a32(S.BUFF + 0x20) + b"\xA3" + a32(CAP + 0xD8) + b"\xA1" + a32(VARS + 0x44)
                         + b"\xA3" + a32(VARS + 0x40) + b"\xE9\0\0\0\0")
    # patch the relative jump of the fake 0x120 stub to the recorder
    here_end = flagcheck + 25
    mu.mem_write(here_end - 4, (rec120 - here_end).to_bytes(4, "little", signed=True))
    s12c = new_code(b"\x8B\x44\x24\x04\x8B\x0D" + a32(VARS + 0x40) + b"\x89\x08\xC2\x04\x00")
    wr(unit_t + 0x120, flagcheck)
    wr(unit_t + 0x12C, s12c)
    A.patch_jump(0x6F277C20, recorder(0xF0, [(1, True)], 8, counter=0x1C8))
    s370 = recorder(0x100, [(0, True)], 4, counter=0x1CC)
    s374 = new_code(b"\xA1" + a32(VARS + 0x1C) + b"\xC3")
    s368 = new_code(b"\xA1" + a32(VARS + 0x20) + b"\xC3")
    for _ in range(cases):
        amount = r.choice([f2u(r.uniform(0, 20)), f2u(r.uniform(-20, 0)), 0, 0x80000000, rbits(r), f2u(4.0)])
        cls = r.choice([0x40000, 0x40000, 0x100000, 0x4000, 0x10, 0x8000, 1, 0x100, r.getrandbits(24)])
        misc = r.random() < 0.6
        im_m = r.choice([0, 0, 0, 1, 2, 0xFFFFFFFF])
        im_p = r.choice([0, 0, 0, 1, 2, 0xFFFFFFFF])
        busl = r.random() < 0.15
        e368 = r.random() < 0.4
        bonus = f2u(r.choice([0.0, 0.5, 1.0, 1.5, 2.0]))
        flags = r.choice([0, 0x4000000, 0x20000000, 0x4000000 | 0x20000000, r.getrandbits(32)])
        f4, f0 = r.choice([(0xFFFFFFFF, 0xFFFFFFFF), (0xFFFFFFFF, 3), (5, 0xFFFFFFFF), (7, 7)])
        life0 = f2u(r.uniform(1, 500))
        life1 = f2u(r.uniform(0, 500))
        S.make_target(False)
        wr(TGT, unit_t)
        wr(TGT + 0x15C, im_m)
        wr(TGT + 0x158, im_p)
        S.make_buff(VT_UF_BUFF, (), (), {0x374: s374, 0x368: s368, 0x370: s370})
        wr(BUFF + 0xEC, amount)
        wr(BUFF + 0x20, flags)
        wr(BUFF + 0xF4, f4)
        wr(BUFF + 0xF0, f0)
        wr(VARS + 0xC, int(misc))
        wr(VARS + 0x14, bonus)
        wr(VARS + 0x18, 0x7777 if busl else 0)
        wr(VARS + 0x1C, cls)
        wr(VARS + 0x20, int(e368))
        wr(VARS + 0x40, life0)
        wr(VARS + 0x44, life1)
        S.clear_cap()
        S.call(0x6F0D54B0, ecx=BUFF)
        rows.append([amount, cls, int(misc), im_m, im_p, int(busl), int(e368), bonus, flags, f4, f0, life0, life1,
                     rd(CAP + 0x1C4), rd(CAP + 0x1C0)] + [rd(CAP + 0xB0 + 4 * i) for i in range(7)]
                    + [rd(CAP + 0xD8), rd(CAP + 0xE0), rd(CAP + 0xE4), rd(CAP + 0x1C8), rd(CAP + 0xF0),
                       rd(CAP + 0x1CC), rd(CAP + 0x100)])


CHECKS = {
    "roar_unit": check_roar_unit,
    "unholy_buff": check_unholy_buff,
    "dot_tick": check_dot_tick,
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
