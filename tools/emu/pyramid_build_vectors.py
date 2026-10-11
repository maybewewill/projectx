#!/usr/bin/env python3
"""Pyramid builder from the cell grid: differential test against the ORIGINAL Game.dll 1.26a.

Usage:
    GAME_DLL=/path/to/Game.dll EMU_PYLIB=<dir with unicorn and pefile> \
        python3 tools/emu/pyramid_build_vectors.py [--cases 300] \
        [--rs crates/sim/tests/pathing/build_data.rs]

What runs in the original: the full rect rebuild `0x6F488330` (ECX = pathing object G, rect taken
from the cell map C+0x54..0x60, flag 0). It calls the level-A builder `0x6F485150` (2x2 cells, four
per-type steps `0x6F483160` with the masks at `0x6FA84E84..90`) and the upper builder `0x6F4881C0`
three times (`0x6F488010` per type). The level objects are created here with dims ceil(W / 2^k) and
the scale floats 2^-(k+1) at +0x68; the rect is the whole cell grid.

Compared, per case: every record word of levels A, B, C, D (the byte +7 of every record starts with
random bits, so the clearing is checked too). The Python model is the rule of `docs/spec/pathing-build.md`
sections 3.4 and 3.5, written from the disassembly. Any difference is a mismatch (exit status 1).

Only cells without an occupant list are generated (low 24 bits = 0xFFFFFF); the occupant walk of the
cell test is not part of this vector set.

Case generator (mirrored in crates/sim/tests/pathing/build_cases.rs): 64-bit LCG, seed s gives state
(s + 1) * 0x9E3779B97F4A7C15, step state = state * 6364136223846793005 + 1442695040888963407,
u32 = state >> 32. Case i: dimensions FIXED_DIMS[i] for the first cases, else 1 + u32 % 40 for each
side; density percent = DENSITY[u32 % 6]; then for every cell in row-major order: one u32 draw r
(blocked when r % 100 < density) and one u32 draw t (top byte t & 0xFF when blocked, else 0).
The records are then initialised with top bytes drawn the same way (the draws are not used by the
Rust test, which only builds from zero).

Output: a generated block (BUILD_CASES, digests) in the given Rust file. The digest is the
multiply-add digest of the words of levels A, B, C, D in row-major order.
"""

import argparse
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
for _p in os.environ.get('EMU_PYLIB', '').split(os.pathsep):
    if _p:
        sys.path.insert(0, _p)
import emu  # noqa: E402  (maps Game.dll into a Unicorn emulator at import time)
from emu import mu  # noqa: E402

M64 = (1 << 64) - 1
DIGEST_INIT = 0xCBF29CE484222325
DIGEST_MUL = 0x100000001B3
FIXED_DIMS = [(1, 1), (1, 2), (2, 1), (2, 2), (3, 3), (1, 40), (40, 1), (40, 40),
              (17, 9), (9, 17), (33, 21), (7, 7)]
DENSITY = [0, 5, 20, 50, 80, 100]
SCALE_BITS = [0x3F000000, 0x3E800000, 0x3E000000, 0x3D800000]   # 0.5, 0.25, 0.125, 0.0625
TYPES = (0, 2, 4, 6)
MASK_ADDR = {0: 0x6FA84E84, 2: 0x6FA84E88, 4: 0x6FA84E8C, 6: 0x6FA84E90}

# Emulator layout (inside the 64 KiB block that emu.py maps at 0x00200000).
DATA = 0x00200000
G = DATA + 0x0000       # pathing object (needs +0x24C and +0x23C..+0x248)
Q = DATA + 0x0400       # query object (+0x1C = C)
C = DATA + 0x0500       # cell map (+0x28 words, +0x3C width, +0x40 height, +0x54 rect)
CELLS = DATA + 0x1000   # up to 40 x 40 words
LEVEL_OBJ = [DATA + 0x0600, DATA + 0x0700, DATA + 0x0800, DATA + 0x0900]
REC = [DATA + 0x4000, DATA + 0x5000, DATA + 0x6000, DATA + 0x7000]
FN_BUILD = 0x6F488330   # rect rebuild: __thiscall(G), (rect pointer or 0, flag)


class Lcg:
    def __init__(self, seed):
        self.s = ((seed + 1) * 0x9E3779B97F4A7C15) & M64

    def u32(self):
        self.s = (self.s * 6364136223846793005 + 1442695040888963407) & M64
        return self.s >> 32


def dims_of(case_index, rng):
    if case_index < len(FIXED_DIMS):
        w, h = FIXED_DIMS[case_index]
    else:
        w = 1 + rng.u32() % 40
        h = 1 + rng.u32() % 40
    return w, h


