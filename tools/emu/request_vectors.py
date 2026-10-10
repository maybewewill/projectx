#!/usr/bin/env python3
"""Whole path requests through the dispatcher: differential test against the ORIGINAL Game.dll 1.26a.

Extends tools/emu/search_vectors.py (the region search model) with
  * mode 2 (two-cell footprint, size class 1): the footprint probes and the mode-2 branches of
    the cell expander, the edge scanners, the corners and the block clamp;
  * the dispatcher 0x6F49A800 (request entry), the waypoint output 0x6F49A470, the direct move
    0x6F49A5E0 and the cell centre helper 0x6F497D30.

Usage:
    GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn+pefile> \
        python3 tools/emu/request_vectors.py [--cases 300] [--rs crates/sim/tests/pathing/request_data.rs]

Each case is a random pyramid (as in search_vectors.py) with 1 to 3 requests. Every request is one
call of the ORIGINAL dispatcher 0x6F49A800 with a path object (a vector of 8-byte points, +0x0C base,
+0x14 grow, +0x18 capacity, +0x1C count). The start and goal are float pairs in memory. Starts are
inside the pyramid (their floor is a valid cell); goals may be anywhere, fractional included.
Size class 0 is mode 1, size class 1 is mode 2 (S+0x90 = 1 << size).

After every request, compared with the Python model:
  * the return value of the dispatcher (1 = direct or found, 0 = partial or straight);
  * the path object: count and every point (x, y bit patterns), in order;
  * S+0x90 (mode), S+0xA0 (flag-marker count) and the region search state (as search_vectors.py).

The model is a transcription of the decompile and the disassembly of the functions listed above.
Two helpers are checked only through the dispatcher's outputs: the cell centre (0x6F497D30) and
the waypoint loop (0x6F49A470). Their softfloat operations are exact for these inputs, so the
model computes them with float32 values and asserts that the results are exact.

Start-up constants (never stored by the file, written by the image's own stubs at start-up):
  0x6FAAE4B8 = 0.25   (stub 0x6F84DD90, a plain store)
  0x6FAAE4D0 = 0.5    (stub 0x6F84DDF0)
  0x6FAAE4D4 = 0.5    (stub 0x6F84DE00)
  0x6FAAE4F4 = 1.0    (stub 0x6F84DE80, I2R(1))
  0x6FAB7748 = -128000.0 as read by S2R from the text "-128000.0" at 0x6F953334 (stub 0x6F860830,
               which the image runs at start-up). S2R is not run here (its import stubs live in
               text_vectors.py); its value 0xC7FA0001 was taken from text_vectors.py's S2R harness.
The run-time stub calls are made in this harness, so the constants are the original's values.

Import hooks: the allocator 0x6F4B39C0 (Storm.dll in the original) is replaced by the Python hook of
pyramid_vectors.py: it returns 1, moves the object's base (+0x0C) to a fresh block at least the
requested size and copies the old contents. The path object grows only through that allocator.
"""

import argparse
import math
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402  (maps Game.dll into a Unicorn emulator at import time)
import pyramid_vectors as pv  # noqa: E402
import search_vectors as sv  # noqa: E402
from pyramid_vectors import (  # noqa: E402
    ENTRY, M32, DIGEST_INIT, S_ADDR, Lcg, Pyr, Harness, mu, fnv_words, state_of,
)
from search_vectors import (  # noqa: E402
    Search, NEG1, NEG2, rd, fx, fy, call_full, state_words, compare_search,
)

DISPATCH = 0x6F49A800
S2R_MARK_BITS = 0xC7FA0001        # -128000.0 (see the module docstring)
CONST_BITS = {
    0x6FAAE4B8: 0x3E800000,       # 0.25
    0x6FAAE4D0: 0x3F000000,       # 0.5
    0x6FAAE4D4: 0x3F000000,       # 0.5
    0x6FAAE4F4: 0x3F800000,       # 1.0
    0x6FAB7748: S2R_MARK_BITS,
}
# (address, stub entry): the stubs that store the pool, run once at start-up
STUBS = (0x6F84DD90, 0x6F84DDF0, 0x6F84DE00, 0x6F84DE80)
MARK_ADDR = 0x6FAB7748
PATH_GROW = 4

