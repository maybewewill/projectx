"""Unicorn check of Heal's autocast comparator 0x6F0DADD0 (docs/spec/abilities-unit-spells.md).

Runs the ORIGINAL function from the user's Game.dll; the only stub is the life-fraction helper 0x6F020640
(replaced by a table lookup so the object model is not needed). Rows: candidate V, candidate fraction, best V, best
fraction, threshold T (float bits, hex) and 1 when the candidate wins, 0 when the best is kept.

Usage: GAME_DLL=/path/Game.dll py tools/emu/unit_spells_emu.py CASES SEED OUT.txt [heal|default]

Mode `default` runs the shared comparator 0x6F023400 of CAbilityAutoTargetSpell (used by Inner Fire, Bloodlust and
the other autocast spells) with its slot 0x464 (always 0 for them) served by a fake vtable.
"""
import os, sys, random, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
mu = emu.mu
DATA = emu.DATA
MODE = sys.argv[4] if len(sys.argv) > 4 else 'heal'
F_CMP = 0x6F0DADD0 if MODE == 'heal' else 0x6F023400
F_FRAC = 0x6F020640
CAND, BEST = 0x1111, 0x2222
VC, VB, TT = DATA + 0x100, DATA + 0x104, DATA + 0x108
fr = {}
OBJ, VT, STUBC = DATA + 0x200, DATA + 0x300, DATA + 0x2000
mu.mem_write(STUBC, bytes([0x31, 0xC0, 0xC3]))
mu.mem_write(OBJ, struct.pack('<I', VT))
mu.mem_write(VT + 0x464, struct.pack('<I', STUBC))
def hook(uc, addr, size, user):
    ecx = uc.reg_read(UC_X86_REG_ECX)
    esp = uc.reg_read(UC_X86_REG_ESP)
    out = struct.unpack('<I', bytes(uc.mem_read(esp + 4, 4)))[0]
    uc.mem_write(out, struct.pack('<I', fr[ecx]))
    ret = struct.unpack('<I', bytes(uc.mem_read(esp, 4)))[0]
    uc.reg_write(UC_X86_REG_ESP, esp + 8)
    uc.reg_write(UC_X86_REG_EIP, ret)
mu.hook_add(UC_HOOK_CODE, hook, begin=F_FRAC, end=F_FRAC)
r = random.Random(int(sys.argv[2]) if len(sys.argv) > 2 else 1)
SP = [0, 0x80000000, 0x7F800000, 0xFF800000, 0x7FC00000, 0x3F800000, 0x42C80000, 0x00000001]
def fb():
    k = r.random()
    if k < 0.15: return r.choice(SP)
    if k < 0.7: return struct.unpack('<I', struct.pack('<f', r.choice([float(r.randint(0, 20)), r.uniform(0, 1), r.uniform(0, 1e5), r.choice([100.0, 90000.0])])))[0]
    return r.getrandbits(32)
rows = []
for _ in range(int(sys.argv[1])):
    vc, vb, t, fc, fbb = fb(), fb(), fb(), fb(), fb()
    if r.random() < 0.25: vb = vc
    if r.random() < 0.25: fbb = fc
    if r.random() < 0.2: t = vc
    mu.mem_write(VC, struct.pack('<I', vc)); mu.mem_write(VB, struct.pack('<I', vb)); mu.mem_write(TT, struct.pack('<I', t))
    fr[CAND] = fc; fr[BEST] = fbb
    res = emu.call(F_CMP, ecx=OBJ if MODE != 'heal' else 0x7777, stack_args=(CAND, VC, BEST, VB, TT))
    rows.append((vc, fc, vb, fbb, t, 1 if res == CAND else (0 if res == BEST else 9)))
out = sys.argv[3]
with open(out, 'w') as fh:
    for row in rows:
        fh.write(' '.join('%08x' % x for x in row[:5]) + ' %d\n' % row[5])
print(len(rows), 'rows;', sum(1 for x in rows if x[5] == 9), 'bad;', sum(x[5] for x in rows if x[5] == 1), 'cand wins')
