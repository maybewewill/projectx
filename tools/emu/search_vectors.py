#!/usr/bin/env python3
"""Region search (A*) on the pyramid: differential test against the ORIGINAL Game.dll 1.26a.

Usage:
    GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn+pefile> \
        python3 tools/emu/search_vectors.py [--cases 300] [--rs crates/sim/tests/pathing/search_data.rs]

Each case is a random pyramid (same generator as pyramid_vectors.py) with a flag table at S+0x3C and
one to three requests. A request is driven through the original request setup 0x6F49A640 (world
points are passed as float pairs in memory; the original converts them to cells) and, when it asks
for a search (return 2), through 0x6F49A240 (start node, then the pop loop 0x6F49A170).

After every request the whole observable state is compared with the Python model, field by field:
the return codes, S+0x2C, the node list (every 36-byte entry), the heap (every 12-byte entry up to
the count), the closed-list head, pops, budget, start/goal ids, best node and best distance.

The model is a transcription of the decompile and the disassembly of:
    0x6F49A640 request setup         0x6F49A3F0 reset (called by the setup)
    0x6F49A240 start node + loop     0x6F49A170 pop loop
    0x6F498C70 relax (no flag)       0x6F498D90 relax with the link cost (flag edges)
    0x6F498C20 unlink, 0x6F498BD0 close, 0x6F498B80 push
    0x6F4995E0..0x6F499720 cell probes, 0x6F4997A0..0x6F499890 diagonal probes
    0x6F499E90 cell expander, 0x6F499FA0 block expander (see BLOCK below)
    0x6F4998F0..0x6F499BF0 edge scanners, 0x6F499CF0..0x6F499E40 corners (mode 1 only)

Mode 2 (two-cell footprint) is not modelled; every request uses size class 0. The block expander
(level >= 1 nodes) is a best reading of the edge and corner scanners and is reported separately.
"""

import argparse
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pyramid_vectors as pv  # noqa: E402  (also maps Game.dll)
from pyramid_vectors import (  # noqa: E402
    ENTRY, M32, M64, DIGEST_INIT, DIGEST_MUL, S_ADDR, Lcg, Lvl, Pyr, Harness,
    model_lookup, mu, call, fnv_words, LEVEL_ADDR, REC_ADDR,
)

A640 = 0x6F49A640
A240 = 0x6F49A240
HEAP_OFF = 0x70          # heap object H = S + 0x70 (base +0x0C, grow +0x14, cap +0x18, count +0x1C)
# lookup results are signed (-1 blocked, -2 refine); node fields (link, prev, parent) are u32
NEG1 = -1
NEG2 = -2
CLOSED_END = 0xFFFFFFFD  # -3, the empty closed-list head

COV = {'probe_relax': 0, 'diag': 0, 'flag_edge': 0, 'block_nodes': 0, 'cell_nodes': 0,
       'edge_split': 0, 'corner_relax': 0, 'budget_exhausted': 0, 'goal_popped': 0}


def isqrt(n):
    """0x6F4BABB0 (see crates/sim/src/pathing/isqrt.rs): Newton iteration with the observed stop rule."""
    n &= M32
    if n < 0x100:
        u = n // 12 + 1
    elif n < 0x10000:
        u = n // 200 + 0x15
    else:
        u = n // 26743 + 0x1BC
    while True:
        q = n // u
        d = u - q                  # signed difference of the old estimate and the quotient
        u = (q + u) // 2          # both operands are non-negative and below 2^31 here
        if -1 <= d <= 1:           # the original loops while (d / 2) != 0 (truncating)
            return u


def absdiff32(a, b):
    d = (a - b) & M32
    if d & 0x80000000:
        d = (-d) & M32
    return d


def scaled_d(ax, ay, bx, by):
    """(24 dx)^2 + (24 dy)^2 in 32-bit arithmetic (the sqrt argument of the cost and heuristic)."""
    ex = (24 * absdiff32(ax, bx)) & M32
    ey = (24 * absdiff32(ay, by)) & M32
    return (ex * ex + ey * ey) & M32