COV = {'direct': 0, 'straight': 0, 'partial': 0, 'reached': 0, 'mode2_req': 0,
       'mode2_search': 0, 'probe_fail_m2': 0, 'edge_fail_m2': 0, 'corner_fail_m2': 0,
       'clamp_m2': 0, 'flag_points': 0, 'frac_start_or_goal': 0, 'centre_pts': 0,
       'wp_nodes': 0, 'wp_mode2_fix': 0, 'probe_relax': 0, 'diag': 0, 'edge_split': 0,
       'corner_relax': 0, 'cell_nodes': 0, 'block_nodes': 0}


def f32_bits(v):
    return struct.unpack('<I', struct.pack('<f', v))[0]


def bits_f32(b):
    return struct.unpack('<f', struct.pack('<I', b & M32))[0]


def exact(v):
    """A float32 value that is exactly representable (the softfloat result must be this value)."""
    assert bits_f32(f32_bits(v)) == v, 'inexact float32 value %r' % v
    return v


def s32(v):
    v &= M32
    return v - (1 << 32) if v & 0x80000000 else v


def cell_of(v):
    """floor, then truncation (0x6F6EF170, 0x6F6EEC20): the setup's cell of a float value."""
    return math.floor(v)


# ---------------------------------------------------------------- mode 2 model

