"""Unicorn vectors for the push-apart neighbour gather of Game.dll 1.26a (docs/spec/movement-7.md section 8).

The real code runs on a synthetic pathing grid G, a synthetic occupant pool and synthetic movers:

  0x6F495D60  gather setup: position of the record's unit (cell + V * t, via the real 0x6F38B0B0), the query box
              pos +- radius scaled by [G+0x68] and floored (0x6F47D5D0, 0x6F47BA20), stored at Q+0x24..+0x30
  0x6F496170  clears the list, tests the box (lo <= hi on both axes) and calls the walker
  0x6F495CA0  cell walker: box clamped to G+0x54..+0x60, outer loop over box[0..2], inner over box[1..3],
              cell index = G+0x3C * outer + inner
  0x6F495B00  occupant chain of one cell (24-bit next, kind byte, node pointer; stamps at node+0x38)
  0x6F493B60  per-node filter (mover tag, life, last speed == 0 (ordered equality 0x6F009AE0), record filter byte / nibble)

Only the position-time call 0x6F497460 (stubbed: writes the elapsed value t), the list append 0x6F47D810 and the list
clear 0x6F46AC70 are answered by the harness (the append records the node pointer).

Vector lines (hex):
  W  gather walk. Fields:
       pitch b54 b58 b5c b60 stamp0 | r0 r1 r2 r3 | self filt nib | ncells cell... | npool (word0 node)... |
       nnodes (flags mover stamp)... | nmovers (tag o14 life speed rec)... | out: count node_index... stamp_end
     (node and mover references are indices; -1 for none)
  S  setup. Fields:
       scale r px? ... see write_s()

Run:  GAME_DLL=C:\\path\\Game.dll py tools/emu/push_gather_emu.py W OUT.txt [N] [SEED]
      GAME_DLL=C:\\path\\Game.dll py tools/emu/push_gather_emu.py S OUT.txt [N] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_EAX  # noqa: E402

mu = emu.mu

BASE = 0x00800000
mu.mem_map(BASE, 0x200000)
Q = BASE + 0x0000
G = BASE + 0x0200
A_OBJ = BASE + 0x0400          # U+0x94 target: +0x2C -> G
U_SELF = BASE + 0x0600         # unit of the query (setup)
CELLS = BASE + 0x10000
POOL = BASE + 0x20000
NODES = BASE + 0x30000         # 0x100 each
MOVERS = BASE + 0x50000        # 0x100 each
RECS = BASE + 0x70000          # 0x40 each
SCR = BASE + 0x90000

TAG = 0x60706375
NIL = 0xFFFFFF


def rd32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def wr16(a, v):
    mu.mem_write(a, struct.pack("<H", v & 0xFFFF))


def wr8(a, v):
    mu.mem_write(a, struct.pack("<B", v & 0xFF))


def f2b(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def b2f(v):
    return struct.unpack("<f", struct.pack("<I", v & 0xFFFFFFFF))[0]


def hx(v):
    return "%X" % (v & 0xFFFFFFFF)


APPENDED = []
T_BITS = [0]


def _ret(uc, n):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ra = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + n)
    uc.reg_write(UC_X86_REG_EIP, ra)


def _append(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    p = struct.unpack("<I", bytes(uc.mem_read(esp + 4, 4)))[0]
    APPENDED.append(struct.unpack("<I", bytes(uc.mem_read(p, 4)))[0])
    uc.reg_write(UC_X86_REG_EAX, 1)
    _ret(uc, 8)


def _clear(uc, address, size, user):
    # 0x6F46AC70(start, count) on the list Q: the harness only ever clears everything
    wr32(Q + 0x1C, 0)
    APPENDED.clear()
    uc.reg_write(UC_X86_REG_EAX, 1)
    _ret(uc, 8)


def _pos_time(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    out = struct.unpack("<I", bytes(uc.mem_read(esp + 4, 4)))[0]
    wr32(out, T_BITS[0])
    uc.reg_write(UC_X86_REG_EAX, out)
    _ret(uc, 4)


def setup():
    mu.hook_add(UC_HOOK_CODE, _append, begin=0x6F47D810, end=0x6F47D810)
    mu.hook_add(UC_HOOK_CODE, _clear, begin=0x6F46AC70, end=0x6F46AC70)
    mu.hook_add(UC_HOOK_CODE, _pos_time, begin=0x6F497460, end=0x6F497460)


def rand_float(rng, kinds):
    k = rng.choice(kinds)
    return {
        "pos": lambda: rng.uniform(0.01, 400.0),
        "zero": lambda: 0.0,
        "negzero": lambda: -0.0,
        "neg": lambda: -rng.uniform(0.01, 10.0),
        "tiny": lambda: rng.choice([0.0005, 0.00099, 0.001, 0.0011, -0.0005, -0.001, -0.0011, 1e-9]),
        "nan": lambda: float("nan"),
        "small": lambda: rng.uniform(0.0, 3.0),
    }[k]()


# --------------------------------------------------------------------------------------------------------------- W
def model_gather(c):
    """Python model of 0x6F496170 (box test) + 0x6F495CA0 + 0x6F495B00 + 0x6F493B60."""
    b = dict(c["b"])
    stamp = [c["stamp0"]]
    pool = c["pool"]            # list of dict(next, kind, node)
    cells = c["cells"]
    nodes = c["nodes"]          # list of dict(flags, mover, stamp)
    movers = c["movers"]
    out = []
    r = c["rect"]
    if not (r[0] <= r[2] and r[1] <= r[3]):
        return out, stamp[0], nodes
    hi3 = min(r[3], b["b60"])
    hi2 = min(r[2], b["b5c"])
    lo1 = max(r[1], b["b58"])
    lo0 = max(r[0], b["b54"])
    if not (lo0 < hi2 and lo1 < hi3):
        return out, stamp[0], nodes
    stamp[0] += 1
    walk = stamp[0]
    # c["self_node"] is always 0 at the walker (the setup clears it) -- not modelled
    for o in range(lo0, hi2):
        for i in range(lo1, hi3):
            idx = c["pitch"] * o + i
            head = cells[idx] & 0xFFFFFF
            if head == NIL:
                continue
            stamp[0] += 1
            cell_stamp = stamp[0]
            cur = head
            while True:
                e = pool[cur]
                nxt = e["next"]
                kind = e["kind"]
                if kind != 2:
                    nd = nodes[e["node"]]
                    s = nd["stamp"]
                    if not (s == walk or s == cell_stamp or s == 0xFFFFFFFF):
                        if kind == 1:
                            if passes(nd, movers, c):
                                out.append(e["node"])
                            nd["stamp"] = walk
                        else:
                            nd["stamp"] = cell_stamp
                if nxt == NIL:
                    break
                cur = nxt
    return out, stamp[0], nodes


def approx_zero(bits):
    v = b2f(bits)
    if v != v:
        return False
    return v == 0.0


def passes(nd, movers, c):
    if nd["flags"] & 0xFF000000:
        return False
    mi = nd["mover"]
    if mi == c["self_mover"]:
        return False
    m = movers[mi]
    if m["tag"] != TAG:
        return False
    if m["o14"] & 0x80000000:
        return False
    life = b2f(m["life"])
    if not (life > 0.0):
        return False
    if not approx_zero(m["speed"]):
        return False
    rec = m["rec"]
    if rec is None:
        return False
    w = rec
    if ((w >> 20) & 0xFF) != c["filt"]:
        return False
    if ((w >> 28) & 0xF) < c["nib"]:
        return False
    return True


def build_w(rng):
    pitch = rng.choice([1, 2, 3, 5, 8, 12])
    height = rng.choice([1, 2, 3, 6, 9])
    c = {"pitch": pitch}
    lo_r = rng.choice([0, 0, 0, 1, 2])
    lo_c = rng.choice([0, 0, 0, 1])
    hi_r = rng.choice([height, height, max(lo_r, height - 1)])
    hi_c = rng.choice([pitch, pitch, max(lo_c, pitch - 1)])
    c["b"] = {"b54": lo_r, "b58": lo_c, "b5c": hi_r, "b60": hi_c}
    c["stamp0"] = rng.choice([0, 5, 100, 0x7FFFFFF0])
    nn = rng.choice([1, 2, 4, 6, 10])
    nm = nn + 1
    movers = []
    base_f = rng.choice([1, 2, 3, 7])
    base_n = rng.choice([0, 0, 1, 5])
    for i in range(nm):
        good = rng.random() < 0.8
        movers.append(dict(
            tag=TAG if rng.random() < 0.95 else 0x1234,
            o14=rng.getrandbits(31) | (0x80000000 if rng.random() < 0.08 else 0),
            life=f2b(rand_float(rng, ["pos"] * 6 + ["zero", "neg", "nan", "negzero"])) if rng.random() < 0.9 else f2b(rand_float(rng, ["zero", "nan"])),
            speed=f2b(rand_float(rng, ["zero"] * 6 + ["tiny", "small", "neg", "nan", "negzero"])),
            rec=None if rng.random() < 0.04 else (
                ((base_f << 20) | (rng.randrange(base_n, 16) << 28) | rng.getrandbits(20)) if good else
                (rng.getrandbits(8) << 20) | (rng.getrandbits(4) << 28) | rng.getrandbits(20)),
        ))
    c["movers"] = movers
    c["self_mover"] = rng.randrange(nm)
    c["filt"] = base_f if rng.random() < 0.9 else rng.choice([1, 2, 3, 7, 0xFF])
    c["nib"] = base_n if rng.random() < 0.9 else rng.choice([0, 1, 5, 15])
    nodes = []
    walk = c["stamp0"] + 1
    for i in range(nn):
        r = rng.random()
        if r < 0.5:
            s = rng.choice([0, 1, 2, 3, 50])
        elif r < 0.65:
            s = 0xFFFFFFFF
        elif r < 0.8:
            s = (walk) & 0xFFFFFFFF
        else:
            s = (walk + rng.randrange(1, 4)) & 0xFFFFFFFF
        nodes.append(dict(
            flags=(rng.getrandbits(24) if rng.random() < 0.9 else rng.getrandbits(32)) if rng.random() < 0.9 else 0,
            mover=rng.randrange(nm), stamp=s))
    for nd in nodes:
        if rng.random() < 0.85:
            nd["flags"] &= 0x00FFFFFF
    if rng.random() < 0.5:
        # dense mode: mostly valid neighbours, so the walk order and the stamps are exercised
        for m in movers:
            if rng.random() < 0.9:
                m.update(tag=TAG, o14=rng.getrandbits(31), life=f2b(rng.uniform(1.0, 100.0)),
                         speed=rng.choice([0, 0x80000000]),
                         rec=(c["filt"] << 20) | (rng.randrange(c["nib"], 16) << 28) | rng.getrandbits(20))
        for nd in nodes:
            nd["flags"] &= 0x00FFFFFF
            if rng.random() < 0.7:
                nd["stamp"] = rng.choice([0, 1, 2, 3, 50])
    c["nodes"] = nodes
    ncell = pitch * height
    cells = []
    pool = []
    for k in range(ncell):
        top = rng.getrandbits(8) << 24
        if rng.random() < 0.45:
            cells.append(top | NIL)
            continue
        n = rng.choice([1, 1, 2, 3, 5, 9])
        idxs = []
        for _ in range(n):
            idxs.append(len(pool))
            pool.append(dict(next=NIL, kind=rng.choice([1, 1, 1, 1, 2, 0, 3, 0x7F]), node=rng.randrange(nn)))
        for a, b in zip(idxs, idxs[1:]):
            pool[a]["next"] = b
        cells.append(top | idxs[0])
    # some unused pool entries so indices are not dense at 0
    c["cells"] = cells
    c["pool"] = pool
    # the box: sometimes degenerate / outside / reversed
    def rr(lo, hi):
        return rng.randrange(lo, hi + 1)
    r = rng.random()
    if r < 0.6:
        a = rr(-1, height); b = rr(-1, pitch)
        c["rect"] = [a, b, a + rr(0, height + 1), b + rr(0, pitch + 1)]
    elif r < 0.7:
        c["rect"] = [rr(-2, height + 2), rr(-2, pitch + 2), rr(-3, height + 3), rr(-3, pitch + 3)]
    else:
        c["rect"] = [lo_r - 1, lo_c - 1, hi_r + 1, hi_c + 1]
    c["rect"] = [max(-3, v) for v in c["rect"]]
    return c


def load_w(c):
    mu.mem_write(Q, b"\0" * 0x100)
    mu.mem_write(G, b"\0" * 0x100)
    wr32(G + 0x28, CELLS)
    wr32(G + 0x3C, c["pitch"])
    wr32(G + 0x40, len(c["cells"]) // c["pitch"])
    wr32(G + 0x54, c["b"]["b54"])
    wr32(G + 0x58, c["b"]["b58"])
    wr32(G + 0x5C, c["b"]["b5c"])
    wr32(G + 0x60, c["b"]["b60"])
    wr32(G + 0x78, POOL)
    wr32(G + 0xAC, NIL)
    wr32(G + 0xB4, c["stamp0"])
    for i, w in enumerate(c["cells"]):
        wr32(CELLS + 4 * i, w)
    for i, e in enumerate(c["pool"]):
        wr32(POOL + 8 * i, (e["kind"] << 24) | e["next"])
        wr32(POOL + 8 * i + 4, NODES + 0x100 * e["node"])
    for i, nd in enumerate(c["nodes"]):
        base = NODES + 0x100 * i
        wr32(base + 0x30, MOVERS + 0x100 * nd["mover"])
        wr32(base + 0x34, nd["flags"])
        wr32(base + 0x38, nd["stamp"])
    for i, m in enumerate(c["movers"]):
        base = MOVERS + 0x100 * i
        mu.mem_write(base, b"\0" * 0x100)
        wr32(base + 0x10, m["tag"])
        wr32(base + 0x14, m["o14"])
        wr32(base + 0x90, m["life"])
        wr32(base + 0xC0, m["speed"])
        if m["rec"] is None:
            wr32(base + 0xAC, 0)
        else:
            wr32(base + 0xAC, RECS + 0x40 * i)
            wr32(RECS + 0x40 * i + 0x20, m["rec"])
    wr32(Q + 0x40, MOVERS + 0x100 * c["self_mover"])
    wr16(Q + 0x50, c["filt"])
    wr8(Q + 0x52, c["nib"])
    wr32(Q + 0x34, G)
    for k, v in enumerate(c["rect"]):
        wr32(Q + 0x24 + 4 * k, v)


def w_line(rng):
    c = build_w(rng)
    out, stamp_end, nodes = model_gather(dict(c, nodes=[dict(n) for n in c["nodes"]]))
    # model_gather mutates nodes copy; re-run on a deep copy for expected state
    nodes_m = [dict(n) for n in c["nodes"]]
    out, stamp_end, _ = model_gather(dict(c, nodes=nodes_m))
    load_w(c)
    APPENDED.clear()
    emu.call(0x6F496170, ecx=Q)
    got = [(a - NODES) // 0x100 for a in APPENDED]
    st = rd32(G + 0xB4)
    gstamps = [rd32(NODES + 0x100 * i + 0x38) for i in range(len(c["nodes"]))]
    mstamps = [n["stamp"] for n in nodes_m]
    ok = (got == out and st == stamp_end and gstamps == mstamps)
    if not ok and os.environ.get('PGDEBUG'):
        print('DBG got', got, 'want', out, 'stamp', st, stamp_end, 'rect', c['rect'], 'b', c['b'], 'pitch', c['pitch'])
        print('   gstamps', gstamps, 'mstamps', mstamps)
    f = ["W", hx(c["pitch"]), hx(c["b"]["b54"]), hx(c["b"]["b58"]), hx(c["b"]["b5c"]), hx(c["b"]["b60"]),
         hx(c["stamp0"])]
    f += [hx(v) for v in c["rect"]]
    f += [hx(c["self_mover"]), hx(c["filt"]), hx(c["nib"])]
    f += [hx(len(c["cells"]))] + [hx(v) for v in c["cells"]]
    f += [hx(len(c["pool"]))]
    for e in c["pool"]:
        f += [hx((e["kind"] << 24) | e["next"]), hx(e["node"])]
    f += [hx(len(c["nodes"]))]
    for n in c["nodes"]:
        f += [hx(n["flags"]), hx(n["mover"]), hx(n["stamp"])]
    f += [hx(len(c["movers"]))]
    for m in c["movers"]:
        f += [hx(m["tag"]), hx(m["o14"]), hx(m["life"]), hx(m["speed"]), hx(0 if m["rec"] is None else 1),
              hx(m["rec"] or 0)]
    f += [hx(len(got))] + [hx(v) for v in got] + [hx(st)] + [hx(v) for v in gstamps]
    return " ".join(f), ok, len(got)


# --------------------------------------------------------------------------------------------------------------- S
def s_line(rng):
    mu.mem_write(Q, b"\0" * 0x100)
    mu.mem_write(G, b"\0" * 0x100)
    mu.mem_write(U_SELF, b"\0" * 0x100)
    wr32(G + 0x28, CELLS)
    pitch = rng.choice([4, 8, 16])
    wr32(G + 0x3C, pitch)
    wr32(G + 0x40, 8)
    for off in (0x54, 0x58):
        wr32(G + off, 0)
    wr32(G + 0x5C, 8)
    wr32(G + 0x60, pitch)
    wr32(G + 0x78, POOL)
    wr32(G + 0xAC, NIL)
    wr32(G + 0xB4, 3)
    scale = rng.choice([1.0, 1.0, 1.0, 0.5, 2.0, 0.03125])
    wr32(G + 0x68, f2b(scale))
    wr32(A_OBJ + 0x2C, G)
    wr32(U_SELF + 0x94, A_OBJ)
    cx = rng.uniform(-2.0, 20.0)
    cy = rng.uniform(-2.0, 20.0)
    vx = rng.choice([0.0, rng.uniform(-8, 8)])
    vy = rng.choice([0.0, rng.uniform(-8, 8)])
    t = rng.choice([0.0, rng.uniform(0.0, 1.0), rng.uniform(0.0, 5.0)])
    wr32(U_SELF + 0x78, f2b(cx))
    wr32(U_SELF + 0x7C, f2b(cy))
    wr32(U_SELF + 0x80, f2b(vx))
    wr32(U_SELF + 0x84, f2b(vy))
    T_BITS[0] = f2b(t)
    radius = rng.choice([5.0, 7.0, 8.0, 9.0, 10.0, rng.uniform(0.1, 12.0)])
    wr32(SCR, f2b(radius))
    filt = rng.getrandbits(8)
    nib = rng.getrandbits(4)
    APPENDED.clear()
    emu.call(0x6F495D60, ecx=Q, stack_args=(U_SELF, SCR, filt, nib))
    rect = [rd32(Q + 0x24 + 4 * k) for k in range(4)]
    px, py = rd32(Q + 0x44), rd32(Q + 0x48)
    f = ["S", hx(f2b(cx)), hx(f2b(cy)), hx(f2b(vx)), hx(f2b(vy)), hx(f2b(t)), hx(f2b(scale)), hx(f2b(radius)),
         hx(filt), hx(nib)]
    f += [hx(px), hx(py), hx(rd32(Q + 0x4C)), hx(rd32(Q + 0x50) & 0xFFFF), hx(rd32(Q + 0x50) >> 16 & 0xFF),
          hx(rd32(Q + 0x34))]
    f += [hx(v) for v in rect]
    return " ".join(f), True, 0


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    kind, out = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 1000
    seed = int(sys.argv[4]) if len(sys.argv) > 4 else 1
    rng = random.Random(seed)
    setup()
    bad = 0
    found = 0
    with open(out, "w") as fh:
        for i in range(n):
            line, ok, cnt = (w_line if kind == "W" else s_line)(rng)
            found += cnt
            if not ok:
                bad += 1
                if bad <= 5:
                    print("MISMATCH case", i, line[:200])
            fh.write(line + "\n")
    print("kind", kind, "cases", n, "mismatches vs python model", bad, "neighbours gathered", found)
    return 0


if __name__ == "__main__":
    sys.exit(main())