def plain_d2(ax, ay, bx, by):
    dx = (ax - bx) & M32
    dy = (ay - by) & M32
    return (dx * dx + dy * dy) & M32


def u32(v):
    return v & M32


# ---------------------------------------------------------------- node list helpers

def rd(node, off):
    return struct.unpack_from('<I', node, off)[0]


def wr(node, off, v):
    struct.pack_into('<I', node, off, v & M32)


def fx(node):
    return rd(node, 0)


def fy(node):
    return rd(node, 4)


class Heap:
    """Same algorithm as crates/sim/src/pathing/heap.rs (verified against 0x6F498580 / 0x6F4985F0)."""

    def __init__(self):
        self.slots = [None]  # slot 0 is the sentinel; count = len(slots)

    def push(self, key, node, stamp):
        self.slots.append(None)
        i = len(self.slots) - 1
        while i > 1:
            parent = i >> 1
            if self.slots[parent][0] < key:
                break
            self.slots[i] = self.slots[parent]
            i = parent
        self.slots[i] = (key, node, stamp)

    def pop(self):
        root = self.slots[1]
        last = self.slots.pop()
        if len(self.slots) > 1:
            last_index = len(self.slots) - 1
            half = last_index >> 1
            hole = 1
            if half != 0:
                while True:
                    child = hole << 1
                    if child < last_index and self.slots[child + 1][0] <= self.slots[child][0]:
                        child += 1
                    if last[0] <= self.slots[child][0]:
                        break
                    self.slots[hole] = self.slots[child]
                    hole = child
                    if child > half:
                        break
            self.slots[hole] = last
        return root


