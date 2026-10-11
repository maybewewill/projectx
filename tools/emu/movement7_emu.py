"""Unicorn vectors for the movement remainder of Game.dll 1.26a (docs/spec/movement-7.md).

Builds on the harness of movement6_emu.py (a synthetic mover `U`, the clock layers at `[0x6FAB73D8]`).

  SN  speed/heading writer 0x6F494160 with the REAL velocity-changed notification (vtable +0x50 = 0x6F494620 ->
      0x6F4A74A0: facing = heading of V when |V|^2 >= 4e-7, and the mover is linked into the world's smoother list)
  FS  facing smoother 0x6F494200 (U+0xC8 chases the facing U+0x8C with a rate U+0xCC; class row of 0x6FAB7440);
      the real finish routine (vtable +0x58 = 0x6F4954F0) runs when the error is below 0.001
  PA  push-apart 0x6F4961C0 on a record R (U = R+0x14): cooldown, previous push applied, neighbour loop,
      random direction from the global generator for coincident neighbours. Stubbed callees (answered by the
      harness): the spatial gather 0x6F495D60 / 0x6F496170 (fills the neighbour list), the placement test 0x6F494E10
      and the position setter 0x6F4737D0 (both recorded).

Lines are text vectors of hex bit patterns. No Game.dll bytes are written into them.

Run:  GAME_DLL=C:\\path\\Game.dll python tools/emu/movement7_emu.py KIND OUT.txt [N] [SEED]
"""
import math
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
import hero_candidate_emu as HC  # noqa: E402
import hero_normaliser_emu as HN  # noqa: E402
import movement6_emu as M6  # noqa: E402

mu = emu.mu
OBJ = HC.OBJ
MGR = HN.MGR
rd32, wr32, f2b, b2f, hx = HN.rd32, HN.wr32, HN.f2b, HN.b2f, HN.hx
SCR = M6.SCR
NODE_A = HC.DATA + 0xC000      # list neighbours of the smoother (prev / next nodes)
NODE_B = HC.DATA + 0xC100

# PA harness layout
PA_R = HC.DATA + 0xB800        # the record R
PA_G = HC.DATA + 0xD100        # the neighbour group object a2
PA_ARR = HC.DATA + 0xD000      # entries of 8 bytes (node pointer, 0)
PA_NODES = HC.DATA + 0xD200    # nodes of 0x40 bytes: +0x30 -> neighbour mover
PA_NB = HC.DATA + 0xC400       # neighbour mover objects, 0x100 each
MAXNB = 8


TABLE_HITS = [0]
# Softfloat entry points that read the game's lookup tables (reciprocal, sine, arc sine): a vector whose run touched
# one of them is "tabled" and is not embedded in the repository (only full runs with GAME_DLL use it).
TABLE_ENTRIES = (0x6F6EF100, 0x6F6EF820, 0x6F6EF8D0, 0x6F6F0140, 0x6F6EFC70, 0x6F6EFB00, 0x6F6F0810, 0x6F6EFE10,
                 0x6F6EFE50, 0x6F6F0060, 0x6F6F07B0)


def _table_hook(uc, address, size, user):
    TABLE_HITS[0] += 1


def setup():
    from unicorn import UC_HOOK_CODE
    for a in TABLE_ENTRIES:
        mu.hook_add(UC_HOOK_CODE, _table_hook, begin=a, end=a)
    M6.setup()
    # the real notification and finish routine
    wr32(HC.VT + 0x50, 0x6F494620)
    wr32(HC.VT + 0x58, 0x6F4954F0)
    wr32(0x6FAAE478, 0x38D1B717)


def fill_unit(base, rng, g, moving=True):
    """A synthetic mover at `base` (position fields only), clock layers g as drawn by M6.grids."""
    o14 = rng.getrandbits(32) & 0x7FFFFFFF
    if rng.random() < 0.25:
        o14 |= 0x80000000
    grid = g[1] if o14 & 0x80000000 else g[0]
    t_g = b2f(grid[0])
    r = rng.random()
    if r < 0.15:
        t0, row0 = t_g, grid[1]
    elif r < 0.8:
        t0, row0 = max(0.0, t_g - rng.uniform(0.0, 0.2)), grid[1]
    else:
        t0, row0 = rng.uniform(0.0, 299.0), max(0, grid[1] - rng.randrange(0, 3))
    vals = dict(o14=o14, t0=f2b(t0), row0=row0)
    wr32(base + 0x14, o14)
    HN.wrf(base + 0x70, t0)
    wr32(base + 0x74, row0)
    return vals


