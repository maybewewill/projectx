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


def _neutral_heal(fn):
    """The V4 Heal checks re-run on the CAbilityNeutralHeal vtable (0x6F8C9AEC): its vtable differs from CAbilityHeal's only in
    slots 0x04 (dtor), 0x1C (code), 0x58 (name) and the order id getters 0x308 / 0x318 / 0x348."""
    def run(cases, r, rows):
        V4.VT_HEAL_ABIL, V4.ABIL_HEAL = 0x6F8C9AEC, 0x416E6865     # 'Anhe'
        fn(cases, r, rows)
    return run


def _swap_ability(fn, old_vt, new_vt, new_code):
    """Re-run a V4 check that hard-codes `make_ability(old_vt, ...)` on another class (the new class's vtable differs from
    the old one only in the slots listed in the spec section)."""
    def run(cases, r, rows):
        orig = A.make_ability

        def make(vt, code, cost):
            orig(new_vt if vt == old_vt else vt, new_code if vt == old_vt else code, cost)
        A.make_ability = make
        fn(cases, r, rows)
    return run


def check_creep_clap_slow(cases, r, rows):
    """Creep Thunder Clap's slow slot 0x430 (0x6F18AA90, structurally Thunder Clap's 0x6F18A930 with the code getter 0x6F156FA0 and
    the buff factory 0x6F188C00): hero test, HeroDur / Dur + (+0x9C), duration != 0 test, movement -DataC, attack speed -DataD
    as they reach the apply slot 0x354. Rows as human_heroes3_emu.check_thunder_slow."""
    import human_heroes3_emu as H3
    H3.install()
    A.patch_jump(0x6F188C00, A.stub_ret(0, H3.BN_NEWBUFF))
    A.patch_jump(0x6F0766A0, A.stub_ret(0, 0))         # "has the hero ability" reference walk: no (as spell_buff_emu.install)
    H3.F_SLOW = 0x6F18AA90
    orig = A.make_ability

    def make(vt, code, cost):
        orig(0x6F8F7044 if vt == 0x6F8F67C4 else vt, 0x41435463 if vt == 0x6F8F67C4 else code, cost)    # 'ACtc'
    A.make_ability = make
    H3.check_thunder_slow(cases, r, rows)


def check_lpurge(cases, r, rows):
    """Lightning Purge (`AIlp`, CAbilityLightningPurge vtable 0x6F8CB464): the V4 Purge check with the Purge effect reached through the
    LP class's own unit-effect slot 0x424 (0x6F10C070, a `jmp 0x6F107E10`) and the LP vtable on the fake ability."""
    vt = 0x6F8CB464
    thunk = A.rd(vt + 0x424)
    orig_make, orig_call = A.make_ability, S.call

    def make(v, code, cost):
        orig_make(vt if v == 0x6F8C46CC else v, 0x41496C70 if v == 0x6F8C46CC else code, cost)       # 'AIlp'

    def call(fn, **kw):
        return orig_call(thunk if fn == 0x6F107E10 else fn, **kw)
    A.make_ability, S.call = make, call
    V4.check_purge_args(cases, r, rows)