class Search:
    """The model of the search object S (only the fields the region search uses)."""

    def __init__(self, p, ftab):
        self.p = p              # Pyr: levels, T, flag_on, gen, grow, cap, entries (W list)
        self.ftab = ftab        # flag table: list of 256 (bit0, x, y)
        self.heap = Heap()
        self.closed_head = CLOSED_END
        self.pops = 0
        self.budget = 0
        self.start = (0, 0)
        self.goal = (0, 0)
        self.start_id = 0
        self.goal_id = 0
        self.best_d2 = 0
        self.best_id = 0
        self.staging = None
        self.mode = 1

    # ---- node accessors
    def node(self, i):
        return self.p.entries[i]

    def seed(self, i):
        n = self.p.entries[i]
        return fx(n), fy(n)

    # ---- 0x6F498580 / 0x6F498B80 push
    def push_node(self, i):
        n = self.node(i)
        wr(n, 8, rd(n, 8) + 1)
        wr(n, 0xC, NEG2)
        wr(n, 0x10, NEG1)
        key = (rd(n, 0x14) + rd(n, 0x18)) & M32
        self.heap.push(key, i, rd(n, 8))

    # ---- 0x6F498C20 unlink from the closed list (returns nothing)
    def unlink(self, i):
        n = self.node(i)
        wr(n, 8, rd(n, 8) + 1)
        nxt = rd(n, 0xC)
        prv = rd(n, 0x10)
        wr(n, 0xC, NEG1)
        wr(n, 0x10, NEG1)
        if prv == M32:
            self.closed_head = nxt
        else:
            wr(self.node(prv), 0xC, nxt)
        if nxt != CLOSED_END:
            wr(self.node(nxt), 0x10, prv)

    # ---- 0x6F498BD0 close (append at the head of the closed list)
    def close(self, i):
        if self.closed_head != CLOSED_END:
            wr(self.node(self.closed_head), 0x10, i)
        n = self.node(i)
        old = self.closed_head
        wr(n, 8, rd(n, 8) + 1)
        wr(n, 0xC, old)
        wr(n, 0x10, NEG1)
        self.closed_head = i

    # ---- 0x6F498C70: relax node `i` from parent `par` (the plain-cost edge)
    def relax(self, i, par, flag_edge=False):
        n = self.node(i)
        pn = self.node(par)
        if flag_edge:
            g_new = (rd(pn, 0x14) + 1) & M32
        else:
            step = isqrt(scaled_d(fx(pn), fy(pn), fx(n), fy(n)))
            g_new = (rd(pn, 0x14) + step) & M32
        h_new = isqrt(scaled_d(fx(n), fy(n), self.goal[0], self.goal[1]))
        link = rd(n, 0xC)
        if link == M32:
            d = plain_d2(fx(n), fy(n), self.goal[0], self.goal[1])
            if d < self.best_d2:
                self.best_d2 = d
                self.best_id = i
        else:
            if g_new >= rd(n, 0x14):
                return
            if link < M32 - 1:
                self.unlink(i)
            else:
                wr(n, 8, rd(n, 8) + 1)
                wr(n, 0xC, NEG1)
                wr(n, 0x10, NEG1)
        n = self.node(i)
        wr(n, 0x1C, par)
        if flag_edge:
            n[0x23] = pn[0x20]
        else:
            n[0x23] = 0
        wr(n, 0x14, g_new)
        wr(n, 0x18, h_new)
        self.push_node(i)

    # ---- probes (mode 1): lookup at level 0; -1 = blocked
    def lookup0(self, x, y):
        return model_lookup(self.p, self.staging, 0, x, y)

    def probe(self, frm, x, y):
        idv = self.lookup0(x, y)
        if idv == NEG1:
            return False
        COV['probe_relax'] += 1
        self.relax(idv, frm)
        return True

    def diag(self, frm, x, y):
        idv = self.lookup0(x, y)
        if idv != NEG1:
            COV['diag'] += 1
            self.relax(idv, frm)

    def link_edge(self, frm, flag, x, y):
        bit0, tx, ty = self.ftab[flag]
        if not bit0 or (x == tx and y == ty):
            return
        idv = self.lookup0(tx, ty)
        if idv != NEG1:
            COV['flag_edge'] += 1
            self.relax(idv, frm, flag_edge=True)

    # ---- 0x6F499E90 cell expander
    def expand_cell(self, i):
        COV['cell_nodes'] += 1
        n = self.node(i)
        x, y, flag = fx(n), fy(n), n[0x20]
        N = self.probe(i, x, y - 1)
        E = self.probe(i, x + 1, y)
        S_ = self.probe(i, x, y + 1)
        W = self.probe(i, x - 1, y)
        if N and E:
            self.diag(i, x + 1, y - 1)     # NE
        if E and S_:
            self.diag(i, x + 1, y + 1)     # SE
        if S_ and W:
            self.diag(i, x - 1, y + 1)     # SW
        if W and N:
            self.diag(i, x - 1, y - 1)     # NW
        if flag != 0:
            self.link_edge(i, flag, x, y)

    # ---- BLOCK (UNVERIFIED best reading): 0x6F499FA0 and its scanners
    def lookup_lvl(self, lvl, x, y):
        """0x6F498A60 at level `lvl` (the block scanners look up at their own level)."""
        return model_lookup(self.p, self.staging, lvl, x, y)

    def edge_scan(self, i, lvl, axis, split, fixed, o1, o2):
        """Mirror of 0x6F4998F0 (axis x) / 0x6F4999F0 (axis y): samples one edge of a block.

        o1 and o2 are one-element lists standing for the caller's out pointers."""
        def at(sp):
            return (sp, fixed) if axis == 'x' else (fixed, sp)
        x, y = at(split)
        uid = self.lookup_lvl(lvl, x, y)
        cur_o1 = o1
        while uid == NEG1 or uid == NEG2:
            if uid != NEG2:
                return
            bvar = lvl
            lvl -= 1
            half = 1 << lvl
            mid = ((split >> bvar) << bvar) + half
            if mid <= split:
                first = mid - (1 if lvl == 0 else self.mode)
                second = split
            else:
                first = split
                second = mid
            COV['edge_split'] += 1
            scratch = [False]
            self.edge_scan(i, lvl, axis, first, fixed, cur_o1, scratch)
            cur_o1 = scratch
            split = second
            x, y = at(split)
            uid = self.lookup_lvl(lvl, x, y)
        self.relax(uid, i)
        if lvl != 0:
            o2[0] = True
            cur_o1[0] = True

    def corner(self, i, lvl, x, y):
        uid = self.lookup_lvl(lvl, x, y)
        while True:
            if uid >= 0:
                COV['corner_relax'] += 1
                self.relax(uid, i)
                return
            if uid != NEG2:
                return
            lvl -= 1
            uid = self.lookup_lvl(lvl, x, y)

    def expand_block(self, i):
        COV['block_nodes'] += 1
        n = self.node(i)
        k = n[0x22]
        x, y = fx(n), fy(n)
        s = 1 << k
        ox = (x >> k) << k
        oy = (y >> k) << k
        top1, top2 = [False], [False]
        right1, right2 = [False], [False]
        bot1, bot2 = [False], [False]
        left1, left2 = [False], [False]
        self.edge_scan(i, k, 'x', x, oy - 1, top1, top2)
        self.edge_scan(i, k, 'y', y, ox + s, right1, right2)
        self.edge_scan(i, k, 'x', x, oy + s, bot1, bot2)
        self.edge_scan(i, k, 'y', y, ox - 1, left1, left2)
        if top2[0] and right1[0]:
            self.corner(i, k, ox + s, oy - 1)
        if right2[0] and bot2[0]:
            self.corner(i, k, ox + s, oy + s)
        if bot1[0] and left2[0]:
            self.corner(i, k, ox - 1, oy + s)
        if left1[0] and top1[0]:
            self.corner(i, k, ox - 1, oy - 1)

    def expand(self, i):
        if self.node(i)[0x22] == 0:
            self.expand_cell(i)
        else:
            self.expand_block(i)

    # ---- 0x6F49A170 pop loop; returns the goal id or -1
    def run(self):
        while True:
            if len(self.heap.slots) <= 1:   # the size counter includes the sentinel
                return NEG1
            pops_old = self.pops
            self.pops = pops_old + 1
            if pops_old >= self.budget:
                COV['budget_exhausted'] += 1
                return NEG1
            key, node, stamp = self.heap.pop()
            n = self.node(node)
            if rd(n, 8) != stamp:
                continue
            wr(n, 0xC, NEG1)
            wr(n, 0x10, NEG1)
            wr(n, 8, stamp + 1)
            if node == self.goal_id:
                COV['goal_popped'] += 1
                return node
            self.expand(node)
            self.close(node)

    # ---- 0x6F49A240: start node, then the loop
    def start_search(self):
        n = self.node(self.start_id)
        wr(n, 0x18, isqrt(scaled_d(self.start[0], self.start[1], self.goal[0], self.goal[1])))
        self.push_node(self.start_id)
        return self.run()


