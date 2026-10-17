"""Unicorn vectors for the path follower's walk step (docs/spec/movement-8.md).

Real code under test: the follower walk step 0x6F49DF00 with the waypoint fetch 0x6F49C930, the reach test 0x6F49C3B0,
the straight-line check 0x6F49D8D0 (cell traces 0x6F4A2070 / 0x6F4A2180 / 0x6F4A2360 / 0x6F4A2550 over 0x6F4A1EA0), the
blocker scan 0x6F49D770, the blocked branch 0x6F49DE90 (0x6F49C460, 0x6F49DBE0) and the detour interval 0x6F49B340 with
its draw 0x6F4BBFE0. The context setter 0x6F49B8E0 runs first, as in 0x6F49E020.

Cases are generated at random (grid with pathing bits, occupant chains with the three entry kinds, nodes with stamps,
flags and owners, movers with velocities and handle pairs, followers with wait/retry/blocker state) and written as one
hex line each: the Rust test (crates/sim/tests/follow_vectors.rs) rebuilds the same world and compares every output.

Line (hex tokens, space separated; counts first):
  W <class_radius_bits> <kind> <A> <B> <w> <h> <counter> <ncells> cells.. <npool> (w0 node)* <nnodes> (flags stamp w40 owner)*
    <nmov> (tag id serial vx vy unit follower_blocker_valid_flag ...)  -- see write_case()

Run:  GAME_DLL=C:\\path\\Game.dll py tools/emu/follow_emu.py OUT.txt [N] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX  # noqa: E402

mu = emu.mu
H = 0x00900000
mu.mem_map(H, 0x200000)
SCRATCH = 0x00E00000
mu.mem_map(SCRATCH, 0x1000)

W = H + 0x0000          # world: RNG state at +0, +0x24C -> C, +0x538 walk counter
C = H + 0x1000          # pathing request context (G750)
GD = H + 0x2000         # grid
TBL = H + 0x3000        # handle table (0x6FAB7788)
CELLS = H + 0x10000
POOL = H + 0x20000
NODES = H + 0x30000     # 0x100 each
MOVS = H + 0x50000      # 0x100 each
FOLS = H + 0x70000      # 0x200 each
UNITS = H + 0x90000     # 0x100 each
ENT1 = H + 0xA0000      # handle entries (8 bytes)
WPS = H + 0xB0000       # waypoint array
LISTD = H + 0xC0000     # trace list data
TAG = 0x60706375
NIL = 0xFFFFFF


def rd32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def f2b(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def b2f(v):
    return struct.unpack("<f", struct.pack("<I", v & 0xFFFFFFFF))[0]


def hx(v):
    return "%X" % (v & 0xFFFFFFFF)


def patch_fs(start, end):
    z = b"\x00\x00\x00\x00"
    s = struct.pack("<I", SCRATCH)
    pairs = [
        (b"\x64\xA1" + z, b"\xA1" + s + b"\x90"),
        (b"\x64\xA3" + z, b"\xA3" + s + b"\x90"),
        (b"\x64\x8B\x05" + z, b"\x8B\x05" + s + b"\x90"),
        (b"\x64\x89\x05" + z, b"\x89\x05" + s + b"\x90"),
        (b"\x64\x8B\x0D" + z, b"\x8B\x0D" + s + b"\x90"),
        (b"\x64\x89\x0D" + z, b"\x89\x0D" + s + b"\x90"),
    ]
    code = bytes(mu.mem_read(start, end - start))
    for pat, repl in pairs:
        i = 0
        while True:
            i = code.find(pat, i)
            if i < 0:
                break
            mu.mem_write(start + i, repl)
            i += len(pat)
        code = bytes(mu.mem_read(start, end - start))


patch_fs(0x6F49B000, 0x6F49E200)
patch_fs(0x6F4A1E00, 0x6F4A2800)

# mover / follower / unit / table layouts -------------------------------------------------------------------------
# mover U: +0x10 tag, +0x14 id, +0x18 serial, +0x80 vx, +0x84 vy, +0x9C/+0xA0 unit handle (id, serial), +0xA8 -> P
# follower P: +0x24/+0x28 reissue point, +0x34.. waypoint container (+0x40 data, +0x50 count), +0x74 index, +0x88 flags,
#   +0x94 wait, +0x98 retry, +0x9C mask, +0xA0 token (0), +0xA4 request token (0), +0xA8/+0xAC blocker handle, +0xB4 radius
# unit O: +0x18 serial, +0x38 stamp, +0x80 flags
# handle table T: +0x0C -> entries, +0x1C count (entry: state -2 = used, object pointer)


def mover_addr(i):
    return MOVS + 0x100 * i


def fol_addr(i):
    return FOLS + 0x200 * i


def unit_addr(j):
    return UNITS + 0x100 * j


def gen_case(rng):
    nm = rng.randint(1, 5)
    nu = nm  # one unit per mover (the movers may share units: unit index below)
    w = rng.randint(6, 14)
    h = rng.randint(6, 14)
    cells = []
    npool = 0
    pool = []
    nnodes = rng.randint(0, 6)
    nodes = []
    for _ in range(nnodes):
        flags = rng.choice([0x1000000 | rng.getrandbits(24), 0x1000000 | 0xFFFFFF, rng.getrandbits(32), 0x1000000 | 0x1])
        stamp = rng.choice([0, 1, 2, 5, 0xFFFFFFFF, rng.getrandbits(8)])
        w40 = rng.choice([0, 0, 0, 0, 1, 0x80000000, 0x10, 0x8FFFFFFF])
        owner = rng.choice([-1] + list(range(nm)) + list(range(nm)))
        nodes.append((flags, stamp, w40, owner))
    counter = rng.choice([0, 1, 2, 5, 100, 0x7FFFFFFE])
    for _ in range(w * h):
        top = rng.choice([0] * 24 + [0x02, 0x08, 0x20, 0xFF, 0x04, 0x10])
        cells.append([top, NIL])
    # chains: random cells get 1..3 entries
    for idx in range(w * h):
        if nnodes and rng.random() < 0.25:
            ln = rng.randint(1, 3)
            heads = []
            for _ in range(ln):
                kind = rng.choice([0x00, 0x01, 0x01, 0x01, 0x02, 0x03])
                node = rng.randrange(nnodes)
                pool.append([kind, node, None])
                heads.append(len(pool) - 1)
            for a, b in zip(heads, heads[1:] + [None]):
                pool[a][2] = NIL if b is None else b
            cells[idx][1] = heads[0]
    movers = []
    for i in range(nm):
        movers.append(dict(
            tag=rng.choice([TAG] * 9 + [0x12345678]),
            vx=rng.choice([0.0, 0.0, f2b(rng.uniform(-6, 6)), f2b(rng.uniform(-0.5, 0.5)), 0x80000000, 0x7FC00000]),
            vy=rng.choice([0.0, 0.0, f2b(rng.uniform(-6, 6)), f2b(rng.uniform(-0.5, 0.5)), 0x80000000]),
            unit=rng.randrange(nu), serial=rng.randint(1, 1000), valid=rng.random() < 0.9,
            id=i,
        ))
    # velocities as bits
    for m in movers:
        for k in ("vx", "vy"):
            if isinstance(m[k], float):
                m[k] = f2b(m[k])
    units = []
    for j in range(nu):
        units.append(dict(serial=rng.randint(1, 1000), flags=rng.choice([0, 8, 0x80, 0x88]), stamp=rng.choice([0, 1, 2, 7, 1000]),
                          valid=rng.random() < 0.95))
    fols = []
    for i in range(nm):
        fols.append(dict(
            flags=rng.choice([0, 0x00010000 | (rng.getrandbits(4) << 16), rng.getrandbits(32) & 0x00FF0000]),
            wait=rng.choice([0, 0, 0, 0, 3]),
            retry=rng.choice([0, 0, 0, 1, 5]),
            blk_mover=rng.choice([-1, -1] + list(range(nm))),
            blk_bad=rng.random() < 0.3,
            mask=rng.choice([0xFF000000 | 0x1, 0x02000001, 0x0A000003, rng.getrandbits(32), 0xFFFFFFFF]),
            radius=f2b(rng.choice([0.2, 0.4, 0.7, 0.99, 1.2, 1.4, 2.0, 3.5])),
            p24=(f2b(rng.uniform(0, w)), f2b(rng.uniform(0, h))),
            own=(rng.randrange(nnodes) if nnodes and rng.random() < 0.7 else -1),
        ))
    if rng.random() < 0.4:
        # crowded style: moving, valid movers with equal nibbles, units standing in the cells
        for m in movers:
            m["tag"] = TAG
            m["valid"] = True
            m["vx"] = f2b(rng.uniform(-4, 4))
            m["vy"] = f2b(rng.uniform(-4, 4))
        for u in units:
            u["valid"] = True
            u["flags"] = rng.choice([0, 8])
        for f in fols:
            f["blk_mover"] = -1
            f["wait"] = 0
            f["flags"] = 0
            f["mask"] = rng.choice([0x0A000003, 0x02000001, 0xFF000001])
        for k, nd in enumerate(nodes):
            nodes[k] = (0x1000000 | rng.choice([1, 3, 0xFFFFFF]), rng.choice([0, 1, 2]), 0, nd[3])
        for p in pool:
            p[0] = 0x01 if rng.random() < 0.8 else p[0]
    me = 0
    units[movers[me]["unit"]]["valid"] = True  # the original dereferences the traced mover's unit unchecked
    movers[me]["valid"] = True
    nwp = rng.randint(1, 4)
    wps = []
    for _ in range(nwp):
        wps.append((f2b(rng.uniform(-2, w + 2)), f2b(rng.uniform(-2, h + 2))))
    idx = rng.randrange(nwp)
    # position: usually within a few cells of the waypoint so the trace step is a real step
    wx, wy = b2f(wps[idx][0]), b2f(wps[idx][1])
    mode = rng.random()
    if mode < 0.6:
        pos = (wx + rng.uniform(-3, 3), wy + rng.uniform(-3, 3))
    else:
        pos = (rng.uniform(-1, w + 1), rng.uniform(-1, h + 1))
    posb = (f2b(pos[0]), f2b(pos[1]))
    return dict(w=w, h=h, cells=cells, pool=pool, nodes=nodes, counter=counter, movers=movers, units=units, fols=fols,
                me=me, wps=wps, idx=idx, pos=posb, rngstate=(rng.getrandbits(32), rng.getrandbits(32)))


def setup_case(c):
    # clear touched areas
    mu.mem_write(H, b"\x00" * 0x1800)
    mu.mem_write(GD, b"\x00" * 0x100)
    nm = len(c["movers"])
    nu = len(c["units"])
    wr32(0x6FAB73D8, W)
    wr32(W, c["rngstate"][0])
    wr32(W + 4, c["rngstate"][1])
    wr32(W + 0x24C, C)
    wr32(W + 0x538, 4000)
    wr32(C + 0x1C, GD)
    # list object at C+0xAC: data +0x0C (C+0xB8), grow +0x14 (C+0xC0), cap +0x18 (C+0xC4), count +0x1C (C+0xC8)
    wr32(C + 0xB8, LISTD)
    wr32(C + 0xC0, 16)
    wr32(C + 0xC4, 64)
    wr32(C + 0xC8, 0)
    wr32(GD + 0x28, CELLS)
    wr32(GD + 0x3C, c["w"])
    wr32(GD + 0x40, c["h"])
    wr32(GD + 0x78, POOL)
    wr32(GD + 0xB4, c["counter"])
    for i, (top, head) in enumerate(c["cells"]):
        wr32(CELLS + 4 * i, (top << 24) | head)
    for i, (kind, node, nxt) in enumerate(c["pool"]):
        wr32(POOL + 8 * i, (kind << 24) | nxt)
        wr32(POOL + 8 * i + 4, NODES + 0x100 * node)
    for i, (flags, stamp, w40, owner) in enumerate(c["nodes"]):
        a = NODES + 0x100 * i
        wr32(a + 0x30, 0 if owner < 0 else mover_addr(owner))
        wr32(a + 0x34, flags)
        wr32(a + 0x38, stamp)
        wr32(a + 0x40, w40)
    # handle table: mover ids 0..nm-1, unit ids nm..nm+nu-1
    wr32(0x6FAB7788, TBL)
    wr32(TBL + 0x0C, ENT1)
    wr32(TBL + 0x1C, nm + nu)
    for i, m in enumerate(c["movers"]):
        a = mover_addr(i)
        wr32(a + 0x10, m["tag"])
        wr32(a + 0x14, i)
        wr32(a + 0x18, m["serial"])
        wr32(a + 0x80, m["vx"])
        wr32(a + 0x84, m["vy"])
        wr32(a + 0x9C, nm + m["unit"])
        wr32(a + 0xA0, c["units"][m["unit"]]["serial"])
        wr32(a + 0xA8, fol_addr(i))
        wr32(ENT1 + 8 * i, 0xFFFFFFFE if m["valid"] else 0)
        wr32(ENT1 + 8 * i + 4, a)
    for j, u in enumerate(c["units"]):
        a = unit_addr(j)
        wr32(a + 0x18, u["serial"])
        wr32(a + 0x38, u["stamp"])
        wr32(a + 0x80, u["flags"])
        wr32(ENT1 + 8 * (nm + j), 0xFFFFFFFE if u["valid"] else 0)
        wr32(ENT1 + 8 * (nm + j) + 4, a)
    for i, f in enumerate(c["fols"]):
        a = fol_addr(i)
        wr32(a + 0x24, f["p24"][0])
        wr32(a + 0x28, f["p24"][1])
        wr32(a + 0x40, WPS)
        wr32(a + 0x50, len(c["wps"]))
        wr32(a + 0x74, c["idx"])
        wr32(a + 0x88, f["flags"])
        wr32(a + 0x94, f["wait"])
        wr32(a + 0x98, f["retry"])
        wr32(a + 0x9C, f["mask"])
        wr32(a + 0xB4, f["radius"])
        wr32(a + 0xA0, 0 if f["own"] < 0 else NODES + 0x100 * f["own"])
        b = f["blk_mover"]
        if b < 0:
            wr32(a + 0xA8, 0xFFFFFFFF)
            wr32(a + 0xAC, 0xFFFFFFFF)
        else:
            wr32(a + 0xA8, b)
            wr32(a + 0xAC, c["movers"][b]["serial"] + (1 if f["blk_bad"] else 0))
    for k, (x, y) in enumerate(c["wps"]):
        wr32(WPS + 8 * k, x)
        wr32(WPS + 8 * k + 4, y)
    # pos and wp buffers
    wr32(SCRATCH + 0x100, c["pos"][0])
    wr32(SCRATCH + 0x104, c["pos"][1])
    wr32(SCRATCH + 0x108, 0)
    wr32(SCRATCH + 0x10C, 0)


def run_case(c):
    setup_case(c)
    me = c["me"]
    P = fol_addr(me)
    U = mover_addr(me)
    emu.call(0x6F49B8E0, ecx=P, stack_args=(U,))
    try:
        r = emu.call(0x6F49DF00, ecx=P, stack_args=(SCRATCH + 0x100, SCRATCH + 0x108))
    except Exception:
        from unicorn.x86_const import UC_X86_REG_EIP as E
        print("fault at", hex(mu.reg_read(E)), "ecx", hex(mu.reg_read(UC_X86_REG_ECX)), "edx", hex(mu.reg_read(UC_X86_REG_EDX)))
        raise
    return r


def collect(c, r):
    nm = len(c["movers"])
    out = [hx(r), hx(rd32(SCRATCH + 0x108)), hx(rd32(SCRATCH + 0x10C))]
    n = rd32(C + 0xC8)
    out.append(hx(n))
    for k in range(n):
        p = rd32(LISTD + 4 * k)
        out.append(hx(0xFFFFFFFF if p == 0 else (p - MOVS) // 0x100))
    out.append(hx(rd32(GD + 0xB4)))
    for i in range(len(c["nodes"])):
        out.append(hx(rd32(NODES + 0x100 * i + 0x38)))
    out.append(hx(rd32(W)))
    out.append(hx(rd32(W + 4)))
    for i in range(nm):
        a = fol_addr(i)
        for off in (0x88, 0x94, 0x98, 0xA8, 0xAC, 0x74, 0x50):
            out.append(hx(rd32(a + off)))
    out.append(hx(rd32(0x6FAB774C) & 0xFFFF))
    return out


def write_case(c):
    t = ["W", hx(len(c["movers"])), hx(len(c["units"])), hx(c["w"]), hx(c["h"]), hx(c["counter"]),
         hx(c["rngstate"][0]), hx(c["rngstate"][1]), hx(c["me"])]
    t.append(hx(len(c["cells"])))
    for top, head in c["cells"]:
        t.append(hx((top << 24) | head))
    t.append(hx(len(c["pool"])))
    for kind, node, nxt in c["pool"]:
        t.append(hx((kind << 24) | nxt))
        t.append(hx(node))
    t.append(hx(len(c["nodes"])))
    for flags, stamp, w40, owner in c["nodes"]:
        t += [hx(flags), hx(stamp), hx(w40), hx(owner)]
    for m in c["movers"]:
        t += [hx(m["tag"]), hx(m["serial"]), hx(m["vx"]), hx(m["vy"]), hx(m["unit"]), hx(1 if m["valid"] else 0)]
    for u in c["units"]:
        t += [hx(u["serial"]), hx(u["flags"]), hx(u["stamp"]), hx(1 if u["valid"] else 0)]
    for f in c["fols"]:
        t += [hx(f["flags"]), hx(f["wait"]), hx(f["retry"]), hx(f["blk_mover"]), hx(1 if f["blk_bad"] else 0),
              hx(f["mask"]), hx(f["radius"]), hx(f["p24"][0]), hx(f["p24"][1]), hx(f["own"])]
    t.append(hx(len(c["wps"])))
    for x, y in c["wps"]:
        t += [hx(x), hx(y)]
    t += [hx(c["idx"]), hx(c["pos"][0]), hx(c["pos"][1])]
    return t


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 400
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    rng = random.Random(seed)
    lines = []
    stats = {}
    if len(sys.argv) > 4 and sys.argv[4] == "A":
        # the request budget 0x6F49ACA0(this, k, now, &out): slots 0 and 1, intervals dword_6F953340[k]
        for _ in range(n):
            k = rng.choice([0, 1])
            last = rng.choice([0, rng.getrandbits(12), rng.getrandbits(32), 4000])
            now = rng.choice([4000, rng.getrandbits(12), rng.getrandbits(32), last + rng.randint(0, 80)]) & 0xFFFFFFFF
            wr32(FOLS + 4 * (31 + k), last)
            r = emu.call(0x6F49ACA0, ecx=FOLS, stack_args=(k, now, SCRATCH + 0x200))
            lines.append("A " + " ".join(hx(v) for v in (k, last, now, r & 0xFF, rd32(FOLS + 4 * (31 + k)), rd32(SCRATCH + 0x200))))
        with open(out, "w") as f:
            f.write("\n".join(lines) + "\n")
        print("gate cases", len(lines))
        return
    while len(lines) < n:
        c = gen_case(rng)
        # skip waypoint-reached cases: the reach branch is covered by crates/sim mover::waypoint_reached
        wx, wy = b2f(c["wps"][c["idx"]][0]), b2f(c["wps"][c["idx"]][1])
        px, py = b2f(c["pos"][0]), b2f(c["pos"][1])
        if (wx - px) ** 2 + (wy - py) ** 2 < 0.6:
            continue
        r = run_case(c)
        o = collect(c, r)
        stats[r] = stats.get(r, 0) + 1
        if (rd32(W), rd32(W + 4)) != c["rngstate"]:
            stats["draw"] = stats.get("draw", 0) + 1
        if rd32(C + 0xC8):
            stats["listed"] = stats.get("listed", 0) + 1
        lines.append(" ".join(write_case(c)) + " | " + " ".join(o))
    with open(out, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("cases", len(lines), "returns", stats)


if __name__ == "__main__":
    main()
