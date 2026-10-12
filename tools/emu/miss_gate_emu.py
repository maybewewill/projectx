"""Unicorn check of the ChanceToMiss branch of the miss gate 0x6F2BEAC0 (Game.dll 1.26a), missile form (a3 = 1).

Companion of evasion_gate_emu.py (which runs the gate with a3 = 0). With a3 != 0 and no evasion selected (walks A and B
fail) the gate takes ONE draw on stream 2 (0x6F2853D0, EDX = 2), reads Misc "ChanceToMiss", and calls the attacker
exemption 0x6F0C7DE0, then compares two terrain levels (attacker cell versus target cell).

Real code that runs: the gate, the helper 0x6F2BEA20, the walks, the iterator, the compare 0x6F01FD80, the exemption
0x6F0C7DE0, the world-to-cell helpers 0x6F0D8DF0 / 0x6F0D8DC0 (soft-float subtract 0x6F6EEEF0 and exponent shift),
the float-to-int 0x6F6EEC20.
Stubbed: RNG 0x6F2853D0 (queue), Misc reader 0x6F009EE0 (value per key, key recorded), the position getter (vtable
slot 0xB8) and the normaliser 0x6F4743A0 (positions supplied directly), the terrain level 0x6F0128C0 (a fixed
asymmetric function of its two integer arguments, arguments recorded).

Usage: GAME_DLL=... py tools/emu/miss_gate_emu.py [cases] [--json out.json] [--rs out.rs]
"""
import json
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import evasion_gate_emu as evg  # noqa: E402  (installs the evasion harness hooks)
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

mu, emu, D = evg.mu, evg.emu, evg.D
w32, r32, f32b, bf32 = evg.w32, evg.r32, evg.f32b, evg.bf32

MISC = 0x6F009EE0
LEVEL = 0x6F0128C0
NORM = 0x6F4743A0
WORLD_PTR = 0x6FAB7368
EXEMPT = 0x6F0C7DE0

EXTRA = 0x00400000
mu.mem_map(EXTRA, 0x10000)
WORLD = EXTRA
POSOBJ_A = EXTRA + 0x100
POSOBJ_T = EXTRA + 0x200
VT_UNIT = EXTRA + 0x400
STUB_B8A = EXTRA + 0x800
STUB_B8T = EXTRA + 0x810
KEY_CHANCE = 0x6F932304

M = {"misc": 0.0, "keys": [], "levels": []}


def level_fn(a, b):
    """Asymmetric deterministic stand-in for the terrain level; small non-negative integers."""
    return (a * 7 + b * 3 + (a ^ b)) % 6


def hook(uc, eip, size, user):
    if eip == MISC:
        out = mu.reg_read(UC_X86_REG_ECX)
        esp = mu.reg_read(UC_X86_REG_ESP)
        M["keys"].append(r32(esp + 4))
        w32(out, f32b(M["misc"]))
        mu.reg_write(UC_X86_REG_EAX, out)
        evg.ret_to(8)
    elif eip == LEVEL:
        a = mu.reg_read(UC_X86_REG_ECX)
        b = mu.reg_read(UC_X86_REG_EDX)
        a = a - (1 << 32) if a & 0x80000000 else a
        b = b - (1 << 32) if b & 0x80000000 else b
        M["levels"].append((a, b))
        mu.reg_write(UC_X86_REG_EAX, level_fn(a, b) & 0xFFFFFFFF)
        evg.ret_to(4)
    elif eip == STUB_B8A or eip == STUB_B8T:
        # position getter (vtable slot 0xB8): returns the position object, no pop (the out pointer stays for NORM)
        mu.reg_write(UC_X86_REG_EAX, POSOBJ_A if eip == STUB_B8A else POSOBJ_T)
        evg.ret_to(0)
    elif eip == NORM:
        obj = mu.reg_read(UC_X86_REG_ECX)
        out = r32(mu.reg_read(UC_X86_REG_ESP) + 4)
        for i in range(3):
            w32(out + 4 * i, r32(obj + 4 * i))
        mu.reg_write(UC_X86_REG_EAX, out)
        evg.ret_to(4)


mu.hook_add(UC_HOOK_CODE, hook)
mu.mem_write(STUB_B8A, b"\xc3")
mu.mem_write(STUB_B8T, b"\xc3")
w32(WORLD_PTR, WORLD)


def f2i_trunc(x):
    return int(x)  # Python int() truncates toward zero, as 0x6F6EEC20 does