class Search2(Search):
    """Region search with the two-cell footprint (mode 2). Mode 1 falls through to the base model."""

    def __init__(self, p, ftab):
        super().__init__(p, ftab)
        self.flagpts = 0

    # level-0 records only: state 00 and inside the level (0x6F498890 and its siblings)
    def F(self, x, y):
        lv = self.p.levels[0]
        if not (0 <= x < lv.w and 0 <= y < lv.h):
            return False
        return state_of(lv.word1[y * lv.w + x], self.p.T) == 0

    def lvl_of(self, idv):
        return self.node(idv)[0x22]

    # footprint tests (names: the original's function, see the module docstring)
    def c_e80(self, x, y):   # 0x6F498E80
        return self.F(x + 1, y)

    def c_ed0(self, x, y):   # 0x6F498ED0
        return self.F(x + 1, y) and self.F(x + 1, y + 1)

    def c_f30(self, x, y):   # 0x6F498F30
        return self.F(x, y + 1) and self.F(x + 1, y + 1)

    def c_f90(self, x, y):   # 0x6F498F90
        return self.F(x, y + 1)

    def c_fe0(self, x, y):   # 0x6F498FE0
        return self.F(x + 1, y - 2) or self.F(x - 1, y)

    def c_050(self, x, y):   # 0x6F499050
        return self.F(x - 2, y + 1) or self.F(x, y - 1)

    def c_110(self, x, y):   # 0x6F499110 (SE diagonal)
        return self.F(x + 1, y + 1)

    def c_170(self, x, y):   # 0x6F499170 (SW diagonal)
        return self.F(x, y + 1)

    def c_210(self, x, y):   # 0x6F499210 (three cells)
        return self.F(x + 1, y) and self.F(x, y + 1) and self.F(x + 1, y + 1)

    def c_4990c0(self, x, y):   # 0x6F4990C0 (NE diagonal)
        return self.F(x + 1, y)

    def c_530(self, x, y):   # 0x6F499530 (NE corner)
        return self.F(x + 1, y)

    def c_590(self, x, y):   # 0x6F499590 (SW corner)
        return self.F(x, y + 1)

    def c_2a0(self, x, y):   # 0x6F4992A0 (left edge, not last)
        return self.F(x, y + 1)

    def c_1c0(self, x, y):   # 0x6F4991C0 (top edge, not last)
        return self.F(x + 1, y)

    def c_2f0(self, x, y):   # 0x6F4992F0 (top edge, last)
        return (self.F(x + 1, y) and self.F(x + 1, y + 1)
                and (self.F(x + 1, y + 2) or self.F(x - 1, y)))

    def c_4a0(self, x, y):   # 0x6F4994A0 (left edge, last)
        return (self.F(x, y + 1) and self.F(x + 1, y + 1)
                and (self.F(x + 2, y + 1) or self.F(x, y - 1)))

    def c_410(self, x, y):   # 0x6F499410 (bottom edge, last)
        return self.c_210(x, y) and ((self.F(x - 1, y) and self.F(x - 1, y + 1))
                                     or (self.F(x + 1, y - 2) and self.F(x + 1, y - 1))
                                     or (self.F(x + 1, y - 1) and self.F(x - 1, y)))

    def c_380(self, x, y):   # 0x6F499380 (right edge, last)
        return self.c_210(x, y) and ((self.F(x, y - 1) and self.F(x + 1, y - 1))
                                     or (self.F(x - 2, y + 1) and self.F(x - 1, y + 1))
                                     or (self.F(x - 1, y + 1) and self.F(x, y - 1)))

    # ---- probes of the cell expander (mode 2 gates; the lookup happens first)
    def probe_n(self, frm, x, y):      # 0x6F4995E0
        idv = self.lookup0(x, y)
        if idv == NEG1:
            return False
        if self.mode == 2:
            if not self.c_e80(x, y):
                COV['probe_fail_m2'] += 1
                return False
            if (x & 1) and self.lvl_of(idv) != 0 and not self.c_fe0(x, y):
                COV['probe_fail_m2'] += 1
                return False
        COV['probe_relax'] += 1
        self.relax(idv, frm)
        return True

    def probe_e(self, frm, x, y):      # 0x6F499660
        idv = self.lookup0(x, y)
        if idv == NEG1:
            return False
        if self.mode == 2 and not self.c_ed0(x, y):
            COV['probe_fail_m2'] += 1
            return False
        COV['probe_relax'] += 1
        self.relax(idv, frm)
        return True

    def probe_s(self, frm, x, y):      # 0x6F4996C0
        idv = self.lookup0(x, y)
        if idv == NEG1:
            return False
        if self.mode == 2 and not self.c_f30(x, y):
            COV['probe_fail_m2'] += 1
            return False
        COV['probe_relax'] += 1
        self.relax(idv, frm)
        return True

    def probe_w(self, frm, x, y):      # 0x6F499720
        idv = self.lookup0(x, y)
        if idv == NEG1:
            return False
        if self.mode == 2:
            if not self.c_f90(x, y):
                COV['probe_fail_m2'] += 1
                return False
            if (y & 1) and self.lvl_of(idv) != 0 and not self.c_050(x, y):
                COV['probe_fail_m2'] += 1
                return False
        COV['probe_relax'] += 1
        self.relax(idv, frm)
        return True

    def diag2(self, frm, x, y, check):  # NE 0x6F4997A0, SE 0x6F4997F0, SW 0x6F499840, NW 0x6F499890
        idv = self.lookup0(x, y)
        if idv == NEG1:
            return
        if self.mode == 2 and check is not None and not check(x, y):
            COV['probe_fail_m2'] += 1
            return
        COV['diag'] += 1
        self.relax(idv, frm)

    def expand_cell(self, i):
        COV['cell_nodes'] += 1
        n = self.node(i)
        x, y, flag = fx(n), fy(n), n[0x20]
        N = self.probe_n(i, x, y - 1)
        E = self.probe_e(i, x + 1, y)
        S_ = self.probe_s(i, x, y + 1)
        W = self.probe_w(i, x - 1, y)
        if N and E:
            self.diag2(i, x + 1, y - 1, self.c_4990c0)
        if E and S_:
            self.diag2(i, x + 1, y + 1, self.c_110)
        if S_ and W:
            self.diag2(i, x - 1, y + 1, self.c_170)
        if W and N:
            self.diag2(i, x - 1, y - 1, None)
        if flag != 0:
            self.link_edge(i, flag, x, y)

    # ---- block expander (level >= 1) with the mode 2 clamp and the mode 2 scanner checks
    def edge_final(self, which, not_last, split, fixed):
        """The mode-2 test at the end of an edge scanner (the scanner returns without relaxing
        when it fails). `which` is the edge: top, bottom, right or left."""
        if which == 'top':
            return self.c_1c0(split, fixed) if not_last else self.c_2f0(split, fixed)
        if which == 'bottom':
            return self.c_210(split, fixed) if not_last else self.c_410(split, fixed)
        if which == 'right':
            return self.c_210(fixed, split) if not_last else self.c_380(fixed, split)
        # left
        return self.c_2a0(fixed, split) if not_last else self.c_4a0(fixed, split)

    def edge_scan(self, i, lvl, which, split, fixed, o1, o2):
        """Mirror of 0x6F4998F0 (top), 0x6F499AF0 (bottom), 0x6F4999F0 (right), 0x6F499BF0 (left)."""
        axis = 'x' if which in ('top', 'bottom') else 'y'

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
            self.edge_scan(i, lvl, which, first, fixed, cur_o1, scratch)
            cur_o1 = scratch
            split = second
            x, y = at(split)
            uid = self.lookup_lvl(lvl, x, y)
        if self.mode == 2:
            base = (split >> lvl) << lvl
            not_last = split < base + (1 << lvl) - 1
            if not self.edge_final(which, not_last, split, fixed):
                COV['edge_fail_m2'] += 1
                return
        self.relax(uid, i)
        if lvl != 0:
            o2[0] = True
            cur_o1[0] = True

    def corner(self, i, lvl, x, y, check):
        uid = self.lookup_lvl(lvl, x, y)
        while True:
            if uid >= 0:
                if self.mode == 2 and check is not None and not check(x, y):
                    COV['corner_fail_m2'] += 1
                    return
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
        if self.mode == 2:
            if ox + s - 2 < x:
                x = ox + s - 2
                COV['clamp_m2'] += 1
            if oy + s - 2 < y:
                y = oy + s - 2
                COV['clamp_m2'] += 1
        top1, top2 = [False], [False]
        right1, right2 = [False], [False]
        bot1, bot2 = [False], [False]
        left1, left2 = [False], [False]
        self.edge_scan(i, k, 'top', x, oy - 1, top1, top2)
        self.edge_scan(i, k, 'right', y, ox + s, right1, right2)
        self.edge_scan(i, k, 'bottom', x, oy + s, bot1, bot2)
        self.edge_scan(i, k, 'left', y, ox - 1, left1, left2)
        if top2[0] and right1[0]:
            self.corner(i, k, ox + s, oy - 1, self.c_530)
        if right2[0] and bot2[0]:
            self.corner(i, k, ox + s, oy + s, self.c_210)
        if bot1[0] and left2[0]:
            self.corner(i, k, ox - 1, oy + s, self.c_590)
        if left1[0] and top1[0]:
            self.corner(i, k, ox - 1, oy - 1, None)

    def expand(self, i):
        if self.node(i)[0x22] == 0:
            self.expand_cell(i)
        else:
            self.expand_block(i)


