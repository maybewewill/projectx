"""Differential tests of the fog-of-war grid writers and state table against the original Game.dll.

Checked (2026-10-11): the state table `0x6f00e7a0`, the three per-row span writers `0x6f406920`
(mode 1 = masked), `0x6f4069f0` (mode 2 = fogged), `0x6f406850` (mode 4 = visible), the single-cell writer
`0x6f3b0e20` and the rectangle writer `0x6f3b0e90`. Each case builds a grid object in emulated memory,
calls the original, and compares every cell of both layers with the Python model below.

Usage (from the repo root):
    GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn, pefile> python3 tools/emu/vision_emu.py --cases 2000
"""
import argparse, os, random, struct, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402  (loads Game.dll into Unicorn)

GRID = 0x00208000      # grid object (0x80 bytes), inside the emu DATA map
VIS = 0x00208200       # +0x30 array (ushort, index = row << shift | col)
MSK = 0x00208600       # +0x2c array
SHIFT = 4              # rows are 16 cells apart in memory; widths/heights up to 16


def write_grid(width, height, a, b, vis, msk):
    emu.mu.mem_write(GRID, b'\0' * 0x80)
    emu.mu.mem_write(GRID + 0x10, struct.pack('<II', a, b))
    emu.mu.mem_write(GRID + 0x2C, struct.pack('<I', MSK))
    emu.mu.mem_write(GRID + 0x30, struct.pack('<I', VIS))
    emu.mu.mem_write(GRID + 0x60, struct.pack('<I', width))
    emu.mu.mem_write(GRID + 0x64, struct.pack('<I', 1 << SHIFT))   # row pitch used by the rectangle writer
    emu.mu.mem_write(GRID + 0x68, struct.pack('<I', SHIFT))
    emu.mu.mem_write(GRID + 0x6C, struct.pack('<I', height))
    for r in range(16):
        for c in range(16):
            i = (r << SHIFT) + c
            emu.mu.mem_write(VIS + 2 * i, struct.pack('<H', vis[r][c]))
            emu.mu.mem_write(MSK + 2 * i, struct.pack('<H', msk[r][c]))


def read_grid():
    vis = [[0] * 16 for _ in range(16)]
    msk = [[0] * 16 for _ in range(16)]
    for r in range(16):
        for c in range(16):
            i = (r << SHIFT) + c
            vis[r][c] = struct.unpack('<H', bytes(emu.mu.mem_read(VIS + 2 * i, 2)))[0]
            msk[r][c] = struct.unpack('<H', bytes(emu.mu.mem_read(MSK + 2 * i, 2)))[0]
    return vis, msk


def model_state(a, b, vis_word, msk_word, mask):
    """Table of 0x6f00e7a0: row = a + 2b, then the first test that holds."""
    table = [(4, 4, 4), (4, 4, 1), (4, 2, 2), (4, 2, 1)]
    row = table[a + 2 * b]
    if vis_word & mask:
        return row[0]
    if mask & ~msk_word & 0xFFFF:
        return row[1]
    return row[2]


def apply_cell(mode, vis, msk, r, c, mask):
    # The arrays are flat in the original (index = row << 4 | col); a column past 15 wraps to the next row.
    r, c = divmod((r << SHIFT) + c, 1 << SHIFT)
    if mode == 1:      # masked: set the mask bit, clear the visible bit
        msk[r][c] |= mask; vis[r][c] &= ~mask & 0xFFFF
    elif mode == 2:    # fogged: clear both
        msk[r][c] &= ~mask & 0xFFFF; vis[r][c] &= ~mask & 0xFFFF
    elif mode == 4:    # visible: clear the mask bit, set the visible bit
        msk[r][c] &= ~mask & 0xFFFF; vis[r][c] |= mask


def span_cells(x0, x1):
    """Cells written by the per-row writer 0x6f406920 / 0x6f4069f0 / 0x6f406850 (x0 >= 0, x1 <= width-1).

    The writer works on 16-bit cells in pairs (one dword store per pair). It stores the start cell
    alone when it is odd, then pairs from the next even cell up to x1 - 1 (or x1 when x1 is odd), then
    stores the cell after the pairs alone when x1 is even. When x0 > x1 the odd start and the tail still
    store, so up to two cells outside the span are written (CONFIRMED by 200 random cases, see the log).
    """
    cells = []
    u = x0 & 1
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
    return cells


