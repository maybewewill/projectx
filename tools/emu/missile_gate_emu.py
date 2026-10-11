"""Unicorn run of the missile arrival and impact-check gates of CMissile (Game.dll 1.26a).

Functions under test (real code, run in the emulator):
  - slot 47, the per-missile update at arrival, VA 0x6F2BECB0 (fastcall, ECX = missile). It asks the missile for its
    target (0x6F2BBA80, real), calls the arrival routine 0x6F2BE8B0 (real) and then the missile's slot 42 (tail jump).
  - slot 45, the impact check, VA 0x6F2C42D0 (fastcall, ECX = missile). When the gate holds and the ability checks
    pass it sets flag 0x20000000 and calls the launch routine 0x6F2C2AF0 (stub). Otherwise it tail-jumps to the
    missile's slot 47 through 0x6F2BB570 (real).
  - the arrival routine 0x6F2BE8B0 (real, thiscall, ECX = missile, 3 stack arguments): the gate, then the target's
    damage routine (target vtable slot 0x120) or the "ability hits" branch that returns without damage.

Stubs (the callee's real code is not part of the question): the target-from-mask resolver 0x6F4786B0, the
arrival point builder 0x6F2BCAC0, the smart-pointer release 0x6F038450, the source ability 0x6F021A00, the
canTarget test 0x6F284EE0, the predicate 0x6F0774E0, the cookie/bookkeeping routine 0x6F2556B0 and the launch
routine 0x6F2C2AF0. The missile's vtable slots 0xA8 (slot 42) and 0xBC (slot 47) and the target's slot 0x1C
(kind) and slot 0x120 (damage) are stubs that record their calls. The real code's SEH frame accesses
(`fs:[0]`) are replaced with NOPs in the emulator copy of the image only; this changes nothing the gates read.

The state the gates read is set per case: the missile flags (+0x20), the target mask (+0xA0/+0xA4: -1 = no target,
0 = bound), whether the handle resolves, the target's kind value versus the constant returned by 0x6F26C1C0 (real),
the source ability, canTarget and the predicate.

Usage (outside the repository for the output):
    GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn, pefile> python3 tools/emu/missile_gate_emu.py \
        --cases 1500 --json <out.json> --rs crates/sim/tests/missile/gate_data.rs
It prints the counts and the mismatches against the Python model (`model47`, `model45`).
"""
import argparse
import json
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402  (maps Game.dll and provides mu, DATA, SENT)
from capstone import Cs, CS_ARCH_X86, CS_MODE_32  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_EAX,
    UC_X86_REG_EIP,
    UC_X86_REG_ESP,
)

mu = emu.mu
DATA = emu.DATA

RUN47 = 0x6F2BECB0
RUN45 = 0x6F2C42D0
ARRIVE = 0x6F2BE8B0
TARGET_OF = 0x6F2BBA80  # real: target from the missile mask
RESOLVE = 0x6F4786B0  # stub: handle -> object
POINT = 0x6F2BCAC0  # stub, thiscall, 3 stack args
RELEASE = 0x6F038450  # stub, thiscall, 1 stack arg
SOURCE = 0x6F021A00  # stub
CAN_TARGET = 0x6F284EE0  # stub, 3 stack args
PREDICATE = 0x6F0774E0  # stub, thiscall, 1 stack arg
COOKIE = 0x6F2556B0  # stub
LAUNCH = 0x6F2C2AF0  # stub, thiscall, 2 stack args
TO47 = 0x6F2BB570  # real: tail jump through the missile's slot 47
KIND_CONST = 0x6F26C1C0  # real: returns the kind constant

MIS = DATA + 0x1000
TGT = DATA + 0x2000
VT_M = DATA + 0x3000
VT_T = DATA + 0x3200
SCR = DATA + 0x3400  # scratch fraction object (refcount at +4); the original passes &DAT_6faae4f4
STUB_BASE = DATA + 0x4200
TGT_HANDLE = 0x77777777
SRC_OBJ = 0x12345678

ST = {}  # per-case state read by the stubs
COUNT = {}
HOOKS = {}


def rd32(a):
    return struct.unpack('<I', bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))


def ret_to(n_pop, eax=None):
    esp = mu.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    if eax is not None:
        mu.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
    mu.reg_write(UC_X86_REG_ESP, esp + 4 + n_pop)
    mu.reg_write(UC_X86_REG_EIP, ret)


def stack_arg(n):
    esp = mu.reg_read(UC_X86_REG_ESP)
    return rd32(esp + 4 + 4 * n)


def bump(name):
    COUNT[name] = COUNT.get(name, 0) + 1


def on_code(uc, eip, size, user):
    h = HOOKS.get(eip)
    if h is not None:
        h(eip)