def call_full(fn, ecx=None, stack_args=()):
    """emu.call without its 200,000-instruction cap (a 400-pop search needs more), with a check
    that the function returned to the sentinel."""
    from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ECX, UC_X86_REG_EAX, UC_X86_REG_EIP
    import emu as _emu
    esp = _emu.TOP
    for a in reversed(stack_args):
        esp -= 4
        mu.mem_write(esp, struct.pack('<I', a & M32))
    esp -= 4
    mu.mem_write(esp, struct.pack('<I', _emu.SENT))
    mu.reg_write(UC_X86_REG_ESP, esp)
    if ecx is not None:
        mu.reg_write(UC_X86_REG_ECX, ecx & M32)
    mu.emu_start(fn, _emu.SENT, timeout=60_000_000, count=0)
    if mu.reg_read(UC_X86_REG_EIP) != _emu.SENT:
        raise RuntimeError('call did not return')
    v = mu.reg_read(UC_X86_REG_EAX)
    return v - (1 << 32) if v & 0x80000000 else v


def model_request(srch, T, flag, scx, scy, gcx, gcy, budget):
    """0x6F49A640 request setup (size class 0). Returns 0, 1 or 2 like the original."""
    p = srch.p
    # reset 0x6F49A3F0 (called first): S+0xA0 = 0, pops = 0, closed head = -3, list erased,
    # heap erased and one sentinel slot reserved (0x6F4865B0 with count 1)
    srch.pops = 0
    srch.closed_head = CLOSED_END
    p.entries = []
    srch.heap = Heap()
    p.T = T
    p.flag_on = flag
    srch.mode = 1
    srch.start = (scx, scy)
    srch.goal = (gcx, gcy)
    w, h = p.levels[0].w, p.levels[0].h
    if not (0 <= scx < w and 0 <= scy < h):
        return 0
    if (scx, scy) != (gcx, gcy):
        p.gen = (p.gen + 1) & M32
        sid = srch.lookup0(scx, scy)
        srch.start_id = sid
        if sid == NEG1:
            srch.goal_id = NEG1
            return 1
        gid = srch.lookup0(gcx, gcy)
        srch.goal_id = gid
        if sid != gid:
            srch.budget = budget          # S+0x98 is written only on this branch
            srch.best_id = sid
            srch.best_d2 = plain_d2(scx, scy, gcx, gcy)
            return 2
        return 1
    return 1


