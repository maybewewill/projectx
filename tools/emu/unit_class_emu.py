"""Unicorn check of the unit classification natives (Game.dll 1.26a), docs/spec/unit-classification.md.

Runs the real code of IsUnitType (0x6F3C7E90), UnitAddType (0x6F3C7BC0), UnitRemoveType (0x6F3C7C20) and
IsUnitIdType (0x6F3B7F60) on synthetic unit / attack ability memory and compares every answer with a Python model, then
runs the three text parsers of the unit type loader (type 0x6F26DA70, movetp 0x6F26DB40, targs 0x6F255250, weapTp
0x6F262D30) on generated strings. The unit resolver (0x6F3BDCB0), the record getters used by IsUnitIdType and the Storm
string functions (ordinals 504, 508, 509) are replaced by code hooks; everything else is the original machine code.

With --emit the vectors are printed as Rust arrays (used by crates/sim/tests/unit_class_vectors.rs and
crates/gamedata/tests/unit_class_vectors.rs). Nothing from the game's data tables is read: the inputs are synthetic.

Run:  GAME_DLL=C:\\path\\Game.dll python tools/emu/unit_class_emu.py [--emit]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ECX, UC_X86_REG_EDX  # noqa: E402

mu = emu.mu
UNIT = emu.DATA + 0x1000
ATK = emu.DATA + 0x2000
VT = emu.DATA + 0x800
STR = emu.DATA + 0x4000
OUT = emu.DATA + 0x4100
SRC = emu.DATA + 0x4200


def w32(a, v):
    mu.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))


def r32(a):
    return struct.unpack('<I', bytes(mu.mem_read(a, 4)))[0]


def s32(v):
    return v - (1 << 32) if v & 0x80000000 else v


# ---- hooks ---------------------------------------------------------------------------------------------------
REC = {}          # id -> dict for the IdType getters


def ret_with(eax, pop=0):
    esp = mu.reg_read(UC_X86_REG_ESP)
    ra = r32(esp)
    mu.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
    mu.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
    mu.reg_write(UC_X86_REG_EIP, ra)


def cstr(a):
    b = bytearray()
    while True:
        c = mu.mem_read(a, 1)[0]
        if c == 0:
            return bytes(b)
        b += bytes([c])
        a += 1


DELIMS = b" ,-\t"
CUR_ID = [0]


def hook(uc, addr, size, ud):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ecx = uc.reg_read(UC_X86_REG_ECX)
    edx = uc.reg_read(UC_X86_REG_EDX)
    if addr == 0x6F3BDCB0:                       # unit resolver (fastcall ecx = handle)
        ret_with(UNIT)
    elif addr == 0x6F29C240:                     # record movetp
        ret_with(REC['movement'])
    elif addr == 0x6F29CB80:                     # weapsOn
        ret_with(REC['weapons_on'])
    elif addr == 0x6F29CC20:                     # targs[w]  (ecx=id, edx=w)
        ret_with(REC['targs'][edx])
    elif addr == 0x6F29CF10:                     # kind[w]
        ret_with(REC['kind'][edx])
    elif addr == 0x6F29C210:                     # classification
        ret_with(REC['class'])
    elif addr == 0x6F32CC80:                     # IsStructure
        ret_with(REC['structure'])
    elif addr == 0x6F2609A0:                     # weapTp text accessor (stdcall 2 args)
        ret_with(STR, 8)
    elif addr in (0x6F4CBF60, 0x6F4CC1E0):       # error logging
        ret_with(0, 0)
    elif addr == 0x6F6EB5E2:                     # ord 509: _strnicmp(a, b, n), stdcall 12
        a = cstr(r32(esp + 4)).lower()
        b = cstr(r32(esp + 8)).lower()
        n = r32(esp + 12)
        a, b = a[:n], b[:n]
        ret_with((a > b) - (a < b), 12)
    elif addr == 0x6F6EB68A:                     # ord 508: strncmp (case-sensitive), stdcall 12
        a = cstr(r32(esp + 4))
        b = cstr(r32(esp + 8))
        n = r32(esp + 12)
        a, b = a[:n], b[:n]
        ret_with((a > b) - (a < b), 12)
    elif addr == 0x6F6EB606:                     # ord 504: SStrTokenize(char** src, char* dst, size, delims, outlen)
        srcp, dst, size, delims = r32(esp + 4), r32(esp + 8), r32(esp + 12), r32(esp + 16)
        p = r32(srcp)
        dl = cstr(delims)
        out = bytearray()
        while True:
            c = mu.mem_read(p, 1)[0]
            if c == 0:
                break
            if c in dl:
                p += 1
                break
            if len(out) + 1 < size:
                out += bytes([c])
            p += 1
        mu.mem_write(dst, bytes(out) + b'\0')
        w32(srcp, p)
        ret_with(0, 20)


for a in (0x6F3BDCB0, 0x6F29C240, 0x6F29CB80, 0x6F29CC20, 0x6F29CF10, 0x6F29C210, 0x6F32CC80, 0x6F2609A0, 0x6F4CBF60,
          0x6F4CC1E0, 0x6F6EB5E2, 0x6F6EB68A, 0x6F6EB606):
    mu.hook_add(UC_HOOK_CODE, hook, begin=a, end=a)

# vtable slot 79 (0x13C): the real "alive?" accessor
w32(VT + 0x13C, 0x6F28B2F0)
w32(UNIT, VT)

# ---- model -----------------------------------------------------------------------------------------------------
EXCL = {0x1, 0x40, 0x80, 0x100}
CLASS_BIT = {9: 0x1, 10: 0x4, 14: 0x2, 15: 0x8, 16: 0x10, 17: 0x20, 18: 0x40, 19: 0x80, 20: 0x800}


def e1(k, t):
    return k == 1 and t not in EXCL


def e2(k, t):
    return 2 <= k <= 8 and t not in EXCL


def e3(k, t):
    return t in EXCL


def enabled(c, i):
    if not (c['atk_flags'] >> (19 + i)) & 1:
        return False
    k, t = c['kind'][i], c['targs'][i]
    if s32(c['c224']) > 0 and e1(k, t):
        return False
    if s32(c['c228']) > 0 and e2(k, t):
        return False
    if s32(c['c22c']) > 0 and e3(k, t):
        return False
    return True


def model_is_unit_type(c, v):
    f5c = c['f5c']
    if v == 0:
        return int(0 <= (c['b33'] - 0x41) <= 0x19 and not (f5c & 0x40000000))
    if v == 1:
        return (f5c >> 8) & 1
    if v == 2:
        return int(bool(f5c & 0x10000))
    if v == 3:
        return (c['mov'] >> 1) & 1
    if v == 4:
        return int(bool(c['mov'] & 0xD))
    if v in (5, 6, 7, 8):
        if not c['atk']:
            return 0
        for i in range(2):
            if not enabled(c, i):
                continue
            k, t = c['kind'][i], c['targs'][i]
            if v == 5 and (t >> 2) & 1:
                return 1
            if v == 6 and (t >> 1) & 1:
                return 1
            if v == 7 and e1(k, t):
                return 1
            if v == 8 and e2(k, t):
                return 1
        return 0
    if v in CLASS_BIT:
        return int(bool(c['cls'] & CLASS_BIT[v]))
    if v in (11, 12, 13, 21, 22, 23, 24):
        return 0                                   # empty ability list
    if v == 25:
        return int(s32(c['eth']) > 0)
    if v == 26:
        return int(s32(c['mim']) > 0)
    return 0


def setup_unit(c):
    mu.mem_write(UNIT + 0x20, b'\0' * 0x400)
    w32(UNIT, VT)
    w32(UNIT + 0x30, c['b33'] << 24)
    w32(UNIT + 0x5C, c['f5c'])
    w32(UNIT + 0x1FC, c['mov'])
    w32(UNIT + 0x248, c['cls'])
    w32(UNIT + 0x158, c['eth'])
    w32(UNIT + 0x15C, c['mim'])
    w32(UNIT + 0x1DC, 0xFFFFFFFF)
    w32(UNIT + 0x1E0, 0xFFFFFFFF)
    mu.mem_write(ATK, b'\0' * 0x300)
    if c['atk']:
        w32(UNIT + 0x1E8, ATK)
        w32(ATK + 0x20, c['atk_flags'])
        for i in range(2):
            w32(ATK + 0xDC + 4 * i, c['kind'][i])
            w32(ATK + 0x218 + 4 * i, c['targs'][i])
        w32(ATK + 0x224, c['c224'])
        w32(ATK + 0x228, c['c228'])
        w32(ATK + 0x22C, c['c22c'])


TARGS = [0, 1, 2, 4, 6, 8, 10, 0x40, 0x80, 0x100, 0x101, 0x22, 0x7, 0x16, 0x200, 0x41, 0x180]
KINDS = [0, 1, 1, 1, 2, 2, 3, 4, 5, 6, 7, 8, 9, 12]


def rand_case(rng):
    return dict(
        b33=rng.choice([0x41, 0x5A, 0x40, 0x5B, 0x68, 0x48, 0x61, 0x00, 0xFF, rng.randrange(256)]),
        f5c=rng.getrandbits(32) & rng.getrandbits(32) if rng.random() < 0.5 else rng.getrandbits(32),
        mov=rng.choice([0, 1, 2, 4, 8, 0x10, 0x20, 0x40, 0xD, rng.getrandbits(7)]),
        cls=rng.getrandbits(12) if rng.random() < 0.7 else rng.getrandbits(32),
        eth=rng.choice([0, 1, 2, 0xFFFFFFFF, 0x80000000, rng.getrandbits(32)]),
        mim=rng.choice([0, 1, 5, 0xFFFFFFFE, rng.getrandbits(32)]),
        atk=rng.random() < 0.85,
        atk_flags=rng.getrandbits(32) if rng.random() < 0.3 else (rng.choice([0, 1, 2, 3]) << 19) | (rng.getrandbits(8) << 3),
        kind=[rng.choice(KINDS), rng.choice(KINDS)],
        targs=[rng.choice(TARGS), rng.choice(TARGS)],
        c224=rng.choice([0, 0, 0, 1, 2, 0xFFFFFFFF]),
        c228=rng.choice([0, 0, 0, 1, 3, 0xFFFFFFFF]),
        c22c=rng.choice([0, 0, 0, 1, 7, 0xFFFFFFFF]),
    )


def run_is_unit_type(n, rng):
    bad = checks = 0
    cases = []
    for _ in range(n):
        c = rand_case(rng)
        setup_unit(c)
        mask = 0
        for v in range(0, 27):
            r = emu.call(0x6F3C7E90, stack_args=(0x100001, v)) & 0xFFFFFFFF
            got = r & 0xFF if v not in (0, 1, 2) else r
            # The functions return a flag in AL (some return the raw masked word in EAX); only truthiness is used.
            got_b = int(bool(r & 0xFF)) if v in (5, 6, 7, 8) else int(bool(r))
            exp = int(bool(model_is_unit_type(c, v)))
            checks += 1
            if got_b != exp:
                bad += 1
                if bad < 12:
                    print('MISMATCH IsUnitType', v, hex(r), exp, c)
            mask |= got_b << v
        c['mask'] = mask
        cases.append(c)
    print(f'IsUnitType: {n} units x 27 values = {checks} checks, {bad} mismatches')
    # out-of-range values (the jump table is guarded by cmp 0x1A)
    setup_unit(cases[0])
    for v in (27, 28, 100, 0xFFFFFFFF):
        r = emu.call(0x6F3C7E90, stack_args=(0x100001, v))
        assert r & 0xFF == 0, (v, hex(r))
    return cases, bad


def run_add_remove(n, rng):
    bad = checks = 0
    for _ in range(n):
        c = rand_case(rng)
        for v in list(range(0, 27)) + [27, 40, 0xFFFFFFFF]:
            for name, fn in (('add', 0x6F3C7BC0), ('rem', 0x6F3C7C20)):
                setup_unit(c)
                r = emu.call(fn, stack_args=(0x100001, v)) & 0xFF
                got = r5 = r32(UNIT + 0x248)
                bit = CLASS_BIT.get(v)
                exp_ok = bit is not None
                exp_word = (c['cls'] | bit if name == 'add' else c['cls'] & ~bit & 0xFFFFFFFF) if bit else c['cls']
                checks += 1
                if (r != 0) != exp_ok or got != exp_word:
                    bad += 1
                    if bad < 10:
                        print('MISMATCH', name, v, hex(r), hex(got), hex(exp_word))
    print(f'UnitAddType/UnitRemoveType: {checks} checks, {bad} mismatches')
    return bad


def model_id_type(rec, v, id_):
    if v == 0:
        return int(0 <= ((id_ >> 24) - 0x41) <= 0x19)
    if v == 2:
        return int(bool(rec['structure']))
    if v == 3:
        return int(rec['movement'] == 2)
    if v == 4:
        return int(rec['movement'] == 0xD)
    if v in (5, 6):
        t = 0
        for w in range(2):
            if (rec['weapons_on'] >> w) & 1:
                t |= rec['targs'][w]
        return (t >> 2) & 1 if v == 5 else (t >> 1) & 1
    if v in (7, 8):
        r = 0
        for w in range(2):
            if (rec['weapons_on'] >> w) & 1:
                k, t = rec['kind'][w], rec['targs'][w]
                r |= int(e1(k, t) if v == 7 else e2(k, t))
        return r
    if v in CLASS_BIT:
        return int(bool(rec['class'] & CLASS_BIT[v]))
    return 0


def run_id_type(n, rng):
    bad = checks = 0
    cases = []
    for _ in range(n):
        rec = dict(structure=rng.choice([0, 1, 5]), movement=rng.choice([0, 1, 2, 4, 8, 0x10, 0x20, 0x40, 0xD]),
                   class_=0, weapons_on=rng.choice([0, 1, 2, 3, 4, 0xF]), kind=[rng.choice(KINDS), rng.choice(KINDS)],
                   targs=[rng.choice(TARGS), rng.choice(TARGS)])
        rec['class'] = rng.getrandbits(12)
        REC.clear()
        REC.update(rec)
        id_ = (rng.choice([0x41, 0x5A, 0x68, 0x40, 0x61, rng.randrange(256)]) << 24) | rng.getrandbits(24)
        mask = 0
        for v in range(0, 27):
            r = emu.call(0x6F3B7F60, stack_args=(id_, v)) & 0xFFFFFFFF
            got = int(bool(r))
            exp = model_id_type(rec, v, id_)
            checks += 1
            if got != exp:
                bad += 1
                if bad < 12:
                    print('MISMATCH IdType', v, hex(r), exp, rec, hex(id_))
            mask |= got << v
        cases.append((id_, dict(rec), mask))
    for v in (27, 99, 0xFFFFFFFF):
        assert emu.call(0x6F3B7F60, stack_args=(0x48000000, v)) & 0xFF == 0
    print(f'IsUnitIdType: {n} records x 27 values = {checks} checks, {bad} mismatches')
    return cases, bad


# ---- parsers ---------------------------------------------------------------------------------------------------
CLASS_NAMES = ['giant', 'undead', 'summoned', 'mechanical', 'peon', 'sapper', 'townhall', 'ancient', 'neutral', 'ward', 'standon', 'tauren']
MOVE_NAMES = ['fly', 'hover', 'foot', 'horse', 'unbuild', 'float', 'amph', '_', '-', 'none', '']
TARGET_NAMES = ['air', 'aliv', 'alive', 'alli', 'allies', 'ally', 'ancient', 'bridge', 'dead', 'deco', 'decoration', 'debr', 'debris',
                'enem', 'enemies', 'enemy', 'grou', 'ground', 'frie', 'friend', 'hero', 'invu', 'invulnerable', 'item', 'mech',
                'mechanical', 'neut', 'neutral', 'none', 'nonancient', 'nonh', 'nonhero', 'nonsapper', 'nots', 'notself', 'orga',
                'organic', 'play', 'player', 'sapper', 'self', 'stru', 'structure', 'tote', 'totem', 'tree', 'vuln', 'vulnerable',
                'wall', 'ward', '_']
KIND_NAMES = ['_', 'normal', 'missile', 'instant', 'artillery', 'msplash', 'mbounce', 'mline', 'aline']


def mangle(rng, name):
    r = rng.random()
    if r < 0.2:
        return name.upper()
    if r < 0.3:
        return name.capitalize()
    if r < 0.4:
        return name[:-1] if len(name) > 1 else name
    if r < 0.45:
        return name + 'x'
    return name


def rand_text(rng, names, maxn=4):
    parts = []
    for _ in range(rng.randrange(0, maxn + 1)):
        t = mangle(rng, rng.choice(names)) if rng.random() < 0.85 else ''.join(rng.choice('abcxyz_') for _ in range(rng.randrange(1, 20)))
        parts.append(t)
    seps = [' ', ',', '-', '\t', ', ', ' ,', ',,']
    out = ''
    for i, p in enumerate(parts):
        out += p + (rng.choice(seps) if i + 1 < len(parts) else rng.choice(['', '', ',', ' ']))
    if rng.random() < 0.1:
        out = rng.choice(['_', '-', ' ', ',']) + out
    return out


def run_parsers(n, rng):
    bad = 0
    vec = {'class': [], 'move': [], 'targs': [], 'kind': []}
    for _ in range(n):
        s = rand_text(rng, CLASS_NAMES)
        mu.mem_write(STR, s.encode('latin1') + b'\0')
        got = emu.call(0x6F26DA70, ecx=STR) & 0xFFFFFFFF
        vec['class'].append((s, got))
        s = rand_text(rng, TARGET_NAMES, 3)
        mu.mem_write(STR, s.encode('latin1') + b'\0')
        got2 = emu.call(0x6F255250, ecx=STR) & 0xFFFFFFFF
        vec['targs'].append((s, got2))
    for _ in range(n):
        s = mangle(rng, rng.choice(MOVE_NAMES)) if rng.random() < 0.9 else 'walk'
        mu.mem_write(STR, s.encode('latin1') + b'\0')
        vec['move'].append((s, emu.call(0x6F26DB40, ecx=STR) & 0xFFFFFFFF))
        s = mangle(rng, rng.choice(KIND_NAMES)) if rng.random() < 0.9 else 'zzz'
        mu.mem_write(STR, s.encode('latin1') + b'\0')
        vec['kind'].append((s, emu.call(0x6F262D30, stack_args=(1, 0)) & 0xFFFFFFFF))
    print('parsers: class/targs/move/kind', {k: len(v) for k, v in vec.items()}, '(compared with the Rust port by the vector tests)')
    return vec


def main():
    emit = '--emit' in sys.argv
    rng = random.Random(0x1A55)
    cases, b1 = run_is_unit_type(1500, rng)
    b2 = run_add_remove(60, rng)
    idc, b3 = run_id_type(1200, rng)
    vec = run_parsers(500, rng)
    if emit:
        out = open(os.path.join(HERE, '..', '..', 'target', 'unit_class_vectors.txt'), 'w')
        out.write('IS_UNIT_TYPE (b33,f5c,mov,cls,eth,mim,atk,atk_flags,kind0,kind1,targs0,targs1,mask)\n')
        for c in cases:
            if c['c224'] or c['c228'] or c['c22c']:
                continue
            out.write('(%d,%#x,%d,%#x,%d,%d,%s,%#x,%d,%d,%#x,%#x,%#x),\n' % (
                c['b33'], c['f5c'], c['mov'], c['cls'], s32(c['eth']), s32(c['mim']), 'true' if c['atk'] else 'false',
                c['atk_flags'], c['kind'][0], c['kind'][1], c['targs'][0], c['targs'][1], c['mask']))
        out.write('ID_TYPE (id,structure,movement,class,weapons_on,kind0,kind1,targs0,targs1,mask)\n')
        for id_, r, m in idc[:300]:
            out.write('(%#x,%d,%d,%#x,%d,%d,%d,%#x,%#x,%#x),\n' % (id_, 1 if r['structure'] else 0, r['movement'], r['class'],
                      r['weapons_on'], r['kind'][0], r['kind'][1], r['targs'][0], r['targs'][1], m))
        for k, rows in vec.items():
            out.write('PARSE_%s\n' % k.upper())
            for s, v in rows[:200]:
                out.write('(%s, %#x),\n' % ('"' + s.replace('\\', '\\\\').replace('"', '\\"').replace('\t', '\\t') + '"', v))
        out.close()
    print('total mismatches:', b1 + b2 + b3)
    return 1 if (b1 + b2 + b3) else 0


if __name__ == '__main__':
    sys.exit(main())
