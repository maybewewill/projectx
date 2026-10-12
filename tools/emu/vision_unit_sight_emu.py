"""Unicorn check of the unit sight writer `0x6f409e00` for radii of 3 and up (S20, docs/spec/vision.md section 14).

The real code that runs: the writer `0x6f409e00`, the shape cache `0x6f409d00`, the shape builder `0x6f409120`, the span
table `0x6f408fd0`, the grid base constructor (the constant 3 by 3 row list at grid+0x18), the span writer, the
rectangle writer, the byte cell writer, the elevation-layer builder `0x6f406660` and the radius conversion `0x6f406ab0`.
Stubbed (outside the function under test, the stubs are listed in the module docstring of `vision_unit_emu.py` plus):
  * `0x6f0128c0` (terrain cell level, decoded and checked in terrain.md): replaced by a Python table of levels with an
    optional "ramp found" flag;
  * `0x6f00f600` (the flying unit's height, result = a float written to the out pointer): replaced by a fixed value.

The Python model below is written from the disassembly (see vision.md section 14) and is compared with the real code
on random grids, random centres (inside the grid, near the edges and outside), random radii, random masks, random
elevation layers (cliff data) and random unit flags. Run:

    GAME_DLL=<Game.dll> py tools/emu/vision_unit_sight_emu.py --shapes 3 40 --cases 600
"""
import argparse
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(HERE, "jasscomp"))
import vision_unit_emu as vu  # noqa: E402
import vision_grid_emu as vg  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

FN_FLY_HEIGHT = 0x6F00F600
FN_ELEV_LAYER = 0x6F406660
FN_RADIUS = 0x6F406AB0


def s16(v):
    v &= 0xFFFF
    return v - 0x10000 if v >= 0x8000 else v


# ---------------------------------------------------------------------------------------------------------------------
# The shape (0x6f409120): records (dx, dy, a, b) in the final order, plus n0 and the span table.
# ---------------------------------------------------------------------------------------------------------------------
# The eight sweeps of the first phase: (primary (dx,dy) per unit of distance d, secondary (dx,dy) per step s).
SWEEPS = [
    ((0, -1), (1, 0)),
    ((0, 1), (-1, 0)),
    ((1, 0), (0, 1)),
    ((-1, 0), (0, -1)),
    ((0, -1), (-1, 0)),
    ((0, 1), (1, 0)),
    ((1, 0), (0, -1)),
    ((-1, 0), (0, 1)),
]
# The four scan quadrants of the second phase: (x0, y0, sx, sy).
QUADRANTS = [(0, 0, 1, -1), (-1, 0, -1, -1), (0, 1, 1, 1), (-1, 1, -1, 1)]


def build_shape(k):
    """Returns (records, n0) with records = list of (dx, dy, a, b); a, b = -1 or 0xFFFF as stored (u16 or -1)."""
    spans = vg.model_span_table(k)
    u = 2 * k + 1
    center = k * (u + 1)
    m = {}                       # block map: map index -> old record number
    old = []                     # phase-1 records: (dx, dy, a_mapidx, b_mapidx)  (None for the central block)
    for dy in (-1, 0, 1):
        for dx in (-1, 0, 1):
            m[center + dy * u + dx] = len(old)
            old.append((dx, dy, None, None))
    n0 = len(old)
    for d in range(1, k + 1):
        hi = spans[k - d][1]
        for t, (p, s) in enumerate(SWEEPS):
            lim = min(d, hi)
            start = 0
            if t >= 4:
                start = 1
                lim -= 1
            for si in range(start, lim + 1):
                dx = d * p[0] + si * s[0]
                dy = d * p[1] + si * s[1]
                idx = center + dy * u + dx
                if idx in m:
                    continue
                back = idx - (p[1] * u + p[0])
                back2 = back - (s[1] * u + s[0])
                if si == 0:
                    a = b = back
                elif si == d:
                    a = b = back2
                else:
                    a, b = back, back2
                m[idx] = len(old)
                old.append((dx, dy, a, b))
    # second phase: scan the four quadrants and renumber
    newmap = {i: v for i, v in m.items() if v < n0}
    recs = [(dx, dy, -1, -1) for (dx, dy, _, _) in old[:n0]]
    for (x0, y0, sx, sy) in QUADRANTS:
        rows = k + (1 if y0 == 0 else 0)
        cols = k + (1 if x0 == 0 else 0)
        rowstart = center + y0 * u + x0
        for _ in range(rows):
            i = rowstart
            for _ in range(cols):
                v = m.get(i)
                if v is not None and v >= n0:
                    dx, dy, a, b = old[v]
                    na = newmap.get(a, 0xFFFF)
                    nb = newmap.get(b, 0xFFFF)
                    newmap[i] = len(recs)
                    recs.append((dx, dy, na, nb))
                i += sx
            rowstart += sy * u
    return recs, n0