def cell(pos, origin):
    # exact for the dyadic values used: subtract, divide by 32 (exponent shift), truncate
    return f2i_trunc((pos - origin) / 32.0)


def model(c):
    """(result, draws, level calls) of the gate for a case without evasion entries."""
    r = bf32(f32b(c["r"]))
    m = bf32(f32b(c["M"]))
    # 0x6F01FD80: returns 1 when M <= r (ordered); the gate then returns 0
    if m <= r:
        return 0, 1, []
    if c["exempt"]:
        return 0, 1, []
    ax, ay = c["a"][0], c["a"][1]
    tx, ty = c["t"][0], c["t"][1]
    ox, oy = c["ox"], c["oy"]
    # hypothesis fixed after the first run (see the arguments logged by the level stub)
    la = level_fn(cell(ax, ox), cell(ay, oy))
    lt = level_fn(cell(tx, ox), cell(ty, oy))
    return (1 if lt > la else 0), 1, [(cell(ax, ox), cell(ay, oy)), (cell(tx, ox), cell(ty, oy))]


def run_case(c, scene=None, queue=None):
    # scene None: no evasion entries (empty ability lists); otherwise the lists built by evasion_gate_emu.build
    if scene is None:
        for u in (evg.UNIT_A, evg.UNIT_T):
            mu.mem_write(u, b"\0" * 0x400)
            w32(u + 4, 100)
            w32(u + 0x1DC, 0xFFFFFFFF)
            w32(u + 0x1E0, 0xFFFFFFFF)
        mu.mem_write(evg.ARR_LO, b"\0" * 0x1000)
        mu.mem_write(evg.ARR_HI, b"\0" * 0x1000)
        w32(evg.TABLE_PTR, evg.TBL)
        w32(evg.TBL + 0xC, evg.ARR_LO)
        w32(evg.TBL + 0x1C, evg.NLO)
        w32(evg.TBL + 0x2C, evg.ARR_HI)
        w32(evg.TBL + 0x3C, evg.NHI)
    mu.mem_write(VT_UNIT, b"\0" * 0x400)
    # separate vtables so that slot 0xB8 differs per unit
    vta, vtt = VT_UNIT, VT_UNIT + 0x200
    w32(vta + 0xB8, STUB_B8A)
    w32(vtt + 0xB8, STUB_B8T)
    w32(evg.UNIT_A, vta)
    w32(evg.UNIT_T, vtt)
    # exemption inputs (attacker): +0x1FC bit 1, +0x5C bit 29, +0x200 <= 0
    if c["exempt"]:
        w32(evg.UNIT_A + 0x1FC, 2)
        w32(evg.UNIT_A + 0x5C, 0x20000000)
        w32(evg.UNIT_A + 0x200, 0)
    else:
        # not exempt through one of the three conditions at random (shape recorded in c["why"])
        why = c["why"]
        w32(evg.UNIT_A + 0x1FC, 0 if why == 0 else 2)
        w32(evg.UNIT_A + 0x5C, 0 if why == 1 else 0x20000000)
        w32(evg.UNIT_A + 0x200, 1 if why == 2 else 0)
    w32(WORLD + 0x6C, f32b(c["ox"]))
    w32(WORLD + 0x70, f32b(c["oy"]))
    for obj, p in ((POSOBJ_A, c["a"]), (POSOBJ_T, c["t"])):
        for i in range(3):
            w32(obj + 4 * i, f32b(p[i]))
    M["misc"] = c["M"]
    M["keys"] = []
    M["levels"] = []
    evg.ST["draws"] = []
    evg.ST["effects"] = []
    evg.ST["queue"] = [f32b(x) for x in (queue if queue is not None else [c["r"]])]
    res = emu.call(evg.GATE, ecx=evg.UNIT_A, edx=evg.UNIT_T, stack_args=(1, 0)) & 0xFF
    return res, len(evg.ST["draws"]), list(M["levels"]), evg.ST["draws"][:], list(M["keys"])