# ---------------------------------------------------------------- setup, waypoints, dispatcher model

def model_setup(srch, T, size, flag, sx, sy, gx, gy, budget):
    """0x6F49A640 (reset included): the cell arguments are the floors of the float inputs."""
    from search_vectors import model_request
    srch.flagpts = 0
    r = model_request(srch, T, flag, sx, sy, gx, gy, budget)
    srch.mode = 1 << (size & 31)
    return r


def model_waypoints(srch, node, pt4, pt5, out):
    """0x6F49A470: the parent chain of `node` as float pairs (bit patterns), start to goal side
    (the array is written goal side first, then both ends are replaced by the exact points)."""
    mode2 = srch.mode == 2
    off = 1.25 if mode2 else 0.75     # (0.5 or 1.0) + 0.25, both exact
    cur = node
    while True:
        COV['wp_nodes'] += 1
        n = srch.node(cur)
        x, y = s32(fx(n)), s32(fy(n))
        lvl = n[0x22]
        if mode2 and lvl != 0:
            size = 1 << lvl
            ox = (x >> lvl) << lvl
            oy = (y >> lvl) << lvl
            if x == ox + size - 1:
                x -= 1
                COV['wp_mode2_fix'] += 1
            if y == oy + size - 1:
                y -= 1
                COV['wp_mode2_fix'] += 1
        out.append((f32_bits(exact(float(x) + off)), f32_bits(exact(float(y) + off))))
        flag_byte = n[0x23]
        if flag_byte != 0:
            out.append((S2R_MARK_BITS, f32_bits(float(flag_byte))))
            srch.flagpts += 1
            COV['flag_points'] += 1
        nxt = rd(n, 0x1C)
        if nxt == M32:
            break
        cur = nxt
    last = len(out) - 1
    out[last] = pt4
    out[0] = pt5


def model_centre(srch, node):
    """0x6F497D30: the cell centre of a node's seed (x + 0.5, y + 0.5)."""
    COV['centre_pts'] += 1
    n = srch.node(node)
    return (f32_bits(exact(float(s32(fx(n))) + 0.5)), f32_bits(exact(float(s32(fy(n))) + 0.5)))


