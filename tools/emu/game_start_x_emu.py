"""Per-player fold input X: Unicorn check of the blob filler 0x6F545A00 and the reader 0x6F5BD190.

Runs the real machine code of 0x6F545A00 (copies a player sub-record into a 17+ byte blob) and of
0x6F5BD190 (two dword reads from a stream reader), with the caller's reader set-up copied from
0x6F5C39CE..0x6F5C3A21. Three lookups are replaced by hooks because they are not needed for the
data flow: the global object (0x6F4C34D0), the per-slot record table (0x6F53E050) and the CRT
copy used for the name (0x6F7E15EC, an indirect jump to memcpy in the import table).

Reports the dword X that the fold at 0x6F5C3A4C uses, the second dword, and whether the fold
runs (the reader position must equal its limit after the two reads, 0x6F5C3A35).

Usage: GAME_DLL=/path/Game.dll python3 -I tools/emu/game_start_x_emu.py [cases]
"""
import os
import random
import struct
import sys

import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP, UC_X86_REG_EIP

PATH = os.environ.get("GAME_DLL")
if not PATH:
    sys.exit("set GAME_DLL to the Game.dll 1.26a path")

pe = pefile.PE(PATH, fast_load=True)
BASE = pe.OPTIONAL_HEADER.ImageBase
SIZE = pe.OPTIONAL_HEADER.SizeOfImage
img = bytes(pe.get_memory_mapped_image())
MAPSZ = (SIZE + 0xFFF) & ~0xFFF

mu = Uc(UC_ARCH_X86, UC_MODE_32)
mu.mem_map(BASE, MAPSZ)
mu.mem_write(BASE, img[:MAPSZ].ljust(MAPSZ, b"\0"))
STACK = 0x0010_0000
mu.mem_map(STACK, 0x10000)
TOP = STACK + 0xF000
DATA = 0x0020_0000
mu.mem_map(DATA, 0x10000)
END = 0x0030_0000            # sentinel return address
mu.mem_map(END & ~0xFFF, 0x1000)

FN_GLOBAL = 0x6F4C34D0       # ecx=index -> object (plain ret)
FN_REC = 0x6F53E050          # ecx=table owner, [esp+4]=id (ret 4)
FN_COPY = 0x6F7E15EC         # memcpy (through the import table)
FN_FILL = 0x6F545A00         # ecx=slot, edx=blob, [esp+4]=player object index (ret 4)
FN_READ = 0x6F5BD190         # ecx=reader, edx=out element (two dwords)

# Hook-side objects.
GLOBAL_OBJ = DATA + 0x000
GLOBAL_TBL = DATA + 0x040
PBASE = DATA + 0x400         # player array base (player object 0 at PBASE + 0x290)
REC_BASE = DATA + 0x2000     # sub-records, one per slot
BLOB = DATA + 0x1000         # the caller's 0x6F5C39C4 blob (esp+0xc4 in the caller)
READER = DATA + 0x1100       # the caller's reader object (esp+0x78)
ELEM = DATA + 0x1200         # the caller's 8-byte element (esp+0x30)
READER_VT = 0x6F932C08

mu.mem_write(GLOBAL_OBJ + 0x10, struct.pack("<I", GLOBAL_TBL))
mu.mem_write(GLOBAL_TBL + 0x8, struct.pack("<I", PBASE))

records = {}                 # slot -> bytes of the sub-record (0x2a + count)
calls = []


def u32(x):
    return x & 0xFFFFFFFF


def pop_ret(uc, nargs):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + nargs)
    uc.reg_write(UC_X86_REG_EIP, ret)


def hook(uc, address, size, user):
    if address == FN_GLOBAL:
        uc.reg_write(UC_X86_REG_EAX, GLOBAL_OBJ)
        pop_ret(uc, 0)
    elif address == FN_REC:
        esp = uc.reg_read(UC_X86_REG_ESP)
        slot = struct.unpack("<B", bytes(uc.mem_read(esp + 4, 1)))[0]
        calls.append(("rec", slot))
        uc.reg_write(UC_X86_REG_EAX, REC_BASE + 0x100 * slot if slot in records else 0)
        pop_ret(uc, 4)
    elif address == FN_COPY:
        esp = uc.reg_read(UC_X86_REG_ESP)
        dst, src, n = struct.unpack("<III", bytes(uc.mem_read(esp + 4, 12)))
        if n:
            uc.mem_write(dst, bytes(uc.mem_read(src, n)))
        uc.reg_write(UC_X86_REG_EAX, dst)
        pop_ret(uc, 0)     # cdecl: the caller removes the arguments


hooks = [
    mu.hook_add(UC_HOOK_CODE, hook, begin=FN_GLOBAL, end=FN_GLOBAL),
    mu.hook_add(UC_HOOK_CODE, hook, begin=FN_REC, end=FN_REC),
    mu.hook_add(UC_HOOK_CODE, hook, begin=FN_COPY, end=FN_COPY),
]


def make_record(key16, flag, slot_byte, name):
    """Sub-record layout (offsets from the start of the record):
    +0x18 byte, +0x19..+0x28 16 bytes (four dwords), +0x29 name length, +0x2a name."""
    rec = bytearray(0x2A + len(name))
    rec[0x18] = 0
    rec[0x19:0x29] = key16
    rec[0x25] = flag & 0xFF           # low byte of the dword at +0x25 (the reader limit)
    rec[0x26] = slot_byte & 0xFF      # byte1 of the same dword (the slot index used at 0x6F5C1B20)
    rec[0x27] = 0
    rec[0x28] = 0
    rec[0x29] = len(name)
    rec[0x2A:] = name
    return bytes(rec)


