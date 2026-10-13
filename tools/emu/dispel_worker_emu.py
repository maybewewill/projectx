"""Unicorn check of the per-unit dispel worker 0x6F0EEF00 of Dispel Magic / Purge / Abolish Magic
(docs/spec/abilities-unit-spells.md section 12.1).

The ORIGINAL worker runs from the user's Game.dll; every callee that needs the object model is replaced by a recording
stub (the relation test 0x6F285B20, the buff finder 0x6F0787D0, the target's virtual slots 0x11C and 0x120, the target
legality 0x6F28D750, the Misc integer reader 0x6F009F90, the damage record constructor 0x6F2556E0, the buff remover
0x6F079FE0, the mana loss 0x6F283DB0 and the ability notification 0x6F052EC0). Each output row is the input description
followed by the ordered list of calls the worker made (the decisions are the control flow), and its return value.

Usage: GAME_DLL=/path/Game.dll py tools/emu/dispel_worker_emu.py CASES SEED OUT.txt
Row: space separated hex inputs, then ' | ' and the call list joined by ';'.
"""
import os, sys, random, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
mu = emu.mu
DATA = emu.DATA
F_WORKER = 0x6F0EEF00
STUBS = {
    # name: (address, callee-popped stack bytes)
    'rel': (0x6F285B20, 4),
    'find': (0x6F0787D0, 0x14),
    'legal': (0x6F28D750, 8),
    'misc': (0x6F009F90, 4),
    'rec': (0x6F2556E0, 0x14),
    'buffs': (0x6F079FE0, 0x24),
    'mana': (0x6F283DB0, 0xC),
    'notify': (0x6F052EC0, 4),
}
T = DATA + 0x800
VT = DATA + 0x1000
S11C, S120 = DATA + 0x2000, DATA + 0x2010
F8 = DATA + 0x1800      # float cells (a8)
F9 = DATA + 0x1900      # float cells (a9)
calls = []
ret = {}


def rd(a, n=4):
    return struct.unpack('<I', bytes(mu.mem_read(a, 4)))[0]


def hook(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    back = rd(esp)
    ecx = uc.reg_read(UC_X86_REG_ECX)
    edx = uc.reg_read(UC_X86_REG_EDX)
    arg = lambda i: rd(esp + 4 + 4 * i)
    name = user
    pop = 0
    val = 0
    if name == 'rel':
        calls.append('rel:%x' % arg(0)); pop = 4; val = ret['rel']
    elif name == 'find':
        calls.append('find:%x:%x,%x,%x,%x' % (arg(0), arg(1), arg(2), arg(3), arg(4))); pop = 0x14; val = ret['find']
    elif name == 'v11c':
        calls.append('v11c'); pop = 0; val = ret['v11c']
    elif name == 'legal':
        calls.append('legal:%x,%x' % (arg(0), arg(1))); pop = 8; val = ret['legal']
    elif name == 'misc':
        s1 = bytes(mu.mem_read(ecx, 8)).split(b'\0')[0].decode()
        s2 = bytes(mu.mem_read(edx, 32)).split(b'\0')[0].decode()
        calls.append('misc:%s.%s:%x' % (s1, s2, arg(0))); pop = 4; val = ret['misc']
    elif name == 'rec':
        calls.append('rec:%08x,%x,%x,%x,%x' % (rd(arg(0)), arg(1), arg(2), arg(3), arg(4))); pop = 0x14; val = ecx
    elif name == 'v120':
        calls.append('apply'); pop = 0x10; val = 0
    elif name == 'buffs':
        calls.append('buffs:' + ','.join('%x' % arg(i) for i in range(9))); pop = 0x24; val = 0
    elif name == 'mana':
        calls.append('mana:%08x,%x' % (rd(arg(1)), arg(2))); pop = 0xC; val = 0
    elif name == 'notify':
        calls.append('notify:%x:%x' % (ecx, arg(0))); pop = 4; val = 0
    uc.reg_write(UC_X86_REG_EAX, val)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
    uc.reg_write(UC_X86_REG_EIP, back)


for nm, (addr, _) in STUBS.items():
    mu.hook_add(UC_HOOK_CODE, hook, begin=addr, end=addr, user_data=nm)
mu.hook_add(UC_HOOK_CODE, hook, begin=S11C, end=S11C, user_data='v11c')
mu.hook_add(UC_HOOK_CODE, hook, begin=S120, end=S120, user_data='v120')
mu.mem_write(VT + 0x11C, struct.pack('<I', S11C))
mu.mem_write(VT + 0x120, struct.pack('<I', S120))
mu.mem_write(T, struct.pack('<I', VT))

r = random.Random(int(sys.argv[2]) if len(sys.argv) > 2 else 1)
FLOATS = [0x00000000, 0x80000000, 0x43480000, 0x3F800000, 0x7FC00000, 0x42C80000, 0xC1200000, 0x00000001]


def b01():
    k = r.random()
    return 0 if k < 0.4 else (1 if k < 0.9 else r.getrandbits(32) | 1)


rows = []
for _ in range(int(sys.argv[1])):
    calls.clear()
    P = r.getrandbits(32)
    ret['rel'] = b01(); ret['find'] = b01(); ret['v11c'] = b01(); ret['legal'] = b01(); ret['misc'] = b01()
    summoned = r.random() < 0.5
    illusion = r.random() < 0.35
    flag20 = r.getrandbits(32) & ~8
    if r.random() < 0.25:
        flag20 |= 8
    t248 = (r.getrandbits(32) & ~4) | (4 if summoned else 0)
    t5c = (r.getrandbits(32) & ~0x40000000) | (0x40000000 if illusion else 0)
    mu.mem_write(T + 0x20, struct.pack('<I', flag20))
    mu.mem_write(T + 0x248, struct.pack('<I', t248))
    mu.mem_write(T + 0x5C, struct.pack('<I', t5c))
    fa8, fa9 = r.choice(FLOATS), r.choice(FLOATS)
    mu.mem_write(F8, struct.pack('<I', fa8))
    mu.mem_write(F9, struct.pack('<I', fa9))
    C = r.getrandbits(32) | 1
    a2 = r.getrandbits(32)
    a3 = 0 if r.random() < 0.3 else (r.getrandbits(32) | 1)
    a4, a5 = b01() if r.random() < 0.7 else 0, b01() if r.random() < 0.7 else 0
    a6, a7 = r.getrandbits(32), r.getrandbits(32)
    a10 = r.getrandbits(32)
    try:
        res = emu.call(F_WORKER, ecx=C, edx=P, stack_args=(T, a2, a3, a4, a5, a6, a7, F8, F9, a10))
    except Exception:
        print('worker run failed at eip %#x after %s' % (mu.reg_read(UC_X86_REG_EIP), calls))
        raise
    rows.append((P, C, flag20, t248, t5c, ret['rel'], ret['find'], ret['v11c'], ret['legal'], ret['misc'], a2, a3, a4, a5, a6, a7,
                 fa8, fa9, a10, res, ';'.join(calls)))
with open(sys.argv[3], 'w') as fh:
    for row in rows:
        fh.write(' '.join('%x' % x for x in row[:20]) + ' | ' + row[20] + '\n')
print(len(rows), 'rows; paths:',
      sum(1 for x in rows if 'apply' in x[20]), 'apply,', sum(1 for x in rows if 'buffs' in x[20]), 'buffs,',
      sum(1 for x in rows if 'mana' in x[20]), 'mana')
