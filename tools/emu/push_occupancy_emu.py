"""Unicorn vectors for the pathing-cell occupant maintenance of Game.dll 1.26a (docs/spec/movement-7.md section 8).

Real code: footprint update 0x6F49FF90 (rect difference 0x6F4BC060, rect insert 0x6F49FDA0, cell insert 0x6F49FCD0),
dirty-cell clean 0x6F49F1E0 (0x6F49F140 -> 0x6F49F030, node check 0x6F49E850) and the gather 0x6F496170 (walker
0x6F495CA0, processor 0x6F495B00, filter 0x6F493B60). Answered by the harness: the pool append 0x6F49EE50, the node
destructor (vtable +0x10), the list clear 0x6F46AC70 and the list append 0x6F47D810.

Line (hex): O pitch rows nnodes nops {M node r0 r1 r2 r3 | C | K node | G r0 r1 r2 r3}* ngather {count node...}*
            ncells cell... npool {word0 node}... free live stamp nwords bitmap... {stamp refs r0 r1 r2 r3}*nnodes
            ndestroyed node...

Run:  GAME_DLL=C:\\path\\Game.dll py tools/emu/push_occupancy_emu.py OUT.txt [N] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
import push_gather_emu as P  # noqa: E402
from push_gather_emu import (  # noqa: E402
    BASE, Q, G, CELLS, POOL, NODES, MOVERS, RECS, SCR, TAG, NIL, rd32, wr32, wr16, wr8, f2b, hx, APPENDED, _ret, mu)
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX  # noqa: E402

BM = BASE + 0xA0000
VT = BASE + 0xA1000
STUBFN = BASE + 0xA2000
DESTROYED = []


def _pool_append(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ent = struct.unpack("<I", bytes(uc.mem_read(esp + 4, 4)))[0]
    n = struct.unpack("<I", bytes(uc.mem_read(esp + 8, 4)))[0]
    cnt = rd32(G + 0x88)
    for k in range(n):
        mu.mem_write(POOL + 8 * (cnt + k), bytes(uc.mem_read(ent + 8 * k, 8)))
    wr32(G + 0x88, cnt + n)
    uc.reg_write(UC_X86_REG_EAX, 1)
    _ret(uc, 8)


def _destroy(uc, address, size, user):
    from unicorn.x86_const import UC_X86_REG_ECX
    ecx = uc.reg_read(UC_X86_REG_ECX)
    DESTROYED.append((ecx - NODES) // 0x100)
    _ret(uc, 4)


def setup_o():
    P.setup()
    mu.hook_add(UC_HOOK_CODE, _pool_append, begin=0x6F49EE50, end=0x6F49EE50)
    mu.hook_add(UC_HOOK_CODE, _destroy, begin=STUBFN, end=STUBFN)


class Occ:
    """Python model of the occupant maintenance."""

    def __init__(self, pitch, rows, nn):
        self.pitch, self.rows = pitch, rows
        self.cells = [NIL] * (pitch * rows)
        self.pool = []          # [w0, node]
        self.free = NIL
        self.live = 0
        self.stamp = 0
        self.words = (pitch * rows + 31) // 32
        self.bm = [0] * self.words
        self.nodes = [dict(stamp=0, refs=0, rect=[0, 0, 0, 0]) for _ in range(nn)]
        self.destroyed = []
        self.bounds = (0, 0, rows, pitch)

    def ins_cell(self, ix, iy, node, kind):
        idx = self.pitch * iy + ix
        if kind == 0:
            self.bm[idx >> 5] |= 1 << (idx & 31)
        cell = self.cells[idx]
        if self.free == NIL:
            e = len(self.pool)
            self.pool.append([(cell & 0xFFFFFF) | kind, node])
        else:
            e = self.free
            self.free = self.pool[e][0] & 0xFFFFFF
            self.pool[e] = [(cell & 0xFFFFFF) | kind, node]
        self.live += 1
        self.nodes[node]["refs"] += 1
        self.cells[idx] = (cell & 0xFF000000) | e

    def ins_rect(self, rect, node, kind):
        b = self.bounds
        hi3 = min(rect[3], b[3]); hi2 = min(rect[2], b[2])
        lo1 = max(rect[1], b[1]); lo0 = max(rect[0], b[0])
        if lo0 < hi2 and lo1 < hi3:
            for o in range(lo0, hi2):
                for i in range(lo1, hi3):
                    self.ins_cell(i, o, node, kind)

    def check_node(self, n):
        nd = self.nodes[n]
        if nd["stamp"] != 0xFFFFFFFF:
            return 1
        if (nd["refs"] & 0xFFFFFF) == 0:
            self.destroyed.append(n)
        return 0

    def clean_cell(self, idx):
        cell = self.cells[idx]
        cur = cell & 0xFFFFFF
        if cur == NIL:
            return
        self.stamp += 1
        st = self.stamp
        prev = NIL
        while True:
            w0, node = self.pool[cur]
            nxt = w0 & 0xFFFFFF
            kind = w0 & 0xFF000000
            nd = None if kind == 0x2000000 else node
            keep = False
            if kind == 0x1000000:
                s = self.nodes[nd]["stamp"]
                if s != 0xFFFFFFFF and s != st:
                    keep = True
            if keep:
                prev = cur
            else:
                if prev == NIL:
                    self.cells[idx] = (self.cells[idx] & 0xFF000000) | nxt
                else:
                    self.pool[prev][0] = (self.pool[prev][0] & 0xFF000000) | nxt
                self.pool[cur][0] = (w0 & 0xFF000000) | self.free
                self.free = cur
                self.live -= 1
                if nd is not None:
                    self.nodes[nd]["refs"] -= 1
            if nd is not None and self.check_node(nd):
                self.nodes[nd]["stamp"] = st
            if nxt == NIL:
                break
            cur = nxt

    def clean_all(self):
        if self.stamp > 0x7FFFFFFF:
            self.stamp = 0
        for w in range(self.words):
            bits = self.bm[w]
            k = 0
            while bits:
                if bits & 1:
                    self.clean_cell(w * 32 + k)
                bits >>= 1
                k += 1
            self.bm[w] = 0

    def move(self, n, new):
        old = self.nodes[n]["rect"]
        if old == new:
            return
        x = [max(old[0], new[0]), max(old[1], new[1]), min(old[2], new[2]), min(old[3], new[3])]
        if x[0] < x[2] and x[1] < x[3]:
            for r in reversed(rect_diff(old, x)):
                self.ins_rect(r, n, 0)
            for r in reversed(rect_diff(new, x)):
                self.ins_rect(r, n, 0x1000000)
        else:
            self.ins_rect(old, n, 0)
            self.ins_rect(new, n, 0x1000000)
        self.nodes[n]["rect"] = list(new)

    def gather(self, r, passing):
        out = []
        b = self.bounds
        if not (r[0] <= r[2] and r[1] <= r[3]):
            return out
        hi3 = min(r[3], b[3]); hi2 = min(r[2], b[2]); lo1 = max(r[1], b[1]); lo0 = max(r[0], b[0])
        if not (lo0 < hi2 and lo1 < hi3):
            return out
        self.stamp += 1
        walk = self.stamp
        for o in range(lo0, hi2):
            for i in range(lo1, hi3):
                head = self.cells[self.pitch * o + i] & 0xFFFFFF
                if head == NIL:
                    continue
                self.stamp += 1
                cs = self.stamp
                cur = head
                while True:
                    w0, node = self.pool[cur]
                    nxt = w0 & 0xFFFFFF
                    kind = w0 & 0xFF000000
                    if kind != 0x2000000:
                        nd = self.nodes[node]
                        s = nd["stamp"]
                        if not (s == walk or s == cs or s == 0xFFFFFFFF):
                            if kind == 0x1000000:
                                if node in passing:
                                    out.append(node)
                                nd["stamp"] = walk
                            else:
                                nd["stamp"] = cs
                    if nxt == NIL:
                        break
                    cur = nxt
        return out


def rect_diff(a, b):
    """0x6F4BC060: the parts of rect a outside rect b (b overlaps a), in the original's order."""
    out = []
    start0 = a[0]
    if b[0] > a[0]:
        out.append([a[0], a[1], b[0], a[3]])
        start0 = b[0]
    end0 = a[2]
    if b[2] < a[2]:
        out.append([b[2], a[1], a[2], a[3]])
        end0 = b[2]
    if b[1] > a[1]:
        out.append([start0, a[1], end0, b[1]])
    if b[3] < a[3]:
        out.append([start0, b[3], end0, a[3]])
    return out