# ---------------------------------------------------------------- cases

def make_search_case(seed):
    g = Lcg(seed + 0x2000000)
    w0 = 8 * (1 + g.below(3))
    h0 = 8 * (1 + g.below(3))
    cell_only = g.below(3) == 0       # levels 1..3 all '11' (walk stops at level 1): cell nodes only
    grow = [1, 2, 4, 8][g.below(4)]
    cap0 = [0, 3][g.below(2)]
    hgrow = [1, 4, 16][g.below(3)]
    dims = [(w0 >> k, h0 >> k) for k in range(4)]
    recs = []
    for k, (w, h) in enumerate(dims):
        lvl = []
        for _ in range(w * h):
            r = g.below(100)
            if k > 0 and cell_only:
                state = 3
            else:
                state = 0 if r < 50 else (1 if r < 65 else (2 if r < 85 else 3))
            lvl.append((state, g.u32() & 0xFFFF))
        recs.append(lvl)
    ftab = []
    for k in range(256):
        bit0 = 1 if (k and g.below(3) == 0) else 0
        ftab.append((bit0, g.below(w0), g.below(h0)))
    reqs = []
    for _ in range(1 + g.below(3)):
        T = 2 * g.below(8)
        flag = g.below(2)
        scx, scy = g.below(w0), g.below(h0)
        gcx, gcy = g.below(w0 + 2) - 1, g.below(h0 + 2) - 1
        budget = [5, 40, 400][g.below(3)]
        reqs.append((T, flag, scx, scy, gcx, gcy, budget))
    return w0, h0, cell_only, grow, cap0, hgrow, recs, ftab, reqs


def build_levels(w0, h0, T, recs):
    levels = []
    for k in range(4):
        w, h = w0 >> k, h0 >> k
        lv = Lvl(w, h)
        for i, (state, hi16) in enumerate(recs[k]):
            word1 = hi16 << 16
            word1 &= ~((1 << (31 - T)) | (1 << (30 - T))) & M32
            word1 |= ((state >> 1) << (31 - T)) | ((state & 1) << (30 - T))
            lv.word1[i] = word1 & M32
        levels.append(lv)
    return levels


def chain_of(entries, node):
    out = []
    cur = node
    steps = 0
    while cur != M32:
        n = entries[cur]
        out.append((fx(n), fy(n)))
        cur = rd(n, 0x1C)
        steps += 1
        if steps > len(entries) + 1:
            COV['parent_cycle'] = COV.get('parent_cycle', 0) + 1
            out.append(('CYCLE', node))
            break
    return out


