"""Diagnostic for vision_unit_emu: prints the cells the unit writer covers versus the circle model, for one case.

Usage: GAME_DLL=<Game.dll> python3 tools/emu/vision_unit_diag.py <w> <h> <radius> <mask-hex> <y0> <x0> <y1> <x1>
The arrays start clear (visible 0, mask 0xFFFF), so a covered cell has its visible bit set.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import vision_unit_emu as vu  # noqa: E402
import vision_grid_emu as vg  # noqa: E402


def cells_of(words, w, h, shift, bit):
    out = set()
    for r in range(h):
        for c in range(w):
            if words[(r << shift) + c] & bit:
                out.add((c, r))
    return out


def main():
    w, h, radius = int(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3])
    mask = int(sys.argv[4], 16)
    y0, x0, y1, x1 = (int(v) for v in sys.argv[5:9])
    shift = max(1, (w - 1).bit_length())
    total = (1 << shift) * h
    vis = [0] * total
    msk = [0xFFFF] * total
    ctx = vu.Ctx()
    gv, gm = ctx.run_unit(w, h, shift, vis, msk, (y0, x0, y1, x1), radius, mask)
    ev, em = vu.model_unit(w, h, shift, vis, msk, (y0, x0, y1, x1), radius, mask)
    got = cells_of(gv, w, h, shift, mask)
    exp = cells_of(ev, w, h, shift, mask)
    print("centre (pathing)", vu.model_centre((y0, x0, y1, x1)), "fog", tuple(v >> 2 for v in vu.model_centre((y0, x0, y1, x1))))
    print("original covers", len(got), "cells; model covers", len(exp))
    print("only original:", sorted(got - exp)[:20])
    print("only model:   ", sorted(exp - got)[:20])
    fx, fy = vu.model_centre((y0, x0, y1, x1))[0] >> 2, vu.model_centre((y0, x0, y1, x1))[1] >> 2
    show(got, exp, fx, fy, radius)


def show(got, exp, cx, cy, r):
    """Prints the window [cx-r-1, cx+r+1] x [cy-r-1, cy+r+1]: '#' both, 'o' original only, 'm' model only."""
    for y in range(cy - r - 1, cy + r + 2):
        row = []
        for x in range(cx - r - 1, cx + r + 2):
            g, e = (x, y) in got, (x, y) in exp
            row.append("#" if g and e else "o" if g else "m" if e else ".")
        print("%4d %s" % (y, "".join(row)))


if __name__ == "__main__":
    main()