def check_immunity(cases, r, rows):
    """The magic immunity counters of a unit: add 0x6F07A3A0 (ecx = unit, arg = flag) and remove 0x6F077320 (same), driven through the
    Magic Immunity ability's init slot 0x6F110730 (flag 1) and removal slot 0x6F110700 (flag 1), and directly with flag 0 (what the
    CBuffAntiMagicShell apply 0x6F110750 / remove 0x6F1107D0 pass). Stubs: the enter / leave side effects 0x6F2AB310 / 0x6F2AC3A0
    (counted), the lookup of the purge debuff 0x6F0787D0 and its removal 0x6F079CC0 (counted), the dispel walker 0x6F079FE0 (nine
    arguments recorded), the passive base slots 0x6F054860 / 0x6F060A30. Row: start+0x15C start+0x160 purge_present ops |
    +0x15C +0x160 | enter calls leave calls | lookups removals | walker calls, walker arguments (9)."""
    S.install()
    A.patch_jump(0x6F2AB310, V4.new_code(b"\xFF\x05" + a32(CAP + 0x100) + b"\xC2\x08\x00"))
    A.patch_jump(0x6F2AC3A0, V4.new_code(b"\xFF\x05" + a32(CAP + 0x104) + b"\xC3"))
    A.patch_jump(0x6F0787D0, V4.new_code(b"\xFF\x05" + a32(CAP + 0x108) + b"\xA1" + a32(VARS + 8) + b"\xC2\x14\x00"))
    A.patch_jump(0x6F079CC0, V4.new_code(b"\xFF\x05" + a32(CAP + 0x10C) + b"\xC2\x04\x00"))
    A.patch_jump(0x6F079FE0, recorder(0x120, [(i, False) for i in range(9)], 0x24, counter=0x110))
    A.patch_jump(0x6F054860, A.stub_ret(8, 0))
    A.patch_jump(0x6F060A30, A.stub_ret(0, 0))
    for _ in range(cases):
        s1 = r.choice([0, 0, 0, 1, 2, 0xFFFFFFFF, 5])
        s2 = r.choice([0, 0, 0, 1, 2, 0xFFFFFFFF, 3])
        purge = r.random() < 0.5
        ops = "".join(r.choice("AaRrBCD") for _ in range(r.randint(1, 10)))
        S.make_target(False)
        wr(TGT + 0x15C, s1)
        wr(TGT + 0x160, s2)
        A.make_ability(0x6F8DDEB4, 0x416D696D, 75)         # any ability object; only +0x30 (the owner unit) is read
        wr(A.OBJ + 0x30, TGT)
        wr(VARS + 8, EX1 if purge else 0)
        S.make_buff(0x6F8D28DC, (0xCC,), (), {0x320: A.stub_ret(0xC, 0)})     # a CBuffAntiMagicShell; base apply 0x320 stubbed
        S.clear_cap()
        mu.mem_write(CAP + 0x100, b"\0" * 0x80)
        for op in ops:
            if op == "B":        # CBuffAntiMagicShell apply 0x354 (0x6F110750): base apply, one flag-0 count, the holder value
                S.call(0x6F110750, ecx=BUFF, stack_args=(TGT, A.OBJ, A.OBJ + 0x10))
            elif op == "C":      # CBuffAntiMagicShellTwo apply 0x354 (0x6F1107F0): base apply and the holder value, no count
                S.call(0x6F1107F0, ecx=BUFF, stack_args=(TGT, A.OBJ, A.OBJ + 0x10))
            elif op == "D":      # CBuffAntiMagicShell removal 0x6F1107D0: one flag-0 count off (unit = buff + 0x30)
                S.call(0x6F1107D0, ecx=BUFF)
            elif op == "A":
                S.call(0x6F110730, stack_args=(TGT, 0))
            elif op == "a":
                S.call(0x6F07A3A0, ecx=TGT, stack_args=(0,))
            elif op == "R":
                S.call(0x6F110700, ecx=A.OBJ)
            else:
                S.call(0x6F077320, ecx=TGT, stack_args=(0,))
        rows.append([s1, s2, int(purge), ops, rd(TGT + 0x15C), rd(TGT + 0x160), rd(CAP + 0x100), rd(CAP + 0x104),
                     rd(CAP + 0x108), rd(CAP + 0x10C), rd(CAP + 0x110)] + [rd(CAP + 0x120 + 4 * i) for i in range(9)])


