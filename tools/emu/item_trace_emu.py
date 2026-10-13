"""Call tracer for item ability slots of Game.dll 1.26a (docs/spec/abilities-items.md).

Runs ONE virtual slot of an ability class on a fake ability object whose level record carries distinct recognisable
values in DataA..DataI, and records every call the slot makes (target, ECX, EDX, the first stack dwords). Calls to
the data getter and the soft-float library run for real; every other call made directly by the slot is a stub that
only records and returns a configurable value. The trace shows which level field reaches which sink, i.e. the
structure of the effect; it is a decoding aid, the numeric cores are checked by item_cores_emu.py.

Usage: GAME_DLL=... py tools/emu/item_trace_emu.py CODE SLOT [ARG ...]   (CODE = rawcode text, SLOT = byte offset hex)
"""
import csv
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from capstone import Cs, CS_ARCH_X86, CS_MODE_32, CS_OP_IMM, CS_OP_REG, CS_OP_MEM  # noqa: E402
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ_UNMAPPED, UC_HOOK_MEM_WRITE_UNMAPPED  # noqa: E402
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESI,  # noqa: E402
                               UC_X86_REG_EDI, UC_X86_REG_EBP, UC_X86_REG_ESP, UC_X86_REG_EIP)

mu = emu.mu
BASE = emu.BASE
md = Cs(CS_ARCH_X86, CS_MODE_32)
md.detail = True
D = emu.DATA
REGS = {'eax': UC_X86_REG_EAX, 'ebx': UC_X86_REG_EBX, 'ecx': UC_X86_REG_ECX, 'edx': UC_X86_REG_EDX,
        'esi': UC_X86_REG_ESI, 'edi': UC_X86_REG_EDI, 'ebp': UC_X86_REG_EBP, 'esp': UC_X86_REG_ESP}

# fake objects (all inside emu.DATA, 64 KiB)
ABIL = D + 0x1000      # the ability
DEF = D + 0x1400       # ability definition (+0x2C nonzero, +0x50 level count, +0x54 records)
REC = D + 0x1800       # level records, 0x68 each
UNIT = D + 0x2000      # target / owner unit
UVT = D + 0x2400       # unit vtable (every slot -> RET stub)
RETSTUB = D + 0x3000
OWNER = D + 0x3400
ALLOW = [(0x6F6EE000, 0x6F6F1000), (0x6F02E500, 0x6F02EE00)]
DATA_NAMES = "ABCDEFGHI"


def f32(x):
    return struct.unpack('<I', struct.pack('<f', x))[0]


DATA_VALS = [f32(1.5 + i * 0.25 + 100 * (i + 1)) for i in range(9)]  # 101.75, 202.0, ... distinct


def fmt(v):
    f = struct.unpack('<f', struct.pack('<I', v))[0]
    if v == 0:
        return '0'
    for i in range(9):
        if v == DATA_VALS[i]:
            return 'Data' + DATA_NAMES[i]
    if v == ABIL:
        return 'ABILITY'
    if v == UNIT:
        return 'UNIT'
    if v == OWNER:
        return 'OWNER'
    if 1e-3 < abs(f) < 1e6 and f == f:
        return '%08x(%g)' % (v, f)
    return '%08x' % v


