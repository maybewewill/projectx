"""Differential tests of the fog grid constructor, the span table and the sight circle against the original Game.dll.

Checked here (2026-10-11), each against a Python transcription of the decompiled logic (docs/spec/vision.md):
  * the constructor `0x6f408d10` (CFogOfWarMap.cpp line 795): width/height = f2i(range/128), row pitch and shift,
    origin copy, scale = cells/range per axis, the initial words (mask 0xFFFF, visible 0);
  * the span table `0x6f408fd0` (octant cache `0x6f408f80`, midpoint `0x6f408800`): the (lo, hi) of all 2R+1 rows;
  * the sight-circle writer `0x6f3ba480`: every cell word (both layers, padding included) for random grids, modes,
    centres and radii (with a power-of-two scale, so the cell mapping is exact).

The emulator cannot run the Storm heap (import slots 401/403/405), the CRT memset dispatcher (0x6f7e11c2) or the
thread-information block (`fs:[0]`, used by the SEH prologues). This harness hooks those: the import slots point at a
bump allocator and stdcall stubs, memset is a plain fill, and the `fs:[0]` accesses of the functions under test are
patched in the emulated copy only. Nothing from the original is written into the repository.

Usage (from the repo root):
    GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn, pefile> python3 tools/emu/vision_grid_emu.py --cases 300
"""
import argparse
import os
import random
import struct
import sys
from fractions import Fraction

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402  (loads Game.dll into Unicorn)
import pefile  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_EAX,
    UC_X86_REG_ECX,
    UC_X86_REG_ESP,
    UC_X86_REG_EIP,
)

mu = emu.mu
GRID = 0x00201000          # grid object (0x80 bytes) in the emu data map (0x200000..0x20FFFF)
ARR_MASK = 0x01000000      # the mask and visible arrays of the circle tests (own 1 MB region, mapped in setup)
ARR_VIS = 0x01080000
PT = 0x00202100            # a point (x, y) float pair
RAD = 0x00202110           # a radius float
SPAN_STRUCT = 0x00202000   # 12-byte struct: [0] capacity, [1] entry count (2R+1), [2] table pointer
HEAP = 0x00400000          # bump heap for the Storm stubs
HEAP_SIZE = 0x00800000
SCRATCH = 0x00E00000       # target of the patched fs:[0] accesses
STUB_ALLOC = 0x00300100    # stdcall stubs (in the emu's return page)
STUB_FREE = 0x00300200
STUB_REALLOC = 0x00300300
STUB_RECT = 0x00300400     # replaces 0x6f00f560 (the playable rectangle getter)
STUB_NOP = 0x00300500      # replaces the constructor's helpers that are not geometry

FN_CTOR = 0x6F408D10
FN_SPAN_TABLE = 0x6F408FD0
FN_CIRCLE = 0x6F3BA480
FN_RECT_GETTER = 0x6F00F560
FN_CTOR_SKIP = (0x6F408CC0, 0x6F4089C0, 0x6F408070)   # fog cache reset, reserve, player masks (not geometry)
MEMSET = 0x6F7E11C2        # cdecl (dst, value, count)
COOKIE_CHECK = 0x6F7E1059  # fastcall (ECX = cookie)
CELL_SIZE_BITS = 0x43000000  # 128.0 (0x6fadb3e8, set at start-up)
CELL_SIZE_BITS2 = 0x42800000  # 64.0 (0x6fadb3ec)
ONE_OVER_128 = 0x3C000000


def u32(x):
    return x & 0xFFFFFFFF


def rd32(addr):
    return struct.unpack("<I", bytes(mu.mem_read(addr, 4)))[0]


def wr32(addr, v):
    mu.mem_write(addr, struct.pack("<I", u32(v)))


def f2b(f):
    return struct.unpack("<I", struct.pack("<f", f))[0]


def b2f(b):
    return struct.unpack("<f", struct.pack("<I", b & 0xFFFFFFFF))[0]


# ---------------------------------------------------------------------------------------------------------------
# Hooks: Storm heap, rectangle getter, skipped constructor helpers, memset, cookie check
# ---------------------------------------------------------------------------------------------------------------

_heap = [HEAP]
_sizes = {}


