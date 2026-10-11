"""Differential tests of the unit sight path against the original Game.dll (2026-10-11).

Checked here, each against a Python model (docs/spec/vision.md section 11.9):
  * the unit cell centre `0x6f474460` (its argument is the unit's mover, unit+0x164): the centre of the unit's
    footprint rectangle in pathing cells (the rectangle at unit+0x98: y0 +0x1c, x0 +0x20, y1 +0x24, x1 +0x28);
    the fast path returns ((x0+x1)/2, (y0+y1)/2) with the original's truncating halves;
  * the unit sight writer `0x6f409e00` (unit, mask, 0, 0): its cells are the sight circle of radius `unit+0x10C`
    (in fog cells) around the fog cell `(x >> 2, y >> 2)` of that centre, in visible mode (mode 4) for the mask. The
    circle is the one of `0x6f3ba480` (`PyGrid` / `model_circle` from vision_grid_emu.py, CONFIRMED there).

The emulator is the JASS harness of tools/emu/jasscomp (it has the TIB at fs:[0], the Storm and CRT import models and
the start-up initialisers, so the globals the shape builder reads are set). The unit is a real CUnit vtable object
(0x6f931934: slot 0xB8 is `unit+0x164`, slot 0xEC the owner). The handle lookup `0x6f03fa30` is replaced by a stub that
returns the unit; its table lookup is not decoded here. The elevation layer (+0x38) is zero (flat).

Stubs, all of them outside the function under test (the stubbed functions are listed so the results can be read):
  * `0x6f03fa30` (handle to object): returns the unit;
  * `0x6f0128c0` (the unit's elevation level and its flag, which read the terrain cliff data): flat terrain, level 0.

Experiment switches (they change the grid's span list, which the writer reads for radii of 3 and up):
  * `VUNIT_SPANS=1`: grid+0x1C = 2k+1 and grid+0x20 = the span table of radius k (the layout of 0x6f408fd0);
  * `VUNIT_1C=<n>` (default 0): grid+0x1C = n with zero pairs at grid+0x20 (the first runs of 2026-10-11).

Results and their reading are in docs/spec/vision.md section 11.9.

Usage (from the repo root):
    GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn, pefile> python3 tools/emu/vision_unit_emu.py --cases 200
"""
import argparse
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(HERE, "jasscomp"))
import jcomp  # noqa: E402  (Game.dll in Unicorn, Storm/CRT models, start-up initialisers)
import vision_grid_emu as vg  # noqa: E402  (PyGrid and model_circle, transcribed from the decompiled logic)
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_EAX,
    UC_X86_REG_ECX,
    UC_X86_REG_EIP,
    UC_X86_REG_ESP,
)

FN_UNIT_SIGHT = 0x6F409E00
FN_CENTRE = 0x6F474460
FN_LOOKUP = 0x6F03FA30      # handle id (ECX) -> object (EAX); a table lookup, replaced by a stub
FN_LEVEL = 0x6F0128C0       # unit elevation level (path of unit+0x1FC != 0x10); replaced by a flat-terrain stub
CUNIT_VT = 0x6F931934       # CUnit vtable (118 slots); slot 0xB8 = unit+0x164 (0x6f28b0e0)
HANDLE = 0x00100123
SPAN_MODE = os.environ.get("VUNIT_SPANS", "0") == "1"
EXTRA_1C = int(os.environ.get("VUNIT_1C", "0"), 0)   # grid+0x1C for the experiments (0 = as in the first runs)


