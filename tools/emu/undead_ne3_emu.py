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
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP,
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