def gen(rng):
    pos = lambda: rng.choice([0.0, 31.5, 32.0, 63.875, -0.5, -32.0, -33.25, 127.5, 128.0, 500.25, -700.5, rng.randrange(-2000, 2000) / 8.0])
    c = {
        "r": rng.choice([0.0, 0.25, 0.5, 0.75, rng.random(), float("nan")]),
        "M": rng.choice([0.0, 0.25, 0.5, 0.75, 1.0, rng.random(), float("nan"), -0.5]),
        "exempt": rng.random() < 0.25,
        "why": rng.randrange(3),
        "ox": rng.choice([0.0, -4096.0, -2048.5, 1024.0, rng.randrange(-4000, 4000) / 8.0]),
        "oy": rng.choice([0.0, -4096.0, -2048.5, 1024.0, rng.randrange(-4000, 4000) / 8.0]),
        "a": [pos(), pos(), 0.0],
        "t": [pos(), pos(), 0.0],
    }
    if rng.random() < 0.15:
        c["t"][0], c["t"][1] = c["a"][0], c["a"][1]
    return c


def mixed(n, rng):
    """Missile form (a3 = 1) with random evasion entries: an evasion that fires returns 1 with its single draw;
    otherwise the ChanceToMiss draw follows (a second draw when the evasion roll was taken and failed)."""
    bad = 0
    stats = {"evaded": 0, "tail_after_roll": 0, "tail_only": 0}
    for i in range(n):
        scene = evg.build(rng, 0)
        c = gen(rng)
        rvals = [rng.choice([rng.random(), 0.5, 0.25, 0.75]) for _ in range(4)]
        rv = [bf32(f32b(x)) for x in rvals]
        res_e, draws_e, eff_e = evg.model(scene, rv[:])
        if res_e:
            exp = (1, len(draws_e), [])
            stats["evaded"] += 1
        else:
            k = len(draws_e)
            c["r"] = rv[k]
            t = model(c)
            exp = (t[0], k + 1, t[2])
            stats["tail_after_roll" if k else "tail_only"] += 1
        res, nd, levels, draws, keys = run_case(c, scene=scene, queue=rvals)
        if (res, nd, levels) != exp or any(d != 2 for d in draws):
            bad += 1
            if bad <= 6:
                print("MIXED MISMATCH", i, "got", res, nd, levels, draws, "exp", exp)
    print("mixed cases", n, "mismatches", bad, stats)
    return bad


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    n = int(args[0]) if args else 600
    rng = random.Random(777)
    bad = 0
    stats = {"miss": 0, "nomiss": 0, "exempt": 0, "skipped_tail": 0, "tail": 0}
    rows = []
    for i in range(n):
        c = gen(rng)
        res, nd, levels, draws, keys = run_case(c)
        exp = model(c)
        ok = (res, nd) == (exp[0], exp[1]) and levels == exp[2] and draws == [2] and keys == [KEY_CHANCE]
        if not ok:
            bad += 1
            if bad <= 6:
                print("MISMATCH", i, c, "got", res, nd, levels, draws, [hex(k) for k in keys], "exp", exp)
        if levels:
            stats["tail"] += 1
        stats["miss" if res else "nomiss"] += 1
        stats["exempt"] += 1 if c["exempt"] else 0
        rows.append({"r": f32b(c["r"]), "M": f32b(c["M"]), "exempt": c["exempt"], "ox": f32b(c["ox"]), "oy": f32b(c["oy"]),
                     "a": [f32b(v) for v in c["a"]], "t": [f32b(v) for v in c["t"]], "res": res})
    print("cases", n, "mismatches", bad, stats)
    bad += mixed(n, random.Random(4242))
    for j, a in enumerate(sys.argv):
        if a == "--json":
            json.dump(rows, open(sys.argv[j + 1], "w"))
        if a == "--rs":
            with open(sys.argv[j + 1], "w") as f:
                f.write("// Generated by tools/emu/miss_gate_emu.py from the real gate 0x6F2BEAC0 (a3 = 1), no evasion entries.\n")
                f.write("// (r, M, exempt, origin x, origin y, attacker xyz, target xyz, result) as f32 bit patterns.\n")
                f.write("pub type Row = (u32, u32, bool, u32, u32, [u32; 3], [u32; 3], bool);\n")
                f.write("pub const ROWS: &[Row] = &[\n")
                for r in rows:
                    f.write("    (0x%08X, 0x%08X, %s, 0x%08X, 0x%08X, [%s], [%s], %s),\n" % (
                        r["r"], r["M"], str(r["exempt"]).lower(), r["ox"], r["oy"],
                        ", ".join("0x%08X" % v for v in r["a"]), ", ".join("0x%08X" % v for v in r["t"]),
                        str(bool(r["res"])).lower()))
                f.write("];\n")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
