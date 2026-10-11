"""Unicorn vectors for the unit mover kernels of Game.dll 1.26a (docs/spec/movement-6.md).

Real code of Game.dll runs on a synthetic unit/mover object (the unit of hero_normaliser_emu, `U` of movement-5):

  VS  velocity set      0x6F493A40 -> 0x6F4A73B0 -> 0x6F4A6FD0 (rebase) + 0x6F4A70D0 (add, clamp) + 0x6F07B870
        in : U+0x78/7C cells, U+0x80/84 V, U+0x88 cap, U+0x70/74 snapshot, U+0x14, layer A/B (T,row,scale), target vec
        out: U+0x78/7C, U+0x80/84, U+0x70/74, flag word U.98+0x40
  CS  cap set          0x6F4A73F0 (same objects; zeroes V and rebases when cap < |V|)
  TA  turn/accel       0x6F493FA0 (U+0xB4/B8/BC, R.d, R.heading, delta, flag)
  AD  angle delta      0x6F493D90 (current heading, vector) -> signed wrapped angle
  HD  heading of V     0x6F4B2DE0
  SP  speed sink       0x6F4747B0 arithmetic (min with mgr+0x80, divide by 32) via 0x6F4A73F0

Only the lookup 0x6F03FA30 and the position-changed notification are answered by the harness. Vectors are text lines of
hex bit patterns; no Game.dll bytes are written into them.

Run:  GAME_DLL=C:\\path\\Game.dll python tools/emu/movement6_emu.py KIND OUT.txt [N] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
import hero_candidate_emu as HC  # noqa: E402
import hero_normaliser_emu as HN  # noqa: E402
import hero_roundtrip_emu as RT  # noqa: E402

mu = emu.mu
OBJ = HC.OBJ
MGR = HN.MGR
rd32, wr32, f2b, b2f, hx = HN.rd32, HN.wr32, HN.f2b, HN.b2f, HN.hx
S_STAT = RT.S_STAT
SCR = HC.DATA + 0xF400
FLT_MAX = 0x7F7FFFFF


def f(bits):
    return b2f(bits)


def rfloat(rng, lo, hi, zero_p=0.0, neg_zero_p=0.0):
    r = rng.random()
    if r < zero_p:
        return 0.0
    if r < zero_p + neg_zero_p:
        return -0.0
    return rng.uniform(lo, hi)


def grids(rng):
    out = []
    for base in (MGR + 0x14, MGR + 0x68):
        t = rfloat(rng, 0.0, 299.0)
        row = rng.randrange(0, 6)
        scale = 300.0 if rng.random() < 0.8 else rng.uniform(1.0, 400.0)
        HN.wrf(base + 0x40, t)
        wr32(base + 0x44, row)
        HN.wrf(base + 0x48, scale)
        out.append((f2b(t), row, f2b(scale)))
    return out


def unit_state(rng, g, moving=True):
    """Random unit: cells, V, cap, snapshot against grid A (or B per the bit)."""
    o14 = rng.getrandbits(32) & 0x7FFFFFFF
    if rng.random() < 0.25:
        o14 |= 0x80000000
    grid = g[1] if o14 & 0x80000000 else g[0]
    # snapshot a little behind the grid clock (the usual case), sometimes far behind or equal
    r = rng.random()
    t_g = f(grid[0])
    if r < 0.12:
        t0 = t_g
        row0 = grid[1]
    elif r < 0.75:
        t0 = max(0.0, t_g - rng.uniform(0.0, 0.2))
        row0 = grid[1]
    elif r < 0.9:
        t0 = rng.uniform(0.0, 299.0)
        row0 = max(0, grid[1] - rng.randrange(0, 3))
    else:
        t0 = t_g + rng.uniform(-3e-4, 3e-4)
        row0 = grid[1]
    cx = rfloat(rng, -200.0, 200.0, 0.03, 0.02)
    cy = rfloat(rng, -200.0, 200.0, 0.03, 0.02)
    vx = rfloat(rng, -20.0, 20.0, 0.1, 0.03) if moving else 0.0
    vy = rfloat(rng, -20.0, 20.0, 0.1, 0.03) if moving else 0.0
    if rng.random() < 0.04:
        cap = FLT_MAX
    else:
        cap = f2b(rfloat(rng, 0.0, 22.0, 0.04))
    wr32(OBJ + 0x14, o14)
    HN.wrf(OBJ + 0x70, t0)
    wr32(OBJ + 0x74, row0)
    HN.wrf(OBJ + 0x78, cx)
    HN.wrf(OBJ + 0x7C, cy)
    HN.wrf(OBJ + 0x80, vx)
    HN.wrf(OBJ + 0x84, vy)
    wr32(OBJ + 0x88, cap if isinstance(cap, int) else f2b(cap))
    wr32(S_STAT + 0x40, rng.getrandbits(32) & 0xDFFFFFFF)
    return dict(o14=o14, t0=f2b(t0), row0=row0, cx=f2b(cx), cy=f2b(cy), vx=f2b(vx), vy=f2b(vy),
                cap=(cap if isinstance(cap, int) else f2b(cap)), flags=rd32(S_STAT + 0x40))


def snap_unit():
    return [rd32(OBJ + o) for o in (0x70, 0x74, 0x78, 0x7C, 0x80, 0x84, 0x88)] + [rd32(S_STAT + 0x40)]


CLAMP_HITS = [0]


def _clamp_hook(uc, address, size, user):
    CLAMP_HITS[0] += 1


def setup():
    from unicorn import UC_HOOK_CODE
    mu.hook_add(UC_HOOK_CODE, _clamp_hook, begin=0x6F07B870, end=0x6F07B870)
    RT.setup_harness()
    # vtable +0x50: the velocity-changed notification of 0x6F4A70D0 (thiscall, no stack arguments); recorded only
    NOTIFY50 = HC.STUB + 0x240
    HC.hook("notify50", NOTIFY50, 0, lambda uc, esp: None)
    wr32(HC.VT + 0x50, NOTIFY50)
    wr32(HN.MGR_PTR, MGR)
    wr32(0x6FAAE478, 0x38D1B717)


def vs_line(rng):
    g = grids(rng)
    u = unit_state(rng, g)
    vec = (f2b(rfloat(rng, -22.0, 22.0, 0.1, 0.02)), f2b(rfloat(rng, -22.0, 22.0, 0.1, 0.02)))
    if rng.random() < 0.1:
        vec = (f2b(rfloat(rng, -1e-6, 1e-6)), f2b(rfloat(rng, -1e-6, 1e-6)))
    wr32(SCR, vec[0])
    wr32(SCR + 4, vec[1])
    CLAMP_HITS[0] = 0
    emu.call(0x6F493A40, ecx=OBJ, stack_args=(SCR, 0))
    out = snap_unit()
    return "VS " + " ".join([hx(u["o14"]), hx(u["t0"]), hx(u["row0"]), hx(u["cx"]), hx(u["cy"]), hx(u["vx"]), hx(u["vy"]),
                             hx(u["cap"]), hx(u["flags"]), hx(g[0][0]), hx(g[0][1]), hx(g[0][2]), hx(g[1][0]), hx(g[1][1]),
                             hx(g[1][2]), hx(vec[0]), hx(vec[1])] + [hx(v) for v in out] + [hx(CLAMP_HITS[0])])


def cs_line(rng):
    g = grids(rng)
    u = unit_state(rng, g)
    cap = f2b(rfloat(rng, 0.0, 22.0, 0.05))
    wr32(SCR, cap)
    CLAMP_HITS[0] = 0
    emu.call(0x6F4A73F0, ecx=OBJ, stack_args=(SCR,))
    out = snap_unit()
    return "CS " + " ".join([hx(u["o14"]), hx(u["t0"]), hx(u["row0"]), hx(u["cx"]), hx(u["cy"]), hx(u["vx"]), hx(u["vy"]),
                             hx(u["cap"]), hx(u["flags"]), hx(g[0][0]), hx(g[0][1]), hx(g[0][2]), hx(g[1][0]), hx(g[1][1]),
                             hx(g[1][2]), hx(cap)] + [hx(v) for v in out] + [hx(CLAMP_HITS[0])])


def ta_line(rng):
    b4 = f2b(rfloat(rng, 0.0, 22.0, 0.05))
    b8 = f2b(rfloat(rng, 0.001, 3.5))
    bc = f2b(rfloat(rng, 0.0, 3.5, 0.05))
    wr32(OBJ + 0xB4, b4)
    wr32(OBJ + 0xB8, b8)
    wr32(OBJ + 0xBC, bc)
    rd_ = f2b(rfloat(rng, 0.0, 22.0, 0.1))
    rh = f2b(rfloat(rng, 0.0, 6.2831855, 0.05))
    d = f2b(rfloat(rng, -3.1415927, 3.1415927, 0.1, 0.05))
    flag = rng.randrange(2)
    RD, RH, DL = SCR, SCR + 4, SCR + 8
    wr32(RD, rd_)
    wr32(RH, rh)
    wr32(DL, d)
    emu.call(0x6F493FA0, ecx=OBJ, stack_args=(RD, RH, DL, flag))
    return "TA " + " ".join([hx(b4), hx(b8), hx(bc), hx(rd_), hx(rh), hx(d), str(flag), hx(rd32(RD)), hx(rd32(RH))])


def ad_line(rng):
    vx = rfloat(rng, -50.0, 50.0, 0.1, 0.02)
    vy = rfloat(rng, -50.0, 50.0, 0.1, 0.02)
    if rng.random() < 0.05:
        vx, vy = rfloat(rng, -1e-5, 1e-5), rfloat(rng, -1e-5, 1e-5)
    cur = f2b(rfloat(rng, 0.0, 6.2831855, 0.05))
    V, HP, OUTP = SCR, SCR + 0x10, SCR + 0x20
    wr32(V, f2b(vx))
    wr32(V + 4, f2b(vy))
    wr32(HP, cur)
    wr32(OUTP, 0xDEADBEEF)
    from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESI
    mu.reg_write(UC_X86_REG_EAX, OUTP)
    mu.reg_write(UC_X86_REG_ESI, V)
    emu.call(0x6F493D90, stack_args=(HP,))
    return "AD " + " ".join([hx(cur), hx(f2b(vx)), hx(f2b(vy)), hx(rd32(OUTP))])


def hd_line(rng):
    vx = rfloat(rng, -50.0, 50.0, 0.1, 0.02)
    vy = rfloat(rng, -50.0, 50.0, 0.1, 0.02)
    if rng.random() < 0.05:
        vx, vy = rfloat(rng, -1e-5, 1e-5), rfloat(rng, -1e-5, 1e-5)
    V, R, OUTP = SCR, SCR + 0x10, SCR + 0x20
    # the caller passes r = |V| as computed by the walk (R+0x20); any positive value exercises the divide
    wr32(V, f2b(vx))
    wr32(V + 4, f2b(vy))
    r = f2b(rfloat(rng, 0.0, 50.0, 0.05))
    wr32(R, r)
    wr32(OUTP, 0xDEADBEEF)
    emu.call(0x6F4B2DE0, ecx=V, stack_args=(OUTP, R))
    return "HD " + " ".join([hx(f2b(vx)), hx(f2b(vy)), hx(r), hx(rd32(OUTP))])


def sv_line(rng):
    g = grids(rng)
    u = unit_state(rng, g)
    facing = rfloat(rng, 0.0, 6.2831855, 0.05)
    wr32(OBJ + 0x8C, f2b(facing))
    wr32(OBJ + 0xC0, 0)
    wr32(OBJ + 0xC4, 0)
    r = rng.random()
    if r < 0.1:
        sp = 0.0
    elif r < 0.17:
        sp = -rng.uniform(0.0, 3.0)
    else:
        sp = rfloat(rng, 0.0, 22.0)
    hd = rfloat(rng, 0.0, 9.0, 0.03) if rng.random() < 0.8 else rfloat(rng, -3.0, 40.0)
    SP, HP = SCR, SCR + 4
    wr32(SP, f2b(sp))
    wr32(HP, f2b(hd))
    CLAMP_HITS[0] = 0
    emu.call(0x6F494160, ecx=OBJ, stack_args=(SP, HP))
    out = snap_unit() + [rd32(OBJ + 0x8C), rd32(OBJ + 0xC0), rd32(OBJ + 0xC4)]
    return "SV " + " ".join([hx(u["o14"]), hx(u["t0"]), hx(u["row0"]), hx(u["cx"]), hx(u["cy"]), hx(u["vx"]), hx(u["vy"]),
                             hx(u["cap"]), hx(u["flags"]), hx(g[0][0]), hx(g[0][1]), hx(g[0][2]), hx(g[1][0]), hx(g[1][1]),
                             hx(g[1][2]), hx(f2b(facing)), hx(f2b(sp)), hx(f2b(hd))] + [hx(v) for v in out] + [hx(CLAMP_HITS[0])])


def na_line(rng):
    r = rng.random()
    if r < 0.5:
        x = rfloat(rng, -7.0, 14.0, 0.03)
    elif r < 0.9:
        x = rfloat(rng, -60.0, 60.0)
    else:
        x = rfloat(rng, -1000.0, 1000.0)
    XP, OUTP = SCR, SCR + 0x10
    wr32(XP, f2b(x))
    wr32(OUTP, 0xDEADBEEF)
    emu.call(0x6F493910, ecx=OUTP, edx=XP)
    return "NA " + " ".join([hx(f2b(x)), hx(rd32(OUTP))])


WG = HC.DATA + 0xB000      # the group object (this of 0x6F4907F0 / 0x6F493010)
WR = HC.DATA + 0xB200      # the record R (44 bytes), owner U = OBJ
WP = HC.DATA + 0xB300      # the path follower P = U+0xA8
PAIR = HC.DATA + 0xB600
WK_STATE = dict(code=0, wp=(0, 0))
A8_TOKEN = 0x12345678


def _install_walk_hooks():
    def h_follow(uc, esp):
        # 0x6F49E020 (this = P; stack: pos*, &target copy, U): the path follower. 0 = open; the copy becomes the waypoint.
        if WK_STATE["code"] == 0:
            ptr = rd32(esp + 8)
            wr32(ptr, WK_STATE["wp"][0])
            wr32(ptr + 4, WK_STATE["wp"][1])
        return WK_STATE["code"]
    HC.hook("follow", 0x6F49E020, 0xC, h_follow)

    def h_valid(uc, esp):
        wr32(rd32(esp + 12), 0)       # *v23 = 0
        return 0
    HC.hook("pathvalid", 0x6F49B470, 0xC, h_valid)
    HC.hook("pathrelease", 0x6F49B5A0, 0, lambda uc, esp: None)


_WALK_READY = [False]


def wk_line(rng):
    if not _WALK_READY[0]:
        _install_walk_hooks()
        _WALK_READY[0] = True
    g = grids(rng)
    moving = rng.random() < 0.7
    u = unit_state(rng, g, moving=moving)
    # a realistic cap/velocity relation most of the time
    cap = rfloat(rng, 0.5, 17.0)
    if rng.random() < 0.05:
        cap = 0.0
    wr32(OBJ + 0x88, f2b(cap))
    if moving and rng.random() < 0.7:
        ang = rng.uniform(0.0, 6.2831855)
        sp = rng.uniform(0.0, cap)
        import math
        HN.wrf(OBJ + 0x80, sp * math.cos(ang))
        HN.wrf(OBJ + 0x84, sp * math.sin(ang))
    facing = rfloat(rng, 0.0, 6.2831855, 0.05)
    wr32(OBJ + 0x8C, f2b(facing))
    b0 = 0.49 if rng.random() < 0.7 else rng.uniform(0.49, 3.0)
    b4 = cap if rng.random() < 0.7 else rng.uniform(0.0, 20.0)
    b8 = rng.uniform(0.001, 1.2) if rng.random() < 0.9 else rng.uniform(1.2, 3.4)
    bc = rng.uniform(0.2, 3.2) if rng.random() < 0.9 else 0.0
    for off, val in ((0xB0, b0), (0xB4, b4), (0xB8, b8), (0xBC, bc)):
        HN.wrf(OBJ + off, val)
    wr32(OBJ + 0x04, 1)
    wr32(OBJ + 0xA8, WP)
    wr32(WP + 0xA4, A8_TOKEN)
    wr32(OBJ + 0xD8, 0)
    mu.mem_write(OBJ + 0xDA, bytes([0]))
    mu.mem_write(OBJ + 0xDB, bytes([0]))
    mu.mem_write(WP + 0x8B, bytes([0]))
    # group and record
    for off in range(0, 0x200, 4):
        wr32(WG + off, 0)
    wr32(WG + 0x28, WR)
    wr32(WG + 0x38, 1)
    for off in range(0, 0x30, 4):
        wr32(WR + off, 0)
    wr32(WR + 0x14, OBJ)
    # the goal and the waypoint, relative to the unit's evaluated position (cells)
    pos = (b2f(u["cx"]), b2f(u["cy"]))
    far = rng.random() < 0.6
    span = 25.0 if far else 1.5
    goal = (pos[0] + rng.uniform(-span, span), pos[1] + rng.uniform(-span, span))
    if rng.random() < 0.2:
        goal = (pos[0] + rng.uniform(-0.6, 0.6), pos[1] + rng.uniform(-0.6, 0.6))   # inside the arrival radius
    if rng.random() < 0.55:
        # a unit already heading for the goal: facing and velocity point at it (some noise)
        import math as _m
        want = _m.atan2(goal[1] - pos[1], goal[0] - pos[0]) % (2 * _m.pi)
        facing_n = (want + rng.uniform(-0.3, 0.3)) % (2 * _m.pi)
        wr32(OBJ + 0x8C, f2b(facing_n))
        if moving:
            sp_n = rng.uniform(0.0, max(cap, 0.0))
            HN.wrf(OBJ + 0x80, sp_n * _m.cos(facing_n))
            HN.wrf(OBJ + 0x84, sp_n * _m.sin(facing_n))
    HN.wrf(WR + 0x18, goal[0])
    HN.wrf(WR + 0x1C, goal[1])
    code = 0 if rng.random() < 0.85 else rng.choice([1, 3, 4, 0x100000])
    if rng.random() < 0.7:
        wp = goal
    else:
        wp = (pos[0] + rng.uniform(-span, span), pos[1] + rng.uniform(-span, span))
    WK_STATE["code"] = code
    WK_STATE["wp"] = (f2b(wp[0]), f2b(wp[1]))
    pre = snap_unit() + [rd32(OBJ + 0x8C), rd32(OBJ + 0xB0), rd32(OBJ + 0xB4), rd32(OBJ + 0xB8), rd32(OBJ + 0xBC),
                         rd32(WR + 0x18), rd32(WR + 0x1C), WK_STATE["wp"][0], WK_STATE["wp"][1], code]
    emu.call(0x6F4907F0, ecx=WG, stack_args=(WR, A8_TOKEN))
    rd_, rh_ = rd32(WR + 0x20), rd32(WR + 0x24)
    emu.call(0x6F493010, ecx=WG, stack_args=(PAIR,))
    out = snap_unit() + [rd32(OBJ + 0x8C), rd32(OBJ + 0xC0), rd32(OBJ + 0xC4)]
    return "WK " + " ".join([hx(rd32(OBJ + 0x14)), hx(g[0][0]), hx(g[0][1]), hx(g[0][2]), hx(g[1][0]), hx(g[1][1]), hx(g[1][2])]
                            + [hx(v) for v in pre] + [hx(rd_), hx(rh_)] + [hx(v) for v in out])


def wp_line(rng):
    r = rng.random()
    x = rfloat(rng, -8.0, 14.0, 0.03) if r < 0.6 else rfloat(rng, -200.0, 200.0)
    XP, OUTP = SCR, SCR + 0x10
    wr32(XP, f2b(x))
    wr32(OUTP, 0xDEADBEEF)
    emu.call(0x6F47C420, ecx=OUTP, edx=XP)
    return "WP " + " ".join([hx(f2b(x)), hx(rd32(OUTP))])


def sk_line(rng):
    # Position::vf6 (0x6F4747B0): min(speed, mgr+0x80) / 32 into the cap, through 0x6F4A73F0
    g = grids(rng)
    u = unit_state(rng, g)
    sp = rfloat(rng, 0.0, 800.0, 0.03) if rng.random() < 0.9 else rfloat(rng, -50.0, 5000.0)
    wr32(HC.WORLD + 0x80, 0x4402AAAB)
    wr32(SCR, f2b(sp))
    CLAMP_HITS[0] = 0
    emu.call(0x6F4747B0, ecx=HC.POSREF, stack_args=(SCR,))
    out = snap_unit()
    return "SK " + " ".join([hx(u["o14"]), hx(u["t0"]), hx(u["row0"]), hx(u["cx"]), hx(u["cy"]), hx(u["vx"]), hx(u["vy"]),
                             hx(u["cap"]), hx(u["flags"]), hx(g[0][0]), hx(g[0][1]), hx(g[0][2]), hx(g[1][0]), hx(g[1][1]),
                             hx(g[1][2]), hx(f2b(sp))] + [hx(v) for v in out] + [hx(CLAMP_HITS[0])])


def st_line(rng):
    # the cell setter 0x6F474250 (SetUnitX/SetUnitPosition) on a unit in motion: delta = P - F against the evaluated
    # position, re-based with the snapshot refresh, velocity unchanged
    g = grids(rng)
    u = unit_state(rng, g)
    w6c = rfloat(rng, -4096.0, 4096.0, 0.05)
    w70 = rfloat(rng, -4096.0, 4096.0, 0.05)
    HN.wrf(HC.WORLD + 0x6C, w6c)
    HN.wrf(HC.WORLD + 0x70, w70)
    x = rfloat(rng, -4096.0, 4096.0, 0.03)
    y = rfloat(rng, -4096.0, 4096.0, 0.03)
    wr32(SCR, f2b(x))
    wr32(SCR + 4, f2b(y))
    emu.call(0x6F474250, ecx=HC.POSREF, stack_args=(SCR, 1))
    out = snap_unit()
    return "ST " + " ".join([hx(u["o14"]), hx(u["t0"]), hx(u["row0"]), hx(u["cx"]), hx(u["cy"]), hx(u["vx"]), hx(u["vy"]),
                             hx(u["cap"]), hx(u["flags"]), hx(g[0][0]), hx(g[0][1]), hx(g[0][2]), hx(g[1][0]), hx(g[1][1]),
                             hx(g[1][2]), hx(f2b(w6c)), hx(f2b(w70)), hx(f2b(x)), hx(f2b(y))] + [hx(v) for v in out])


KINDS = dict(ST=st_line, WP=wp_line, SK=sk_line, WK=wk_line, SV=sv_line, NA=na_line, VS=vs_line, CS=cs_line, TA=ta_line, AD=ad_line, HD=hd_line)


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
