"""Unicorn checks for S37 (docs/spec/abilities-undead-ne-heroes.md, section S37).

ORIGINAL code of the user's Game.dll runs; the agent world is replaced by hooks that record their arguments.
`CHECK=name GAME_DLL=... py tools/emu/undead_ne4_emu.py [--dump DIR] [N] [SEED]`
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
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
DATA = emu.DATA
wr, rd = A.wr, A.rd
f2u, u2f = A.f2u, A.u2f
hook_fn, stub_slot = U3.hook_fn, U3.stub_slot

SOFT = {  # name -> address (out = ECX, a = *EDX, b = *[ESP+4], ret 4)
    "add": 0x6F6EF000, "sub": 0x6F6EEEF0, "mul": 0x6F6EEE20, "div": 0x6F6EFE10,
}
SQRT, I2R, F2I = 0x6F6EF750, 0x6F6EF350, 0x6F6EEC20


class Trace:
    """Records every soft-float operation (op, operands, result) of the code that runs."""

    def __init__(self):
        self.ops = []
        self.hooks = []
        for name, addr in SOFT.items():
            self.hooks.append(mu.hook_add(UC_HOOK_CODE, self._bin(name), begin=addr, end=addr))
        self.hooks.append(mu.hook_add(UC_HOOK_CODE, self._un("sqrt"), begin=SQRT, end=SQRT))
        self.hooks.append(mu.hook_add(UC_HOOK_CODE, self._i2r(), begin=I2R, end=I2R))

    def stop(self):
        for h in self.hooks:
            mu.hook_del(h)

    def _post(self, ret, fn):
        h = []

        def cb(uc2, addr, size, user):
            fn(uc2)
            mu.hook_del(h[0])

        h.append(mu.hook_add(UC_HOOK_CODE, cb, begin=ret, end=ret))

    def _bin(self, name):
        def cb(uc, addr, size, user):
            esp = uc.reg_read(UC_X86_REG_ESP)
            out = uc.reg_read(UC_X86_REG_ECX)
            a = rd(uc.reg_read(UC_X86_REG_EDX))
            b = rd(rd(esp + 4))
            self._post(rd(esp), lambda u: self.ops.append((name, a, b, rd(out))))
        return cb

    def _un(self, name):
        def cb(uc, addr, size, user):
            esp = uc.reg_read(UC_X86_REG_ESP)
            out = uc.reg_read(UC_X86_REG_ECX)
            a = rd(uc.reg_read(UC_X86_REG_EDX))
            self._post(rd(esp), lambda u: self.ops.append((name, a, None, rd(out))))
        return cb

    def _i2r(self):
        def cb(uc, addr, size, user):
            esp = uc.reg_read(UC_X86_REG_ESP)
            out = uc.reg_read(UC_X86_REG_ECX)
            a = uc.reg_read(UC_X86_REG_EDX)
            self._post(rd(esp), lambda u: self.ops.append(("i2r", a, None, rd(out))))
        return cb


def fmt(op):
    n, a, b, r = op
    fa = "%g" % u2f(a) if n != "i2r" else str(a)
    if b is None:
        return "%s(%s) = %g" % (n, fa, u2f(r))
    return "%s(%s, %g) = %g" % (n, fa, u2f(b), u2f(r))


def hexf(x):
    return "%08x" % (x & 0xFFFFFFFF)


def rand_f(r, lo, hi):
    return r.uniform(lo, hi)


# ---------------------------------------------------------------------------------------------------------------
# Carrion Swarm launch geometry: `CAbilityCarrionSwarm` effect core 0x6F1DD4F0

SW_LVL = A.LVL


def setup_swarm_launch():
    A.setup_stubs()
    A.make_ability(0x6F91150C, 0x41556373, 0)
    st = {}
    uvt = DATA + 0x9000
    mu.mem_write(uvt, bytes(0x800))
    pvt = DATA + 0x9900
    mu.mem_write(pvt, bytes(0x100))
    wr(A.UNIT, uvt)
    s_b8 = stub_slot()
    # unit slot 0xB8 returns the position object (unit + 0x164) and pops nothing (CUnit `0x6F28B0E0`)
    mu.mem_write(s_b8, bytes([0xB8]) + struct.pack("<I", A.UNIT + 0x164) + bytes([0xC3]))
    wr(uvt + 0xB8, s_b8)
    wr(A.UNIT + 0x164, pvt)
    s_face = stub_slot()
    hook_fn(s_face, lambda uc, esp, ecx: (wr(rd(esp + 4), f2u(st["facing"])), 0)[1], 4)
    wr(pvt + 0x1C, s_face)

    def norm(uc, esp, ecx):  # the position normaliser 0x6F4743A0 (ret 4): the caster's x, y, z
        out = rd(esp + 4)
        wr(out, f2u(st["cx"]))
        wr(out + 4, f2u(st["cy"]))
        wr(out + 8, 0)
        return out

    hook_fn(0x6F4743A0, norm, 4)
    cap = {}

    def creator(uc, esp, ecx):  # the missile creator (ability slot 0x42C = 0x6F1F2A80): 12 stack arguments
        args = [rd(esp + 4 + 4 * i) for i in range(12)]
        cap["args"] = args
        cap["vals"] = [[rd(a + 4 * k) for k in range(4)] if 0x100000 <= a < 0x110000 else None for a in args]
        return 0

    hook_fn(0x6F1F2A80, creator, 0x30)
    hook_fn(0x6F1CF440, lambda uc, esp, ecx: 0, 20)

    def bounds(uc, esp, ecx):
        for i, v in enumerate((-9000.0, -9000.0, 9000.0, 9000.0)):
            wr(ecx + 4 * i, f2u(v))
        return ecx

    hook_fn(0x6F00F560, bounds, 0)
    zs = stub_slot()
    mu.mem_write(zs, bytes([0xD9, 0xEE, 0xC2, 0x0C, 0x00]))   # fldz ; ret 12 (terrain height 0)
    mu.mem_write(0x6F24F3E0, bytes([0xE9]) + struct.pack("<i", zs - (0x6F24F3E0 + 5)))
    hook_fn(0x6F040830, lambda uc, esp, ecx: 0, 12)
    hook_fn(0x6F011300, lambda uc, esp, ecx: ecx, 4)
    hook_fn(0x6F02EE20, lambda uc, esp, ecx: 0, 8)
    return st, cap


def swarm_launch(st, cap, area, datac, cx, cy, tx, ty, facing):
    lvl = SW_LVL
    wr(lvl + 0x18, f2u(area))
    wr(lvl + 0x28, f2u(datac))
    wr(lvl + 0x20, f2u(75.0))
    wr(lvl + 0x24, f2u(300.0))
    wr(lvl + 0x2C, f2u(200.0))
    st.update(cx=cx, cy=cy, facing=facing)
    cap.clear()
    wr(A.OBJ + 0x300, f2u(tx))
    wr(A.OBJ + 0x304, f2u(ty))
    try:
        emu.call(0x6F1DD4F0, ecx=A.OBJ, stack_args=(A.OBJ + 0x300, A.OBJ + 0x304, 0))
    except Exception:
        if "vals" not in cap:    # the run ends in the unmodelled tail after the creator call
            raise
    v = cap["vals"]
    return v[0][0], v[0][1], v[1][0], v[1][1]


def run_swarm_launch(cases, r):
    st, cap = setup_swarm_launch()
    rows = []
    for i in range(cases):
        area = r.choice((100.0, 125.0, 0.0, rand_f(r, 10, 400)))
        datac = r.choice((800.0, 0.0, rand_f(r, 50, 1800)))
        cx, cy = rand_f(r, -6000, 6000), rand_f(r, -6000, 6000)
        mode = r.choice(("far", "far", "far", "near", "edge", "same"))
        if mode == "far":
            tx, ty = cx + rand_f(r, -1500, 1500), cy + rand_f(r, -1500, 1500)
        elif mode == "near":
            tx, ty = cx + rand_f(r, -19, 19), cy + rand_f(r, -14, 14)
        elif mode == "edge":
            tx, ty = cx + 20.0, cy
        else:
            tx, ty = cx, cy
        facing = r.choice((0.0, rand_f(r, 0, 6.2831853), rand_f(r, -3.2, 3.2)))
        ans = swarm_launch(st, cap, area, datac, cx, cy, tx, ty, facing)
        rows.append((f2u(area), f2u(datac), f2u(cx), f2u(cy), f2u(tx), f2u(ty), f2u(facing)) + ans)
    return rows


# ---------------------------------------------------------------------------------------------------------------
# Carrion Swarm missile tick: `CMissileCarrionSwarm` message 0xD01C1 handler 0x6F1EF6D0

SM = DATA + 0x8000
SW_UNITS = DATA + 0xA000


def setup_swarm_tick():
    A.setup_stubs()
    log = []
    state = {}
    mu.mem_write(SM, bytes(0x400))
    wr(SM, 0x6F8FBAAC)
    pvt = U3.PVT
    mu.mem_write(pvt, bytes(0x80))
    pset = stub_slot()

    def propset(uc, esp, ecx):   # property setter: value = *arg, stored at +4 of the property object
        val = rd(rd(esp + 4))
        wr(ecx + 4, val)
        log.append(("set", ecx - SM, val))
        return ecx + 4

    hook_fn(pset, propset, 8)
    wr(pvt, pset)

    def pos_hook(uc, esp, ecx):  # position of the missile (0x6F47A580, ECX = missile + 0x78)
        out = rd(esp + 4)
        wr(out, f2u(state["px"]))
        wr(out + 4, f2u(state["py"]))
        wr(out + 8, 0)
        return out

    hook_fn(0x6F47A580, pos_hook, 4)
    hook_fn(0x6F4786B0, lambda uc, esp, ecx: 0, 0)      # handle -> unit: the caster is gone (no presentation calls)
    uvt = DATA + 0x9C00
    mu.mem_write(uvt, bytes(0x400))
    sl = stub_slot()
    hook_fn(sl, lambda uc, esp, ecx: (wr(rd(esp + 4), f2u(state["life"][ecx])), rd(esp + 4))[1], 4)
    wr(uvt + 0x12C, sl)
    sp = stub_slot()
    hook_fn(sp, lambda uc, esp, ecx: 5, 0)
    wr(uvt + 0xEC, sp)
    sd = stub_slot()
    hook_fn(sd, lambda uc, esp, ecx: 0, 0)
    wr(uvt, sd)

    def builder(uc, esp, ecx):   # the group builder 0x6F2A3270 (cdecl, 7 arguments)
        a = [rd(esp + 4 + 4 * i) for i in range(7)]
        state["builder"] = (a[2], a[3], a[4], a[5], a[6])    # kind, radius, x, y, mask
        wr(a[1] + 0xC, len(state["units"]))
        for i, u in enumerate(state["units"]):
            wr(a[1] + 0x10 + 4 * i, u)
        return 0

    hook_fn(0x6F2A3270, builder, 0)

    def hit(uc, esp, ecx):       # 0x6F1DD3A0(unit, &amount, ...): damage the unit by amount * factor
        unit, dptr = rd(esp + 4), rd(esp + 8)
        d = u2f(rd(dptr))
        state["hits"].append((unit, rd(dptr)))
        before = state["life"][unit]
        state["life"][unit] = u2f(f2u(max(0.0, before - d * state["factor"].get(unit, 1.0))))
        return 0

    hook_fn(0x6F1DD3A0, hit, 12)
    hook_fn(0x6F4778F0, lambda uc, esp, ecx: state.setdefault("timer", []).append((ecx - SM, rd(rd(esp + 4)), rd(esp + 8))) or 0, 20)

    def vec_ctor(uc, esp, ecx):
        ptr, size, cnt = rd(esp + 4), rd(esp + 8), rd(esp + 12)
        mu.mem_write(ptr, bytes(size * cnt))
        return 0

    hook_fn(0x6F7E16C5, vec_ctor, 0x14)
    hook_fn(0x6F7E1662, lambda uc, esp, ecx: 0, 0x10)

    def memset(uc, esp, ecx):
        ptr, val, n = rd(esp + 4), rd(esp + 8), rd(esp + 12)
        mu.mem_write(ptr, bytes([val & 255]) * n)
        return ptr

    hook_fn(0x6F7E11C2, memset, 0)
    hook_fn(0x6F29EF80, lambda uc, esp, ecx: 0, 8)
    hook_fn(0x6F2A3EA0, lambda uc, esp, ecx: 0, 12)
    return state, log, uvt


def swarm_tick(state, log, uvt, p1, datac, area, datad, dataa, cap, pos, lives, factors):
    pvt = U3.PVT
    mu.mem_write(SM + 0x80, bytes(0x300))
    for off in (0x90, 0x98, 0xA0, 0xC8, 0xD0, 0xD8, 0xE0, 0xEC):
        wr(SM + off, pvt)
    wr(SM + 0x94, f2u(p1[0]))
    wr(SM + 0x9C, f2u(p1[1]))
    wr(SM + 0xA4, f2u(datac))
    wr(SM + 0xCC, f2u(area))
    wr(SM + 0xD4, f2u(datad))
    wr(SM + 0xDC, f2u(dataa))
    wr(SM + 0xE4, f2u(cap))
    wr(SM + 0xF0, f2u(3.0))
    wr(SM + 0xB8, 0xFFFFFFFF)
    wr(SM + 0xBC, 0xFFFFFFFF)
    state.update(px=pos[0], py=pos[1], units=[], hits=[], life={}, factor={})
    state.pop("timer", None)
    state.pop("builder", None)
    for i, life in enumerate(lives):
        u = SW_UNITS + i * 0x200
        mu.mem_write(u, bytes(0x200))
        wr(u, uvt)
        state["units"].append(u)
        state["life"][u] = u2f(f2u(life))
        state["factor"][u] = factors[i]
    log.clear()
    emu.call(0x6F1EF6D0, ecx=SM)
    afters = [f2u(state["life"][u]) for u in state["units"]]
    return dict(
        builder=state["builder"], hits=[h[1] for h in state["hits"]], afters=afters,
        cap=rd(SM + 0xE4), ec=rd(SM + 0xEC + 4), timer=state.get("timer"),
    )


def run_swarm_tick(cases, r):
    import math
    state, log, uvt = setup_swarm_tick()
    rows = []
    for i in range(cases):
        p1 = (rand_f(r, -4000, 4000), rand_f(r, -4000, 4000))
        datac = r.choice((800.0, 0.0, rand_f(r, 100, 1500)))
        area = r.choice((100.0, 125.0, rand_f(r, 20, 300)))
        datad = r.choice((300.0, 125.0, rand_f(r, 20, 500)))
        dataa = r.choice((75.0, 130.0, 200.0, rand_f(r, 1, 300), 0.0))
        cap = r.choice((300.0, 1000.0, 0.0, 0.005, 0.01, 0.0105, 0.011, 0.0115, rand_f(r, 0, 2400), rand_f(r, 0, 40)))
        frac = r.choice((0.0, 1.0, rand_f(r, -0.2, 1.3)))
        ang = rand_f(r, 0, 6.2831853)
        dist = datac * frac if datac else rand_f(r, 0, 900)
        pos = (p1[0] + dist * math.cos(ang), p1[1] + dist * math.sin(ang))
        n = r.choice((0, 1, 2, 3, 4, 5, 8, 12, r.randint(0, 20)))
        lives = [r.choice((rand_f(r, 1, 2500), rand_f(r, 1, 120), 0.5)) for _ in range(n)]
        factors = [r.choice((1.0, 1.0, 0.5, 0.0, rand_f(r, 0, 1.5))) for _ in range(n)]
        out = swarm_tick(state, log, uvt, p1, datac, area, datad, dataa, cap, pos, lives, factors)
        rows.append(((f2u(p1[0]), f2u(p1[1]), f2u(datac), f2u(area), f2u(datad), f2u(dataa), f2u(cap), f2u(pos[0]), f2u(pos[1])),
                     [f2u(x) for x in lives], out))
    return rows


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump = args[1]
        args = args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 1
    which = os.environ.get("CHECK", "launch")
    if dump:
        os.makedirs(dump, exist_ok=True)
    r = random.Random(seed)
    if which == "launch":
        rows = run_swarm_launch(cases, r)
        for row in rows[:6]:
            print("launch", " ".join(hexf(x) for x in row[:7]), "|", " ".join(hexf(x) for x in row[7:]))
        print("launch: %d cases run" % len(rows))
        if dump:
            with open(os.path.join(dump, "swarm_launch.txt"), "w") as fh:
                for row in rows:
                    fh.write(" ".join(hexf(x) for x in row[:7]) + " | " + " ".join(hexf(x) for x in row[7:]) + "\n")
    if which == "tick":
        rows = run_swarm_tick(cases, r)
        for head, lives, out in rows[:4]:
            print("tick", out["builder"], out["hits"][:3], hexf(out["cap"]), out["timer"])
        print("tick: %d cases run" % len(rows))
        if dump:
            with open(os.path.join(dump, "swarm_tick.txt"), "w") as fh:
                for head, lives, out in rows:
                    b = out["builder"]
                    fh.write(" ".join(hexf(x) for x in head) + " " + str(len(lives)) + " | "
                             + " ".join(hexf(x) for x in lives) + " | "
                             + " ".join(hexf(x) for x in out["afters"]) + " | "
                             + "%s %s %s | %s | %s %s %d\n" % (
                                 hexf(b[1]), hexf(b[2]), hexf(b[3]),
                                 " ".join(hexf(x) for x in out["hits"]) or "-",
                                 hexf(out["cap"]), hexf(out["ec"]), 1 if out["timer"] else 0))


if __name__ == "__main__":
    main()