# ---------------------------------------------------------------------------------------------------------------- SN
def sn_line(rng):
    g = M6.grids(rng)
    u = M6.unit_state(rng, g)
    facing = M6.rfloat(rng, 0.0, 6.2831855, 0.05)
    wr32(OBJ + 0x8C, f2b(facing))
    wr32(OBJ + 0xC0, 0)
    wr32(OBJ + 0xC4, 0)
    listed = rng.random() < 0.4
    wr32(OBJ + 4, 0x11110000 if listed else 0)
    wr32(OBJ + 8, 0x22220000 if listed else 0)
    wr32(HN.MGR + 0x440, 0)
    r = rng.random()
    if r < 0.1:
        sp = 0.0
    elif r < 0.17:
        sp = -rng.uniform(0.0, 3.0)
    else:
        sp = M6.rfloat(rng, 0.0, 22.0)
    hd = M6.rfloat(rng, 0.0, 9.0, 0.03) if rng.random() < 0.8 else M6.rfloat(rng, -3.0, 40.0)
    SP, HP = SCR, SCR + 4
    wr32(SP, f2b(sp))
    wr32(HP, f2b(hd))
    M6.CLAMP_HITS[0] = 0
    emu.call(0x6F494160, ecx=OBJ, stack_args=(SP, HP))
    out = M6.snap_unit() + [rd32(OBJ + 0x8C), rd32(OBJ + 0xC0), rd32(OBJ + 0xC4), rd32(OBJ + 4), rd32(OBJ + 8)]
    return "SN " + " ".join([hx(u["o14"]), hx(u["t0"]), hx(u["row0"]), hx(u["cx"]), hx(u["cy"]), hx(u["vx"]), hx(u["vy"]),
                             hx(u["cap"]), hx(u["flags"]), hx(g[0][0]), hx(g[0][1]), hx(g[0][2]), hx(g[1][0]),
                             hx(g[1][1]), hx(g[1][2]), hx(f2b(facing)), hx(f2b(sp)), hx(f2b(hd)), hx(1 if listed else 0)]
                            + [hx(v) for v in out] + [hx(M6.CLAMP_HITS[0])])


# ---------------------------------------------------------------------------------------------------------------- FS
def fs_line(rng):
    cls = rng.randrange(16) if rng.random() < 0.3 else rng.randrange(9)
    wr32(OBJ + 0xD8, (rng.getrandbits(32) & ~0xF00) | (cls << 8))
    two_pi = 6.2831855
    face = M6.rfloat(rng, 0.0, two_pi, 0.03)
    r = rng.random()
    if r < 0.15:
        cur = face + rng.uniform(-0.002, 0.002)          # near the finish threshold
    elif r < 0.25:
        cur = face
    else:
        cur = M6.rfloat(rng, 0.0, two_pi, 0.03)
    r = rng.random()
    if r < 0.2:
        rate = 0.0
    elif r < 0.7:
        rate = rng.uniform(-0.3, 0.3)
    elif r < 0.95:
        rate = rng.uniform(-3.0, 3.0)
    else:
        rate = rng.uniform(-50.0, 50.0)
    linked = rng.random() < 0.7
    wr32(OBJ + 0x8C, f2b(face))
    wr32(OBJ + 0xC8, f2b(cur))
    wr32(OBJ + 0xCC, f2b(rate))
    # list nodes: prev = NODE_A, next = NODE_B, linked both ways
    if linked:
        wr32(OBJ + 4, NODE_A)
        wr32(OBJ + 8, NODE_B)
    else:
        wr32(OBJ + 4, 0)
        wr32(OBJ + 8, 0)
    wr32(NODE_A + 8, OBJ)
    wr32(NODE_B + 4, OBJ)
    TABLE_HITS[0] = 0
    emu.call(0x6F494200, ecx=OBJ)
    out = [rd32(OBJ + 0xC8), rd32(OBJ + 0xCC), rd32(OBJ + 4), rd32(OBJ + 8), rd32(NODE_A + 8), rd32(NODE_B + 4)]
    return "FS " + " ".join([hx(cls), hx(f2b(face)), hx(f2b(cur)), hx(f2b(rate)), hx(1 if linked else 0)]
                            + [hx(v) for v in out] + [hx(1 if TABLE_HITS[0] else 0)])


# ---------------------------------------------------------------------------------------------------------------- PA
PA = dict(gather=None, placed=None, set_to=None, ok=0)