def run_fill_and_read(slot, rec):
    """Caller side of 0x6F5C39B3..0x6F5C3A29 for one slot. Returns (ok, X, flag, pos, limit)."""
    records.clear()
    records[slot] = rec
    mu.mem_write(REC_BASE + 0x100 * slot, rec)
    mu.mem_write(BLOB, b"\0" * 0x20)            # the caller zeroes out[0] and out[0x10]
    mu.mem_write(PBASE + 0x290 + 0x304 * 0 + 0x30, b"")  # placeholder (lookup is hooked)

    # Call 0x6F545A00 as the caller does: ecx = slot, edx = blob, push 0.
    esp = TOP - 0x800
    mu.mem_write(esp, struct.pack("<I", END))      # return address
    mu.mem_write(esp + 4, struct.pack("<I", 0))    # stack argument (player object index 0)
    mu.reg_write(UC_X86_REG_ESP, esp)
    mu.reg_write(UC_X86_REG_ECX, slot)
    mu.reg_write(UC_X86_REG_EDX, BLOB)
    mu.reg_write(UC_X86_REG_EIP, FN_FILL)
    mu.emu_start(FN_FILL, END, count=5000)
    ok = mu.reg_read(UC_X86_REG_EAX) != 0
    if not ok:
        return (False, None, None, None, None)
    blob = bytes(mu.mem_read(BLOB, 0x20))

    # Reader set-up: the caller reads its limit from out[0xc] (esp+0xd0), data at out[0xd] (esp+0xd1).
    limit = blob[0x0C]
    reader = struct.pack("<6I", READER_VT, BLOB + 0xD, 0, 0xFFFFFFFF, limit, 0)
    mu.mem_write(READER, reader)
    elem = ELEM
    mu.mem_write(elem, b"\0" * 8)
    esp = TOP - 0x800
    mu.mem_write(esp, struct.pack("<I", END))
    mu.reg_write(UC_X86_REG_ESP, esp)
    mu.reg_write(UC_X86_REG_ECX, READER)
    mu.reg_write(UC_X86_REG_EDX, elem)
    mu.reg_write(UC_X86_REG_EIP, FN_READ)
    mu.emu_start(FN_READ, END, count=2000)
    x, y = struct.unpack("<II", bytes(mu.mem_read(elem, 8)))
    rd = bytes(mu.mem_read(READER, 0x18))
    pos = struct.unpack("<I", rd[0x14:0x18])[0]
    return (True, x, y, pos, limit, blob)


def expected_x(rec, slot):
    """X from the model: bytes out[0xd..0x10] = (+0x25 dword >> 8) and the count byte at +0x29."""
    d25 = struct.unpack("<I", rec[0x25:0x29])[0]
    count = rec[0x29]
    return ((d25 >> 8) & 0xFFFFFF) | (count << 24)


def print_vectors():
    """Fixed vectors for crates/engine/src/game_start.rs tests (sub-record bytes -> X)."""
    key = bytes(range(0x10, 0x20))
    for flag, slot_byte, name in [(8, 0x16, b"Ab"), (8, 0x00, b"Xyz1"), (4, 0x03, b"Q")]:
        rec = make_record(key, flag, slot_byte, name)
        ok, x, y, pos, limit, blob = run_fill_and_read(0, rec)
        print(f"flag={flag:#04x} slot_byte={slot_byte:#04x} name={name!r} "
              f"record={rec.hex()} ok={ok} X={x:#010x} Y={y:#010x} pos={pos} limit={limit} "
              f"fold={pos == limit} model_X={expected_x(rec, 0):#010x}")


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "vectors":
        print_vectors()
        return
    cases = int(sys.argv[1]) if len(sys.argv) > 1 else 200
    rnd = random.Random(0x2026_1011)
    bad = 0
    fold_runs = 0
    for i in range(cases):
        slot = rnd.randrange(0, 12)
        key = bytes(rnd.getrandbits(8) for _ in range(16))
        name = bytes(rnd.randrange(1, 255) for _ in range(rnd.randrange(0, 12)))
        flag = rnd.choice([8, 8, 8, 1, 0, 4, 16])
        rec = make_record(key, flag, rnd.randrange(0, 256), name)
        ok, x, y, pos, limit, blob = run_fill_and_read(slot, rec)
        if not ok:
            print("fill failed", slot)
            bad += 1
            continue
        want_x = expected_x(rec, slot)
        fold = pos == limit
        fold_runs += fold
        if fold and x != want_x:
            bad += 1
            print("X mismatch", hex(x), hex(want_x))
        if i < 6:
            print(f"slot={slot} flag={flag:#x} count={rec[0x29]} limit={limit} pos={pos} "
                  f"X={x:#010x} Y={y:#010x} fold={fold} blob={blob[:0x12].hex()}")
    print(f"cases {cases}, fold runs {fold_runs}, X mismatches {bad}")
    for h in hooks:
        mu.hook_del(h)
    if bad:
        sys.exit(1)


if __name__ == "__main__":
    main()