def status_of(ret640, ret240, start_id, best_id, goal_id, entries):
    """The dispatcher's (0x6F49A800) outcome: 'direct', 'reached', 'straight', 'partial'."""
    if ret640 == 1:
        return 'direct', []
    if ret240 != NEG1:
        return 'reached', chain_of(entries, ret240)
    if start_id == best_id:
        return 'straight', []
    return 'partial', chain_of(entries, best_id)


def state_words(srch, ret640, ret240):
    p = srch.p
    words = [ret640 & M32, ret240 & M32, p.gen & M32, len(p.entries), srch.pops,
             srch.closed_head, srch.start_id, srch.goal_id, srch.best_id, srch.best_d2]
    for lv in p.levels:
        for i in range(lv.w * lv.h):
            words.append(lv.stamp[i])
            words.append(lv.word1[i])
    for e in p.entries:
        words += [fx(e), fy(e), rd(e, 8), rd(e, 0xC), rd(e, 0x10), rd(e, 0x14), rd(e, 0x18),
                  rd(e, 0x1C), e[0x20], e[0x22], e[0x23]]
    words.append(len(srch.heap.slots))
    for key, node, stamp in srch.heap.slots[1:]:
        words += [key, node, stamp]
    return words


def compare_search(srch, ret640m, ret640e, ret240m, ret240e, h, mism, where):
    """Compares the emulated S and node/heap memory with the model."""
    S = S_ADDR
    rdv = lambda off: struct.unpack('<I', bytes(mu.mem_read(S + off, 4)))[0]  # noqa: E731
    ok = (ret640m == ret640e and ret240m == ret240e)
    if ret640m != ret640e or ret240m != ret240e:
        mism.append((where, 'returns', ret640m, ret640e, ret240m, ret240e))
    fields = [
        ('pops', rdv(0x9C), srch.pops),
        ('closed', rdv(0x94), srch.closed_head),
        ('c4', rdv(0xC4), srch.start_id),
        ('c8', rdv(0xC8), srch.goal_id),
        ('d0', rdv(0xD0), srch.best_id),
        ('cc', rdv(0xCC), srch.best_d2),
        ('budget', rdv(0x98), srch.budget),
    ]
    for name, emu_v, mod_v in fields:
        if (emu_v & M32) != (mod_v & M32):
            ok = False
            mism.append((where, name, emu_v, mod_v))
    hcount = rdv(0x8C)
    if hcount != len(srch.heap.slots):
        ok = False
        mism.append((where, 'heap count', hcount, len(srch.heap.slots)))
    if hcount > 1 and hcount == len(srch.heap.slots):
        base = rdv(0x7C)
        raw = bytes(mu.mem_read(base, 12 * hcount))
        for i in range(1, hcount):
            e = struct.unpack_from('<III', raw, 12 * i)
            if e != srch.heap.slots[i]:
                ok = False
                mism.append((where, 'heap entry', i, e, srch.heap.slots[i]))
                break
    pyr_ok = pv.compare_state(srch.p, ret640m, ret640e, h, mism, where)
    return ok and pyr_ok