def stub(name, n_pop, value_key=None, value=0):
    def f(eip):
        bump(name)
        v = ST.get(value_key, value) if value_key else value
        ret_to(n_pop, v)
    return f


def dmg_stub(eip):
    # target slot 0x120: thiscall, 4 stack args (out, record, flag, 0); ret 0x10
    bump('dmg')
    ST['dmg_flag'] = stack_arg(2)
    ret_to(0x10, 0)


def resolve_stub(eip):
    bump('resolve')
    ret_to(0, TGT if ST['resolve'] else 0)


def patch_fs(start, span=0x300):
    """Replace the instructions that touch fs: with NOPs in the emulator copy (SEH frames)."""
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    code = bytes(mu.mem_read(start, span))
    for ins in md.disasm(code, start):
        if 'fs:' in ins.op_str:
            mu.mem_write(ins.address, b'\x90' * ins.size)


def setup():
    mu.hook_add(UC_HOOK_CODE, on_code)
    patch_fs(RUN47)
    patch_fs(RUN45)
    patch_fs(ARRIVE)
    # stubs for callees, each on its own address in the stub area
    addrs = {}
    for i, key in enumerate(['rec42', 'rec47', 'kind', 'dmg', 'launch_v', 'x']):
        addrs[key] = STUB_BASE + 0x10 * i
    HOOKS[addrs['rec42']] = stub('rec42', 0)
    HOOKS[addrs['rec47']] = stub('rec47', 0)
    HOOKS[addrs['kind']] = stub('kind', 0, 'kind')
    HOOKS[addrs['dmg']] = dmg_stub
    HOOKS[addrs['launch_v']] = stub('launch', 8)
    HOOKS[addrs['x']] = stub('x', 0)
    # missile vtable: slot 42 (0xA8) and slot 47 (0xBC) are recording stubs
    for off in range(0, 0x100, 4):
        wr32(VT_M + off, addrs['x'])
    wr32(VT_M + 0xA8, addrs['rec42'])
    wr32(VT_M + 0xBC, addrs['rec47'])
    # target vtable: slot 0x1C (kind) and 0x120 (damage)
    for off in range(0, 0x140, 4):
        wr32(VT_T + off, addrs['x'])
    wr32(VT_T + 0x1C, addrs['kind'])
    wr32(VT_T + 0x120, addrs['dmg'])
    wr32(MIS, VT_M)
    wr32(TGT, VT_T)
    wr32(SCR, addrs['x'])
    wr32(SCR + 4, 2)
    # real hooks for the stubbed callees (TARGET_OF, the mask lookup, runs as real code)
    HOOKS[RESOLVE] = resolve_stub
    HOOKS[POINT] = stub('point', 0xC)
    HOOKS[RELEASE] = stub('release', 4)
    HOOKS[SOURCE] = stub('source', 0, 'src', 0)
    HOOKS[CAN_TARGET] = stub('cantarget', 0xC, 'can', 0)
    HOOKS[PREDICATE] = stub('predicate', 4, 'pred', 0)
    HOOKS[COOKIE] = stub('cookie', 0)
    HOOKS[LAUNCH] = stub('launch', 8)
    return addrs


def set_missile(flags, bound):
    wr32(MIS + 0x20, flags)
    if bound:
        wr32(MIS + 0xA0, 0)
        wr32(MIS + 0xA4, 0)
    else:
        wr32(MIS + 0xA0, 0xFFFFFFFF)
        wr32(MIS + 0xA4, 0xFFFFFFFF)


FRACTION_CONST = 0x6FAAE4F4  # DAT_6faae4f4 = 1.0f, passed as the fraction; its +4 word is a count the code decrements
FRACTION_BYTES = {}


def run_case(fn, flags, bound, resolve, kok, src, can, pred):
    COUNT.clear()
    if not FRACTION_BYTES:
        FRACTION_BYTES['v'] = bytes(mu.mem_read(FRACTION_CONST, 8))
    mu.mem_write(FRACTION_CONST, FRACTION_BYTES['v'])
    ST.update({'kind': KIND_OK if kok else KIND_BAD, 'resolve': resolve, 'src': SRC_OBJ if src else 0,
               'can': 1 if can else 0, 'pred': 1 if pred else 0, 'dmg_flag': None})
    set_missile(flags, bound)
    emu.call(fn, ecx=MIS)
    out = dict(COUNT)
    out['flags_after'] = rd32(MIS + 0x20)
    out['dmg_flag'] = ST['dmg_flag']
    return out


KIND_OK = 0x2B773375  # the constant 0x6F26C1C0 returns (real code)
KIND_BAD = 0x2B773374


