"""Unicorn check of Raise Dead's autocast comparator CAbilityRaiseDead slot 0x458 = 0x6F0F5950 (docs/spec/abilities-unit-spells.md
section 12.5). The ORIGINAL function runs; the fallback to the shared comparator 0x6F023400 runs too (its slot 0x464 is served
by a fake vtable returning 0, as in tools/emu/unit_spells_emu.py). Candidate and best are fake units whose +0x5C flag word has
bit 8 (the dead flag) set or clear.

Row: dead_c dead_b vc fc(0) vb fb(0) t result(1 = candidate wins, 0 = best kept). The Rust test
(crates/sim/tests/raise_dead_vectors.rs) recomputes each row with `unit_spells::raise_dead_prefers`.

Usage: GAME_DLL=/path/Game.dll py tools/emu/raise_dead_cmp_emu.py CASES SEED OUT.txt
"""
import os, sys, random, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu
from unicorn.x86_const import *
mu = emu.mu
DATA = emu.DATA
F_CMP = 0x6F0F5950
OBJ, VT, STUBC = DATA + 0x200, DATA + 0x300, DATA + 0x2000
CAND, BEST = DATA + 0x1000, DATA + 0x1100
VC, VB, TT = DATA + 0x100, DATA + 0x104, DATA + 0x108
mu.mem_write(STUBC, bytes([0x31, 0xC0, 0xC3]))
mu.mem_write(OBJ, struct.pack('<I', VT))
mu.mem_write(VT + 0x464, struct.pack('<I', STUBC))
r = random.Random(int(sys.argv[2]) if len(sys.argv) > 2 else 1)
SP = [0, 0x80000000, 0x7F800000, 0xFF800000, 0x7FC00000, 0x3F800000, 0x42C80000, 0x00000001]


def fb():
    k = r.random()
    if k < 0.15:
        return r.choice(SP)
    return struct.unpack('<I', struct.pack('<f', r.choice([float(r.randint(0, 20)), r.uniform(0, 1e5), r.choice([100.0, 90000.0])])))[0]


rows = []
for _ in range(int(sys.argv[1])):
    dc, db = r.random() < 0.5, r.random() < 0.5
    flags_c = (r.getrandbits(32) & ~0x100) | (0x100 if dc else 0)
    flags_b = (r.getrandbits(32) & ~0x100) | (0x100 if db else 0)
    mu.mem_write(CAND + 0x5C, struct.pack('<I', flags_c))
    mu.mem_write(BEST + 0x5C, struct.pack('<I', flags_b))
    vc, vb, t = fb(), fb(), fb()
    if r.random() < 0.25:
        vb = vc
    if r.random() < 0.2:
        t = vc
    mu.mem_write(VC, struct.pack('<I', vc))
    mu.mem_write(VB, struct.pack('<I', vb))
    mu.mem_write(TT, struct.pack('<I', t))
    res = emu.call(F_CMP, ecx=OBJ, stack_args=(CAND, VC, BEST, VB, TT))
    rows.append((int(dc), int(db), vc, vb, t, 1 if res == CAND else (0 if res == BEST else 9)))
with open(sys.argv[3], 'w') as fh:
    for row in rows:
        fh.write('%d %d %08x %08x %08x %d\n' % row)
print(len(rows), 'rows;', sum(1 for x in rows if x[5] == 9), 'bad;', sum(x[5] for x in rows if x[5] == 1), 'cand wins')