def check_invuln(cases, r, rows):
    """The invulnerability counter of a unit (+0xE8, flag bit 8 of +0x20): add 0x6F0796F0 (ecx = unit, arg = notify) and remove 0x6F079730,
    driven through the Invulnerable ability's init 0x6F10FAE0 (notify 1) and removal 0x6F10FB10 (notify 1) and directly with notify 0
    (what Divine Shield passes: `0x6F0796F0(1)` in its apply, so notify 1 too). Stubs: the enter / leave notices 0x6F2AB310 /
    0x6F2AC3A0 and the refreshes 0x6F26FE20 / 0x6F26FE10 (counted), the bonus-apply slot 0x2F4 and the base removal 0x6F0D1CD0.
    Row: start_count start_flags ops | count flags | enter leave refresh_a refresh_b."""
    S.install()
    A.patch_jump(0x6F2AB310, V4.new_code(b"\xFF\x05" + a32(CAP + 0x100) + b"\xC2\x08\x00"))
    A.patch_jump(0x6F2AC3A0, V4.new_code(b"\xFF\x05" + a32(CAP + 0x104) + b"\xC3"))
    A.patch_jump(0x6F26FE20, V4.new_code(b"\xFF\x05" + a32(CAP + 0x108) + b"\xC3"))
    A.patch_jump(0x6F26FE10, V4.new_code(b"\xFF\x05" + a32(CAP + 0x10C) + b"\xC2\x04\x00"))
    A.patch_jump(0x6F0D1CD0, A.stub_ret(0, 0))
    for _ in range(cases):
        s1 = r.choice([0, 0, 0, 1, 2, 0xFFFFFFFF, 5])
        s2 = r.choice([0, 8, 0x20, 0x28, r.getrandbits(32)])
        ops = "".join(r.choice("IRia") for _ in range(r.randint(1, 10)))
        S.make_target(False)
        wr(TGT + 0xE8, s1)
        wr(TGT + 0x20, s2)
        A.make_ability(0x6F8D25BC, 0x41767574, 75)         # CAbilityInvulnerable, 'Avul'
        mu.mem_write(S.FVT, bytes(mu.mem_read(0x6F8D25BC, 0x400)))
        wr(S.FVT + 0x2F4, A.stub_ret(0xC, 0))
        wr(A.OBJ, S.FVT)
        wr(A.OBJ + 0x30, TGT)
        S.clear_cap()
        mu.mem_write(CAP + 0x100, b"\0" * 0x20)
        for op in ops:
            if op == "I":
                S.call(0x6F10FAE0, ecx=A.OBJ, stack_args=(TGT, 0))
            elif op == "R":
                S.call(0x6F10FB10, ecx=A.OBJ)
            elif op == "i":
                S.call(0x6F0796F0, ecx=TGT, stack_args=(0,))
            else:
                S.call(0x6F079730, ecx=TGT, stack_args=(0,))
        rows.append([s1, s2, ops, rd(TGT + 0xE8), rd(TGT + 0x20), rd(CAP + 0x100), rd(CAP + 0x104), rd(CAP + 0x108),
                     rd(CAP + 0x10C)])


VT_HOWL_ABIL, ABIL_HOWL, VT_HOWL_BUFF = 0x6F8A25B4, 0x414E6874, 0x6F8A29AC        # CAbilityHowlOfTerror 'ANht', CBuffHowlOfTerror


def check_howl_unit(cases, r, rows):
    """Howl of Terror's per-unit slot 0x3EC (0x6F0BAF20) -> creator 0x6F0BAE90 -> the CBuffHowlOfTerror apply slot 0x354 (the Inner Fire
    buff apply 0x6F12CA40) -> the attack / armor / regeneration helpers: the V8 Roar check on the Howl classes. Rows as V8 roar_unit:
    A B C D | duration argument | weapons (8 words) | slot-0x320 arg1 arg2 | counter deltas | stored amounts +0xE0/+0xE4 | armor value, calls |
    life regen value, calls | mana regen value, calls."""
    S.install()
    s320 = S.slot_capture2()
    s324 = A.stub_ret(4, 0)
    A.patch_jump(0x6F0B5F10, A.stub_ret(0, BUFF))
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
        A.make_ability(VT_HOWL_ABIL, ABIL_HOWL, 75)
        level(A.LVL, 0, 0, [da, db, dc, dd])
        S.make_target(False)
        base0, base1 = r.randint(0, 5), r.randint(0, 5)
        S.make_attack(weap, base0, base1)
        S.make_buff(VT_HOWL_BUFF, (0xCC, 0xD4, 0xE8, 0xF0, 0xF8), (), {0x320: s320, 0x324: s324})
        wr(durbox, dur)
        wr(durbox + 8, 0x1234)
        S.clear_cap()
        S.call(0x6F0BAF20, ecx=A.OBJ, stack_args=(TGT, durbox + 8, durbox))
        rows.append([da, db, dc, dd, dur] + [x for w in weap for x in w]
                    + [rd(CAP + 0x80), rd(CAP + 0x84), u32(rd(ATK + 0xAC) - base0), u32(rd(ATK + 0xB0) - base1),
                       rd(S.BUFF + 0xE0), rd(S.BUFF + 0xE4), rd(CAP + 0x30), rd(CAP + 0x34), rd(CAP + 0x40), rd(CAP + 0x44),
                       rd(CAP + 0x50), rd(CAP + 0x54)])