def model_dispatch(srch, T, size, flag, start, goal, budget, out):
    """0x6F49A800. start and goal are (x, y) float32 bit-pattern pairs. Returns
    (ret, setup_code, loop_code), or None when the start is outside the pyramid (not modelled)."""
    sxf, syf = bits_f32(start[0]), bits_f32(start[1])
    gxf, gyf = bits_f32(goal[0]), bits_f32(goal[1])
    del out[:]
    r = model_setup(srch, T, size, flag, cell_of(sxf), cell_of(syf), cell_of(gxf), cell_of(gyf),
                    budget)
    if r == 0:
        return None
    if r == 1:
        out.append(goal)
        COV['direct'] += 1
        return 1, 1, -1
    loop = srch.start_search()
    if loop != NEG1:
        model_waypoints(srch, loop, start, goal, out)
        COV['reached'] += 1
        return 1, 2, loop
    if srch.start_id == srch.best_id:
        out.append(start)
        COV['straight'] += 1
        return 0, 2, -1
    centre = model_centre(srch, srch.best_id)
    model_waypoints(srch, srch.best_id, start, centre, out)
    COV['partial'] += 1
    return 0, 2, -1


# ---------------------------------------------------------------- cases

def make_request_case(seed):
    g = Lcg(seed + 0x3000000)
    w0 = 8 * (1 + g.below(3))
    h0 = 8 * (1 + g.below(3))
    cell_only = g.below(3) == 0
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
                # more open than the region-search cases, so that goals are often reached
                state = 0 if r < 78 else (1 if r < 86 else (2 if r < 95 else 3))
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
        size = g.below(2)
        # start: inside the pyramid, fractional (eighths); goal: may be outside, fractional
        sxf = g.below(8 * w0) / 8.0
        syf = g.below(8 * h0) / 8.0
        if g.below(4) != 0:     # three quarters of the goals are inside the pyramid
            gxf = g.below(8 * w0) / 8.0
            gyf = g.below(8 * h0) / 8.0
        else:
            gxf = (g.below(8 * (w0 + 2)) - 8) / 8.0
            gyf = (g.below(8 * (h0 + 2)) - 8) / 8.0
        budget = [5, 40, 400][g.below(3)]
        reqs.append((T, flag, size, sxf, syf, gxf, gyf, budget))
    return w0, h0, cell_only, grow, cap0, hgrow, recs, ftab, reqs


