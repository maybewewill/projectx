#!/usr/bin/env python3
"""Probe: run the real unit-type table (0x6FAB445C) constructor, insert and creep walk in Unicorn.

Stand-ins (none is original code):
  * Storm ordinal 401 (IAT 0x6F86D57C) = allocator, stdcall (size, file, line, flags); bump allocator, zeroed.
  * Storm ordinal 405 (IAT 0x6F86D58C) = free, stdcall (ptr, file, line, flags); no-op.

Usage: GAME_DLL=/path/to/Game.dll python3 tools/emu/unit_order_probe.py
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import softfloat_vectors as sv  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_EAX  # noqa: E402

T = 0x6FAB445C           # the unit-type table object
VTABLE = 0x6F931BB8      # final vftable written by the static initializer after the constructor
CTOR = 0x6F2A0880        # table constructor (static init)
INSERT = 0x6F2A6190      # hash insert (thiscall, 3 stack args), returns the new node in EAX
IAT_ALLOC = 0x6F86D57C   # Storm ordinal 401
IAT_FREE = 0x6F86D58C    # Storm ordinal 405
STUB_ALLOC = 0x00780000
STUB_FREE = 0x00780010
NODE_ALLOC = 0x6F2A3010  # slot-1 node allocator (stdcall, 1 argument)
NODE_SIZE = 0x2C0
FREE_WRAP = 0x6F6EB558  # wrapper of the free import (stdcall, 4 arguments)
HEAP = 0x00800000
HEAP_SIZE = 0x00400000


def rd(g, a):
    return struct.unpack("<I", bytes(g.mu.mem_read(a, 4)))[0]


def wr(g, a, v):
    g.mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def ret_from(uc, value, pops):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pops)
    uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_EIP, ret)


def stack_args(uc, n):
    esp = uc.reg_read(UC_X86_REG_ESP)
    return [struct.unpack("<I", bytes(uc.mem_read(esp + 4 + 4 * k, 4)))[0] for k in range(n)]


def install(g):
    mu = g.mu
    mu.mem_map(HEAP, HEAP_SIZE)
    mu.mem_map(0x00780000, 0x1000)
    state = {"next": HEAP, "allocs": []}
    wr(g, IAT_ALLOC, STUB_ALLOC)
    wr(g, IAT_FREE, STUB_FREE)

    def alloc_hook(uc, address, size, user):
        n, _file, _line, _flags = stack_args(uc, 4)
        p = state["next"]
        state["next"] = (p + max(n, 1) + 15) & ~15
        if state["next"] > HEAP + HEAP_SIZE:
            raise RuntimeError("probe heap exhausted")
        uc.mem_write(p, b"\0" * max(n, 1))
        state["allocs"].append((p, n))
        ret_from(uc, p, 16)

    def free_hook(uc, address, size, user):
        ret_from(uc, 0, 16)

    mu.hook_add(UC_HOOK_CODE, alloc_hook, begin=STUB_ALLOC, end=STUB_ALLOC)
    mu.hook_add(UC_HOOK_CODE, free_hook, begin=STUB_FREE, end=STUB_FREE)
    # the node allocator used by table slot 1 (stdcall, 1 argument): stand-in = zeroed heap block of the
    # record size (the original's pool and constructor are not needed for the list order; see the notes)
    def node_hook(uc, address, size, user):
        p = state["next"]
        state["next"] = (p + NODE_SIZE + 15) & ~15
        uc.mem_write(p, b"\0" * NODE_SIZE)
        ret_from(uc, p, 4)

    mu.hook_add(UC_HOOK_CODE, node_hook, begin=NODE_ALLOC, end=NODE_ALLOC)
    # the free wrapper used by the table code (stdcall, 4 arguments), no-op as in the other harnesses
    mu.hook_add(UC_HOOK_CODE, free_hook, begin=FREE_WRAP, end=FREE_WRAP)
    return state


TEB = 0x00700000


def patch_fs(g):
    """Rewrite fs:[0] accesses in the emulated copy (Unicorn 2.1 has no FS base): an absolute TEB slot instead.

    The whole .text is swept from every function start (a byte after ret or int3 padding); each decoded
    instruction that addresses fs:[0] with a plain displacement is rewritten in place (same length).
    Only the emulated copy changes; the file is not touched.
    """
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32  # local import: only this harness needs it

    md = Cs(CS_ARCH_X86, CS_MODE_32)
    mu = g.mu
    mu.mem_map(TEB, 0x1000)
    img = g.img
    base = g.base
    text_lo, text_hi = 0x6F000000, 0x6FA00000
    n = 0
    for off in range(0x1000, len(img) - 16):
        a = base + off
        if not (text_lo <= a < text_hi):
            continue
        if img[off - 1] not in (0xC3, 0xCC, 0xC2) or img[off] in (0xCC, 0x90):  # function starts
            continue
        pos = off
        while True:
            chunk = img[pos:pos + 16]
            ins = next(md.disasm(chunk, base + pos), None)
            if ins is None:
                break
            if ins.bytes[0] == 0x64 and "fs:" in ins.op_str:
                raw = bytes(ins.bytes)
                if raw[1] in (0xA1, 0xA3) and raw[2:6] == b"\0\0\0\0":
                    new = bytes([raw[1]]) + struct.pack("<I", TEB) + b"\x90"
                elif raw[1] in (0x8B, 0x89) and len(raw) == 7 and raw[3:7] == b"\0\0\0\0":
                    new = bytes([raw[1], raw[2]]) + struct.pack("<I", TEB) + b"\x90"
                else:
                    new = None
                if new is not None:
                    mu.mem_write(base + pos, new)
                    n += 1
            if ins.mnemonic == "int3":
                break
            pos += ins.size
            if pos >= len(img) or img[pos] == 0xCC:  # padding: the function has ended
                break
    return n


def make_table(g):
    g.call(CTOR, ecx=T)
    wr(g, T, VTABLE)


def insert(g, key):
    """Call the real insert with the builder's arguments (key, 0, 0); returns the node pointer."""
    return g.call(INSERT, ecx=T, stack=[key, 0, 0])