def dump_shape(ctx, k, pitch=64):
    H = ctx.H
    ctx.build_grid(64, 64, 6, [0] * 4096, [0xFFFF] * 4096, radius=k)
    shape = H.call(0x6F409D00, ecx=ctx.grid + 0x78, stack=(k, pitch))
    n0 = H.rd32(shape + 0x20)
    tot = H.rd32(shape + 0x24)
    recs_p = H.rd32(shape + 0x30)
    recs = []
    for i in range(tot):
        b = recs_p + 20 * i
        w4 = H.rd32(b + 4)
        a = H.rd32(b + 8)
        bb = H.rd32(b + 12)
        off = vu.Ctx.signed(H.rd32(b))
        recs.append((s16(w4 & 0xFFFF), s16(w4 >> 16), vu.Ctx.signed(a), vu.Ctx.signed(bb), off))
    return recs, n0


def check_shapes(ctx, lo, hi):
    bad = 0
    for k in range(lo, hi + 1):
        got, n0g = dump_shape(ctx, k)
        exp, n0e = build_shape(k)
        okk = n0g == n0e and len(got) == len(exp)
        if okk:
            for (g, e) in zip(got, exp):
                if (g[0], g[1], g[2], g[3]) != e or g[4] != e[0] + 64 * e[1]:
                    okk = False
                    break
        if not okk:
            bad += 1
            print("shape mismatch k=%d (got %d records, model %d)" % (k, len(got), len(exp)))
    print("shapes k=%d..%d: %d mismatches of %d" % (lo, hi, bad, hi - lo + 1))
    return bad


# ---------------------------------------------------------------------------------------------------------------------
# The writer model.
# ---------------------------------------------------------------------------------------------------------------------
class Model:
    def __init__(self, w, h, shift, vis, msk, elev):
        self.pg = vg.PyGrid(w, h, shift, vis, msk)
        self.elev = list(elev)
        self.w, self.h, self.shift = w, h, shift
        self.shapes = {}

    def shape(self, k):
        if k not in self.shapes:
            self.shapes[k] = build_shape(k) + (vg.model_span_table(k),)
        return self.shapes[k]

    def cell_ok(self, i):
        return 0 <= i < len(self.pg.vis)

    def write_cell(self, i, and_mask, or_mask):
        if self.cell_ok(i):
            self.pg.msk[i] &= and_mask
            self.pg.vis[i] |= or_mask

    def elev_at(self, i):
        return s16(self.elev[i]) if 0 <= i < len(self.elev) else 0


