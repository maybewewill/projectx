"""Unicorn probe of the unit creation placement (docs/spec/unit-position.md, section 8.2).

Runs the real placement body `0x6F29F990` (clamp, terrain height, placement frame, the init `CUnit::vf107` = `0x6F2A0E30`
through vtable slot 107, and the read-back) on a synthetic unit object and world. Recorded per run:
  - the arguments and the frame block that `vf107` receives (its first 0x200 bytes),
  - every call of the cell setter `0x6F474250` (ECX, EDX and its three stack words at entry),
  - the unit's cells (+0x78 / +0x7C) at the end.

Everything the placement and `vf107` call directly, and that is not on the cell-setter chain (REAL), is STUBBED: the
answer is 0 (or the value of SPECIAL), and the callee cleanup is read from the callee's own first `ret imm`
(capstone). The stub list is printed, so every result says what was not run. Imports: the Storm heap is a bump
allocator, memset is a fill, the rest answer 0. Unmapped data is mapped on demand (zero pages); an unmapped code fetch
stops the run and is reported in the log.

Run:  GAME_DLL=/path/to/Game.dll python3 tools/emu/unit_create_emu.py
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
import capstone  # noqa: E402
import pefile  # noqa: E402
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_UNMAPPED, UC_MEM_FETCH_UNMAPPED  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_ECX, UC_X86_REG_EAX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP,
)

mu = emu.mu
CREATE = 0x6F29F990
CREATE_END = 0x6F29FD28
TYPE_COPY = 0x6F29F3E0           # answered 1 (the cell fields it writes are not reproduced; see the spec)
FLAG_32D0B0 = 0x6F32D0B0         # answered with the case flag (non-zero runs the corner clamp 0x6F3E0B50)
VF107 = 0x6F2A0E30
VF107_END = 0x6F2A2300
COOKIE_CHECK = 0x6F7E1059        # __security_check_cookie (compares the frame cookie with [0x6FAAE140])
LOOKUP_POSREF = 0x6F03FA30      # handle lookup of the position reference (ecx = index, edx = tag), answered with the unit
VF46 = 0x6F2A1E74                # read-back (normaliser through unit+0x164), vtable slot 46 (0xB8)
SETTER = 0x6F474250              # cell setter
WORLD_PTR = 0x6FAB7368           # agile world object: playable rectangle at +0x6C..+0x78
STUB = 0x00380000
STUB_SIZE = 0x20000
HEAP = 0x00400000
HEAP_SIZE = 0x00800000
WORLD = 0x00210000
UNIT = 0x00220000
VT = 0x00224000
ARGS = 0x00230000

# Real (not stubbed): the setter chain, the read-back, the clamps and the softfloat helpers (0x6F6E0000..0x6F6FFFFF).
REAL = {
    SETTER, 0x6F4737D0, 0x6F4A6FD0, 0x6F497440, 0x6F4A7380, 0x6F483430, 0x6F4743A0, 0x6F4742F0, 0x6F497460,
    0x6F4885A0, 0x6F483270, 0x6F483E70, 0x6F4743C0, 0x6F38B0B0, VF46, 0x6F469490, 0x6F3E0B50, VF107,
}
CHAIN = [SETTER, 0x6F4737D0, 0x6F4A6FD0, 0x6F497440, 0x6F4A7380, 0x6F483430, 0x6F4743A0, 0x6F3E0B50,
         0x6F469490, 0x6F4742F0, 0x6F497460, 0x6F4885A0, 0x6F483270, 0x6F38B0B0, 0x6F4743C0, VF46]

CASE = {"flag": 0}
SPECIAL = {
    TYPE_COPY: lambda esp: 1,
    FLAG_32D0B0: lambda esp: CASE["flag"],
    # the setter maps the position reference (unit+0x164: index +8, tag +0xC) to the unit object: the synthetic unit
    LOOKUP_POSREF: lambda esp: UNIT,
}

LOG = []
SETTER_CALLS = []
VF107_CALLS = []
STUBBED = {}
_md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
_image = []
_heap = [HEAP]


def rd32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def f2b(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def image():
    if not _image:
        pe = pefile.PE(emu.PATH, fast_load=True)
        _image.append((emu.BASE, bytes(pe.get_memory_mapped_image())))
    return _image[0]


def calls_in(start, end):
    """Direct call and jump targets of [start, end) that lie outside that range."""
    base, img = image()
    out = set()
    for i in _md.disasm(img[start - base:end - base], start):
        if i.mnemonic in ("call", "jmp") and i.op_str.startswith("0x"):
            t = int(i.op_str, 16)
            if not (start <= t < end):
                out.add(t)
    return out


def ret_pop(addr):
    """Callee-cleanup bytes of the function at addr: the immediate of its first `ret` (0 for a plain ret)."""
    base, img = image()
    for i in _md.disasm(img[addr - base:addr - base + 6000], addr):
        if i.mnemonic == "ret":
            return int(i.op_str, 16) if i.op_str else 0
    return 0


def _map_on_demand(uc, access, addr, size, value, user):
    page = addr & ~0xFFF
    if access == UC_MEM_FETCH_UNMAPPED or page >= 0x7F000000:
        LOG.append("UNMAPPED %s %08x" % ("fetch" if access == UC_MEM_FETCH_UNMAPPED else "data", addr))
        return False
    uc.mem_map(page, 0x1000)
    uc.mem_write(page, b"\0" * 0x1000)
    return True


def _alloc(n):
    n = max(16, (n + 15) & ~15)
    p = _heap[0]
    _heap[0] = p + n
    return p


def _stub(name, pop, handler):
    """stdcall-style stub: handler(esp) gives EAX; the callee pops `pop` bytes of arguments."""
    def cb(uc, addr, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = rd32(esp)
        LOG.append(name)
        value = handler(esp)
        uc.reg_write(UC_X86_REG_EAX, (value or 0) & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
        uc.reg_write(UC_X86_REG_EIP, ret)
    return cb


def _cdecl(name, handler):
    def cb(uc, addr, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = rd32(esp)
        LOG.append(name)
        value = handler(esp)
        uc.reg_write(UC_X86_REG_EAX, (value or 0) & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_EIP, ret)
        uc.reg_write(UC_X86_REG_ESP, esp + 4)
    return cb


def _storm_alloc(esp):           # SMemAlloc(size, file, line, flags), 4 stdcall args
    return _alloc(rd32(esp + 4))


def _storm_realloc(esp):         # SMemReAlloc(ptr, size, file, line, flags), 5 stdcall args
    return _alloc(rd32(esp + 8))


def _memset(esp):                # memset(dst, value, count), cdecl
    dst, val, n = rd32(esp + 4), rd32(esp + 8) & 0xFF, rd32(esp + 12)
    if n:
        mu.mem_write(dst, bytes([val]) * n)
    return dst


IMPORTS = {
    "Storm.dll!#401": (16, _storm_alloc),
    "Storm.dll!#403": (16, lambda esp: 1),
    "Storm.dll!#405": (20, _storm_realloc),
    "KERNEL32.dll!EnterCriticalSection": (4, lambda esp: 0),
    "KERNEL32.dll!LeaveCriticalSection": (4, lambda esp: 0),
}


def install_imports():
    pe = pefile.PE(emu.PATH, fast_load=True)
    pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"]])
    mu.mem_map(STUB, STUB_SIZE)
    mu.mem_write(STUB, b"\xc3" * STUB_SIZE)
    mu.mem_map(HEAP, HEAP_SIZE)
    n = 0
    for entry in pe.DIRECTORY_ENTRY_IMPORT:
        for imp in entry.imports:
            name = "%s!%s" % (entry.dll.decode(), imp.name.decode() if imp.name else "#%d" % imp.ordinal)
            addr = STUB + 16 * n
            if name == "MSVCR80.dll!memset":
                cb = _cdecl(name, _memset)
            else:
                pop, handler = IMPORTS.get(name, (0, lambda esp: 0))
                cb = _stub(name, pop, handler)
            mu.hook_add(UC_HOOK_CODE, cb, begin=addr, end=addr)
            wr32(imp.address, addr)
            n += 1
    return n


def _record_setter(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    pair = rd32(esp + 4)
    SETTER_CALLS.append({
        "ecx": uc.reg_read(UC_X86_REG_ECX),
        "edx": uc.reg_read(UC_X86_REG_EDX),
        "args": [rd32(esp + 4 + 4 * i) for i in range(3)],
        # the pair the placement / read-back passes (two float bit patterns, read at entry)
        "pair": (rd32(pair), rd32(pair + 4)),
    })


READBACK = []
READBACK_RET = 0x6F2A1EA3        # the instruction after the read-back call `call 0x6F474250` at 0x6F2A1E9E


def _after_readback(uc, addr, size, user):
    """Stops the run once the creation read-back has returned: records the unit's cells at that point."""
    READBACK.append((rd32(UNIT + 0x78), rd32(UNIT + 0x7C)))
    uc.emu_stop()