BAMS, BAM2 = 0x42616D73, 0x42616D32
VT_AMS_ABIL, ABIL_AMS = 0x6F8DDEB4, 0x4161_6D73       # CAbilityAntiMagicShell, 'Aams'
VT_AMI_ABIL, ABIL_AMI = 0x6F8DE2E4, 0x4161_6D69       # CAbilityAntiMagicShellInstant, 'Aami'
EX1, EX2, NEW2 = S.M + 0x5A00, S.M + 0x5B00, S.M + 0x5C00        # existing Bams, existing Bam2, a new Bam2 (BUFF is the new Bams)


def check_ams_effect(cases, r, rows, vt=VT_AMS_ABIL, code=ABIL_AMS, entry=0x6F13C900, instant=False):
    """The Anti-Magic Shell effect 0x6F13C900 (ecx = ability, arg = target; the instant class enters through 0x6F143040 with the owner).
    Real: the level getters, the hero test 0x6F1D2660, 0x6F02F880 (Dur + ability+0x9C), HeroDur, the code getters 0x6F110530 (Bams)
    and 0x6F1105B0 (Bam2), 0x6F13C790. Stubs: the dispel worker 0x6F0EEF00 (recorded), the buff lookup 0x6F0787D0 (existing Bams / Bam2
    by code), the buff factories 0x6F139D80 (new Bams = BUFF) / 0x6F139E50 (new Bam2 = NEW2), the registration, the list item 0x6F02EE20.
    Row: DataA DataB DataC DataD dur herodur extra hero ex_bams ex_bam2 flag100 listitem | worker (12 words) | slot 0x324 calls +
    (ecx, edx, arg) | slot 0x354 calls + (ecx, edx, target, dur, data) | slot 0x358 calls + (ecx, edx, ref, dur, data)."""
    S.install()
    A.patch_jump(0x6F0EEF00, recorder(0x100, V4.WORKER_SPEC, 0x28, regs=True))
    V4.unit_vtables()
    V4.ret_var(0x6F02EE20, 8, 0x30)
    V4.ret_var(0x6F1D2660, 0, 4)
    # if code == Bams: eax = [VARS+8]; elif code == Bam2: eax = [VARS+12]; else eax = 0 (retn 0x14 each)
    look = new_code(b"\x8B\x44\x24\x04"
                    + b"\x3D" + a32(BAMS) + b"\x75\x08" + b"\xA1" + a32(VARS + 8) + b"\xC2\x14\x00"
                    + b"\x3D" + a32(BAM2) + b"\x75\x08" + b"\xA1" + a32(VARS + 12) + b"\xC2\x14\x00"
                    + b"\x31\xC0\xC2\x14\x00")
    A.patch_jump(0x6F0787D0, look)
    A.patch_jump(0x6F139D80, A.stub_ret(0, BUFF))
    A.patch_jump(0x6F139E50, A.stub_ret(0, NEW2))
    A.patch_jump(0x6F079CA0, A.stub_ret(4, 0))
    s324 = recorder(0x1C0, [(0, True)], 4, regs=True, counter=0x1E8)
    s354 = recorder(0x180, [(0, False), (1, True), (2, True)], 0xC, regs=True, counter=0x1E0)
    s358 = recorder(0x1A0, [(0, True), (1, True), (2, True)], 0xC, regs=True, counter=0x1E4)
    mu.mem_write(S.FVT, b"\0" * 0x400)
    wr(S.FVT + 0x324, s324)
    wr(S.FVT + 0x354, s354)
    wr(S.FVT + 0x358, s358)
    for _ in range(cases):
        da, db, dc, dd = (rbits(r) for _ in range(4))
        if r.random() < 0.6:
            dc = r.choice([0, 0, 0x80000000, 0x7FC00000, f2u(1.0), rbits(r)])
        dur, hdur = rbits(r), rbits(r)
        extra = rbits(r) if r.random() < 0.4 else 0
        hero = r.random() < 0.5
        ex1, ex2 = r.random() < 0.3, r.random() < 0.3
        f100 = r.random() < 0.2
        item = r.getrandbits(32)
        A.make_ability(vt, code, 75)
        level(A.LVL, dur, hdur, [da, db, dc, dd])
        wr(A.OBJ + 0x9C, extra)
        wr(A.OBJ + 0x30, A.UNIT)
        wr(A.UNIT, V4.VTU)
        S.make_target(False)
        wr(TGT, V4.VTT)
        wr(TGT + 0x5C, 0x100 if f100 else 0)
        for b in (BUFF, EX1, EX2, NEW2):
            mu.mem_write(b, b"\0" * 0x200)
            wr(b, S.FVT)
        S.clear_cap()
        wr(VARS + 4, int(hero))
        wr(VARS + 8, EX1 if ex1 else 0)
        wr(VARS + 12, EX2 if ex2 else 0)
        wr(VARS + 0x30, item)
        if instant:
            S.call(0x6F143040, ecx=A.OBJ)       # takes the owner unit itself as the target
        else:
            S.call(entry, ecx=A.OBJ, stack_args=(TGT,))
        tail = []
        for base in (0x1C0, 0x180, 0x1A0):
            cnt = {0x1C0: 0x1E8, 0x180: 0x1E0, 0x1A0: 0x1E4}[base]
            tail += [rd(CAP + cnt)] + [rd(CAP + base + 4 * i) for i in range(5 if base != 0x1C0 else 3)] + [0] * (0 if base != 0x1C0 else 2)
        rows.append([da, db, dc, dd, dur, hdur, extra, int(hero), int(ex1), int(ex2), int(f100), item]
                    + V4.worker_row() + tail)