def model_writer(M, mask, owner, f5c, f200, f1fc, k_u16, arg3, arg4, px, py, level_fn, fly_value):
    """level_fn(px, py) -> (level, found); fly_value = the float height truncated (f2i of the stub value)."""
    if (mask & 0xFFF) == 0 and owner >= 12:
        return
    F = bool(f5c & 0x20000000) and s32(f200) <= 0
    cx, cy = px >> 2, py >> 2
    k = 4 if arg3 else k_u16
    pg = M.pg
    if k == 0:
        return
    if k <= 2:
        pg.rect(4, mask, cx - 1, cy - 1, cx + 1, cy + 1)
        if k == 2:
            pg.cell(4, mask, cx - 2, cy)
            pg.cell(4, mask, cx + 2, cy)
            pg.cell(4, mask, cx, cy - 2)
            pg.cell(4, mask, cx, cy + 2)
        return
    clipx = not (cx >= k and cx + k <= M.w - 1)
    clipy = not (cy >= k and cy + k <= M.h - 1)
    recs, n0, spans = M.shape(k)
    pitch = 1 << M.shift
    c0 = (cy << M.shift) + cx
    # level
    if f1fc == 0x10:
        ax = (fly_value + 0x100) & 0xFFFF
        found = False
    else:
        lvl, found = level_fn(px, py)
        ax = (lvl << 7) & 0xFFFF
    if found:
        cxv = M.elev[c0] if 0 <= c0 < len(M.elev) else 0
        if not (s16(ax) > s16(cxv)):
            ax = cxv & 0xFFFF
    L = s16(((ax + 0x2040) & 0xFFFFFF80) - 0x2000)
    and_mask = (~mask) & 0xFFFF
    or_mask = 0 if arg4 else mask & 0xFFFF
    fl = [0] * len(recs)
    if clipx or clipy:
        if F:
            left = max(cx - k, 0)
            right = min(cx + k, M.w - 1)
            top = max(cy - k, 0)
            bottom = min(cy + k, M.h - 1)
            j0 = -(cy - k) if cy - k < 0 else 0
            if top > bottom:
                return
            j = j0
            for row in range(top, bottom + 1):
                lo, hi = spans[j]
                x0 = max(lo + cx, left)
                x1 = min(hi + cx, right)
                pg.span(4, mask, row, x0, x1)
                j += 1
            return
        # third path
        for i in range(n0):
            fl[i] = 2
        left = 0 if cx - 1 <= 0 else cx - 1
        right = min(cx + 1, M.w - 1)
        skip = -(cy - 1) if cy - 1 < 0 else 0
        top = 0 if cy - 1 <= 0 else cy - 1
        bottom = min(cy + 1, M.h - 1)
        n = 0
        if top <= bottom:
            j = skip
            for row in range(top, bottom + 1):
                lo, hi = (-1, 1)               # the constant 3 by 3 list at grid+0x18
                x0 = max(lo + cx, left)
                x1 = min(hi + cx, right)
                for x in range(x0, x1 + 1):
                    i = (row << M.shift) + x
                    fl[n] = 1 if M.elev_at(i) > L else 0
                    n += 1
                    M.write_cell(i, and_mask, or_mask)
                j += 1
        for r in range(n0, len(recs)):
            dx, dy, a, b = recs[r]
            f = fl[b] | fl[a] if (a >= 0 and b >= 0) else 0
            fl[r] = f
            if f & 2:
                continue
            x = dx + cx
            if not (0 <= x < M.w):
                fl[r] = f | 2
                continue
            y = dy + cy
            if not (0 <= y < M.h):
                fl[r] = f | 2
                continue
            sel = f & 1
            i = c0 + dx + dy * pitch
            if sel:
                pass
            else:
                M.write_cell(i, and_mask, or_mask)
            fl[r] = f | (1 if M.elev_at(i) > L else 0)
        return
    if F:
        for j, row in enumerate(range(cy - k, cy + k + 1)):
            lo, hi = spans[j]
            pg.span(4, mask, row, lo + cx, hi + cx)
        return
    # phase A: the constant 3 by 3 block, then phase B: the records beyond it
    n = 0
    for row in range(cy - 1, cy + 2):
        for x in range(cx - 1, cx + 2):
            i = (row << M.shift) + x
            fl[n] = 1 if M.elev_at(i) > L else 0
            n += 1
            M.write_cell(i, and_mask, or_mask)
    for r in range(n0, len(recs)):
        dx, dy, a, b = recs[r]
        f = fl[b] | fl[a]
        sel = f & 1
        i = c0 + dx + dy * pitch
        if not sel:
            M.write_cell(i, and_mask, or_mask)
        fl[r] = f | (1 if M.elev_at(i) > L else 0)


def s32(v):
    v &= 0xFFFFFFFF
    return v - (1 << 32) if v >= 1 << 31 else v


