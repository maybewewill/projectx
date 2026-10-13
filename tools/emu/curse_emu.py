"""Unicorn check of Curse's attacker-side miss decision (CBuffCurse::EvadeAttack 0x6F07F850, helper 0x6F07C4C0).

docs/spec/abilities-unit-spells.md section 12. The ORIGINAL function runs from the user's Game.dll; the only stubs are
the assert helper 0x6F537E80 (no effect) and the stream draw 0x6F2853D0 (returns a pointer to the draw value this
harness wants, so the RNG state is not needed; the call is recorded to check that exactly one draw per decision is taken
on the stream 2, EDX = 2).

Each output row: chance bits, draw bits, result (1 = evades / misses, 0 = hits). The Rust test
(crates/sim/tests/curse_vectors.rs) re-computes every row with `unit_spells::curse_evades` (soft sub + the epsilon
compare) and also checks the walk-B score slot 0x6F07F8A0 (returns the holder value).

Usage: GAME_DLL=/path/Game.dll py tools/emu/curse_emu.py CASES SEED OUT.txt
"""
import os, sys, random, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
mu = emu.mu
DATA = emu.DATA
F_EVADE = 0x6F07F850
F_SCORE = 0x6F07F8A0
F_ASSERT = 0x6F537E80
F_DRAW = 0x6F2853D0
BUFF = DATA + 0x400          # fake CBuffCurse: holder at +0xCC (vptr), value at +0xD0
DRAWBUF = DATA + 0x500
OUT = DATA + 0x600
state = {'draw': 0, 'draws': 0, 'stream': None}


def hook(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = struct.unpack('<I', bytes(uc.mem_read(esp, 4)))[0]
    if addr == F_ASSERT:
        # cdecl with 3 stack arguments (the caller cleans up)
        uc.reg_write(UC_X86_REG_ESP, esp + 4)
        uc.reg_write(UC_X86_REG_EIP, ret)
    elif addr == F_DRAW:
        ecx = uc.reg_read(UC_X86_REG_ECX)
        state['stream'] = uc.reg_read(UC_X86_REG_EDX)
        state['draws'] += 1
        uc.mem_write(DRAWBUF, struct.pack('<I', state['draw']))
        uc.reg_write(UC_X86_REG_EAX, DRAWBUF)
        uc.reg_write(UC_X86_REG_ESP, esp + 4)
        uc.reg_write(UC_X86_REG_EIP, ret)


mu.hook_add(UC_HOOK_CODE, hook, begin=F_ASSERT, end=F_ASSERT)
mu.hook_add(UC_HOOK_CODE, hook, begin=F_DRAW, end=F_DRAW)

r = random.Random(int(sys.argv[2]) if len(sys.argv) > 2 else 1)
SP = [0, 0x80000000, 0x7F800000, 0xFF800000, 0x7FC00000, 0x3F800000, 0x00000001, 0x3EA8F5C3, 0x3A83126F, 0x3A83126E]


def fb():
    k = r.random()
    if k < 0.12:
        return r.choice(SP)
    if k < 0.8:
        return struct.unpack('<I', struct.pack('<f', r.choice([r.uniform(0, 1), r.uniform(0, 1), r.uniform(0, 0.01), r.uniform(0.3, 0.36)])))[0]
    return r.getrandbits(32)


rows = []
bad = 0
for _ in range(int(sys.argv[1])):
    chance, draw = fb(), fb()
    k = r.random()
    if k < 0.15:
        draw = chance
    elif k < 0.3:
        # within and around the epsilon
        c = struct.unpack('<f', struct.pack('<I', chance))[0]
        if c == c and abs(c) < 1e6:
            draw = struct.unpack('<I', struct.pack('<f', c + r.choice([-1, 1]) * r.choice([0.0004, 0.0009, 0.001, 0.0011, 0.002])))[0]
    mu.mem_write(BUFF + 0xCC, struct.pack('<II', 0x6F876D38, chance))
    state['draw'] = draw
    state['draws'] = 0
    res = emu.call(F_EVADE, ecx=BUFF, stack_args=(0x1111, 0x2222))
    if state['draws'] != 1 or state['stream'] != 2:
        bad += 1
    # score slot: out pointer, two more arguments (ret 0xC)
    emu.call(F_SCORE, ecx=BUFF, stack_args=(OUT, 0, 0))
    score = struct.unpack('<I', bytes(mu.mem_read(OUT, 4)))[0]
    if score != chance:
        bad += 1
    rows.append((chance, draw, res & 0xFF if res in (0, 1) else res))
with open(sys.argv[3], 'w') as fh:
    for row in rows:
        fh.write('%08x %08x %d\n' % row)
print(len(rows), 'rows;', bad, 'harness problems;', sum(1 for x in rows if x[2] == 1), 'evade;',
      'eps bits %08x' % struct.unpack('<I', bytes(mu.mem_read(0x6FAAE47C, 4)))[0])