BUCKET_INIT = 0x6F2A0BF0  # builds the 4-bucket array (mask 3) through the growable-array reserve at T+0x14
HASH = 0x6F4C8520        # raw code hash (ecx = address of the packed code dword)
LOOKUP = 0x6F29A8C0      # table lookup (thiscall, table in ECX)
CREEP = 0x6F29FD90       # creep candidate walk (ECX = level, DL = tileset char, 3 stack flags)
APPEND = 0x6F09EAF0      # growable array append (thiscall, ECX = array), returns the new slot
DRAW = 0x6F285320        # shared draw (ECX = stream, EDX = count)
LIST_HEAD = 0x6FAB4468   # table+0xC (the walk's head)
LIST_BASE = 0x6FAB4460   # table+4 (the walk's base)


def pack_code(text):
    return struct.unpack(">I", text.encode("ascii"))[0]


def setup_table(g):
    """Table as the static initializer leaves it, plus a 4-bucket array in the state 0x6F2A0BF0 gives.

    The real initializer (BUCKET_INIT) faults in this harness (its growable-array reserve path, 0x6F293DD0,
    returns to a bad address for every free-stub convention tried), so the buckets are written here.
    Valid while the table does not grow (score-based growth; the first growth faults, see the notes).
    """
    st = install(g)
    make_table(g)
    arr = HEAP + 0x1000
    st["next"] = arr + 12 * 4 + 16
    wr(g, T + 0x1C, arr)
    wr(g, T + 0x24, 3)
    for i in range(4):
        c = arr + 12 * i
        wr(g, c, 4)
        wr(g, c + 4, c + 4)
        wr(g, c + 8, (~(c + 4)) & 0xFFFFFFFF)
    return st


def add_record(g, code_dword, race=8, level=0):
    """Insert one record the way the builder does: node from insert, then hash, raw code and fields."""
    cell = HEAP + 0x2000 + 0x10 * 0  # scratch dword for the hash key
    wr(g, cell, code_dword)
    h = g.call(HASH, ecx=cell)
    node = g.call(INSERT, ecx=T, stack=[h, 0, 0])
    wr(g, node + 0x00, h)
    wr(g, node + 0x14, code_dword)
    wr(g, node + 0x68, level)
    wr(g, node + 0x1C0, race)
    wr(g, node + 0x28C, 0)
    wr(g, node + 0x290, 0)
    wr(g, node + 0x1A8, 0)
    return node


def run_creep(g, st, level=-1):
    """Run the real creep walk; the append stand-in records each visited record in order."""
    visits = []
    slots = []

    def append_hook(uc, address, size, user):
        slot = HEAP + 0x3000 + 4 * len(slots)
        slots.append(slot)
        ret_from(uc, slot, 0)

    def draw_hook(uc, address, size, user):
        ret_from(uc, 0, 0)

    from unicorn import UC_HOOK_CODE as H_
    h1 = g.mu.hook_add(H_, append_hook, begin=APPEND, end=APPEND)
    h2 = g.mu.hook_add(H_, draw_hook, begin=DRAW, end=DRAW)
    try:
        g.call(CREEP, ecx=level & 0xFFFFFFFF, edx=0, stack=[1, 1, 1])
    finally:
        g.mu.hook_del(h1)
        g.mu.hook_del(h2)
    for slot in slots:
        visits.append(rd(g, slot))
    return visits


def main():
    path = os.environ.get("GAME_DLL")
    if not path:
        print("set GAME_DLL", file=sys.stderr)
        return 2
    g = sv.Game(path)
    patch_fs(g)
    st = setup_table(g)
    order = ["hfoo", "nwol", "ngrd", "nskm", "ncrp"]
    nodes = [add_record(g, pack_code(c)) for c in order]
    print("inserted", order, "nodes", [hex(n) for n in nodes])
    print("table", [hex(rd(g, T + 4 * k)) for k in range(12)])
    print("creep visits", [hex(v) for v in run_creep(g, st)])
    return 0


if __name__ == "__main__":
    sys.exit(main())