# ---------------------------------------------------------------------------------------------------------------------
# The Unicorn side: stubs for the terrain functions, a writer runner with the extra unit fields.
# ---------------------------------------------------------------------------------------------------------------------
class Ctx2(vu.Ctx):
    def __init__(self):
        super().__init__()
        self.levels = {}            # (px, py) -> (level, found)
        self.default_level = (0, False)
        self.fly_bits = 0
        self.H.mu.hook_add(UC_HOOK_CODE, self._fly_stub, begin=FN_FLY_HEIGHT, end=FN_FLY_HEIGHT)

    def _level_stub(self, uc, addr, size, user):
        # 0x6f0128c0: ecx = cell x, edx = cell y, [esp+4] = out found flag pointer (ret 4)
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = self.H.rd32(esp)
        out = self.H.rd32(esp + 4)
        x = vu.Ctx.signed(uc.reg_read(UC_X86_REG_ECX))
        y = vu.Ctx.signed(uc.reg_read(UC_X86_REG_EDX))
        lvl, found = self.levels.get((x, y), self.default_level)
        if out:
            self.H.wr32(out, 1 if found else 0)
        uc.reg_write(UC_X86_REG_EAX, lvl & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_ESP, esp + 8)
        uc.reg_write(UC_X86_REG_EIP, ret)

    def _fly_stub(self, uc, addr, size, user):
        # 0x6f00f600: ecx = out pointer, edx = cell x, [esp+4] = cell y (ret 4); writes a float to *ecx, returns ecx
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = self.H.rd32(esp)
        out = uc.reg_read(UC_X86_REG_ECX)
        self.H.wr32(out, self.fly_bits)
        uc.reg_write(UC_X86_REG_EAX, out)
        uc.reg_write(UC_X86_REG_ESP, esp + 8)
        uc.reg_write(UC_X86_REG_EIP, ret)

    def run_writer(self, w, h, shift, vis, msk, elev, rect, k, mask, owner, f5c, f200, f1fc, arg3, arg4):
        H = self.H
        total = (1 << shift) * h
        self.build_grid(w, h, shift, vis, msk, k)
        # the grid base constructor 0x6f408c00 sets the constant row list: three entries (-1, 1)
        for j in range(3):
            H.wr32(self.spans + 8 * j, 0xFFFFFFFF)
            H.wr32(self.spans + 8 * j + 4, 1)
        H.wr32(self.grid + 0x1C, 3)
        H.wr32(self.grid + 0x20, self.spans)
        H.mu.mem_write(self.elev, b"".join(struct.pack("<H", v & 0xFFFF) for v in elev[:total]))
        self.build_unit(rect, k)
        H.wr32(self.unit + 0x5C, f5c)
        H.wr32(self.unit + 0x200, f200 & 0xFFFFFFFF)
        H.wr32(self.unit + 0x1FC, f1fc)
        H.wr32(self.unit + 0x58, owner)
        self.trace.clear()
        H.call(vu.FN_UNIT_SIGHT, ecx=self.grid, stack=(self.unit, mask, arg3, arg4))
        out_vis = list(struct.unpack("<%dH" % total, bytes(H.mu.mem_read(self.vis_arr, 2 * total))))
        out_msk = list(struct.unpack("<%dH" % total, bytes(H.mu.mem_read(self.mask_arr, 2 * total))))
        return out_vis, out_msk