def _install_pa_hooks():
    def h_gather(uc, esp):
        # this = G (ecx); stack: U, T row pointer, radius byte, nibble; retn 0x10
        PA["gather"] = (rd32(esp + 4), rd32(esp + 8), rd32(esp + 12), rd32(esp + 16))
        return None
    HC.hook("pa_gather", 0x6F495D60, 16, h_gather)
    HC.hook("pa_gather2", 0x6F496170, 0, lambda uc, esp: None)

    def h_place(uc, esp):
        p = rd32(esp + 4)
        PA["placed"] = (rd32(p), rd32(p + 4))
        return PA["ok"]
    HC.hook("pa_place", 0x6F494E10, 4, h_place)

    def h_set(uc, esp):
        p = rd32(esp + 4)
        PA["set_to"] = (rd32(p), rd32(p + 4), rd32(esp + 8))
        return None
    HC.hook("pa_set", 0x6F4737D0, 8, h_set)


_PA_READY = [False]


def pa_line(rng):
    if not _PA_READY[0]:
        _install_pa_hooks()
        _PA_READY[0] = True
    g = M6.grids(rng)
    # own mover (U = OBJ): position fields and last speed
    mine = fill_unit(OBJ, rng, g)
    cx = M6.rfloat(rng, -100.0, 100.0, 0.03)
    cy = M6.rfloat(rng, -100.0, 100.0, 0.03)
    moving = rng.random() < 0.3
    vx = M6.rfloat(rng, -6.0, 6.0, 0.1) if moving else 0.0
    vy = M6.rfloat(rng, -6.0, 6.0, 0.1) if moving else 0.0
    HN.wrf(OBJ + 0x78, cx)
    HN.wrf(OBJ + 0x7C, cy)
    HN.wrf(OBJ + 0x80, vx)
    HN.wrf(OBJ + 0x84, vy)
    r = rng.random()
    last = 0.0 if r < 0.7 else (-rng.uniform(0.0, 2.0) if r < 0.8 else rng.uniform(0.001, 12.0))
    wr32(OBJ + 0xC0, f2b(last))
    # the record
    cls = rng.randrange(5) if rng.random() < 0.9 else rng.randrange(5, 16)
    cool = 0 if rng.random() < 0.7 else rng.randrange(1, 8)
    byte_a = rng.getrandbits(8)
    nib_b = rng.getrandbits(4)
    flags = (nib_b << 28) | (byte_a << 20) | (cls << 16) | cool
    flags |= rng.getrandbits(16) << 0 if False else 0
    for off in range(0, 0x30, 4):
        wr32(PA_R + off, 0)
    wr32(PA_R + 0x14, OBJ)
    wr32(PA_R + 0x20, flags)
    r = rng.random()
    if r < 0.35:
        pvx = pvy = 0.0
    else:
        pvx, pvy = rng.uniform(-1.5, 1.5), rng.uniform(-1.5, 1.5)
        if rng.random() < 0.1:
            pvx = 0.0
    HN.wrf(PA_R + 0x18, pvx)
    HN.wrf(PA_R + 0x1C, pvy)
    PA["ok"] = rng.randrange(2)
    # RNG state of the global generator (A, B), inside the grid manager object
    ra, rb = rng.getrandbits(32), rng.getrandbits(32)
    wr32(MGR + 0, ra)
    wr32(MGR + 4, rb)
    # neighbours
    n = rng.choice([0, 1, 1, 2, 3, 4, 6, MAXNB])
    t0row = [5.0, 7.0, 8.0, 9.0, 10.0][cls] if cls < 5 else 0.0
    nbs = []
    for k in range(n):
        base = PA_NB + 0x100 * k
        vals = fill_unit(base, rng, g)
        r = rng.random()
        if r < 0.15:
            dx = dy = 0.0                      # coincident: the random-direction branch
        elif r < 0.25:
            dx, dy = rng.uniform(-4e-4, 4e-4), rng.uniform(-4e-4, 4e-4)
        elif r < 0.85:
            ang = rng.uniform(0.0, 6.2831855)
            dd = rng.uniform(0.0, 1.1 * (t0row or 5.0))
            dx, dy = dd * math.cos(ang), dd * math.sin(ang)
        else:
            dx, dy = rng.uniform(-30.0, 30.0), rng.uniform(-30.0, 30.0)
        # place the neighbour so that its evaluated position is near (own position + d): use the same snapshot
        # and zero velocity for the neighbour most of the time
        nvx = M6.rfloat(rng, -4.0, 4.0, 0.3)
        nvy = M6.rfloat(rng, -4.0, 4.0, 0.3)
        HN.wrf(base + 0x78, cx + dx)
        HN.wrf(base + 0x7C, cy + dy)
        HN.wrf(base + 0x80, nvx)
        HN.wrf(base + 0x84, nvy)
        nbs.append((vals, cx + dx, cy + dy, nvx, nvy))
        node = PA_NODES + 0x40 * k
        wr32(node + 0x30, base)
        wr32(PA_ARR + 8 * k, node)
        wr32(PA_ARR + 8 * k + 4, 0)
    wr32(PA_G + 0x0C, PA_ARR)
    wr32(PA_G + 0x1C, n)
    PA["gather"] = None
    PA["placed"] = None
    PA["set_to"] = None
    emu.call(0x6F4961C0, ecx=PA_R, stack_args=(PA_G,))
    out = [rd32(PA_R + 0x18), rd32(PA_R + 0x1C), rd32(PA_R + 0x20), rd32(MGR), rd32(MGR + 4)]
    gat = PA["gather"]
    placed = PA["placed"]
    st = PA["set_to"]
    parts = ["PA", hx(cls), hx(flags), hx(f2b(pvx)), hx(f2b(pvy)), hx(f2b(last)), hx(PA["ok"]),
             hx(mine["o14"]), hx(mine["t0"]), hx(mine["row0"]), hx(f2b(cx)), hx(f2b(cy)), hx(f2b(vx)), hx(f2b(vy)),
             hx(g[0][0]), hx(g[0][1]), hx(g[0][2]), hx(g[1][0]), hx(g[1][1]), hx(g[1][2]), hx(ra), hx(rb), hx(n)]
    for vals, nx, ny, nvx, nvy in nbs:
        parts += [hx(vals["o14"]), hx(vals["t0"]), hx(vals["row0"]), hx(f2b(nx)), hx(f2b(ny)), hx(f2b(nvx)), hx(f2b(nvy))]
    parts += [hx(v) for v in out]
    parts += [hx(0 if gat is None else 1), hx(0 if gat is None else gat[2]), hx(0 if gat is None else gat[3]),
              hx(0 if placed is None else 1), hx(0 if placed is None else placed[0]), hx(0 if placed is None else placed[1]),
              hx(0 if st is None else 1)]
    return " ".join(parts)