def _alloc(n):
    p = _heap[0]
    _heap[0] += (max(n, 1) + 0x1F) & ~0x1F
    if _heap[0] > HEAP + HEAP_SIZE:
        raise RuntimeError("heap exhausted")
    _sizes[p] = n
    if n:
        mu.mem_write(p, b"\0" * n)
    return p


def _stub(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    if addr == STUB_ALLOC:                 # SMemAlloc(size, file, line, flags): 4 stdcall args
        res, nargs = _alloc(rd32(esp + 4)), 4
    elif addr == STUB_FREE:                # SMemFree(ptr, file, line, flags)
        res, nargs = 1, 4
    elif addr == STUB_REALLOC:             # SMemReAlloc(ptr, size, file, line, flags): 5 stdcall args
        old, n = rd32(esp + 4), rd32(esp + 8)
        res = _alloc(n)
        if old:
            keep = min(_sizes.get(old, 0), n)
            if keep:
                mu.mem_write(res, bytes(mu.mem_read(old, keep)))
        nargs = 5
    else:
        raise RuntimeError("unknown stub %#x" % addr)
    uc.reg_write(UC_X86_REG_EAX, res)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + 4 * nargs)
    uc.reg_write(UC_X86_REG_EIP, ret)


RECT = [0.0, 0.0, 0.0, 0.0]            # (min y, min x, max y, max x) as float32, read by the rect stub


def _rect_stub(uc, addr, size, user):
    # 0x6f00f560 is __fastcall(ECX = out) returning out: the playable rectangle, copied to out.
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    out = uc.reg_read(UC_X86_REG_ECX)
    for i, v in enumerate(RECT):
        wr32(out + 4 * i, f2b(v))
    uc.reg_write(UC_X86_REG_EAX, out)
    uc.reg_write(UC_X86_REG_ESP, esp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


def _nop_stub(uc, addr, size, user):
    # Constructor helpers that are not geometry. 0x6f4089c0 is thiscall with one argument; the others take none.
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    pops = 4 if addr == 0x6F4089C0 else 0
    uc.reg_write(UC_X86_REG_EAX, 0)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pops)
    uc.reg_write(UC_X86_REG_EIP, ret)