def setup(vtable, level_fields=None, flags=0):
    for a in (ABIL, DEF, REC, UNIT, UVT, RETSTUB, OWNER):
        mu.mem_write(a, bytes(0x400))
    mu.mem_write(RETSTUB, b'\x31\xC0\xC3')
    for i in range(0x200):
        mu.mem_write(UVT + 4 * i, struct.pack('<I', RETSTUB))
    mu.mem_write(UNIT, struct.pack('<I', UVT))
    mu.mem_write(OWNER, struct.pack('<I', UVT))
    mu.mem_write(ABIL, struct.pack('<I', vtable))
    mu.mem_write(ABIL + 0x20, struct.pack('<I', flags))
    mu.mem_write(ABIL + 0x30, struct.pack('<I', UNIT))
    mu.mem_write(ABIL + 0x34, struct.pack('<I', OWNER))
    mu.mem_write(ABIL + 0x50, struct.pack('<I', 0))
    mu.mem_write(ABIL + 0x54, struct.pack('<I', DEF))
    mu.mem_write(DEF + 0x2C, struct.pack('<I', 1))
    mu.mem_write(DEF + 0x50, struct.pack('<I', 1))
    mu.mem_write(DEF + 0x54, struct.pack('<I', REC))
    rec = bytearray(0x68)
    for i, v in enumerate(DATA_VALS):
        struct.pack_into('<I', rec, 0x20 + 4 * i, v)
    struct.pack_into('<I', rec, 0x04, f32(0.5))     # Cast
    struct.pack_into('<I', rec, 0x08, f32(33.0))    # Dur
    struct.pack_into('<I', rec, 0x0C, f32(44.0))    # HeroDur
    struct.pack_into('<I', rec, 0x10, 55)           # Cost
    struct.pack_into('<I', rec, 0x14, f32(6.0))     # Cool
    struct.pack_into('<I', rec, 0x18, f32(250.0))   # Area
    struct.pack_into('<I', rec, 0x1C, f32(600.0))   # Rng
    mu.mem_write(REC, bytes(rec))
    if level_fields:
        for off, v in level_fields.items():
            mu.mem_write(ABIL + off, struct.pack('<I', v))


def ret_cleanup(target, depth=0):
    """Bytes the callee pops (`ret N`): the first `ret` of the function, or (a function that ends in a tail jump and
    has no `ret` before its padding) the cleanup of the jump target."""
    off = target - BASE
    last_jmp = None
    for ins in md.disasm(emu.img[off:off + 800], target):
        if ins.mnemonic == 'ret':
            return int(ins.op_str, 16) if ins.op_str else 0
        if ins.mnemonic == 'int3':
            break
        if ins.mnemonic == 'jmp' and ins.operands[0].type == CS_OP_IMM:
            t = ins.operands[0].imm
            if not (target <= t < ins.address + 0x400 and t > ins.address):
                last_jmp = t
    if last_jmp is not None and depth < 4:
        return ret_cleanup(last_jmp, depth + 1)
    return 0


def factory_row(code):
    rows = list(csv.reader(open(os.path.join(HERE, '..', '..', 'docs', 'research', 'ability-atlas', 'factory.tsv'),
                                encoding='utf-8'), delimiter='\t'))
    h = rows[0]
    return [dict(zip(h, r)) for r in rows[1:] if r[0] == code][0]


