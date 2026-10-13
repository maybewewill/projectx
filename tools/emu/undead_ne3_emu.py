"""Unicorn checks for the S33 undead / night elf buff classes (docs/spec/abilities-undead-ne-heroes.md, section S33).

ORIGINAL code of the user's Game.dll is run for real, the agent world is replaced by small hooks that record
their arguments. Checks (CHECK=name; `--dump DIR` writes the vector file read by the Rust test):

  frost     `CBuffFrost` slot 0x360 (`0x6F199F40`): the slow strengths of Frost Nova. With the 4th argument zero
            both strengths come from the Misc keys `FrostMoveSpeedDecrease` / `FrostAttackSpeedDecrease`
            (reader `0x6F009EE0`, default 0.0) and are handed to the shared setter `0x6F0D6500` negated;
            a non-zero 4th argument gives two negated zeros. File `frost.txt`:
            `flag moveMisc attackMisc | arg3 arg4` (hex float bits).
  pulse     `CBuffEntanglingRoots` periodic pulse (slot 0x364 = `0x6F0D54B0`): amount, sleep buff, Misc gate,
            immunity flags -> the damage record or heal given to the target. File `pulse.txt`.

Usage:  GAME_DLL=/path/Game.dll CHECK=frost py tools/emu/undead_ne3_emu.py [--dump DIR] [CASES] [SEED]
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
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP,
    UC_X86_REG_ESP,
)

mu = emu.mu
DATA = emu.DATA
wr, rd = A.wr, A.rd
f2u, u2f = A.f2u, A.u2f

VT_FROST = 0x6F9076A4          # CBuffFrost
VT_ROOTS = 0x6F897764          # CBuffEntanglingRoots
VT_STRIKE = 0x6F91FD04         # CBuffShadowStrike
BUFF = DATA + 0x7000
KEY_MOVE = 0x6F8FA124          # "FrostMoveSpeedDecrease"
KEY_ATTACK = 0x6F8FA13C        # "FrostAttackSpeedDecrease"
F_FROST_SLOT360 = 0x6F199F40
F_MISC_REAL = 0x6F009EE0
F_BASE_SETTER = 0x6F0D6500
F_RELEASE = 0x6F279230


def hook_fn(addr, handler, pop, thiscall=False):
    """At `addr` call handler(uc, esp, ecx) -> eax and return to the caller, popping `pop` argument bytes."""
    def cb(uc, address, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = rd(esp)
        eax = handler(uc, esp, uc.reg_read(UC_X86_REG_ECX)) or 0
        uc.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
        uc.reg_write(UC_X86_REG_EIP, ret)
    return mu.hook_add(UC_HOOK_CODE, cb, begin=addr, end=addr)


def run_frost(cases, r):
    A.setup_stubs()
    misc = {}
    rec = []

    def misc_real(uc, esp, ecx):
        key = rd(esp + 4)
        wr(ecx, misc[key])
        return ecx

    def setter(uc, esp, ecx):
        a1, a2, a3, a4 = (rd(esp + 4 + 4 * i) for i in range(4))
        rec.append((rd(a3), rd(a4)))
        return 0

    hook_fn(F_MISC_REAL, misc_real, 8)
    hook_fn(F_BASE_SETTER, setter, 16)
    hook_fn(F_RELEASE, lambda uc, esp, ecx: 0, 0)
    mu.mem_write(BUFF, b"\0" * 0x400)
    wr(BUFF, VT_FROST)
    out1, out2 = DATA + 0x7800, DATA + 0x7810
    rows = []
    for _ in range(cases):
        def pick():
            return r.choice((0.0, 0.5, 0.25, r.uniform(0.0, 1.0), r.uniform(-1.0, 0.0), 1.0, -0.0))
        m, a = pick(), pick()
        flag = r.choice((0, 0, 1, 7))
        misc[KEY_MOVE] = f2u(m)
        misc[KEY_ATTACK] = f2u(a)
        rec.clear()
        emu.call(F_FROST_SLOT360, ecx=BUFF, stack_args=(out1, out2, 0x1234, flag))
        assert len(rec) == 1
        rows.append((flag, f2u(m), f2u(a), rec[0][0], rec[0][1]))
    return rows


F_PULSE = 0x6F0D54B0
F_LOOKUP = 0x6F0787D0
F_MISC_BOOL = 0x6F009F90
F_CLASS_A, F_CLASS_B = 0x6F2555E0, 0x6F2555F0
F_RECORD = 0x6F2556E0
F_ADDLIFE = 0x6F277C20
F_ATTACKER = 0x6F4786B0
F_MUL = 0x6F6EEE20
UVT = DATA + 0x5000
TUNIT = DATA + 0x2000


def stub_slot():
    a = A._stub_next[0]
    mu.mem_write(a, b"\x90" * 8)
    A._stub_next[0] += 0x20
    return a


def run_pulse(cases, r, vt):
    """Base periodic pulse `0x6F0D54B0` of a buff class (`vt` = CBuffEntanglingRoots)."""
    A.setup_stubs()
    st = {}
    calls = []

    hook_fn(F_LOOKUP, lambda uc, esp, ecx: calls.append(("lookup", rd(esp + 4))) or st["sleep"], 20)
    hook_fn(F_MISC_BOOL, lambda uc, esp, ecx: st["gate"], 4)
    hook_fn(F_CLASS_A, lambda uc, esp, ecx: st["ca"], 0)
    hook_fn(F_CLASS_B, lambda uc, esp, ecx: st["cb"], 0)

    def record(uc, esp, ecx):
        amt_ptr, cls, att, flags, zero = (rd(esp + 4 + 4 * i) for i in range(5))
        calls.append(("record", rd(amt_ptr), cls, att, flags, rd(BUFF + 0x20) & 0x2000000))
        return ecx

    hook_fn(F_RECORD, record, 20)
    hook_fn(F_ADDLIFE, lambda uc, esp, ecx: calls.append(("heal", rd(rd(esp + 8)))) or 0, 8)
    hook_fn(F_ATTACKER, lambda uc, esp, ecx: 0x7777, 0)

    def misc_real(uc, esp, ecx):
        wr(ecx, st["ethereal"])
        return ecx

    hook_fn(F_MISC_REAL, misc_real, 8)

    def mul(uc, esp, ecx):
        edx = uc.reg_read(UC_X86_REG_EDX)
        a, b = rd(edx), rd(rd(esp + 4))
        calls.append(("mul", a, b))
        wr(ecx, f2u(u2f(a) * u2f(b)))
        return ecx

    s12c, s120 = stub_slot(), stub_slot()
    hook_fn(s12c, lambda uc, esp, ecx: (wr(rd(esp + 4), 0x42C80000), rd(esp + 4))[1], 4)
    hook_fn(s120, lambda uc, esp, ecx: calls.append(("damage", 1)) or 0, 16)
    mu.mem_write(UVT, b"\0" * 0x400)
    wr(UVT + 0x12C, s12c)
    wr(UVT + 0x120, s120)
    rows = []
    for _ in range(cases):
        amt = r.choice((0.0, -0.0, 15.0, 25.0, -5.0, 0.25, r.uniform(0.001, 100), r.uniform(-50, -0.001), float("nan")))
        st.update(sleep=r.choice((0, 0, 0, 1)), gate=r.choice((0, 1)), ca=r.choice((0, 1)), cb=r.choice((0, 1)),
                  ethereal=f2u(r.choice((1.0, 0.5, 2.0))))
        bflags = r.choice((0, 0, 0x4000000, 0x100, 0x8000000, 0x8000100))
        c158 = r.choice((0, 0, 1))
        c15c = r.choice((0, 0, 1))
        att = r.choice((-1, 5))
        mu.mem_write(TUNIT, b"\0" * 0x400)
        wr(TUNIT, UVT)
        wr(TUNIT + 0x158, c158)
        wr(TUNIT + 0x15C, c15c)
        mu.mem_write(BUFF, b"\0" * 0x400)
        wr(BUFF, vt)
        wr(BUFF + 0x30, TUNIT)
        wr(BUFF + 0x20, bflags)
        wr(BUFF + 0xEC, f2u(amt))
        wr(BUFF + 0xF0, att & 0xFFFFFFFF)
        wr(BUFF + 0xF4, att & 0xFFFFFFFF)
        calls.clear()
        emu.call(F_PULSE, ecx=BUFF)
        rows.append((dict(st), bflags, c158, c15c, att, f2u(amt), list(calls), rd(BUFF + 0x20)))
    return rows


F_BASE_APPLY = 0x6F0D5390
F_SMARTPTR = 0x6F0419C0
F_TIMER_ARM = 0x6F477A80


def run_apply_roles():
    """Which argument of the base apply `0x6F0D5390` becomes the stored pulse amount (`+0xEC` property value)?"""
    A.setup_stubs()
    prop = stub_slot()
    seen = {}
    # property object at +0xE8: vtable pointer; slot 0 = set(valuePtr, flag) recording the value read through it
    pvt = DATA + 0x5800
    mu.mem_write(pvt, b"\0" * 0x40)
    wr(pvt, prop)
    hook_fn(prop, lambda uc, esp, ecx: seen.setdefault("value", rd(rd(esp + 4))) and 0, 8)
    hook_fn(F_SMARTPTR, lambda uc, esp, ecx: seen.setdefault("smart", rd(esp + 4)) and 0, 4)
    hook_fn(F_TIMER_ARM, lambda uc, esp, ecx: seen.setdefault("timer", rd(esp + 4)) and 0, 4)
    s320 = stub_slot()
    s358 = stub_slot()
    hook_fn(s320, lambda uc, esp, ecx: seen.setdefault("s320", tuple(rd(esp + 4 + 4 * i) for i in range(3))) and 0, 12)
    hook_fn(s358, lambda uc, esp, ecx: 0x1234, 20)
    vt = DATA + 0x5C00
    mu.mem_write(vt, b"\0" * 0x500)
    for i in range(0x400 // 4):
        wr(vt + 4 * i, rd(VT_ROOTS + 4 * i))
    wr(vt + 0x320, s320)
    wr(vt + 0x358, s358)
    mu.mem_write(BUFF, b"\0" * 0x400)
    wr(BUFF, vt)
    wr(BUFF + 0xE8, pvt)
    ptrs = [DATA + 0x6100, DATA + 0x6110, DATA + 0x6120, DATA + 0x6130]
    vals = [0x41700000, 0x41800000, 0x41900000, 0x41A00000]
    for p, v in zip(ptrs, vals):
        wr(p, v)
    emu.call(F_BASE_APPLY, ecx=BUFF, stack_args=(ptrs[0], ptrs[1], ptrs[2], ptrs[3], 0))
    return seen, ptrs, vals


VT_AB_ROOTS = 0x6F8A6824
F_ROOTS_EFFECT = 0x6F0B8290
F_ROOTS_APPLY = 0x6F0B8220
F_IS_HERO = 0x6F1D2660
F_SRC_INFO = 0x6F042B20


def run_roots_effect():
    """`CAbilityEntanglingRoots` effect `0x6F0B8290`: what reaches the buff apply helper `0x6F0B8220`."""
    A.setup_stubs()
    A.make_ability(VT_AB_ROOTS, 0x41456572, 0)
    seen = {}
    TG = DATA + 0x6400
    mu.mem_write(TG, b"\0" * 0x200)
    for hero in (0, 1):
        # level record: Dur (+0x08) = 9.0, HeroDur (+0x0C) = 3.0, DataA (+0x20) = 15.0
        wr(A.LVL + 0x08, f2u(9.0))
        wr(A.LVL + 0x0C, f2u(3.0))
        wr(A.LVL + 0x20, f2u(15.0))
        wr(OBJ_9C := A.OBJ + 0x9C, f2u(0.0))
        h1 = hook_fn(F_IS_HERO, lambda uc, esp, ecx, hero=hero: hero, 0)
        h2 = hook_fn(F_SRC_INFO, lambda uc, esp, ecx: ecx, 8)

        def apply(uc, esp, ecx):
            ebx = uc.reg_read(UC_X86_REG_EBX)
            seen[hero] = dict(
                ebx_value=rd(ebx), args=[(rd(esp + 4 + 4 * i), rd(rd(esp + 4 + 4 * i))) for i in range(3)],
                edi=uc.reg_read(UC_X86_REG_EDI))
            return 0

        h3 = hook_fn(F_ROOTS_APPLY, apply, 12)
        emu.call(F_ROOTS_EFFECT, ecx=A.OBJ, stack_args=(TG,))
        for h in (h1, h2, h3):
            mu.hook_del(h)
    return seen, TG


def fmt_calls(calls):
    out = []
    for c in calls:
        out.append(c[0] + ":" + ":".join("%x" % x for x in c[1:]) if len(c) > 1 else c[0])
    return " ".join(out) or "none"


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump = args[1]
        args = args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 7
    which = os.environ.get("CHECK", "frost")
    if dump:
        os.makedirs(dump, exist_ok=True)
    if which == "roots":
        seen, tg = run_roots_effect()
        for hero, d in seen.items():
            print("roots effect hero=%d" % hero, {k: v for k, v in d.items()})
        print("target", hex(tg))
    if which == "apply":
        seen, ptrs, vals = run_apply_roles()
        print("apply: pointers", [hex(p) for p in ptrs], "values", [hex(v) for v in vals])
        print("apply: seen", {k: (hex(v) if isinstance(v, int) else tuple(hex(x) for x in v)) for k, v in seen.items()})
    if which == "pulse":
        rows = run_pulse(cases, random.Random(seed), VT_ROOTS)
        for st, bf, c158, c15c, att, amt, calls, after in rows[:14]:
            print("pulse", st, hex(bf), c158, c15c, att, u2f(amt), fmt_calls(calls), hex(after))
        if dump:
            with open(os.path.join(dump, "pulse.txt"), "w") as fh:
                for st, bf, c158, c15c, att, amt, calls, after in rows:
                    fh.write("%d %d %d %d %08x %x %d %d %d %08x | %s\n" % (
                        st["sleep"], st["gate"], st["ca"], st["cb"], st["ethereal"], bf, c158, c15c,
                        1 if att != -1 else 0, amt, fmt_calls(calls)))
    if which == "frost":
        rows = run_frost(cases, random.Random(seed))
        for row in rows[:8]:
            print("frost flag=%d move=%g attack=%g -> arg3=%g arg4=%g" % (
                row[0], u2f(row[1]), u2f(row[2]), u2f(row[3]), u2f(row[4])))
        bad = 0
        for flag, m, a, a3, a4 in rows:
            em, ea = (0x80000000, 0x80000000) if flag else (m ^ 0x80000000, a ^ 0x80000000)
            if (a3, a4) != (em, ea):
                bad += 1
        print("frost: %d cases, %d mismatches against the negation rule" % (len(rows), bad))
        if dump:
            with open(os.path.join(dump, "frost.txt"), "w") as fh:
                for row in rows:
                    fh.write("%d %08x %08x | %08x %08x\n" % row)


if __name__ == "__main__":
    main()