def model_span(mode, width, height, vis, msk, row, x0, x1, mask):
    """Per-row writer: nothing if the row is outside; clamp the start to 0 and the end to width-1."""
    if not (0 <= row < height):
        return
    x0 = max(x0, 0)
    x1 = min(x1, width - 1)
    for c in span_cells(x0, x1):
        apply_cell(mode, vis, msk, row, c, mask)


def model_rect(mode, width, height, vis, msk, x0, y0, x1, y1, mask):
    """Rectangle writer 0x6f3b0e90: clamp, then the loops run over [x0, x1) x [y0, y1) (end exclusive)."""
    x0 = max(x0, 0); y0 = max(y0, 0)
    x1 = min(x1, width - 1); y1 = min(y1, height - 1)
    for r in range(y0, y1):
        for c in range(x0, x1):
            apply_cell(mode, vis, msk, r, c, mask)


def rnd_grid(rng):
    return [[rng.getrandbits(16) for _ in range(16)] for _ in range(16)]


def copy(g):
    return [r[:] for r in g]


def run(cases, seed):
    rng = random.Random(seed)
    stats = {'table': [0, 0], 'span': [0, 0], 'cell': [0, 0], 'rect': [0, 0]}

    # 1. State table 0x6f00e7a0 (thiscall: ECX = grid, stack: vis word, msk word, mask).
    for a in (0, 1):
        for b in (0, 1):
            emu.mu.mem_write(GRID + 0x10, struct.pack('<II', a, b))
            for _ in range(max(1, cases // 4)):
                vw = rng.getrandbits(16) | 0xF000
                mw = rng.getrandbits(12)
                mask = 1 << rng.randrange(12)
                if rng.random() < 0.3:
                    mask = rng.getrandbits(12) or 1
                got = emu.call(0x6F00E7A0, ecx=GRID, stack_args=(vw, mw, mask))
                stats['table'][0] += 1
                stats['table'][1] += (got & 0xFFFFFFFF) != model_state(a, b, vw, mw, mask)

    # 2. Span writers, single-cell writer and rectangle writer on random grids.
    fns = {1: 0x6F406920, 2: 0x6F4069F0, 4: 0x6F406850}
    for _ in range(cases):
        width = rng.randrange(1, 17)
        height = rng.randrange(1, 17)
        vis = rnd_grid(rng)
        msk = rnd_grid(rng)
        mode = rng.choice([1, 2, 4])
        mask = rng.choice([1 << rng.randrange(12), rng.getrandbits(12) or 1]) & 0xFFFF

        write_grid(width, height, 0, 1, vis, msk)
        row = rng.randrange(-1, 17)
        x0 = rng.randrange(-3, 18)
        x1 = rng.randrange(x0, 20)
        emu.call(fns[mode], ecx=GRID, stack_args=(row, x0, x1, mask))
        gv, gm = read_grid()
        mv, mm = copy(vis), copy(msk)
        model_span(mode, width, height, mv, mm, row, x0, x1, mask)
        stats['span'][0] += 1
        stats['span'][1] += (gv != mv or gm != mm)

        write_grid(width, height, 0, 1, vis, msk)
        x = rng.randrange(0, 16); y = rng.randrange(0, 16)
        emu.call(0x6F3B0E20, ecx=GRID, stack_args=(mode, mask, x, y))
        gv, gm = read_grid()
        mv, mm = copy(vis), copy(msk)
        apply_cell(mode, mv, mm, y, x, mask)
        stats['cell'][0] += 1
        stats['cell'][1] += (gv != mv or gm != mm)

        write_grid(width, height, 0, 1, vis, msk)
        x0 = rng.randrange(-2, 18); y0 = rng.randrange(-2, 18)
        x1 = rng.randrange(0, 18); y1 = rng.randrange(0, 18)
        emu.call(0x6F3B0E90, ecx=GRID, stack_args=(mode, mask, x0, y0, x1, y1))
        gv, gm = read_grid()
        mv, mm = copy(vis), copy(msk)
        model_rect(mode, width, height, mv, mm, x0, y0, x1, y1, mask)
        stats['rect'][0] += 1
        stats['rect'][1] += (gv != mv or gm != mm)
    return stats


if __name__ == '__main__':
    ap = argparse.ArgumentParser()
    ap.add_argument('--cases', type=int, default=400)
    ap.add_argument('--seed', type=int, default=1)
    args = ap.parse_args()
    st = run(args.cases, args.seed)
    for k, (n, bad) in st.items():
        print('%-6s cases=%d mismatches=%d' % (k, n, bad))