def run_cases(ctx, n, seed, verbose=False):
    rng = random.Random(seed)
    bad = 0
    tested = 0
    cells_changed = 0
    paths = {}
    for case in range(n):
        w = rng.randrange(8, 44)
        h = rng.randrange(8, 44)
        shift = max(1, (w - 1).bit_length())
        stride = 1 << shift
        total = stride * h
        vis = [rng.getrandbits(16) if rng.random() < 0.3 else 0 for _ in range(total)]
        msk = [rng.getrandbits(16) if rng.random() < 0.5 else 0xFFFF for _ in range(total)]
        # cliff data: blocky elevation words (multiples of 128) with noise
        elev = [0] * total
        style = rng.randrange(4)
        for r in range(h):
            for c in range(w):
                if style == 0:
                    v = 0
                elif style == 1:
                    v = (rng.randrange(0, 4)) << 7
                elif style == 2:
                    v = ((c // 5 + r // 7) % 4) << 7
                else:
                    v = rng.choice([0, 0, 0, 128, 256, 384, 1920, -128 & 0xFFFF, rng.getrandbits(16)])
                elev[(r << shift) + c] = v
        k = rng.choice([0, 1, 2, 3, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14])
        mask = rng.choice([1 << rng.randrange(12), rng.getrandbits(12) or 1, rng.getrandbits(16), 0xF000 & rng.getrandbits(16)])
        owner = rng.choice([0, 3, 11, 12, 15])
        f5c = rng.choice([0, 0, 0x20000000, 0x20000000 | rng.getrandbits(8)])
        f200 = rng.choice([0, 0, 0, 1, -1, 5])
        f1fc = rng.choice([0, 0, 0, 0, 0x10, 0x20])
        arg3 = 1 if rng.random() < 0.08 else 0
        arg4 = 1 if rng.random() < 0.15 else 0
        # centre in pathing cells; mostly interior, sometimes at/outside the edges
        mode = rng.random()
        if mode < 0.55:
            cxf = rng.randrange(max(1, min(k, w // 2) if k else 1), max(max(1, min(k, w // 2) if k else 1) + 1, w - max(k, 1)))
            cyf = rng.randrange(max(1, min(k, h // 2) if k else 1), max(max(1, min(k, h // 2) if k else 1) + 1, h - max(k, 1)))
        elif mode < 0.9:
            cxf = rng.randrange(-2, w + 3)
            cyf = rng.randrange(-2, h + 3)
        else:
            cxf = rng.randrange(-6, w + 8)
            cyf = rng.randrange(-6, h + 8)
        # a negative footprint half takes the float slow path of the centre function (not under test): keep cells >= 0
        cxf = max(cxf, 0)
        cyf = max(cyf, 0)
        rect = (cyf * 4 + rng.randrange(0, 4), cxf * 4 + rng.randrange(0, 4))
        rect = (rect[0], rect[1], rect[0], rect[1])
        px, py = rect[1], rect[0]
        cxcell, cycell = px >> 2, py >> 2
        # the centre must stay readable in the harness arrays for the elevation / flag reads
        if k >= 3 and not (0 <= cycell < h and 0 <= cxcell < w):
            if rng.random() < 0.8:
                continue
        # terrain level for the unit's pathing cell
        ctx.levels = {}
        lvl = rng.choice([0, 0, 1, 2, 3, 4, 15, -1])
        found = rng.random() < 0.25
        ctx.levels[(px, py)] = (lvl, found)
        ctx.fly_bits = struct.unpack("<I", struct.pack("<f", float(rng.choice([0.0, 0.0, 100.0, 300.0, 700.0, -50.0]))))[0]
        fly_value = int(struct.unpack("<f", struct.pack("<I", ctx.fly_bits))[0])
        try:
            gv, gm = ctx.run_writer(w, h, shift, vis, msk, elev, rect, k, mask, owner, f5c, f200, f1fc, arg3, arg4)
        except Exception as exc:
            print("case %d crashed: %s (w=%d h=%d k=%d c=(%d,%d))" % (case, exc, w, h, k, cxcell, cycell))
            bad += 1
            continue
        M = Model(w, h, shift, vis, msk, elev)
        model_writer(M, mask, owner, f5c, f200, f1fc, k, arg3, arg4, px, py, lambda x, y: ctx.levels[(x, y)], fly_value)
        tested += 1
        cells_changed += sum(1 for i in range(total) if gv[i] != vis[i] or gm[i] != msk[i])
        key = (k >= 3, bool(0 <= cxcell and cxcell < w and 0 <= cycell < h), "clip" if (k >= 3 and not (cxcell >= k and cxcell + k <= w - 1 and cycell >= k and cycell + k <= h - 1)) else "in", "F" if (f5c & 0x20000000 and f200 <= 0) else "-")
        paths[key] = paths.get(key, 0) + 1
        if gv != M.pg.vis or gm != M.pg.msk:
            bad += 1
            if bad <= 5:
                diff = [i for i in range(total) if gv[i] != M.pg.vis[i] or gm[i] != M.pg.msk[i]]
                print("MISMATCH case %d: w=%d h=%d k=%d c=(%d,%d) mask=%#x F=%s fly=%#x arg3=%d arg4=%d diff cells %d first %s" % (
                    case, w, h, k, cxcell, cycell, mask, bool(f5c & 0x20000000 and f200 <= 0), f1fc, arg3, arg4, len(diff),
                    [(i >> shift, i & (stride - 1)) for i in diff[:6]]))
                if os.environ.get("VDEBUG"):
                    for i in diff[:40]:
                        print("   cell (row %d col %d) got vis %04x msk %04x model vis %04x msk %04x orig vis %04x msk %04x lvl=%s" % (i >> shift, i & (stride - 1), gv[i], gm[i], M.pg.vis[i], M.pg.msk[i], vis[i], msk[i], ctx.levels))
    print("writer cases tested=%d mismatches=%d changed_cells=%d" % (tested, bad, cells_changed))
    for key in sorted(paths, key=str):
        print("   path", key, paths[key])
    return bad


# ---------------------------------------------------------------------------------------------------------------------
# The elevation layer builder 0x6f406660 and the radius conversion 0x6f406ab0.
# ---------------------------------------------------------------------------------------------------------------------
def check_elevation_layer(ctx, n, seed):
    rng = random.Random(seed)
    bad = 0
    for case in range(n):
        w = rng.randrange(2, 14)
        h = rng.randrange(2, 14)
        shift = max(1, (w - 1).bit_length())
        stride = 1 << shift
        total = stride * h
        ctx.build_grid(w, h, shift, [0] * total, [0xFFFF] * total, 0)
        H = ctx.H
        H.mu.mem_write(ctx.elev, b"\xaa" * (2 * total))
        ctx.levels = {}
        ctx.default_level = (rng.choice([-1, 0, 1]), False)
        for _ in range(rng.randrange(0, 40)):
            ctx.levels[(rng.randrange(0, 4 * w), rng.randrange(0, 4 * h))] = (rng.choice([-1, 0, 1, 2, 3, 5, 14, 15]), False)
        H.call(FN_ELEV_LAYER, ecx=ctx.grid)
        got = struct.unpack("<%dH" % total, bytes(H.mu.mem_read(ctx.elev, 2 * total)))
        for r in range(h):
            for c in range(w):
                best = -1                        # di starts at 0xFFFF and the compare is signed 16 bit
                for i in range(4):               # x outer, y inner (the order does not matter for a maximum)
                    for j in range(4):
                        lv, _ = ctx.levels.get((c * 4 + i, r * 4 + j), ctx.default_level)
                        v = s16((lv << 7) & 0xFFFF)
                        if v > best:
                            best = v
                if got[(r << shift) + c] != (best & 0xFFFF):
                    bad += 1
                    if bad < 4:
                        print("elevation mismatch", case, r, c, got[(r << shift) + c], best & 0xFFFF)
    ctx.default_level = (0, False)
    print("elevation layer cases=%d mismatches=%d" % (n, bad))
    return bad


# ---------------------------------------------------------------------------------------------------------------------
# Vectors for the Rust tests (crates/sim/tests/vision_unit/vectors.rs): every expected value is read from the original.
# ---------------------------------------------------------------------------------------------------------------------
M32 = 0xFFFFFFFF


class XS:
    """xorshift32, the same generator as the Rust test."""

    def __init__(self, seed):
        self.x = (seed or 1) & M32

    def next(self):
        x = self.x
        x ^= (x << 13) & M32
        x ^= x >> 17
        x ^= (x << 5) & M32
        self.x = x
        return x


def fnv64(words, width):
    h = 0xCBF29CE484222325
    for v in words:
        for b in range(width):
            h ^= (v >> (8 * b)) & 0xFF
            h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def gen_arrays(seed, shift, h, style):
    rng = XS(seed)
    pitch = 1 << shift
    vis, msk, elev = [], [], []
    for i in range(pitch * h):
        a, b, c = rng.next(), rng.next(), rng.next()
        vis.append((a & 0xFFFF) if (b & 3) == 0 else 0)
        msk.append((a >> 16) if (b & 4) else 0xFFFF)
        col, row = i & (pitch - 1), i >> shift
        if style == 0:
            e = 0
        elif style == 1:
            e = (c % 4) << 7
        elif style == 2:
            e = ((col // 5 + row // 7 + (seed & 3)) % 4) << 7
        else:
            e = [0, 0, 128, 256, 384, 1920, 0xFF80, c & 0xFFFF][c % 8]
        elev.append(e)
    return vis, msk, elev


def lev(x, y, seed):
    return ((x * 131 + y * 197 + seed * 31) % 9) - 2


def write_vectors(ctx, path, n_writer, seed):
    out = []
    out.append("// Generated by tools/emu/vision_unit_sight_emu.py --rs (values read from the original Game.dll, S20).")
    out.append("// Do not edit. FNV-1a 64 hashes are over little-endian words.")
    out.append("/// (k, record count, hash of (dx, dy, a, b) as i32 words, central parents as -1)")
    out.append("pub const SHAPES: &[(i32, usize, u64)] = &[")
    for k in [3, 4, 5, 6, 7, 9, 12, 16, 20, 30, 45, 70]:
        recs, n0 = dump_shape(ctx, k)
        words = []
        for (dx, dy, a, b, off) in recs:
            words += [dx & M32, dy & M32, a & M32, b & M32]
        out.append("    (%d, %d, 0x%016X)," % (k, len(recs), fnv64(words, 4)))
    out.append("];")
    out.append("/// (width, height, seed, layer words row by row; level(x, y) = ((x*131 + y*197 + seed*31) % 9) - 2)")
    out.append("pub const ELEVATION: &[(i32, i32, i32, &[u16])] = &[")
    rng = random.Random(seed)
    H = ctx.H
    for case in range(24):
        w = rng.randrange(1, 9)
        h = rng.randrange(1, 9)
        sd = rng.randrange(0, 50)
        shift = max(1, (w - 1).bit_length())
        total = (1 << shift) * h
        ctx.build_grid(w, h, shift, [0] * total, [0xFFFF] * total, 0)
        H.mu.mem_write(ctx.elev, b"\xaa" * (2 * total))
        ctx.levels = {(x, y): (lev(x, y, sd), False) for x in range(4 * w) for y in range(4 * h)}
        H.call(FN_ELEV_LAYER, ecx=ctx.grid)
        got = struct.unpack("<%dH" % total, bytes(H.mu.mem_read(ctx.elev, 2 * total)))
        words = [got[(r << shift) + c] for r in range(h) for c in range(w)]
        out.append("    (%d, %d, %d, &[%s])," % (w, h, sd, ", ".join(str(v) for v in words)))
    out.append("];")
    out.append("/// (value bits, scale bits, radius in fog cells as stored: low 16 bits)")
    out.append("pub const RADIUS: &[(u32, u32, u16)] = &[")
    ptr = ctx.outp
    for _ in range(160):
        v = rng.choice([0.0, -50.0, 0.4, 64.0, 128.0, 200.0, 256.0, 384.0, 512.0, 800.0, 1100.0, 1400.0, 1800.0,
                        rng.uniform(-100.0, 2000.0), rng.uniform(0.0, 600.0)])
        sc = rng.choice([1.0 / 128, 1.0 / 128, 1.0 / 128, 1.0 / 130.0, 1.0 / 125.5, 1.0 / 256, 0.0])
        vb = struct.unpack("<I", struct.pack("<f", v))[0]
        sb = struct.unpack("<I", struct.pack("<f", sc))[0]
        H.wr32(ctx.grid + 0x70, sb)
        # the float is passed by value in the stack argument (the function takes the address of that slot)
        r = H.call(FN_RADIUS, ecx=ctx.grid, stack=(vb,))
        out.append("    (0x%08X, 0x%08X, %d)," % (vb, sb, r & 0xFFFF))
    out.append("];")
    out.append("/// (seed, w, h, style, k, mask, owner, ignore_cliffs, float_move, arg3, arg4, px, py, level, found, float_height, hash)")
    out.append("#[allow(clippy::type_complexity)]")
    out.append("pub const WRITER: &[(u32, i32, i32, u8, u16, u32, u32, bool, bool, bool, bool, i32, i32, i32, bool, i32, u64)] = &[")
    rng = random.Random(seed + 1)
    done = 0
    guard = 0
    paths = {}
    while done < n_writer and guard < n_writer * 20:
        guard += 1
        w = rng.randrange(10, 48)
        h = rng.randrange(10, 48)
        shift = max(1, (w - 1).bit_length())
        style = rng.randrange(4)
        cs = rng.getrandbits(32) | 1
        k = rng.choice([0, 1, 2, 3, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14])
        mask = rng.choice([1 << rng.randrange(12), rng.getrandbits(12) or 1, rng.getrandbits(16), 0xF000 & rng.getrandbits(16)])
        owner = rng.choice([0, 3, 11, 12, 15])
        F = rng.random() < 0.3
        fl = rng.random() < 0.15
        a3 = rng.random() < 0.08
        a4 = rng.random() < 0.15
        r0 = rng.random()
        if r0 < 0.5 and k >= 3 and w - 1 - k > k and h - 1 - k > k:
            cx = rng.randrange(k, w - k)
            cy = rng.randrange(k, h - k)
        elif r0 < 0.75:
            cx = rng.randrange(0, w)
            cy = rng.randrange(0, h)
        else:
            cx = rng.randrange(0, w + 4)
            cy = rng.randrange(0, h + 4)
        if k >= 3 and not (0 <= cy < h and 0 <= cx < w):
            continue
        px, py = cx * 4 + rng.randrange(0, 4), cy * 4 + rng.randrange(0, 4)
        lvl = rng.choice([0, 0, 1, 2, 3, 4, 15, -1])
        found = rng.random() < 0.25
        fh = rng.choice([0, 0, 100, 300, 700, -50])
        vis, msk, elev = gen_arrays(cs, shift, h, style)
        ctx.levels = {(px, py): (lvl, found)}
        ctx.fly_bits = struct.unpack("<I", struct.pack("<f", float(fh)))[0]
        f5c = 0x20000000 if F else rng.choice([0, 0x20000000])
        f200 = 0 if F else (5 if f5c else 0)
        f1fc = 0x10 if fl else 0
        rect = (py, px, py, px)
        gv, gm = ctx.run_writer(w, h, shift, vis, msk, elev, rect, k, mask, owner, f5c, f200, f1fc, 1 if a3 else 0, 1 if a4 else 0)
        M = Model(w, h, shift, vis, msk, elev)
        model_writer(M, mask, owner, f5c, f200, f1fc, k, 1 if a3 else 0, 1 if a4 else 0, px, py,
                     lambda x, y: ctx.levels[(x, y)], fh)
        assert gv == M.pg.vis and gm == M.pg.msk, "model disagrees with the original for a vector case"
        hsh = fnv64(gv + gm, 2)
        key = (k >= 3, "clip" if (k >= 3 and not (cx >= k and cx + k <= w - 1 and cy >= k and cy + k <= h - 1)) else "in", F)
        paths[key] = paths.get(key, 0) + 1
        out.append("    (%d, %d, %d, %d, %d, 0x%X, %d, %s, %s, %s, %s, %d, %d, %d, %s, %d, 0x%016X)," % (
            cs, w, h, style, k, mask, owner, str(F).lower(), str(fl).lower(), str(a3).lower(), str(a4).lower(), px, py,
            lvl, str(found).lower(), fh, hsh))
        done += 1
    out.append("];")
    print("vector paths:", paths)
    with open(path, "w", newline="\n") as f:
        f.write("\n".join(out) + "\n")
    print("wrote", path)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--shapes", nargs=2, type=int, default=[3, 40])
    ap.add_argument("--cases", type=int, default=600)
    ap.add_argument("--seed", type=int, default=20)
    ap.add_argument("--layer-cases", type=int, default=40)
    ap.add_argument("--rs", default=None, help="write the Rust test vectors to this file and exit")
    ap.add_argument("--rs-cases", type=int, default=220)
    args = ap.parse_args()
    ctx = Ctx2()
    if args.rs:
        write_vectors(ctx, args.rs, args.rs_cases, args.seed)
        return 0
    total_bad = 0
    total_bad += check_shapes(ctx, args.shapes[0], args.shapes[1])
    total_bad += check_elevation_layer(ctx, args.layer_cases, args.seed)
    total_bad += run_cases(ctx, args.cases, args.seed)
    print("TOTAL mismatches:", total_bad)
    return 1 if total_bad else 0


if __name__ == "__main__":
    sys.exit(main())
