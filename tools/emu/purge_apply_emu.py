"""Unicorn run of CBuffPurge's apply slot 0x354 = 0x6F0EF400 (docs/spec/abilities-unit-spells.md section 12.2).

The original apply code runs; the buff object is built by hand (the six float holders with the real holder vtable
0x6F876D38, ctor values read from the constructor 0x6F0FE700), the target unit has no mover or attack object (so the
sinks are skipped), and the callees that need the object model are recording stubs: the timer start 0x6F4778F0, the unit
reference assign 0x6F0419C0, the hero test 0x6F1D2660 and the buff's slot 0x320. Prints the holder values and the timer
starts, from which the slow schedule is read (the timeouts are soft divisions, bit exact).

Usage: GAME_DLL=/path/Game.dll py tools/emu/purge_apply_emu.py [DataA DataB DataD DataE hero]
"""
import os, sys, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
mu = emu.mu
DATA = emu.DATA
F_APPLY = 0x6F0EF400
B = DATA + 0x2000
VT = DATA + 0x3000
STUB320 = DATA + 0x3400
TARGET = DATA + 0x4000
CREATOR = DATA + 0x4400
CELLS = DATA + 0x5000
HOLDER_VT = 0x6F876D38
rec = []


def rd(a):
    return struct.unpack('<I', bytes(mu.mem_read(a, 4)))[0]


def fl(x):
    return struct.unpack('<I', struct.pack('<f', x))[0]


def hook(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    back = rd(esp)
    ecx = uc.reg_read(UC_X86_REG_ECX)
    arg = lambda i: rd(esp + 4 + 4 * i)
    pop = 0
    val = 0
    if user == 'timer':
        # ecx = holder object, args: (&timeout, message, owner, 1, 0)
        rec.append('timer start holder=+%#x timeout=%08x msg=%x a3=%x a4=%x a5=%x' % (ecx - B, rd(arg(0)), arg(1), arg(2), arg(3), arg(4)))
        pop = 0x14
    elif user == 'ref':
        rec.append('ref assign +%#x <- %x' % (ecx - B, arg(0)))
        pop = 4
    elif user == 'hero':
        val = HERO[0]
        pop = 0
    elif user == 'v320':
        rec.append('vf320 target=%x dur=%08x flag=%x' % (arg(0), rd(arg(1)), arg(2)))
        pop = 0xC
    uc.reg_write(UC_X86_REG_EAX, val)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
    uc.reg_write(UC_X86_REG_EIP, back)


HERO = [0]
for nm, a in (('timer', 0x6F4778F0), ('ref', 0x6F0419C0), ('hero', 0x6F1D2660)):
    mu.hook_add(UC_HOOK_CODE, hook, begin=a, end=a, user_data=nm)
mu.hook_add(UC_HOOK_CODE, hook, begin=STUB320, end=STUB320, user_data='v320')
mu.mem_write(VT + 0x320, struct.pack('<I', STUB320))
mu.mem_write(B, struct.pack('<I', VT))
for off in (0xD8, 0xE0, 0xE8, 0xF0, 0xF8, 0x100):
    mu.mem_write(B + off, struct.pack('<II', HOLDER_VT, 0))
mu.mem_write(B + 0xCC, struct.pack('<II', 0xFFFFFFFF, 0xFFFFFFFF))
mu.mem_write(B + 0x30, struct.pack('<I', CREATOR))
mu.mem_write(TARGET + 0x1E8, struct.pack('<I', 0))
mu.mem_write(TARGET + 0x1EC, struct.pack('<I', 0))
mu.mem_write(CREATOR + 0x1E8, struct.pack('<I', 0))
mu.mem_write(CREATOR + 0x1EC, struct.pack('<I', 0))
vals = [float(x) for x in sys.argv[1:5]] if len(sys.argv) >= 5 else [5.0, 0.0, 0.0, 0.0]
HERO[0] = int(sys.argv[5]) if len(sys.argv) > 5 else 0
dur = float(os.environ.get("DUR", "15"))
# arguments: target, &Dur, creator, &DataA, &DataB, &DataD, &DataE (pointers to float cells)
cells = [dur] + vals
for i, v in enumerate(cells):
    mu.mem_write(CELLS + 4 * i, struct.pack('<I', fl(v)))
args = (TARGET, CELLS, CREATOR, CELLS + 4, CELLS + 8, CELLS + 12, CELLS + 16)
emu.call(F_APPLY, ecx=B, stack_args=args)
for line in rec:
    print(line)
names = {0xD8: 'flagA', 0xE0: 'flagB', 0xE8: 'stepA', 0xF0: 'stepB', 0xF8: 'pauseD', 0x100: 'pauseE'}
for off, nm in names.items():
    print('holder +%#x (%s) value %08x = %g' % (off, nm, rd(B + off + 4), struct.unpack('<f', struct.pack('<I', rd(B + off + 4)))[0]))