# ---------------------------------------------------------------------------------------------------------------- WR
FP = HC.DATA + 0xCE00          # the path follower object P
WR_STATE = dict(c380=0, c2e0=0, c2e0_ret=0)
_WR_READY = [False]


def wr_line(rng):
    if not _WR_READY[0]:
        def h_c380(uc, esp):
            WR_STATE["c380"] += 1
            return 0
        HC.hook("wr_c380", 0x6F49C380, 8, h_c380)

        def h_c2e0(uc, esp):
            WR_STATE["c2e0"] += 1
            return WR_STATE["c2e0_ret"]
        HC.hook("wr_c2e0", 0x6F49C2E0, 8, h_c2e0)
        _WR_READY[0] = True
    # position and waypoint (cells); a share of the cases sits on the 0.49 radius
    px = M6.rfloat(rng, -50.0, 50.0, 0.1)
    py = M6.rfloat(rng, -50.0, 50.0, 0.1)
    r = rng.random()
    if r < 0.15:
        wx, wy = px + 0.49, py                     # on the radius along x (a tie when |p| is small enough to be exact)
        if rng.random() < 0.5:
            px = py = 0.0
            wx, wy = 0.49, 0.0
    elif r < 0.5:
        ang = rng.uniform(0.0, 6.2831855)
        d = 0.49 * rng.uniform(0.97, 1.03)
        wx, wy = px + d * math.cos(ang), py + d * math.sin(ang)
    else:
        wx, wy = px + rng.uniform(-3.0, 3.0), py + rng.uniform(-3.0, 3.0)
    idx = rng.choice([0, 0, 1, 2, 5, 17])
    cnt = rng.choice([0, 0, 1, 3, 8])
    wait = rng.choice([0, 0, 3, 19, 20, 21])
    flags = rng.getrandbits(32) & ~0x100000
    if rng.random() < 0.5:
        flags |= 0x100000
    blocked = rng.getrandbits(8)
    wr32(FP + 0x74, idx)
    wr32(FP + 0x78, cnt)
    wr32(FP + 0x88, flags)
    wr32(FP + 0x94, wait)
    wr32(FP + 0x98, blocked)
    WR_STATE["c380"] = 0
    WR_STATE["c2e0"] = 0
    WR_STATE["c2e0_ret"] = rng.randrange(2)
    POS, WP = SCR, SCR + 8
    wr32(POS, f2b(px))
    wr32(POS + 4, f2b(py))
    wr32(WP, f2b(wx))
    wr32(WP + 4, f2b(wy))
    ret = emu.call(0x6F49C3B0, ecx=FP, stack_args=(POS, WP))
    return "WR " + " ".join([hx(f2b(px)), hx(f2b(py)), hx(f2b(wx)), hx(f2b(wy)), hx(idx), hx(cnt), hx(wait), hx(flags),
                             hx(blocked), hx(WR_STATE["c2e0_ret"]), hx(ret), hx(rd32(FP + 0x74)), hx(rd32(FP + 0x88)),
                             hx(rd32(FP + 0x94)), hx(rd32(FP + 0x98)), hx(WR_STATE["c380"]), hx(WR_STATE["c2e0"])])


