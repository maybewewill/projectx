"""Game-start seeding checks against the original Game.dll under Unicorn (docs/spec/game-start.md).

Runs three machine-code snippets of Game.dll 1.26a and compares them with the model:

1. The slot-fold step at 0x6F5C3A4C..0x6F5C3A5A: S := rotl(S xor X, 3).
2. The seed-selection block at 0x6F5C3F72..0x6F5C3FB5 of the game setup function: the 0x8000
   flag of the settings object chooses between the fold value S and the constant 0x77617233,
   the value goes to the global generator (0x6F4AB660) and the 45 streams are initialised from it
   (0x6F24F1E0). The two helpers are replaced by hooks that record their arguments.
3. The seed-from-argument site at 0x6F5C1160.. is NOT run here (see the spec: open).

Usage: GAME_DLL=/path/Game.dll python3 -I tools/emu/game_start_emu.py [cases]
"""
import os
import random
import struct
import sys

import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP,
                               UC_X86_REG_EIP)

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

# Globals read by the snippets.
GAME_PTR = 0x6FAB65F4        # -> game object; +0x30 -> settings object
G_PTR = 0x6FAB73D8           # -> global generator state (A, B)
GAME_OBJ = DATA + 0x000
SETTINGS = DATA + 0x100
GEN_STATE = DATA + 0x200
mu.mem_write(GAME_PTR, struct.pack("<I", GAME_OBJ))
mu.mem_write(G_PTR, struct.pack("<I", GEN_STATE))
mu.mem_write(GAME_OBJ + 0x30, struct.pack("<I", SETTINGS))

FOLD_BEGIN, FOLD_END = 0x6F5C3A4C, 0x6F5C3A5B      # mov eax,[esp+30]; xor eax,[esp+44]; rol; mov [esp+44],eax
SEL_BEGIN, SEL_END = 0x6F5C3F72, 0x6F5C3FB5
SEED_G = 0x6F4AB660          # generator seeder (thiscall, one stack argument, ret 4)
INIT_STREAMS = 0x6F24F1E0    # stream initialiser (thiscall ecx = seed)

ESP_OFF_S = 0x44             # fold accumulator
ESP_OFF_GAME = 0x18          # game object pointer in the setup frame


def u32(x):
    return x & 0xFFFFFFFF


def rotl(x, n):
    return u32((x << n) | (x >> (32 - n)))


def run_fold_step(s, x):
    frame = TOP - 0x100
    mu.mem_write(frame + 0x30, struct.pack("<I", x))
    mu.mem_write(frame + ESP_OFF_S, struct.pack("<I", s))
    mu.reg_write(UC_X86_REG_ESP, frame)
    mu.reg_write(UC_X86_REG_EIP, FOLD_BEGIN)
    mu.emu_start(FOLD_BEGIN, FOLD_END, count=16)
    return struct.unpack("<I", bytes(mu.mem_read(frame + ESP_OFF_S, 4)))[0]


def run_selection(flags, s):
    """Returns (generator seed argument, stream seed argument, final ecx of the stream call)."""
    mu.mem_write(SETTINGS + 0x38, struct.pack("<I", flags))
    frame = TOP - 0x400
    mu.mem_write(frame + ESP_OFF_GAME, struct.pack("<I", GAME_OBJ))
    mu.mem_write(frame + ESP_OFF_S, struct.pack("<I", s))
    mu.reg_write(UC_X86_REG_ESP, frame)
    calls = []

    def hook(uc, address, size, user):
        if address == SEED_G:
            esp = uc.reg_read(UC_X86_REG_ESP)
            arg = struct.unpack("<I", bytes(uc.mem_read(esp + 4, 4)))[0]
            calls.append(("G", uc.reg_read(UC_X86_REG_ECX), arg))
            ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
            uc.reg_write(UC_X86_REG_ESP, esp + 8)     # ret 4
            uc.reg_write(UC_X86_REG_EIP, ret)
        elif address == INIT_STREAMS:
            esp = uc.reg_read(UC_X86_REG_ESP)
            calls.append(("S", uc.reg_read(UC_X86_REG_ECX), None))
            ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
            uc.reg_write(UC_X86_REG_ESP, esp + 4)
            uc.reg_write(UC_X86_REG_EIP, ret)

    h = mu.hook_add(UC_HOOK_CODE, hook, begin=SEED_G, end=SEED_G)
    h2 = mu.hook_add(UC_HOOK_CODE, hook, begin=INIT_STREAMS, end=INIT_STREAMS)
    mu.reg_write(UC_X86_REG_EIP, SEL_BEGIN)
    mu.emu_start(SEL_BEGIN, SEL_END, count=200)
    mu.hook_del(h)
    mu.hook_del(h2)
    assert mu.reg_read(UC_X86_REG_EIP) == SEL_END, "selection block did not reach its end"
    g = [c for c in calls if c[0] == "G"]
    st = [c for c in calls if c[0] == "S"]
    assert len(g) == 1 and len(st) == 1, calls
    return g[0][1], g[0][2], st[0][1]


def model_selection(flags, s):
    seed = 0x7761_7233 if flags & 0x8000 else s
    return seed


def main():
    cases = int(sys.argv[1]) if len(sys.argv) > 1 else 300
    rnd = random.Random(0x2026_1011)
    mism = 0
    for _ in range(cases):
        s, x = rnd.getrandbits(32), rnd.getrandbits(32)
        got = run_fold_step(s, x)
        want = rotl(s ^ x, 3)
        if got != want:
            mism += 1
            print("fold mismatch", hex(s), hex(x), hex(got), hex(want))
    print(f"fold step: {cases} cases, {mism} mismatches")

    mism = 0
    for i in range(cases):
        flags = rnd.getrandbits(32)
        if i % 3 == 0:
            flags |= 0x8000
        elif i % 3 == 1:
            flags &= ~0x8000
        s = rnd.getrandbits(32)
        g_ecx, g_arg, s_ecx = run_selection(flags, s)
        want = model_selection(flags, s)
        if g_ecx != GEN_STATE or g_arg != want or s_ecx != want:
            mism += 1
            print("selection mismatch", hex(flags), hex(s), hex(g_arg), hex(s_ecx), hex(want))
    print(f"seed selection: {cases} cases, {mism} mismatches")
    if mism:
        sys.exit(1)


if __name__ == "__main__":
    main()