def dump_real(pitch, rows, nn):
    cells = [rd32(CELLS + 4 * i) for i in range(pitch * rows)]
    cnt = rd32(G + 0x88)
    pool = [[rd32(POOL + 8 * i), (rd32(POOL + 8 * i + 4) - NODES) // 0x100] for i in range(cnt)]
    words = (pitch * rows + 31) // 32
    bm = [rd32(BM + 4 * i) for i in range(words)]
    nodes = []
    for i in range(nn):
        b = NODES + 0x100 * i
        nodes.append(dict(stamp=rd32(b + 0x38), refs=rd32(b + 0x3C), rect=[rd32(b + 0x1C + 4 * k) for k in range(4)]))
    return dict(cells=cells, pool=pool, free=rd32(G + 0xAC), live=rd32(G + 0xB0), stamp=rd32(G + 0xB4), bm=bm,
                nodes=nodes)


def s32(v):
    return v & 0xFFFFFFFF


def to_signed(v):
    return v - (1 << 32) if v >= (1 << 31) else v


def dump_model(m):
    return dict(cells=list(m.cells), pool=[list(e) for e in m.pool], free=m.free, live=m.live, stamp=m.stamp,
                bm=list(m.bm), nodes=[dict(n, rect=[s32(v) for v in n["rect"]]) for n in m.nodes])


def o_line(rng):
    pitch = rng.choice([3, 5, 8, 12])
    rows = rng.choice([3, 4, 7])
    nn = rng.choice([2, 4, 7])
    m = Occ(pitch, rows, nn)
    mu.mem_write(G, b"\0" * 0x100)
    mu.mem_write(Q, b"\0" * 0x100)
    mu.mem_write(CELLS, b"\0" * (4 * pitch * rows))
    mu.mem_write(POOL, b"\0" * 0x4000)
    mu.mem_write(BM, b"\0" * 0x100)
    wr32(VT + 0x10, STUBFN)
    mu.mem_write(STUBFN, b"\xc2\x04\x00")
    for i in range(pitch * rows):
        wr32(CELLS + 4 * i, NIL)
    wr32(G + 0x28, CELLS)
    wr32(G + 0x38, pitch * rows)
    wr32(G + 0x3C, pitch)
    wr32(G + 0x40, rows)
    wr32(G + 0x54, 0); wr32(G + 0x58, 0); wr32(G + 0x5C, rows); wr32(G + 0x60, pitch)
    wr32(G + 0x78, POOL)
    wr32(G + 0x88, 0)
    wr32(G + 0x98, BM)
    wr32(G + 0xA8, m.words)
    wr32(G + 0xAC, NIL)
    wr32(G + 0xB0, 0)
    wr32(G + 0xB4, 0)
    for i in range(nn):
        b = NODES + 0x100 * i
        mu.mem_write(b, b"\0" * 0x100)
        wr32(b, VT)
        wr32(b + 0x2C, G)
        wr32(b + 0x30, MOVERS + 0x100 * (i + 1))
    for i in range(nn + 1):
        b = MOVERS + 0x100 * i
        mu.mem_write(b, b"\0" * 0x100)
        wr32(b + 0x10, TAG)
        wr32(b + 0x90, f2b(0.5))
        wr32(b + 0xAC, RECS + 0x40 * i)
        wr32(RECS + 0x40 * i + 0x20, 1 << 20)
    wr32(Q + 0x40, MOVERS)           # the query's own mover: index 0, no node
    wr16(Q + 0x50, 1)
    wr8(Q + 0x52, 0)
    wr32(Q + 0x34, G)
    DESTROYED.clear()
    ops = []
    outs = []
    for _ in range(rng.choice([3, 6, 10, 16])):
        r = rng.random()
        if r < 0.55:
            n = rng.randrange(nn)
            o = m.nodes[n]["rect"]
            if rng.random() < 0.7 and o != [0, 0, 0, 0]:
                a = o[0] + rng.randrange(-1, 2); b = o[1] + rng.randrange(-1, 2)
                new = [a, b, a + rng.randrange(1, 4), b + rng.randrange(1, 4)]
                if rng.random() < 0.15:
                    new = list(o)
            else:
                a = rng.randrange(-1, rows + 1); b = rng.randrange(-1, pitch + 1)
                new = [a, b, a + rng.randrange(0, 4), b + rng.randrange(0, 4)]
            ops.append(("M", n, new))
            m.move(n, new)
            for k, v in enumerate(new):
                wr32(SCR + 4 * k, s32(v))
            emu.call(0x6F49FF90, ecx=NODES + 0x100 * n, stack_args=(SCR,))
        elif r < 0.7:
            ops.append(("C",))
            m.clean_all()
            emu.call(0x6F49F1E0, ecx=G)
        elif r < 0.78:
            n = rng.randrange(nn)
            ops.append(("K", n))
            m.nodes[n]["stamp"] = 0xFFFFFFFF
            wr32(NODES + 0x100 * n + 0x38, 0xFFFFFFFF)
        else:
            a = rng.randrange(-1, rows + 1); b = rng.randrange(-1, pitch + 1)
            box = [a, b, a + rng.randrange(0, rows + 2), b + rng.randrange(0, pitch + 2)]
            ops.append(("G", box))
            want = m.gather(box, set(range(nn)))
            for k, v in enumerate(box):
                wr32(Q + 0x24 + 4 * k, s32(v))
            APPENDED.clear()
            emu.call(0x6F496170, ecx=Q)
            got = [(a2 - NODES) // 0x100 for a2 in APPENDED]
            outs.append(got)
            if got != want:
                return None, False, 0, ("gather", got, want, box)
    real = dump_real(pitch, rows, nn)
    mod = dump_model(m)
    if real != mod or DESTROYED != m.destroyed:
        for k in real:
            if real[k] != mod[k]:
                return None, False, 0, (k, real[k], mod[k], ops)
        return None, False, 0, ("destroyed", DESTROYED, m.destroyed)
    f = ["O", hx(pitch), hx(rows), hx(nn), hx(len(ops))]
    for op in ops:
        if op[0] == "M":
            f += ["M", hx(op[1])] + [hx(s32(v)) for v in op[2]]
        elif op[0] == "C":
            f += ["C"]
        elif op[0] == "K":
            f += ["K", hx(op[1])]
        else:
            f += ["G"] + [hx(s32(v)) for v in op[1]]
    f += [hx(len(outs))]
    for o in outs:
        f += [hx(len(o))] + [hx(v) for v in o]
    f += [hx(len(mod["cells"]))] + [hx(v) for v in mod["cells"]]
    f += [hx(len(mod["pool"]))]
    for e in mod["pool"]:
        f += [hx(e[0]), hx(e[1])]
    f += [hx(mod["free"]), hx(mod["live"]), hx(mod["stamp"]), hx(len(mod["bm"]))] + [hx(v) for v in mod["bm"]]
    for n in mod["nodes"]:
        f += [hx(n["stamp"]), hx(n["refs"])] + [hx(v) for v in n["rect"]]
    f += [hx(len(m.destroyed))] + [hx(v) for v in m.destroyed]
    return " ".join(f), True, sum(len(o) for o in outs), None


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 1000
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    rng = random.Random(seed)
    setup_o()
    bad = found = 0
    with open(out, "w") as fh:
        for i in range(n):
            line, ok, cnt, dbg = o_line(rng)
            found += cnt
            if not ok:
                bad += 1
                if bad <= 3:
                    print("MISMATCH", i, dbg)
            else:
                fh.write(line + "\n")
    print("O cases", n, "mismatches vs python model", bad, "gathered", found)
    return 0


if __name__ == "__main__":
    sys.exit(main())