def run_case(h, seed, staging_holder, mism, spots):
    case = make_search_case(seed)
    w0, h0, cell_only, grow, cap0, hgrow, recs, ftab_raw, reqs = case
    levels = build_levels(w0, h0, 0, recs)
    p = Pyr(levels, 0, 0, grow)
    p.cap = cap0
    h.write_state(p, cap0)
    mu.mem_write(S_ADDR + 0x84, struct.pack('<I', hgrow))   # heap grow step
    mu.mem_write(S_ADDR + 0x88, struct.pack('<I', 0))       # heap capacity
    mu.mem_write(S_ADDR + 0x7C, struct.pack('<I', 0))       # heap base
    tab = h.arena
    h.arena += 256 * 12 + 16
    raw = bytearray()
    for bit0, tx, ty in ftab_raw:
        raw += struct.pack('<III', bit0, tx, ty)
    mu.mem_write(tab, bytes(raw))
    mu.mem_write(S_ADDR + 0x3C, struct.pack('<I', tab))
    srch = Search(p, ftab_raw)
    dig = DIGEST_INIT
    for ri, (T, flag, scx, scy, gcx, gcy, budget) in enumerate(reqs):
        srch.staging = bytes(mu.mem_read(pv.STAGING, ENTRY))
        src = h.arena
        dst = h.arena + 8
        h.arena += 16
        # the setup floors and truncates these floats (0x6F6EF170 / 0x6F6EEC20): they are cells
        mu.mem_write(src, struct.pack('<ff', float(scx), float(scy)))
        mu.mem_write(dst, struct.pack('<ff', float(gcx), float(gcy)))
        where = (seed, ri)
        r640m = model_request(srch, T, flag, scx, scy, gcx, gcy, budget)
        r640e = call_full(A640, ecx=S_ADDR, stack_args=(T, src, dst, budget, 0, flag))
        r240m = NEG1
        r240e = NEG1
        if r640m == 2:
            r240m = srch.start_search()
            r240e = call_full(A240, ecx=S_ADDR)
        # compare
        compare_search(srch, r640m, r640e, r240m, r240e, h, mism, where)
        st, path = status_of(r640m, r240m, srch.start_id, srch.best_id, srch.goal_id, srch.p.entries)
        if spots is not None and len(spots) < 40:
            spots.append((seed, ri, st, path))
        dig = fnv_words(dig, state_words(srch, r640m, r240m))
    return dig


def main_search():
    ap = argparse.ArgumentParser()
    ap.add_argument('--cases', type=int, default=300)
    ap.add_argument('--seed0', type=int, default=0)
    ap.add_argument('--rs', help='Rust test file whose generated block is replaced')
    a = ap.parse_args()
    h = Harness()
    staging_holder = [bytes(mu.mem_read(pv.STAGING, ENTRY))]
    mism = []
    digests = []
    spots = []
    for c in range(a.cases):
        seed = a.seed0 + c
        dig = run_case(h, seed, staging_holder, mism, spots if c < 40 else None)
        digests.append((seed, dig))
    print('search cases %d, mismatches %d' % (a.cases, len(mism)))
    print('coverage', COV)
    for m in mism[:20]:
        print('  MISMATCH', m)
    if mism:
        sys.exit(1)
    if a.rs:
        splice(a.rs, digests, spots)
        print('wrote %d case digests to %s' % (len(digests), a.rs))


BEGIN = '// BEGIN GENERATED (tools/emu/search_vectors.py)'
END = '// END GENERATED'


def splice(path, digests, spots):
    lines = [BEGIN,
             '// Recorded from the original Game.dll 1.26a by tools/emu/search_vectors.py.',
             '// (seed, digest) per case: the case is regenerated from the seed (see main.rs).',
             'pub const SEARCH_CASES: &[(u64, u64)] = &[']
    for seed, dig in digests:
        lines.append('    (%d, 0x%016X),' % (seed, dig))
    lines.append('];')
    lines.append('// (seed, request, status, cells goal side first) for the first requests of the first cases.')
    lines.append('pub type Spot = (u64, usize, &\'static str, &\'static [(i32, i32)]);')
    lines.append('pub const SEARCH_SPOTS: &[Spot] = &[')
    for seed, ri, st, cells_path in spots:
        cells = ', '.join('(%d, %d)' % (x, y) for x, y in cells_path)
        lines.append('    (%d, %d, "%s", &[%s]),' % (seed, ri, st, cells))
    lines.append('];')
    lines.append(END)
    block = '\n'.join(lines) + '\n'
    src = open(path).read()
    if BEGIN in src:
        i = src.index(BEGIN)
        j = src.index(END) + len(END) + 1
        src = src[:i] + block + src[j:]
    else:
        src = src + '\n' + block
    open(path, 'w').write(src)


if __name__ == '__main__':
    main_search()