def make_case(case_index):
    """Returns (seed, width, height, top bytes row-major). Seed = case index."""
    rng = Lcg(case_index)
    w, h = dims_of(case_index, rng)
    density = DENSITY[rng.u32() % len(DENSITY)]
    tops = []
    for _ in range(w * h):
        r = rng.u32()
        t = rng.u32() & 0xFF
        tops.append(t if r % 100 < density else 0)
    return w, h, tops


def ceil_div(a, b):
    return -(-a // b)


def level_dims(w, h):
    """Dims of levels A, B, C, D: ceil(W / 2^k) for k = 1..4 (LIKELY, see the notes)."""
    out = []
    dw, dh = w, h
    for _ in range(4):
        dw, dh = ceil_div(dw, 2), ceil_div(dh, 2)
        out.append((dw, dh))
    return out


# ---------- Python model (docs/spec/pathing-build.md 3.4 and 3.5) ----------

def cell_blocked(tops, w, h, x, y, mask):
    if not (0 <= x < w and 0 <= y < h):
        return True
    return (tops[y * w + x] << 24) & mask & 0xFF000000 != 0


def set_field(word1, t, val):
    """2-bit state val (1 = 01, 2 = 10) in bits (30 - t, 31 - t)."""
    return (word1 | ((val << 30) >> t)) & 0xFFFFFFFF


def model_level_a(tops, w, h, dw, dh):
    out = [[0] * dw for _ in range(dh)]
    for y in range(dh):
        for x in range(dw):
            word = 0
            for t in TYPES:
                mask = MASK_ADDR_VALUES[t]
                blocked = 0
                for cx, cy in ((2 * x, 2 * y), (2 * x + 1, 2 * y),
                               (2 * x + 1, 2 * y + 1), (2 * x, 2 * y + 1)):
                    if cell_blocked(tops, w, h, cx, cy, mask):
                        blocked += 1
                if blocked == 4:
                    word = set_field(word, t, 1)
                elif blocked > 0:
                    word = set_field(word, t, 2)
            out[y][x] = word
    return out


def child_class(child, cw, ch, t, x, y):
    if not (0 <= x < cw and 0 <= y < ch):
        return 'BAD'
    word = child[y][x]
    st = (word << t) & 0xC0000000
    tag = (word >> 16) & 0xFF
    if st == 0x40000000:
        return 'BAD'
    if st == 0x80000000 or tag != 0:
        return 'BLOCK'
    return 'GOOD'


def model_upper(child, cw, ch, pw, ph):
    out = [[0] * pw for _ in range(ph)]
    for y in range(ph):
        for x in range(pw):
            word = 0
            for t in TYPES:
                cls = [child_class(child, cw, ch, t, 2 * x, 2 * y),
                       child_class(child, cw, ch, t, 2 * x + 1, 2 * y),
                       child_class(child, cw, ch, t, 2 * x + 1, 2 * y + 1),
                       child_class(child, cw, ch, t, 2 * x, 2 * y + 1)]
                if 'BLOCK' in cls:
                    word = set_field(word, t, 2)
                    continue
                bad = cls.count('BAD')
                if bad == 4:
                    word = set_field(word, t, 1)
                elif bad > 0:
                    word = set_field(word, t, 2)
            out[y][x] = word
    return out


MASK_ADDR_VALUES = {}


def read_masks():
    for t, a in MASK_ADDR.items():
        MASK_ADDR_VALUES[t] = struct.unpack('<I', bytes(mu.mem_read(a, 4)))[0]
    return MASK_ADDR_VALUES


# ---------- Emulator side ----------

def run_call(fn, ecx, stack_args):
    """Like emu.call, without the instruction cap (a full 40 x 40 rebuild runs over 200k
    instructions, and emu.call stops there silently)."""
    esp = emu.TOP
    for a in reversed(stack_args):
        esp -= 4
        mu.mem_write(esp, struct.pack('<I', a & 0xFFFFFFFF))
    esp -= 4
    mu.mem_write(esp, struct.pack('<I', emu.SENT))
    mu.reg_write(emu.UC_X86_REG_ESP, esp)
    mu.reg_write(emu.UC_X86_REG_ECX, ecx & 0xFFFFFFFF)
    mu.emu_start(fn, emu.SENT, timeout=600_000_000, count=0)


def wr32(a, v):
    mu.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))


def rd32(a):
    return struct.unpack('<I', bytes(mu.mem_read(a, 4)))[0]