def model47(f, present, kok, src, can, pred):
    """Python model of slot 47 (0x6F2BECB0 with the arrival routine 0x6F2BE8B0)."""
    res = {'dmg': 0, 'dmg_flag': None, 'rec42': 0, 'rec47': 0, 'launch': 0, 'flags_after': f}
    if present:
        blocked = (f & 0x30000000) == 0 and (f & 2) != 0 and kok and src and can and pred
        if not blocked:
            res['dmg'] = 1
            res['dmg_flag'] = (f >> 1) & 1
    res['rec42'] = 1
    return res


def model45(f, present, kok, src, can, pred):
    """Python model of slot 45 (0x6F2C42D0)."""
    res = {'dmg': 0, 'dmg_flag': None, 'rec42': 0, 'rec47': 0, 'launch': 0, 'flags_after': f}
    gate = (f & 0x30000000) == 0 and (f & 2) != 0 and present
    if gate and kok and src and can and pred:
        res['launch'] = 1
        res['flags_after'] = f | 0x20000000
    else:
        res['rec47'] = 1
    return res


def normalise(out):
    return {
        'dmg': out.get('dmg', 0),
        'dmg_flag': out.get('dmg_flag'),
        'rec42': out.get('rec42', 0),
        'rec47': out.get('rec47', 0),
        'launch': out.get('launch', 0),
        'flags_after': out['flags_after'],
    }


def random_flags(rng):
    f = 0
    for bit in (0x2, 0x10000000, 0x20000000, 0x4, 0x1, 0x8000, 0x40000000):
        if rng.random() < 0.5:
            f |= bit
    return f


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--cases', type=int, default=1500)
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--json')
    ap.add_argument('--rs')
    args = ap.parse_args()
    setup()
    rng = random.Random(args.seed)
    rows = []
    mismatches = 0
    counts = {'47': 0, '45': 0}
    for i in range(args.cases):
        for which in ('47', '45'):
            fn = RUN47 if which == '47' else RUN45
            flags = random_flags(rng)
            bound = rng.random() < 0.85
            resolve = rng.random() < 0.85
            kok = rng.random() < 0.8
            src = rng.random() < 0.8
            can = rng.random() < 0.7
            pred = rng.random() < 0.4
            present = bound and resolve
            got = normalise(run_case(fn, flags, bound, resolve, kok, src, can, pred))
            model = model47 if which == '47' else model45
            want = model(flags, present, kok, src, can, pred)
            want = {k: want[k] for k in got}
            counts[which] += 1
            if got != want:
                mismatches += 1
                if mismatches <= 10:
                    print('MISMATCH', which, hex(flags), bound, resolve, kok, src, can, pred, got, want)
            rows.append({'which': which, 'flags': flags, 'bound': bound, 'resolve': resolve, 'kok': kok,
                         'src': src, 'can': can, 'pred': pred, 'got': got})
    print(f"cases: slot47={counts['47']} slot45={counts['45']} mismatches={mismatches}")
    if args.json:
        with open(args.json, 'w') as fh:
            json.dump(rows, fh)
    if args.rs:
        write_rs(args.rs, rows)
    return 1 if mismatches else 0


def write_rs(path, rows):
    lines = [
        '//! Generated by tools/emu/missile_gate_emu.py (Unicorn run of the real slot 47 / slot 45 code of',
        '//! Game.dll 1.26a). Do not edit by hand. Each row: slot, flags, bound, resolve, kind matches, source,',
        '//! canTarget, predicate; then the observed dmg, damage flag, slot-42 calls, slot-47 calls, launch calls and',
        '//! the flags after the call.',
        '',
        '/// One run: (slot, flags, bound, resolve, kind_ok, source, can_target, predicate, dmg, dmg_flag (2 = none),',
        '///  rec42, rec47, launch, flags_after).',
        'pub type Row = (u8, u32, bool, bool, bool, bool, bool, bool, u8, u8, u8, u8, u8, u32);',
        '',
        'pub const MISSILE_GATE_CASES: &[Row] = &[',
    ]
    for r in rows:
        g = r['got']
        flag = 2 if g['dmg_flag'] is None else g['dmg_flag']
        lines.append(
            '    (%d, %#x, %s, %s, %s, %s, %s, %s, %d, %d, %d, %d, %d, %#x),' % (
                47 if r['which'] == '47' else 45, r['flags'],
                str(r['bound']).lower(), str(r['resolve']).lower(), str(r['kok']).lower(),
                str(r['src']).lower(), str(r['can']).lower(), str(r['pred']).lower(),
                g['dmg'], flag, g['rec42'], g['rec47'], g['launch'], g['flags_after']))
    lines.append('];')
    lines.append('')
    with open(path, 'w') as fh:
        fh.write('\n'.join(lines))


if __name__ == '__main__':
    sys.exit(main())