class Ctx:
    """The harness state: the emulator, the unit, the grid and the output buffer (all in the harness heap)."""

    def __init__(self):
        self.H = jcomp.Harness(path=os.environ["GAME_DLL"])
        self.n_init, self.failed = jcomp.run_crt_init(self.H)
        self.unit = self.H.alloc(0x300, align=32)
        self.blk = self.H.alloc(0x40, align=32)
        self.grid = self.H.alloc(0x100, align=32)
        self.shape_ptrs = self.H.alloc(0x400, align=32)
        self.outp = self.H.alloc(0x10, align=16)
        self.mask_arr = self.H.alloc(0x20000, align=32)
        self.vis_arr = self.H.alloc(0x20000, align=32)
        self.elev = self.H.alloc(0x20000, align=32)
        self.spans = self.H.alloc(0x400, align=32)
        self.H.mu.hook_add(UC_HOOK_CODE, self._lookup_stub, begin=FN_LOOKUP, end=FN_LOOKUP)
        self.H.mu.hook_add(UC_HOOK_CODE, self._level_stub, begin=FN_LEVEL, end=FN_LEVEL)
        self.trace = []
        self.H.mu.hook_add(UC_HOOK_CODE, self._trace_hook, begin=self.H.base, end=self.H.base + 0x00A00000)

    def _trace_hook(self, uc, addr, size, user):
        self.trace.append(addr)
        if len(self.trace) > 64:
            del self.trace[:32]

    def _level_stub(self, uc, addr, size, user):
        # 0x6f0128c0 (thiscall, one stack argument: the out flag, ret 4): returns the unit's elevation level in EAX
        # and writes the flag. It reads the terrain cliff data (TerrainArt\CliffTypes) when it is not stubbed. Flat
        # terrain is assumed: level 0 and flag 0 (see the module docstring and vision.md section 11.9).
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = self.H.rd32(esp)
        out = self.H.rd32(esp + 4)
        self.H.wr32(out, 0)
        uc.reg_write(UC_X86_REG_EAX, 0)
        uc.reg_write(UC_X86_REG_ESP, esp + 8)
        uc.reg_write(UC_X86_REG_EIP, ret)

    def _lookup_stub(self, uc, addr, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = self.H.rd32(esp)
        ecx = uc.reg_read(UC_X86_REG_ECX)
        uc.reg_write(UC_X86_REG_EAX, self.unit if ecx == HANDLE else 0)
        uc.reg_write(UC_X86_REG_ESP, esp + 4)
        uc.reg_write(UC_X86_REG_EIP, ret)

    def build_unit(self, rect, radius):
        """A unit with footprint rectangle rect = (y0, x0, y1, x1) in pathing cells and sight radius `radius`."""
        H = self.H
        H.mu.mem_write(self.unit, b"\0" * 0x300)
        H.mu.mem_write(self.blk, b"\0" * 0x40)
        H.wr32(self.unit, CUNIT_VT)
        H.wr32(self.unit + 0x98, self.blk)
        y0, x0, y1, x1 = rect
        H.wr32(self.blk + 0x1C, y0)
        H.wr32(self.blk + 0x20, x0)
        H.wr32(self.blk + 0x24, y1)
        H.wr32(self.blk + 0x28, x1)
        H.wr32(self.unit + 0x10C, radius)
        H.wr32(self.unit + 0x164 + 8, HANDLE)

    def build_grid(self, w, h, shift, vis, msk, radius=0):
        H = self.H
        stride = 1 << shift
        total = stride * h
        H.mu.mem_write(self.grid, b"\0" * 0x100)
        H.mu.mem_write(self.shape_ptrs, b"\0" * 0x400)
        H.wr32(self.grid + 0x2C, self.mask_arr)
        H.wr32(self.grid + 0x30, self.vis_arr)
        H.wr32(self.grid + 0x38, self.elev)
        H.wr32(self.grid + 0x60, w)
        H.wr32(self.grid + 0x64, stride)
        H.wr32(self.grid + 0x68, shift)
        H.wr32(self.grid + 0x6C, h)
        # shape cache at grid+0x78: count and capacity large enough that the test never grows it
        H.wr32(self.grid + 0x78 + 4, 0x100)
        H.wr32(self.grid + 0x78 + 8, 0x100)
        H.wr32(self.grid + 0x78 + 0xC, self.shape_ptrs)
        # grid+0x1C (a count compared with the radius) and grid+0x20 (a pointer to 8-byte pairs): set by the caller
        # of the writer in the game; the value used here is the one passed to build_grid (EXTRA_1C, EXTRA_20).
        if SPAN_MODE:
            # grid+0x1C = 2R+1 and grid+0x20 = the (lo, hi) span table of radius R (the 0x6f408fd0 format)
            H.wr32(self.grid + 0x1C, 2 * radius + 1 if radius >= 3 else 0)
            for j, (lo, hi) in enumerate(vg.model_span_table(radius) if radius >= 3 else []):
                H.wr32(self.spans + 8 * j, lo)
                H.wr32(self.spans + 8 * j + 4, hi)
            H.wr32(self.grid + 0x20, self.spans)
        else:
            H.wr32(self.grid + 0x1C, EXTRA_1C)
            H.wr32(self.grid + 0x20, self.elev)
        H.mu.mem_write(self.mask_arr, b"".join(struct.pack("<H", v) for v in msk[:total]))
        H.mu.mem_write(self.vis_arr, b"".join(struct.pack("<H", v) for v in vis[:total]))
        H.mu.mem_write(self.elev, b"\0" * (2 * total))

    def run_unit(self, w, h, shift, vis, msk, rect, radius, mask):
        """Runs the unit writer and returns the resulting (vis, msk) lists."""
        total = (1 << shift) * h
        self.build_grid(w, h, shift, vis, msk, radius)
        self.build_unit(rect, radius)
        self.trace.clear()
        try:
            self.H.call(FN_UNIT_SIGHT, ecx=self.grid, stack=(self.unit, mask, 0, 0))
        except Exception as exc:  # report where the original stopped; the harness must not hide a fault
            eip = self.H.mu.reg_read(UC_X86_REG_EIP)
            raise RuntimeError("unit writer w=%d h=%d r=%d mask=%#x rect=%s: %s at eip %#x; last %s" % (
                w, h, radius, mask, rect, exc, eip, [hex(x) for x in self.trace[-10:]]))
        out_vis = list(struct.unpack("<%dH" % total, bytes(self.H.mu.mem_read(self.vis_arr, 2 * total))))
        out_msk = list(struct.unpack("<%dH" % total, bytes(self.H.mu.mem_read(self.mask_arr, 2 * total))))
        return out_vis, out_msk

    def run_centre(self, rect):
        """Runs the centre function on the unit's mover. Returns (x, y) as the original wrote them (pathing cells)."""
        self.build_unit(rect, 0)
        self.H.wr32(self.outp, 0)
        self.H.wr32(self.outp + 4, 0)
        self.H.call(FN_CENTRE, ecx=self.unit + 0x164, stack=(self.outp,))
        return self.signed(self.H.rd32(self.outp)), self.signed(self.H.rd32(self.outp + 4))

    @staticmethod
    def signed(v):
        return v - (1 << 32) if v >= 1 << 31 else v


def model_centre(rect):
    y0, x0, y1, x1 = rect
    return (x0 + x1) // 2, (y0 + y1) // 2


def model_unit(w, h, shift, vis, msk, rect, radius, mask):
    """Expected words: the sight circle of radius `radius` cells around the fog cell of the rectangle centre."""
    cx, cy = model_centre(rect)
    fx, fy = cx >> 2, cy >> 2
    pg = vg.PyGrid(w, h, shift, vis, msk)
    if radius > 0:
        vg.model_circle(pg, 4, mask, fx, fy, radius)
    return pg.vis, pg.msk


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cases", type=int, default=200)
    ap.add_argument("--seed", type=int, default=2026)
    ap.add_argument("--centre-only", action="store_true")
    args = ap.parse_args()
    ctx = Ctx()
    print(f"start-up initialisers run: {ctx.n_init}, failed: {len(ctx.failed)}")
    rng = random.Random(args.seed)
    stats = {"centre": [0, 0], "unit": [0, 0], "unit_cells": [0], "zero_mask": [0, 0]}

    # 1. the centre function alone
    for _ in range(args.cases):
        x0 = rng.randrange(0, 1200)
        x1 = x0 + rng.randrange(0, 12)
        y0 = rng.randrange(0, 1200)
        y1 = y0 + rng.randrange(0, 12)
        rect = (y0, x0, y1, x1)
        got = ctx.run_centre(rect)
        exp = model_centre(rect)
        stats["centre"][0] += 1
        if got != exp:
            stats["centre"][1] += 1
            if stats["centre"][1] <= 3:
                print("centre mismatch rect", rect, "got", got, "expected", exp)
    if args.centre_only:
        print("centre", stats["centre"])
        return

    # 2. the sight writer on random grids, radii 0..12 cells, random masks (some with no player bits)
    for _ in range(args.cases):
        w = rng.randrange(4, 40)
        h = rng.randrange(4, 40)
        shift = max(1, (w - 1).bit_length())
        stride = 1 << shift
        total = stride * h
        vis = [rng.getrandbits(16) for _ in range(total)]
        msk = [rng.getrandbits(16) for _ in range(total)]
        radius = rng.randrange(0, 13)
        if rng.random() < 0.1:
            mask = rng.getrandbits(16) & 0xF000       # no player bits in the low 12 bits: expected no change
        else:
            mask = rng.choice([1 << rng.randrange(12), rng.getrandbits(12) or 1])
        cx = rng.randrange(0, 4 * w + 4)
        cy = rng.randrange(0, 4 * h + 4)
        rect = (cy, cx, cy + rng.randrange(0, 2), cx + rng.randrange(0, 2))
        if (mask & 0xFFF) == 0:
            gv, gm = ctx.run_unit(w, h, shift, vis, msk, rect, radius, mask)
            stats["zero_mask"][0] += 1
            if (gv, gm) != (vis, msk):
                stats["zero_mask"][1] += 1
            continue
        gv, gm = ctx.run_unit(w, h, shift, vis, msk, rect, radius, mask)
        ev, em = model_unit(w, h, shift, vis, msk, rect, radius, mask)
        stats["unit"][0] += 1
        same = gv == ev and gm == em
        stats["unit"][1] += not same
        stats["unit_cells"][0] += sum(1 for i in range(total) if gv[i] != vis[i] or gm[i] != msk[i])
        if not same and stats["unit"][1] <= 3:
            print("unit mismatch w=%d h=%d r=%d mask=%#x rect=%s" % (w, h, radius, mask, rect))
    for k, v in stats.items():
        print("%-12s %s" % (k, v))


if __name__ == "__main__":
    main()