def _memset_stub(uc, addr, size, user):
    # CRT memset: cdecl (dst, value, count). The caller pops the three arguments; the return address is popped here.
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    dst, val, n = rd32(esp + 4), rd32(esp + 8) & 0xFF, rd32(esp + 12)
    if n:
        mu.mem_write(dst, bytes([val]) * n)
    uc.reg_write(UC_X86_REG_EAX, dst)
    uc.reg_write(UC_X86_REG_ESP, esp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


def _cookie_stub(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    uc.reg_write(UC_X86_REG_EIP, rd32(esp))
    uc.reg_write(UC_X86_REG_ESP, esp + 4)


def patch_fs(start, end):
    """Rewrites `mov eax, fs:[0]` and `mov fs:[0], eax` (6 bytes each) inside [start, end) in the emulated copy."""
    n = 0
    z = b"\x00\x00\x00\x00"
    s = struct.pack("<I", SCRATCH)
    # (prefixed form, replacement of the same length): the segment prefix 0x64 is dropped and the disp32 points at
    # the scratch page (the value read is 0; the value written is discarded).
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
            n += 1
            i += len(pat)
        code = bytes(mu.mem_read(start, end - start))
    return n


def setup():
    """Maps the scratch pages, installs the hooks and the import-slot redirection, patches fs:[0]."""
    mu.mem_map(HEAP, HEAP_SIZE)
    mu.mem_map(SCRATCH, 0x1000)
    mu.mem_map(ARR_MASK, 0x100000)
    for a in (STUB_ALLOC, STUB_FREE, STUB_REALLOC, STUB_RECT, STUB_NOP):
        mu.mem_write(a, b"\xC3")
    for a in (STUB_ALLOC, STUB_FREE, STUB_REALLOC):
        mu.hook_add(UC_HOOK_CODE, _stub, begin=a, end=a)
    mu.hook_add(UC_HOOK_CODE, _rect_stub, begin=FN_RECT_GETTER, end=FN_RECT_GETTER)
    mu.hook_add(UC_HOOK_CODE, _nop_stub, begin=STUB_NOP, end=STUB_NOP)
    for a in FN_CTOR_SKIP:
        mu.hook_add(UC_HOOK_CODE, _nop_stub, begin=a, end=a)
    mu.hook_add(UC_HOOK_CODE, _memset_stub, begin=MEMSET, end=MEMSET)
    mu.hook_add(UC_HOOK_CODE, _cookie_stub, begin=COOKIE_CHECK, end=COOKIE_CHECK)
    pe = pefile.PE(emu.PATH, fast_load=True)
    pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"]])
    slots = {}
    for entry in pe.DIRECTORY_ENTRY_IMPORT:
        if entry.dll.decode().lower() != "storm.dll":
            continue
        for imp in entry.imports:
            if imp.ordinal in (401, 403, 405):
                slots[imp.ordinal] = imp.address   # absolute slot address (pefile)
    if len(slots) != 3:
        raise RuntimeError("Storm import slots not found: %r" % slots)
    wr32(slots[401], STUB_ALLOC)
    wr32(slots[403], STUB_FREE)
    wr32(slots[405], STUB_REALLOC)
    wr32(SCRATCH, 0)                                  # the value fs:[0] would read
    # Start-up constants (zero in the file; values from tools/emu/runtime_consts.py): the cell size 128.0 and 64.0.
    wr32(0x6FADB3E8, CELL_SIZE_BITS)
    wr32(0x6FADB3EC, CELL_SIZE_BITS2)
    # Function bodies under test (the circle writer is about 0x2E0 bytes; the others are smaller).
    patch_fs(0x6F3BA480, 0x6F3BA480 + 0x320)
    patch_fs(0x6F408D10, 0x6F408D10 + 0x200)
    patch_fs(0x6F408FD0, 0x6F408FD0 + 0x200)
    return slots


# ---------------------------------------------------------------------------------------------------------------
# Constructor
# ---------------------------------------------------------------------------------------------------------------

def run_ctor(min_y, min_x, max_y, max_x):
    """Runs the constructor for a playable rectangle (float32 values). Returns the grid fields, and the words."""
    RECT[:] = [min_y, min_x, max_y, max_x]
    mu.mem_write(GRID, b"\0" * 0x80)
    emu.call(FN_CTOR, ecx=GRID)
    g = {
        "base": rd32(GRID + 0x28),
        "rect": [rd32(GRID + 0x44 + 4 * i) for i in range(4)],
        "width": rd32(GRID + 0x60),
        "pitch": rd32(GRID + 0x64),
        "shift": rd32(GRID + 0x68),
        "height": rd32(GRID + 0x6C),
        "sx": rd32(GRID + 0x70),
        "sy": rd32(GRID + 0x74),
        "mask_arr": rd32(GRID + 0x2C),
        "vis_arr": rd32(GRID + 0x30),
    }
    cells = g["pitch"] * g["height"]
    g["words_ok"] = all(
        struct.unpack("<H", bytes(mu.mem_read(g["mask_arr"] + 2 * i, 2)))[0] == 0xFFFF
        and struct.unpack("<H", bytes(mu.mem_read(g["vis_arr"] + 2 * i, 2)))[0] == 0
        for i in range(cells))
    return g


def f32_round(q, mode):
    """A rational rounded to float32: mode 'nearest' or 'trunc' (toward zero). Returns the bits."""
    if q == 0:
        return 0
    sign = 0x80000000 if q < 0 else 0
    q = abs(q)
    e = 0
    while q >= 2:
        q /= 2
        e += 1
    while q < 1:
        q *= 2
        e -= 1
    m = q * (1 << 23)                      # 23 fraction bits relative to 1.0
    k = int(m) if mode == "trunc" else round(m)
    if k == (1 << 23):
        k = 1 << 22
        e += 1
    return sign | ((e + 127) << 23) | (k & 0x7FFFFF)


# ---------------------------------------------------------------------------------------------------------------
# Span table and circle (Python transcription of the decompiled logic)
# ---------------------------------------------------------------------------------------------------------------

def model_octant(r):
    """Midpoint circle of 0x6f408800: the x of every south-east step, from (0, r)."""
    rec = []
    x, y = 0, r
    d = 3 - 2 * r
    delta = -4 * r + 10
    east = 6
    while True:
        if d < 0:
            cur, nxt = east, delta + 4
        else:
            rec.append(x)
            cur, nxt = delta, delta + 8
            y -= 1
        d += cur
        east += 4
        x += 1
        delta = nxt
        if not (x <= y):
            break
    return rec


def model_span_table(r):
    """The (lo, hi) pairs of 0x6f408fd0 for radius r >= 1: 2r+1 entries, row j = dy j - r."""
    n = (2 * r + 1) & 0xFFFF
    oct_ = model_octant(r)
    t = [[0, 0] for _ in range(n)]
    t[r] = [-r, r]
    level = 1
    lo_k, hi_k = -r, r
    i_lo, i_hi = r - 1, r + 1
    for k, x in enumerate(oct_):
        t[k] = [-x, x]
        t[2 * r - k] = [-x, x]
        if level <= x:
            cnt = x - level + 1
            level += cnt
            for _ in range(cnt):
                t[i_lo] = [lo_k, hi_k]
                t[i_hi] = [lo_k, hi_k]
                i_lo -= 1
                i_hi += 1
        lo_k += 1
        hi_k -= 1
    return t


def run_span_table(radius):
    """Calls the span builder with a fresh struct and reads back the (lo, hi) of its 2R+1 entries."""
    mu.mem_write(SPAN_STRUCT, b"\0" * 12)
    emu.call(FN_SPAN_TABLE, ecx=radius, edx=SPAN_STRUCT)
    count = rd32(SPAN_STRUCT + 4)
    table = rd32(SPAN_STRUCT + 8)
    out = []
    for i in range(count):
        lo = struct.unpack("<i", bytes(mu.mem_read(table + 8 * i, 4)))[0]
        hi = struct.unpack("<i", bytes(mu.mem_read(table + 8 * i + 4, 4)))[0]
        out.append([lo, hi])
    return out


class PyGrid:
    """Flat cell arrays with the original's indexing: index = (row << shift) + col (no clamp in the cell writer)."""

    def __init__(self, w, h, shift, vis, msk):
        self.w, self.h, self.shift = w, h, shift
        self.pitch = 1 << shift
        self.vis = list(vis)
        self.msk = list(msk)

    def idx(self, col, row, byte_wrap=True):
        # The single-cell writer 0x6f3b0e20 reads both coordinates as BYTES (movzx byte [esp+..]): col and row
        # wrap modulo 256 (CONFIRMED by disassembly; the first circle run exposed it: a column of -1 is 255). The
        # span writer 0x6f406920 uses full-width integers (CONFIRMED by disassembly).
        if byte_wrap:
            return ((row & 0xFF) << self.shift) + (col & 0xFF)
        return (row << self.shift) + col

    def cell(self, mode, mask, col, row, byte_wrap=True):
        i = self.idx(col, row, byte_wrap)
        if i < 0 or i >= len(self.vis):
            return                          # outside the array (the original writes past it; not modelled)
        if mode == 1:
            self.msk[i] |= mask
            self.vis[i] &= ~mask & 0xFFFF
        elif mode == 2:
            self.msk[i] &= ~mask & 0xFFFF
            self.vis[i] &= ~mask & 0xFFFF
        elif mode == 4:
            self.msk[i] &= ~mask & 0xFFFF
            self.vis[i] |= mask

    def span(self, mode, mask, row, x0, x1):
        """0x6f406920 / 0x6f4069f0 / 0x6f406850: nothing outside the rows; clamp x0 >= 0 and x1 <= w-1; pairwise."""
        if not (0 <= row < self.h):
            return
        x0 = max(x0, 0)
        x1 = min(x1, self.w - 1)
        u = x0 & 1
        cells = []
        if u:
            cells.append(x0)
        p = x0 + u
        e = x1 - (1 if x1 % 2 == 0 else 0)
        if p < e:
            count = ((e - p - 1) >> 1) + 1
            for k in range(count):
                cells += [p + 2 * k, p + 2 * k + 1]
            p += 2 * count
        if x1 % 2 == 0:
            cells.append(p)
        for c in cells:
            self.cell(mode, mask, c, row, byte_wrap=False)

    def rect(self, mode, mask, x0, y0, x1, y1):
        """0x6f3b0e90: clamp the start to 0 and the end to w-1 / h-1; the loops are [x0,x1) x [y0,y1)."""
        x0 = max(x0, 0)
        y0 = max(y0, 0)
        x1 = min(x1, self.w - 1)
        y1 = min(y1, self.h - 1)
        for r in range(y0, y1):
            for c in range(x0, x1):
                self.cell(mode, mask, c, r, byte_wrap=False)


def model_circle(g, mode, mask, cx, cy, r):
    """0x6f3ba480 after the mapper: cx, cy are the clamped centre cells, r the radius in cells."""
    if r == 0:
        return
    if r == 1:
        g.rect(mode, mask, cx - 1, cy - 1, cx + 1, cy + 1)
        return
    if r == 2:
        g.rect(mode, mask, cx - 1, cy - 1, cx + 1, cy + 1)
        g.cell(mode, mask, cx - 2, cy)
        g.cell(mode, mask, cx + 2, cy)
        g.cell(mode, mask, cx, cy - 2)
        g.cell(mode, mask, cx, cy + 2)
        return
    table = model_span_table(r)
    y0 = cy - r
    y1 = cy + r
    if g.h <= cy + r:
        y1 = g.h
    j = 0
    if y0 < 0:
        j = -y0
        y0 = 0
    row = y0
    while row < y1:
        lo, hi = table[j]
        x0 = cx + lo
        x0 = 0 if x0 < 0 else x0
        x1 = cx + hi
        if g.w <= x1:
            x1 = g.w
        g.span(mode, mask, row, x0, x1)
        row += 1
        j += 1


def run_circle(w, h, shift, vis, msk, mode, mask, px, py, radius):
    """Runs the original circle writer on a grid of the given size with exact (power-of-two) scale and origin 0."""
    stride = 1 << shift
    mu.mem_write(GRID, b"\0" * 0x80)
    wr32(GRID + 0x2C, ARR_MASK)
    wr32(GRID + 0x30, ARR_VIS)
    wr32(GRID + 0x44, 0)
    wr32(GRID + 0x48, 0)
    wr32(GRID + 0x60, w)
    wr32(GRID + 0x64, stride)
    wr32(GRID + 0x68, shift)
    wr32(GRID + 0x6C, h)
    wr32(GRID + 0x70, ONE_OVER_128)
    wr32(GRID + 0x74, ONE_OVER_128)
    total = stride * h
    mu.mem_write(ARR_MASK, b"".join(struct.pack("<H", v) for v in msk[:total]))
    mu.mem_write(ARR_VIS, b"".join(struct.pack("<H", v) for v in vis[:total]))
    mu.mem_write(PT, struct.pack("<ff", px, py))
    mu.mem_write(RAD, struct.pack("<f", radius))
    try:
        emu.call(FN_CIRCLE, ecx=GRID, stack_args=(mode, mask, PT, RAD))
    except Exception as exc:       # report where the original stopped (the harness must not hide a fault)
        from unicorn.x86_const import UC_X86_REG_EIP
        raise RuntimeError("circle w=%d h=%d mode=%d r=%.0f px=%.0f py=%.0f: %s at %#x" % (
            w, h, mode, radius, px, py, exc, mu.reg_read(UC_X86_REG_EIP)))
    out_msk = list(struct.unpack("<%dH" % total, bytes(mu.mem_read(ARR_MASK, 2 * total))))
    out_vis = list(struct.unpack("<%dH" % total, bytes(mu.mem_read(ARR_VIS, 2 * total))))
    return out_vis, out_msk


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cases", type=int, default=200, help="constructor and circle cases")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--radii", type=int, default=160, help="span table radii 3..N")
    ap.add_argument("--vectors", action="store_true", help="print the constructor vectors for the Rust tests")
    ap.add_argument("--wide", action="store_true", help="also circles on grids wider than 256 cells")
    args = ap.parse_args()
    slots = setup()
    print("storm slots:", {k: hex(v) for k, v in slots.items()})
    rng = random.Random(args.seed)
    stats = {"ctor_size": [0, 0], "ctor_scale": [0, 0, 0], "ctor_words": [0, 0], "span": [0, 0],
             "circle": [0, 0], "circle_cells": [0, 0]}

    # 1. Constructor: integer rectangles (exact range). Vectors (rect, width, height, scale bits) are printed for the
    #    Rust tests (crates/sim/src/vision.rs).
    vectors = []
    for _ in range(args.cases):
        mn_x = rng.randint(-6000, 6000)
        mn_y = rng.randint(-6000, 6000)
        rx = rng.randint(130, 9000)
        ry = rng.randint(130, 9000)
        mx_x, mx_y = mn_x + rx, mn_y + ry
        g = run_ctor(float(mn_y), float(mn_x), float(mx_y), float(mx_x))
        w = int(rx / 128)
        h = int(ry / 128)
        stats["ctor_size"][0] += 1
        stats["ctor_size"][1] += not (g["width"] == w and g["height"] == h)
        stats["ctor_words"][0] += 1
        stats["ctor_words"][1] += not g["words_ok"]
        # The scale is a soft-float quotient (width / range): checked bit-exactly in the Rust tests from the
        # vectors this harness prints (not against a rational rounding, which the original does not use).
        if len(vectors) < 40:
            vectors.append((float(mn_y), float(mn_x), float(mx_y), float(mx_x), g["width"], g["height"],
                            g["sx"], g["sy"]))
        # origin copy: +0x44 = min y, +0x48 = min x
        if g["rect"] != [f2b(float(mn_y)), f2b(float(mn_x)), f2b(float(mx_y)), f2b(float(mx_x))]:
            print("rect copy mismatch", mn_x, mn_y)

    # 2. Span table for every radius in 3..N.
    for r in range(3, args.radii + 1):
        got = run_span_table(r)
        stats["span"][0] += 1
        if got != model_span_table(r):
            stats["span"][1] += 1
            if stats["span"][1] <= 3:
                print("span mismatch r=%d" % r, got[:4], model_span_table(r)[:4])

    # 3. Circle writer on random grids: both layers, padding included.
    for case in range(args.cases):
        # Half of the cases use grids wider than 256 cells: the cell writer reads its coordinates as bytes.
        wide = args.wide and case % 2 == 1
        w = rng.randint(200, 300) if wide else rng.randint(1, 40)
        h = rng.randint(1, 40)
        shift = max(1, (w - 1).bit_length())
        stride = 1 << shift
        total = stride * h
        vis = [rng.getrandbits(16) for _ in range(total)]
        msk = [rng.getrandbits(16) for _ in range(total)]
        mode = rng.choice([1, 2, 4])
        mask = rng.choice([1 << rng.randrange(12), rng.getrandbits(12) or 1]) & 0xFFFF
        radius = float(rng.randint(0, 12 * 128 + 127))     # 0..12 cells, most of them with a fraction of a cell
        span_w = w if wide else 40          # wide grids: centres over the whole grid (columns past 255 too)
        px = float(rng.randint(-200, span_w * 128 + 200))
        py = float(rng.randint(-200, h * 128 + 200))
        cx = max(0, min(w - 1, int(px / 128) if px >= 0 else 0))
        cy = max(0, min(h - 1, int(py / 128) if py >= 0 else 0))
        r = int(radius / 128)                  # f2i(mul(radius, 1/128)): zero below 128 units
        gv, gm = run_circle(w, h, shift, vis, msk, mode, mask, px, py, radius)
        pg = PyGrid(w, h, shift, vis, msk)
        model_circle(pg, mode, mask, cx, cy, r)
        stats["circle"][0] += 1
        same = gv == pg.vis and gm == pg.msk
        stats["circle"][1] += not same
        stats["circle_cells"][0] += sum(1 for i in range(total) if gv[i] != vis[i] or gm[i] != msk[i])
        if not same and stats["circle"][1] <= 3:
            print("circle mismatch w=%d h=%d mode=%d r=%d c=(%d,%d)" % (w, h, mode, r, cx, cy))
    for k, v in stats.items():
        print("%-12s %s" % (k, v))
    if args.vectors:
        for v in vectors:
            print("ctor_vector", v)


if __name__ == "__main__":
    main()
