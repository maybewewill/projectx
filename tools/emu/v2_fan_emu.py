"""Unicorn check V2-1: Fan of Knives (`AEfk`) damage plan, the area launcher `0x6F2C5C40` (docs/spec/abilities-hero-extra.md).

ORIGINAL launcher code is run; the agent world is replaced by hooks:
  * group builder `0x6F2A3270` -> writes `n` candidates (8-byte records {unit, key}) into the collector struct
    (count at +0x14, record array at +0x18);
  * sort `0x6F4C6210`, ctor/dtor `0x6F2C4C80`/`0x6F2C4CF0`, smart-pointer copy `0x6F02A560`: no-ops;
  * per-unit missile `0x6F2C4A40`: records the damage field `[this+0x34]` (the launcher's running amount) at entry.
Inputs per case: DataA (damage), DataB (pool / "max damage"), F2I(DataC) (limit), n candidates.
Output: the list of per-unit amounts handed to the missile creator, in order.

File `fan_plan.txt` (hex float bits): `A B limit n | amt0 amt1 ...`

Usage:  GAME_DLL=/path/Game.dll py tools/emu/v2_fan_emu.py [--dump DIR] [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
import undead_ne3_emu as U3  # noqa: E402

mu = emu.mu
DATA = emu.DATA
wr, rd = A.wr, A.rd
f2u, u2f = A.f2u, A.u2f
hook_fn = U3.hook_fn

F_LAUNCH = 0x6F2C5C40
F_GROUP = 0x6F2A3270
F_SORT = 0x6F4C6210
F_CTOR = 0x6F2C4C80
F_DTOR = 0x6F2C4CF0
F_SPCOPY = 0x6F02A560
F_MISSILE = 0x6F2C4A40

SCR = DATA + 0xE000          # scratch: argument cells
ENT = DATA + 0xE400          # candidate records
PTRS = DATA + 0xE800         # dummy pointed-to blocks


def run(cases, r):
    A.setup_stubs()
    state = {"n": 0}
    amounts = []

    def group(uc, esp, ecx):
        st = rd(esp + 8)
        n = state["n"]
        for i in range(n):
            wr(ENT + 8 * i, 0x1000 + i)
            wr(ENT + 8 * i + 4, 0)
        wr(st + 0x14, n)
        wr(st + 0x18, ENT)
        return 0

    hook_fn(F_GROUP, group, 0)
    hook_fn(F_SORT, lambda uc, esp, ecx: 0, 8)
    hook_fn(F_CTOR, lambda uc, esp, ecx: 0, 0)
    hook_fn(F_DTOR, lambda uc, esp, ecx: 0, 0)
    hook_fn(F_SPCOPY, lambda uc, esp, ecx: 0, 4)
    hook_fn(F_MISSILE, lambda uc, esp, ecx: amounts.append(rd(ecx + 0x34)) and 0 or 0, 4)

    # pointed-to dummy blocks (zeros), one per unknown pointer argument
    mu.mem_write(PTRS, b"\0" * 0x400)
    rows = []
    for _ in range(cases):
        a = r.choice((0.0, r.uniform(1, 300), float(r.randrange(10, 300)), r.uniform(0.001, 5)))
        b = r.choice((0.0, 0.0, r.uniform(1, 1000), float(r.randrange(100, 900)), a * r.randrange(1, 6)))
        lim = r.choice((0, 0, r.randrange(1, 8)))
        n = r.randrange(0, 13)
        state["n"] = n
        del amounts[:]
        wr(SCR + 0x00, f2u(a))        # DataA
        wr(SCR + 0x04, f2u(b))        # DataB
        wr(SCR + 0x08, f2u(r.uniform(0, 600)))   # Area
        # 13 stack args: unit, unit2, flags, &Area, &DataA, &DataB, limit, v42C, v430, v434, &DataD, owner, unit3
        stack = [PTRS + 0x20, PTRS + 0x40, 0x10000, SCR + 0x08, SCR + 0x00, SCR + 0x04, lim,
                 PTRS + 0x60, PTRS + 0x80, PTRS + 0xA0, PTRS + 0xC0, PTRS + 0xE0, PTRS + 0x100]
        try:
            emu.call(F_LAUNCH, ecx=PTRS + 0x200, edx=852526, stack_args=stack)
        except Exception as e:
            from unicorn.x86_const import UC_X86_REG_EIP, UC_X86_REG_ESP
            raise SystemExit("launcher failed at eip=%#x: %s" % (mu.reg_read(UC_X86_REG_EIP), e))
        rows.append((f2u(a), f2u(b), lim, n, list(amounts)))
    return rows


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump = args[1]
        args = args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 11
    rows = run(cases, random.Random(seed))
    for a, b, lim, n, am in rows[:10]:
        print("fan A=%g B=%g limit=%d n=%d ->" % (u2f(a), u2f(b), lim, n), [round(u2f(x), 4) for x in am])
    if dump:
        os.makedirs(dump, exist_ok=True)
        with open(os.path.join(dump, "fan_plan.txt"), "w") as fh:
            for a, b, lim, n, am in rows:
                fh.write("%08x %08x %d %d | %s\n" % (a, b, lim, n, " ".join("%08x" % x for x in am)))
    print("cases", len(rows))


if __name__ == "__main__":
    main()