# ---------------------------------------------------------------------------------------------------------------- GL
GG = HC.DATA + 0xB000          # a group object (this of 0x6F4900A0 / 0x6F490C30)
GREC = HC.DATA + 0xB400        # its 44-byte records (up to 12)
GUNITS = HC.DATA + 0xC400      # unit objects, 0x100 each
GLEAD = HC.DATA + 0xB300       # the leader object (+0x20 limit, +0x24 minimum)
GOUT = HC.DATA + 0xB2F0
FLT_MAX_BITS = 0x7F7FFFFF


def gl_line(rng):
    n = rng.choice([0, 1, 2, 3, 4, 5, 7, 8, 9, 11, 12])
    for off in range(0, 0x200, 4):
        wr32(GG + off, 0)
    wr32(GG + 0x28, GREC)
    wr32(GG + 0x38, n)
    gflags = rng.getrandbits(8) if rng.random() < 0.5 else 0
    mu.mem_write(GG + 0x80, bytes([gflags]))
    leader = rng.random() < 0.5
    wr32(GG + 0x7C, GLEAD if leader else 0)
    lim = FLT_MAX_BITS if rng.random() < 0.5 else f2b(rng.uniform(0.5, 20.0))
    lmin = FLT_MAX_BITS if rng.random() < 0.5 else f2b(rng.uniform(0.5, 20.0))
    wr32(GLEAD + 0x20, lim)
    wr32(GLEAD + 0x24, lmin)
    recs = []
    for k in range(n):
        u = GUNITS + 0x100 * k
        r = rng.random()
        cap = FLT_MAX_BITS if r < 0.1 else (0 if r < 0.15 else f2b(rng.uniform(0.2, 22.0)))
        wr32(u + 0x88, cap)
        uflag = rng.getrandbits(8)
        mu.mem_write(u + 0xDB, bytes([uflag]))
        rflags = rng.getrandbits(32) & ~0x210000
        if rng.random() < 0.25:
            rflags |= 0x200000
        if rng.random() < 0.1:
            rflags |= 0x010000
        rec = GREC + 44 * k
        wr32(rec + 0x14, u)
        wr32(rec + 0x28, rflags)
        recs.append((cap, rflags, uflag))
    wr32(GOUT, 0)
    emu.call(0x6F4900A0, ecx=GG, stack_args=(GOUT,))
    limit = rd32(GOUT)
    lead_min = rd32(GLEAD + 0x24)
    gate = emu.call(0x6F490C30, ecx=GG) & 0xFF
    parts = ["GL", hx(n), hx(gflags), hx(1 if leader else 0), hx(lim), hx(lmin)]
    for cap, rflags, uflag in recs:
        parts += [hx(cap), hx(rflags), hx(uflag)]
    parts += [hx(limit), hx(lead_min), hx(gate)]
    return " ".join(parts)


def wn_line(rng):
    # the whole walk of movement6_emu (real 0x6F4907F0 + 0x6F493010) with the real velocity notification
    return "WN" + M6.wk_line(rng)[2:]


KINDS = dict(SN=sn_line, FS=fs_line, PA=pa_line, WN=wn_line, WR=wr_line, GL=gl_line)


def main():
    kind = sys.argv[1]
    out = sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 1000
    seed = int(sys.argv[4]) if len(sys.argv) > 4 else 1
    rng = random.Random(seed)
    setup()
    with open(out, "w") as fh:
        for _ in range(n):
            fh.write(KINDS[kind](rng) + "\n")
    print("wrote %d %s vectors to %s" % (n, kind, out))


if __name__ == "__main__":
    main()