def run_case(h, seed, mism, spots):
    case = make_request_case(seed)
    w0, h0, cell_only, grow, cap0, hgrow, recs, ftab_raw, reqs = case
    levels = sv.build_levels(w0, h0, 0, recs)
    p = Pyr(levels, 0, 0, grow)
    p.cap = cap0
    h.write_state(p, cap0)
    mu.mem_write(S_ADDR + 0x84, struct.pack('<I', hgrow))
    mu.mem_write(S_ADDR + 0x88, struct.pack('<I', 0))
    mu.mem_write(S_ADDR + 0x7C, struct.pack('<I', 0))
    tab = h.arena
    h.arena += 256 * 12 + 16
    raw = bytearray()
    for bit0, tx, ty in ftab_raw:
        raw += struct.pack('<III', bit0, tx, ty)
    mu.mem_write(tab, bytes(raw))
    mu.mem_write(S_ADDR + 0x3C, struct.pack('<I', tab))
    # the path object: base 0, grow PATH_GROW, capacity 0, count 0 (erased by the dispatcher)
    pobj = h.arena
    h.arena += 32
    mu.mem_write(pobj, b'\0' * 32)
    mu.mem_write(pobj + 0x14, struct.pack('<I', PATH_GROW))
    srch = Search2(p, ftab_raw)
    dig = DIGEST_INIT
    for ri, (T, flag, size, sxf, syf, gxf, gyf, budget) in enumerate(reqs):
        srch.staging = bytes(mu.mem_read(pv.STAGING, ENTRY))
        src = h.arena
        dst = h.arena + 8
        h.arena += 16
        mu.mem_write(src, struct.pack('<ff', sxf, syf))
        mu.mem_write(dst, struct.pack('<ff', gxf, gyf))
        if size:
            COV['mode2_req'] += 1
        if any(v != int(v) for v in (sxf, syf, gxf, gyf)):
            COV['frac_start_or_goal'] += 1
        start = (f32_bits(sxf), f32_bits(syf))
        goal = (f32_bits(gxf), f32_bits(gyf))
        where = (seed, ri)
        out = []
        res = model_dispatch(srch, T, size, flag, start, goal, budget, out)
        if res is None:
            raise AssertionError('start outside the pyramid (generator bug) %r' % (where,))
        ret_m, setup_m, loop_m = res
        if size and setup_m == 2:
            COV['mode2_search'] += 1
        ret_e = call_full(DISPATCH, ecx=S_ADDR, stack_args=(T, pobj, src, dst, budget, size, flag))
        ret_e &= M32
        if ret_e != ret_m:
            mism.append((where, 'ret', ret_m, ret_e))
        count = struct.unpack('<I', bytes(mu.mem_read(pobj + 0x1C, 4)))[0]
        base = struct.unpack('<I', bytes(mu.mem_read(pobj + 0xC, 4)))[0]
        emu_pts = []
        if count:
            raw = bytes(mu.mem_read(base, 8 * count))
            emu_pts = [struct.unpack_from('<II', raw, 8 * i) for i in range(count)]
        if emu_pts != out:
            mism.append((where, 'points', out[:6], emu_pts[:6], len(out), count))
        mode_e = struct.unpack('<I', bytes(mu.mem_read(S_ADDR + 0x90, 4)))[0]
        if mode_e != srch.mode:
            mism.append((where, 'mode', srch.mode, mode_e))
        a0 = struct.unpack('<I', bytes(mu.mem_read(S_ADDR + 0xA0, 4)))[0]
        if a0 != srch.flagpts:
            mism.append((where, 'flag points', srch.flagpts, a0))
        compare_search(srch, setup_m, setup_m, loop_m, loop_m, h, mism, where)
        words = state_words(srch, setup_m, loop_m)
        words += [ret_m & M32, len(out)]
        for bx, by in out:
            words += [bx, by]
        words.append(srch.flagpts)
        dig = fnv_words(dig, words)
        if spots is not None and ri < 3:
            spots.append((seed, ri, ret_m, [(bx, by) for bx, by in out]))
    return dig


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--cases', type=int, default=300)
    ap.add_argument('--seed0', type=int, default=0)
    ap.add_argument('--rs', help='Rust test file whose generated block is replaced')
    a = ap.parse_args()
    for stub in STUBS:
        emu.call(stub)
    for addr, v in CONST_BITS.items():
        mu.mem_write(addr, struct.pack('<I', v))
    h = Harness()
    mism = []
    digests = []
    spots = []
    for c in range(a.cases):
        seed = a.seed0 + c
        dig = run_case(h, seed, mism, spots if c < 40 else None)
        digests.append((seed, dig))
    print('request cases %d, mismatches %d' % (a.cases, len(mism)))
    print('coverage', COV)
    for m in mism[:20]:
        print('  MISMATCH', m)
    if mism:
        sys.exit(1)
    if a.rs:
        splice(a.rs, digests, spots)
        print('wrote %d case digests to %s' % (len(digests), a.rs))


BEGIN = '// BEGIN GENERATED (tools/emu/request_vectors.py)'
END = '// END GENERATED'


def splice(path, digests, spots):
    lines = [BEGIN,
             '// Recorded from the original Game.dll 1.26a by tools/emu/request_vectors.py.',
             '// (seed, digest) per case: the case is regenerated from the seed (see cases.rs).',
             'pub const REQUEST_CASES: &[(u64, u64)] = &[']
    for seed, dig in digests:
        lines.append('    (%d, 0x%016X),' % (seed, dig))
    lines.append('];')
    lines.append('// (seed, request, return code, waypoints as x and y float bit patterns).')
    lines.append('pub type Spot = (u64, usize, u32, &\'static [(u32, u32)]);')
    lines.append('pub const REQUEST_SPOTS: &[Spot] = &[')
    for seed, ri, ret, pts in spots:
        body = ', '.join('(0x%08X, 0x%08X)' % (x, y) for x, y in pts)
        lines.append('    (%d, %d, %d, &[%s]),' % (seed, ri, ret, body))
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
    main()
