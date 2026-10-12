"""Unicorn harness for the life / mana regeneration of a unit in Game.dll 1.26a (docs/spec/agile.md section 9).

A unit's life and mana are agile properties (CPoReal, wrappers `Float` embedded in the unit at +0x98 and +0xB8). The base regeneration
is a *forever continuous modifier* on those properties. The real routines run here on a synthetic unit object:

  0x6F282FC0   life regeneration refresh (creates / re-rates / releases the modifier at unit+0xB4, rate at unit+0xB0)
  0x6F283F60   mana regeneration refresh (modifier at unit+0xD8, rate at unit+0xD4)
  0x6F28CAF0 / 0x6F28CB00   life regen flag 0x40 on / off (unit+0x20)       0x6F28CB10 / 0x6F28CB20   flag 0x80 on / off
  0x6F28D570 / 0x6F28D580   mana regen flag 0x100 on / off (unit+0x20)
  0x6F28CB30   add a delta to the life rate (sink object +0xAC, value at +0xB0), then refresh
  0x6F28D590   add a delta to the mana rate (sink object +0xD0, value at +0xD4), then refresh
  0x6F021D50   the sink object's vt[0] (the creation path writes the type's regenHP / regenMana through it)
  0x6F477400   set maximum (upper limit), 0x6F477350 set value, 0x6F4773A0 value

and the property machinery of tools/emu/agile_engine_emu.py (modifier creator, rate setter 0x6F4A9B00, destroy 0x6F478920 ->
0x6F4A6970, tick pump 0x6F46E9E0).

Usage:  GAME_DLL=/path/Game.dll py -I tools/emu/regen_emu.py gen OUT.txt COUNT SEED
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import agile_engine_emu as A  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP  # noqa: E402
from agile_engine_emu import rd32, wr32, f2b, b2f, call, mu, TMP, BASE, VT_W_FLOAT  # noqa: E402

U = BASE + 0x60000          # the synthetic unit
SINK_VT = 0x6F876D38        # the "float cell" class (vt[0] stores the value at +4)
TICK = A.TICK
NEG_FLT_MAX = 0xFF7FFFFF
FLT_MAX = 0x7F7FFFFF

LIFE_REFRESH = 0x6F282FC0
MANA_REFRESH = 0x6F283F60


FS_SCRATCH = BASE + 0x1E0000


def delete_hook(uc, address, size, user):
    """0x6F6EB558: operator delete(ptr, file, line, flag), stdcall with four arguments: nothing is freed."""
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + 16)
    uc.reg_write(UC_X86_REG_EIP, ret)


_hooked = [False]


def setup(d):
    if not _hooked[0]:
        mu.hook_add(UC_HOOK_CODE, delete_hook, begin=0x6F6EB558, end=0x6F6EB558)
        _hooked[0] = True
    mu.msr_write(0xC0000100, FS_SCRATCH)   # fs base: the SEH frames of the routines read and write fs:[0]
    A.install_hooks()
    A.init_world()
    wr32(A.Q + 0x40, d["now"])
    wr32(A.Q + 0x44, d["row"])
    mu.mem_write(U, bytes(0x400))
    lp = A.make_prop(d["lbase"], d["llo"], d["lhi"])
    mp = A.make_prop(d["mbase"], d["mlo"], d["mhi"])
    # embedded Float wrappers: vtable, then handle index / generation at +8 / +0xC of the wrapper
    wr32(U + 0x98, VT_W_FLOAT)
    wr32(U + 0xA0, rd32(lp + 0x14))
    wr32(U + 0xA4, rd32(lp + 0x18))
    wr32(U + 0xB8, VT_W_FLOAT)
    wr32(U + 0xC0, rd32(mp + 0x14))
    wr32(U + 0xC4, rd32(mp + 0x18))
    wr32(U + 0xAC, SINK_VT)
    wr32(U + 0xD0, SINK_VT)
    wr32(U + 0xB0, d["lrate"])
    wr32(U + 0xD4, d["mrate"])
    wr32(U + 0x20, d["f20"])
    wr32(U + 0x5C, d["f5c"])
    return lp, mp


def snapshot(lp, mp):
    def mod_state(ptr):
        p = rd32(U + ptr)
        if not p:
            return "-"
        # the wrapper's real modifier object
        idx, gen = rd32(p + 8), rd32(p + 0xC)
        ent = rd32(A.HT_ARR + idx * 8 + 4) if idx != 0xFFFFFFFF and idx < 4000 else 0
        if not ent or rd32(A.HT_ARR + idx * 8) != 0xFFFFFFFE:
            return "x"
        return "%08x" % rd32(ent + 0x50)

    lw = U + 0x98
    mw = U + 0xB8
    call(0x6F4773A0, ecx=lw, stack_args=(TMP + 0x20,))
    lv = rd32(TMP + 0x20)
    call(0x6F4773A0, ecx=mw, stack_args=(TMP + 0x20,))
    mv = rd32(TMP + 0x20)
    return "%08x %08x %08x %08x %s %s %08x %08x %08x %08x" % (
        lv, mv, rd32(lp + 0x7C), rd32(mp + 0x7C), mod_state(0xB4), mod_state(0xD8),
        rd32(lp + 0x78), rd32(mp + 0x78), rd32(U + 0xB0), rd32(U + 0xD4))


def do_op(op, lp, mp):
    k = op[0]
    if k == "I":                       # creation-time refresh of both modifiers (0x6F28CAA0 calls them in this order)
        call(LIFE_REFRESH, ecx=U)
        call(MANA_REFRESH, ecx=U)
    elif k == "LON":
        call(0x6F28CAF0, ecx=U)
    elif k == "LOFF":
        call(0x6F28CB00, ecx=U)
    elif k == "LDIS":
        call(0x6F28CB10, ecx=U)
    elif k == "LEN":
        call(0x6F28CB20, ecx=U)
    elif k == "MDIS":
        call(0x6F28D570, ecx=U)
    elif k == "MEN":
        call(0x6F28D580, ecx=U)
    elif k == "LADD":
        wr32(TMP + 0x40, op[1])
        call(0x6F28CB30, ecx=U, stack_args=(TMP + 0x40,))
    elif k == "MADD":
        wr32(TMP + 0x40, op[1])
        call(0x6F28D590, ecx=U, stack_args=(TMP + 0x40,))
    elif k == "DEAD":                  # the death handler sets the dead flag (+0x5C bit 0x100) then refreshes both
        wr32(U + 0x5C, rd32(U + 0x5C) | 0x100)
        call(LIFE_REFRESH, ecx=U)
        call(MANA_REFRESH, ecx=U)
    elif k == "ALIVE":                 # revive: flag cleared, refreshes
        wr32(U + 0x5C, rd32(U + 0x5C) & ~0x100)
        call(LIFE_REFRESH, ecx=U)
        call(MANA_REFRESH, ecx=U)
    elif k == "MAXL":                  # set the maximum life (upper limit of the property): 0x6F477400, no refresh of the modifier
        wr32(TMP + 0x50, op[1])
        call(0x6F477400, ecx=U + 0x98, stack_args=(TMP + 0x50,))
    elif k == "MAXM":
        wr32(TMP + 0x50, op[1])
        call(0x6F477400, ecx=U + 0xB8, stack_args=(TMP + 0x50,))
    elif k == "SETL":                  # set the value: 0x6F477350 (value add)
        wr32(TMP + 0x50, op[1])
        call(0x6F477350, ecx=U + 0x98, stack_args=(TMP + 0x50,))
    elif k == "SETM":
        wr32(TMP + 0x50, op[1])
        call(0x6F477350, ecx=U + 0xB8, stack_args=(TMP + 0x50,))
    elif k == "TICK":
        for _ in range(op[1]):
            A.advance(TICK)
    else:
        raise RuntimeError(op)


def op_text(op):
    if len(op) == 1:
        return op[0]
    if op[0] == "TICK":
        return "TICK %d" % op[1]
    return "%s %08x" % (op[0], op[1])


def rate_pick(rnd):
    r = rnd.random()
    if r < 0.12:
        return 0.0
    if r < 0.5:
        return rnd.choice([0.25, 0.5, 1.0, 2.0, 0.01, 0.035, 0.27, 0.6, 3.0, 0.133, 0.9, 1.25])
    if r < 0.6:
        return rnd.choice([1e-7, -1e-7, 1.9e-7, 2e-7, 2.1e-7, -2.5e-7, 8e-7])
    if r < 0.72:
        return -rnd.choice([0.5, 1.0, 0.25, 2.0])
    return rnd.uniform(-1.0, 6.0)


def gen_case(rnd):
    r = rnd.random()
    if r < 0.5:
        now, row = 0, 0
    elif r < 0.85:
        now, row = f2b(rnd.uniform(0.0, 299.0)), rnd.randrange(0, 3)
    else:
        now, row = f2b(rnd.uniform(297.0, 299.99)), rnd.randrange(0, 3)

    def stat(is_mana):
        hi = rnd.choice([0.0, 1.0, 50.0, 100.0, 300.0, 420.0, 1000.0, 1483.2, 2500.5]) if is_mana else \
            rnd.choice([1.0, 55.0, 100.0, 220.0, 450.0, 800.0, 1500.5, 3000.0])
        if rnd.random() < 0.08:
            hi = rnd.choice([0.0, 1.0])
        lo = rnd.choice([0.0, 0.0, 0.0, -5.0, None])
        base = rnd.choice([0.0, hi, hi * 0.5, hi * 0.9, rnd.uniform(0, max(hi, 1.0)), rnd.uniform(0, 10.0), 1.0])
        return (f2b(base), f2b(lo) if lo is not None else NEG_FLT_MAX, f2b(hi))

    lb, ll, lh = stat(False)
    mb, ml, mh = stat(True)
    d = dict(now=now, row=row, lbase=lb, llo=ll, lhi=lh, mbase=mb, mlo=ml, mhi=mh,
             lrate=f2b(rate_pick(rnd)), mrate=f2b(rate_pick(rnd)),
             f20=rnd.choice([0, 0x40, 0x40, 0x40, 0x80, 0xC0, 0x100, 0x140]), f5c=rnd.choice([0, 0, 0, 0x100]))
    ops = [("I",)]
    for _ in range(rnd.randrange(4, 10)):
        r = rnd.random()
        if r < 0.20:
            ops.append(("TICK", rnd.choice([1, 1, 2, 5, 13, 40, 200, 211, 600])))
        elif r < 0.26:
            ops.append((rnd.choice(["LON", "LOFF", "LDIS", "LEN"]),))
        elif r < 0.31:
            ops.append((rnd.choice(["MDIS", "MEN"]),))
        elif r < 0.45:
            ops.append(("LADD", f2b(rate_pick(rnd))))
        elif r < 0.57:
            ops.append(("MADD", f2b(rate_pick(rnd))))
        elif r < 0.63:
            ops.append((rnd.choice(["DEAD", "ALIVE"]),))
        elif r < 0.70:
            ops.append(("MAXL", f2b(rnd.choice([100.0, 150.0, 0.0, 2000.0, 55.5, -10.0]))))
        elif r < 0.77:
            ops.append(("MAXM", f2b(rnd.choice([100.0, 0.0, 300.0, 5.0, 1.0, -3.0]))))
        elif r < 0.88:
            ops.append(("SETL", f2b(rnd.choice([0.0, 1.0, 40.0, 99.5, rnd.uniform(0, 500)]))))
        else:
            ops.append(("SETM", f2b(rnd.choice([0.0, 10.0, 77.7, rnd.uniform(0, 300)]))))
    ops.append(("TICK", rnd.choice([1, 3, 100, 400])))
    return d, ops


def run_case(n, d, ops):
    lp, mp = setup(d)
    out = ["CASE %d %08x %d %08x %08x %08x %08x %08x %08x %08x %08x %x %x" % (
        n, d["now"], d["row"], d["lbase"], d["llo"], d["lhi"], d["mbase"], d["mlo"], d["mhi"], d["lrate"], d["mrate"],
        d["f20"], d["f5c"])]
    for op in ops:
        do_op(op, lp, mp)
        out.append("OP %s" % op_text(op))
        out.append("S %s" % snapshot(lp, mp))
    out.append("END")
    return out


def long_case(rnd, n):
    """A 160 s run: the 150 s timer of the forever modifier fires and does nothing."""
    d, ops = gen_case(rnd)
    d["lrate"] = f2b(rnd.choice([0.25, 0.5, 1.5]))
    d["mrate"] = f2b(rnd.choice([0.01, 0.9, 2.0]))
    d["f20"] = 0x40
    d["f5c"] = 0
    d["lhi"] = f2b(5000.0)
    d["mhi"] = f2b(5000.0)
    d["lbase"] = f2b(1.0)
    d["mbase"] = f2b(0.0)
    d["now"], d["row"] = 0, 0
    ops = [("I",), ("TICK", 1000), ("LADD", f2b(0.75)), ("TICK", 15000), ("MADD", f2b(-0.5)), ("TICK", 16500)]
    return d, ops


def gen(path, count, seed):
    rnd = random.Random(seed)
    lines = ["# Unit life / mana regeneration scenarios (docs/spec/agile.md section 9): tools/emu/regen_emu.py gen on the real routines.",
             "# CASE n clock row lbase llo lhi mbase mlo mhi lrate mrate flags20 flags5c | OP <op> | S life mana lslope mslope lmod mmod lbase mbase lrate mrate | END",
             "# lmod / mmod: '-' no modifier object, 'x' released, else the rate stored in the modifier object"]
    nops = 0
    for n in range(count):
        d, ops = gen_case(rnd)
        lines += run_case(n, d, ops)
        nops += len(ops)
        if n % 50 == 0:
            print("case", n, flush=True)
    for k in range(3):
        d, ops = long_case(rnd, count + k)
        lines += run_case(count + k, d, ops)
        print("long case", k, flush=True)
    with open(path, "w", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote", path, "cases", count + 3, "ops", nops)


def probe():
    d = dict(now=0, row=0, lbase=f2b(100.0), llo=f2b(0.0), lhi=f2b(500.0), mbase=f2b(50.0), mlo=f2b(0.0), mhi=f2b(200.0),
             lrate=f2b(0.25), mrate=f2b(0.5), f20=0x40, f5c=0)
    for line in run_case(0, d, [("I",), ("TICK", 200), ("LADD", f2b(1.0)), ("TICK", 200), ("DEAD",), ("TICK", 10), ("ALIVE",),
                                ("TICK", 10), ("LDIS",), ("MADD", f2b(-0.5)), ("TICK", 3)]):
        print(line)


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "probe"
    if cmd == "probe":
        probe()
    elif cmd == "gen":
        gen(sys.argv[2], int(sys.argv[3]), int(sys.argv[4]) if len(sys.argv) > 4 else 1)