def trace_call(fn, ecx, args=(), stub_ret=None, stub_fn=None, into=(), span=0x600, edx=None, cleanup=None):
    """Runs `fn` with ECX = `ecx` and the stack `args`; every call made directly by `fn` (and by the functions listed
    in `into`) is stubbed and recorded unless its target is in ALLOW or in `into`. `stub_ret[target]` is the value
    returned in EAX; `stub_fn[target](uc, ecx, edx, stack)` computes it instead. Returns
    (EAX, trace lines, structured calls) where a call is (target, ecx, edx, stack dwords, tail_jump)."""
    sites = {}
    for f0 in (fn,) + tuple(into):
        for ins in md.disasm(emu.img[f0 - BASE:f0 - BASE + span], f0):
            if ins.mnemonic == 'call':
                sites[ins.address] = ins
            elif ins.mnemonic == 'jmp' and ins.operands[0].type == CS_OP_IMM and not (f0 <= ins.operands[0].imm < f0 + 0x300):
                sites[ins.address] = ins
    trace = []
    calls = []
    stub_ret = stub_ret or {}
    stub_fn = stub_fn or {}

    def target_of(ins, uc):
        op = ins.operands[0]
        if op.type == CS_OP_IMM:
            return op.imm
        if op.type == CS_OP_REG:
            return uc.reg_read(REGS[ins.reg_name(op.reg)])
        if op.type == CS_OP_MEM:
            m = op.mem
            a = m.disp
            if m.base:
                a += uc.reg_read(REGS[ins.reg_name(m.base)])
            if m.index:
                a += uc.reg_read(REGS[ins.reg_name(m.index)]) * m.scale
            return struct.unpack('<I', bytes(uc.mem_read(a & 0xffffffff, 4)))[0]
        return 0

    def hook(uc, address, size, user):
        ins = sites.get(address)
        if ins is None:
            return
        tgt = target_of(ins, uc) & 0xffffffff
        for lo, hi in ALLOW:
            if lo <= tgt < hi:
                return
        if tgt in into:
            return
        esp = uc.reg_read(UC_X86_REG_ESP)
        stack = struct.unpack('<6I', bytes(uc.mem_read(esp, 24)))
        c = uc.reg_read(UC_X86_REG_ECX)
        d = uc.reg_read(UC_X86_REG_EDX)
        try:
            n = ret_cleanup(tgt)
        except Exception:
            n = 0
        if cleanup and tgt in cleanup:
            n = cleanup[tgt]
        shown = []
        for v in stack[:max(n // 4, 0) if n else 3]:
            sv = fmt(v)
            if (D <= v < D + 0x10000 or 0x100000 <= v < 0x110000) and v not in (ABIL, UNIT, OWNER):
                sv += '->' + fmt(struct.unpack('<I', bytes(uc.mem_read(v, 4)))[0])
            shown.append(sv)
        tail = ins.mnemonic == 'jmp'
        trace.append('%s %08x ecx=%s edx=%s cleanup=%d args=[%s]' % ('TAILJMP' if tail else 'call', tgt, fmt(c), fmt(d), n, ', '.join(shown)))
        calls.append((tgt, c, d, stack, tail))
        if tgt in stub_fn:
            rv = stub_fn[tgt](uc, c, d, stack) & 0xffffffff
        else:
            rv = stub_ret.get(tgt, 0)
        uc.reg_write(UC_X86_REG_EAX, rv)
        if tail:
            ret = struct.unpack('<I', bytes(uc.mem_read(esp, 4)))[0]
            uc.reg_write(UC_X86_REG_ESP, esp + 4 + n)
            uc.reg_write(UC_X86_REG_EIP, ret)
        else:
            uc.reg_write(UC_X86_REG_ESP, esp + n)
            uc.reg_write(UC_X86_REG_EIP, address + size)

    def unm(uc, access, address, size, value, user):
        base = address & ~0xfff
        try:
            uc.mem_map(base, 0x1000)
        except Exception:
            return False
        return True

    h1 = mu.hook_add(UC_HOOK_CODE, hook, begin=min((fn,) + tuple(into)), end=max((fn,) + tuple(into)) + span)
    h2 = mu.hook_add(UC_HOOK_MEM_READ_UNMAPPED, unm)
    h3 = mu.hook_add(UC_HOOK_MEM_WRITE_UNMAPPED, unm)
    try:
        r = emu.call(fn, ecx=ecx, edx=edx, stack_args=args)
    finally:
        mu.hook_del(h1)
        mu.hook_del(h2)
        mu.hook_del(h3)
    return r, trace, calls


def run(code, slot, args=(), stub_ret=None, vt_override=None, flags=0, level_fields=None, quiet=False, fn_override=None, into=()):
    row = factory_row(code)
    vt = int(row['vtable'], 16) if vt_override is None else vt_override
    setup(vt, level_fields, flags)
    fn = fn_override or struct.unpack('<I', bytes(mu.mem_read(vt + slot, 4)))[0]
    try:
        r, trace, _ = trace_call(fn, ABIL, args, stub_ret=stub_ret, into=into)
        res = 'ret=%08x' % r
    except Exception as e:
        res = 'ERR %s' % e
        trace = []
    if not quiet:
        print('%s slot %#x fn=%08x %s' % (code, slot, fn, res))
        for t in trace:
            print('   ', t)
    return trace


if __name__ == '__main__':
    code = sys.argv[1]
    slot = int(sys.argv[2], 16)
    into = [int(a[5:], 16) for a in sys.argv[3:] if a.startswith('into=')]
    args = [int(a, 0) for a in sys.argv[3:] if not a.startswith('into=')]
    run(code, slot, args, into=into)
