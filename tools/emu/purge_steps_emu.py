"""Unicorn run of CBuffPurge's decay: the apply slot 0x6F0EF400 and then the movement update handler 0x6F0D9EE0 (message
0xD01C1) and the attack update handler 0x6F0D9F80 (message 0xD01C3), N times each (docs/spec/abilities-unit-spells.md
section 12.2). The original code runs; the movement sink 0x6F202FF0 and the attack speed sink 0x6F0C9090 are recording
stubs. Output rows: DataA bits, DataB bits, Dur bits, then the timeouts of the two timers and, for each step, the value
the movement sink received (the initial one first) and the attack sink values.

Usage: GAME_DLL=/path/Game.dll py tools/emu/purge_steps_emu.py CASES SEED OUT.txt
Row: dataA dataB dur | moveTimeout attackTimeout | move sink values (initial + 12 updates) | attack sink values (same)
"""
import os, sys, struct, random
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
mu = emu.mu
DATA = emu.DATA
F_APPLY, F_MOVE, F_ATTACK = 0x6F0EF400, 0x6F0D9EE0, 0x6F0D9F80
B, VT, TARGET, CREATOR, CELLS = DATA + 0x2000, DATA + 0x3000, DATA + 0x4000, DATA + 0x4400, DATA + 0x5000
MOVER, ATTACK = DATA + 0x6000, DATA + 0x6400
HOLDER_VT = 0x6F876D38
move, att, timers = [], [], []


def rd(a):
    return struct.unpack('<I', bytes(mu.mem_read(a, 4)))[0]


def hook(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    back = rd(esp)
    arg = lambda i: rd(esp + 4 + 4 * i)
    pop = 0
    if user == 'timer':
        timers.append(rd(arg(0))); pop = 0x14
    elif user == 'ref':
        pop = 4
    elif user == 'move':
        move.append(rd(arg(0))); pop = 4
    elif user == 'att':
        att.append(rd(arg(0))); pop = 8
    elif user == 'v320':
        pop = 0xC
    uc.reg_write(UC_X86_REG_EAX, 0)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
    uc.reg_write(UC_X86_REG_EIP, back)


STUB320 = DATA + 0x3400
for nm, a in (('timer', 0x6F4778F0), ('ref', 0x6F0419C0), ('hero', 0x6F1D2660), ('move', 0x6F202FF0), ('att', 0x6F0C9090), ('v320', STUB320)):
    mu.hook_add(UC_HOOK_CODE, hook, begin=a, end=a, user_data=nm)
mu.mem_write(VT + 0x320, struct.pack('<I', STUB320))
fl = lambda x: struct.unpack('<I', struct.pack('<f', x))[0]
r = random.Random(int(sys.argv[2]) if len(sys.argv) > 2 else 1)
rows = []
for _ in range(int(sys.argv[1])):
    move.clear(); att.clear(); timers.clear()
    mu.mem_write(B, struct.pack('<I', VT))
    for off in (0xD8, 0xE0, 0xE8, 0xF0, 0xF8, 0x100):
        mu.mem_write(B + off, struct.pack('<II', HOLDER_VT, 0))
    mu.mem_write(B + 0xCC, struct.pack('<II', 0xFFFFFFFF, 0xFFFFFFFF))
    mu.mem_write(B + 0x30, struct.pack('<I', TARGET))
    mu.mem_write(TARGET + 0x1EC, struct.pack('<I', MOVER))
    mu.mem_write(TARGET + 0x1E8, struct.pack('<I', ATTACK))
    a = r.choice([5.0, 4.0, 2.0, 1.0, 3.0, 10.0, 7.0, 0.5, 1.5, r.uniform(0.3, 12.0), 0.0])
    b = r.choice([0.0, 1.0, 5.0, r.uniform(0.3, 12.0)])
    dur = r.choice([15.0, 5.0, 10.0, 60.0, r.uniform(1.0, 30.0)])
    for i, v in enumerate([dur, a, b, 0.0, 0.0]):
        mu.mem_write(CELLS + 4 * i, struct.pack('<I', fl(v)))
    emu.call(F_APPLY, ecx=B, stack_args=(TARGET, CELLS, CREATOR, CELLS + 4, CELLS + 8, CELLS + 12, CELLS + 16))
    for _k in range(12):
        emu.call(F_MOVE, ecx=B)
        emu.call(F_ATTACK, ecx=B)
    rows.append((fl(a), fl(b), fl(dur), list(timers), list(move), list(att)))
with open(sys.argv[3], 'w') as fh:
    for a, b, d, tm, mv, at in rows:
        fh.write('%08x %08x %08x | %s | %s | %s\n' % (a, b, d, ' '.join('%08x' % x for x in tm), ' '.join('%08x' % x for x in mv), ' '.join('%08x' % x for x in at)))
print(len(rows), 'rows')