def run_original(w, h, tops, rng):
    """Runs the full rebuild in the emulator. Returns levels A..D as lists of rows of word1."""
    dims = level_dims(w, h)
    # cell words: top byte from the case, low 24 bits = no occupant
    words = [((t << 24) | 0xFFFFFF) & 0xFFFFFFFF for t in tops]
    mu.mem_write(CELLS, b''.join(struct.pack('<I', v) for v in words))
    wr32(C + 0x28, CELLS)
    wr32(C + 0x3C, w)
    wr32(C + 0x40, h)
    # rect of the whole grid at C+0x54..+0x60. The original reads it as (y0, x0, y1, x1): the row
    # loop takes entries 0 and 2, the column loop entries 1 and 3 (checked by the mismatch probe).
    for i, v in enumerate((0, 0, h, w)):
        wr32(C + 0x54 + 4 * i, v)
    wr32(Q + 0x1C, C)
    wr32(G + 0x24C, Q)
    for k in range(4):
        obj = LEVEL_OBJ[k]
        dw, dh = dims[k]
        wr32(obj + 0x28, REC[k])
        wr32(obj + 0x3C, dw)
        wr32(obj + 0x40, dh)
        wr32(obj + 0x68, SCALE_BITS[k])
        wr32(G + 0x23C + 4 * k, obj)
        # random top byte (the builder must clear it), region and tag bits zero
        init = []
        for _ in range(dw * dh):
            init.append(((rng.u32() & 0xFF) << 24) & 0xFFFFFFFF)
        mu.mem_write(REC[k], b''.join(struct.pack('<II', 0, v) for v in init))
    run_call(FN_BUILD, G, (0, 0))
    out = []
    for k in range(4):
        dw, dh = dims[k]
        raw = bytes(mu.mem_read(REC[k], dw * dh * 8))
        words1 = [struct.unpack_from('<I', raw, 8 * i + 4)[0] for i in range(dw * dh)]
        out.append([words1[y * dw:(y + 1) * dw] for y in range(dh)])
    return out


def model_build(w, h, tops):
    dims = level_dims(w, h)
    a = model_level_a(tops, w, h, *dims[0])
    b = model_upper(a, *dims[0], *dims[1])
    c = model_upper(b, *dims[1], *dims[2])
    d = model_upper(c, *dims[2], *dims[3])
    return [a, b, c, d]


def digest(levels):
    h = DIGEST_INIT
    for level in levels:
        for row in level:
            for v in row:
                h = (h * DIGEST_MUL + v) & M64
    return h


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--cases', type=int, default=300)
    ap.add_argument('--rs', help='Rust file whose generated block is replaced')
    a = ap.parse_args()
    read_masks()
    print('masks:', {t: hex(v) for t, v in MASK_ADDR_VALUES.items()})

    mismatches = 0
    digests = []
    records = 0
    for i in range(a.cases):
        w, h, tops = make_case(i)
        rng = Lcg(0x5EED0000 + i)
        got = run_original(w, h, tops, rng)
        exp = model_build(w, h, tops)
        records += sum(len(r) * len(r[0]) for r in exp)
        if got != exp:
            mismatches += 1
            if mismatches <= 5:
                for k, name in enumerate('ABCD'):
                    if got[k] != exp[k]:
                        print('MISMATCH case %d (%dx%d) level %s' % (i, w, h, name))
                        break
        digests.append((i, w, h, digest(exp)))
    print('cases %d, records %d, mismatches %d' % (a.cases, records, mismatches))
    if mismatches:
        sys.exit(1)
    if a.rs:
        splice_rs(a.rs, digests)
        print('wrote %d case digests to %s' % (len(digests), a.rs))


BEGIN = '// BEGIN GENERATED (tools/emu/pyramid_build_vectors.py)'
END = '// END GENERATED'


def splice_rs(path, digests):
    lines = [BEGIN,
             '// Recorded from the original Game.dll 1.26a by tools/emu/pyramid_build_vectors.py.',
             '// (case index, width, height, digest of levels A..D); regenerated by build_cases.rs.',
             'pub const BUILD_CASES: &[(u32, u32, u32, u64)] = &[']
    for i, w, h, dig in digests:
        lines.append('    (%d, %d, %d, 0x%016X),' % (i, w, h, dig))
    lines.append('];')
    lines.append(END)
    block = '\n'.join(lines) + '\n'
    src = open(path).read() if os.path.exists(path) else ''
    if BEGIN in src:
        i = src.index(BEGIN)
        j = src.index(END) + len(END) + 1
        src = src[:i] + block + src[j:]
    else:
        src = src + ('\n' if src else '') + block
    open(path, 'w').write(src)


if __name__ == '__main__':
    main()