def check_ami_effect(cases, r, rows):
    check_ams_effect(cases, r, rows, VT_AMI_ABIL, ABIL_AMI, 0x6F143040, True)


CHECKS = {
    "howl_unit": check_howl_unit,
    "invuln": check_invuln,
    "ams_effect": check_ams_effect,
    "ami_effect": check_ami_effect,
    "immunity": check_immunity,
    "lpurge_args": check_lpurge,
    "frenzy": check_frenzy,
    "creep_clap_slow": check_creep_clap_slow,
    "itemdispel_args": _swap_ability(V4.check_dispel_args, 0x6F8C3A24, 0x6F8C4F34, 0x41496469),     # 'AIdi'
    "disenchant_args": _swap_ability(V4.check_dispel_args, 0x6F8C3A24, 0x6F8C3E5C, 0x41646368),     # 'Adch'
    "nheal_effect": _neutral_heal(V4.check_heal_effect),
    "nheal_target": _neutral_heal(V4.check_heal_target),
    "nheal_filter": _neutral_heal(V4.check_heal_filter),
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
            print(" ".join(("%08x" % x) if isinstance(x, int) else x for x in row))
        if dump:
            os.makedirs(dump, exist_ok=True)
            with open(os.path.join(dump, name + ".txt"), "w") as fh:
                for row in rows:
                    fh.write(" ".join(("%08x" % x) if isinstance(x, int) else x for x in row) + "\n")


if __name__ == "__main__":
    main()