def _record_vf107(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    block = rd32(esp + 8)
    VF107_CALLS.append({
        "ecx": uc.reg_read(UC_X86_REG_ECX),
        "arg0": rd32(esp + 4),
        "block": block,
        "block_dump": bytes(mu.mem_read(block, 0x200)) if block else b"",
    })


def _set_unit_slot(uc, addr, size, user):
    """Before 0x6F29FB8F the placement reads its unit object from [esp+0x14]. No write to that slot is visible
    in the body (the writer is inside a stubbed callee, UNSURE), so the synthetic unit is stored there. The
    assumption is explicit: the object identity does not change the cells the init writes."""
    esp = uc.reg_read(UC_X86_REG_ESP)
    wr32(esp + 0x14, UNIT)


def install():
    """Installs the hooks once; the emulator state persists across cases."""
    mu.hook_add(UC_HOOK_MEM_UNMAPPED, _map_on_demand)
    install_imports()
    for a in (WORLD, UNIT, VT, ARGS):
        mu.mem_map(a & ~0xFFF, 0x1000)
    wr32(WORLD_PTR, WORLD)
    wr32(UNIT, VT)
    for slot in range(0, 0x200, 4):
        wr32(VT + slot, STUB + 0x1000 + 16 * (slot // 4))
    wr32(VT + 0xB8, VF46)
    wr32(VT + 0x1AC, VF107)
    # the notify of the position reference (vtable slot +0x54, one stack argument) answers as a stub with pop 4
    notify = STUB + 0x5000
    mu.hook_add(UC_HOOK_CODE, _stub("notify", 4, lambda esp: 0), begin=notify, end=notify)
    wr32(VT + 0x54, notify)
    # the position reference at unit+0x164 is an object with the same (synthetic) vtable
    wr32(UNIT + 0x164, VT)
    mu.hook_add(UC_HOOK_CODE, _stub("vcall", 0, lambda esp: 0),
                begin=STUB + 0x1000, end=STUB + 0x1000 + 16 * 0x80)
    # the stack-cookie check (__security_check_cookie): the emulated frames carry no cookie, so it returns at once
    mu.hook_add(UC_HOOK_CODE, _stub("cookie", 0, lambda esp: 0), begin=COOKIE_CHECK, end=COOKIE_CHECK)
    targets = calls_in(CREATE, CREATE_END) | calls_in(VF107, VF107_END)
    for f in CHAIN:
        targets |= calls_in(f, f + 0x200)
    for t in sorted(targets):
        if t in REAL or t in (CREATE, COOKIE_CHECK) or not (0x6F000000 <= t < 0x6FC00000):
            continue
        if 0x6F6E0000 <= t < 0x6F700000:           # softfloat and math helpers run for real
            continue
        name = "%08x" % t
        STUBBED[t] = name
        special = SPECIAL.get(t, lambda esp: 0)
        mu.hook_add(UC_HOOK_CODE, _stub(name, ret_pop(t), special), begin=t, end=t)
    mu.hook_add(UC_HOOK_CODE, _record_setter, begin=SETTER, end=SETTER)
    mu.hook_add(UC_HOOK_CODE, _record_vf107, begin=VF107, end=VF107)
    mu.hook_add(UC_HOOK_CODE, _set_unit_slot, begin=0x6F29FB8F, end=0x6F29FB8F)
    mu.hook_add(UC_HOOK_CODE, _after_readback, begin=READBACK_RET, end=READBACK_RET)
    return len(STUBBED)


def run_case(x, y, facing, flags, rect, flag=0):
    """One placement; returns the recorded calls and the final cells of the unit."""
    CASE["flag"] = flag
    mu.mem_write(WORLD + 0x6C, struct.pack("<4f", *rect))
    wr32(ARGS, f2b(x))
    wr32(ARGS + 4, f2b(y))
    wr32(ARGS + 8, f2b(facing))
    wr32(ARGS + 12, flags)
    for lst in (LOG, SETTER_CALLS, VF107_CALLS, READBACK):
        del lst[:]
    try:
        emu.call(CREATE, ecx=UNIT, edx=WORLD, stack_args=(ARGS, ARGS + 4, ARGS + 8, ARGS + 12, 0, 0))
    except RuntimeError:
        pass                     # the run stops after the read-back (READBACK_RET), not at the return
    return {
        "log": list(LOG),
        "setter": list(SETTER_CALLS),
        "vf107": [{k: v for k, v in c.items() if k != "block_dump"} for c in VF107_CALLS],
        "block": VF107_CALLS[0]["block_dump"] if VF107_CALLS else b"",
        "cells": READBACK[0] if READBACK else (rd32(UNIT + 0x78), rd32(UNIT + 0x7C)),
    }


def main():
    print("stubbed functions:", install())
    r = run_case(100.0, 200.0, 0.0, 0, (0.0, 0.0, 4096.0, 4096.0))
    for k in ("log", "setter", "vf107"):
        print(k, r[k][:12])
    print("cells", [hex(c) for c in r["cells"]])


if __name__ == "__main__":
    main()
