"""Row experiment of the unit sight writer `0x6f409e00` (k >= 3 span path), docs/spec/vision.md section 13.5.

The writer is driven on a flat 64 x 64 grid with a centre at fog cell (32, 32), flag 0 (the span path). The span list
that the writer reads (grid+0x1C = count, grid+0x20 = (lo, hi) pairs per row, relative to the centre column) is set by
this script, so the output shows how the writer uses a given list. It is NOT the game's producer of that list.

Usage (from the repo root):
    GAME_DLL=/path/to/Game.dll python3 tools/emu/vision_unit_rows_emu.py --k 3 --count 7
    GAME_DLL=/path/to/Game.dll python3 tools/emu/vision_unit_rows_emu.py --k 3 --count 7 --wide 50

With --wide the pairs are (-W, W) on every row (the grid edge is the only clip); otherwise pair i is (-(i+1), i+1).
"""
import argparse
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(HERE, "jasscomp"))
import vision_unit_emu as vu  # noqa: E402  (the harness, the unit object and the stubs of section 11.9)

SIZE = 64
SHIFT = 6
CX = CY = 32


def run(ctx, k, count, pairs, mode_mask=1):
    total = (1 << SHIFT) * SIZE
    vis = [0] * total
    msk = [0xFFFF] * total
    ctx.build_grid(SIZE, SIZE, SHIFT, vis, msk, radius=k)
    H = ctx.H
    H.wr32(ctx.grid + 0x1C, count)
    for j, (lo, hi) in enumerate(pairs):
        H.wr32(ctx.spans + 8 * j, lo & 0xFFFFFFFF)
        H.wr32(ctx.spans + 8 * j + 4, hi & 0xFFFFFFFF)
    H.wr32(ctx.grid + 0x20, ctx.spans)
    rect = (CY * 4, CX * 4, CY * 4, CX * 4)   # centre pathing cell (128, 128) = fog cell (32, 32)
    ctx.build_unit(rect, k)
    H.mu.mem_write(ctx.mask_arr, b"".join(struct.pack("<H", v) for v in msk))
    H.mu.mem_write(ctx.vis_arr, b"".join(struct.pack("<H", v) for v in vis))
    H.call(vu.FN_UNIT_SIGHT, ecx=ctx.grid, stack=(ctx.unit, mode_mask, 0, 0))
    out = struct.unpack("<%dH" % total, bytes(H.mu.mem_read(ctx.vis_arr, 2 * total)))
    cells = {}
    for r in range(SIZE):
        for c in range(SIZE):
            if out[(r << SHIFT) + c] & mode_mask:
                cells.setdefault(r - CY, []).append(c - CX)
    return cells


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--k", type=int, default=3)
    ap.add_argument("--count", type=int, default=7)
    ap.add_argument("--wide", type=int, default=0)
    args = ap.parse_args()
    if args.wide:
        pairs = [(-args.wide, args.wide)] * args.count
    else:
        pairs = [(-(i + 1), i + 1) for i in range(args.count)]
    ctx = vu.Ctx()
    cells = run(ctx, args.k, args.count, pairs)
    print("k=%d count=%d pairs=%s" % (args.k, args.count, "wide %d" % args.wide if args.wide else "(-(i+1), i+1)"))
    for dy in sorted(cells):
        xs = cells[dy]
        print("dy %3d  x %4d..%-4d n=%d" % (dy, min(xs), max(xs), len(xs)))


if __name__ == "__main__":
    main()
