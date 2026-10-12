"""Unicorn check of the kill experience gates of the death handler `0x6F2A4880` (docs/spec/heroes.md section 13).

Real routines run:
  0x6F26DE50  structure test of the candidate builder (ECX = killer, one stack argument, passed 0): returns 1 when the
              killer's flag word (+0x5C) has bit 0x10000 (a structure) -> the builder stops unless BuildingKillsGiveExp
  0x6F3A3400  (ECX = game) mask bit test: player index a, owner index b -> (mask of a's row at ext+0x38) has bit b;
              this is the passive alliance row; 0x6F285B00 = not that bit (the killer/victim gate)
  0x6F3A34B0  (ECX = game) shared-experience row at ext+0x68: player index a, owner index b

Stand-ins (hooks): the player lookup 0x6F3A1650, the player extension 0x6F41B420, and the mask read 0x6F473170 (it
records the address it is asked for, which must be ext + 0x38 or ext + 0x68). The vectors give the mask, the bit and the
original's result; crates/sim/tests/hero_kill_gate_vectors.rs compares them with the model.

Run:  GAME_DLL=/path/to/Game.dll py tools/emu/hero_kill_gate_emu.py OUT.txt [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_EAX, UC_X86_REG_ECX  # noqa: E402

D = emu.DATA
KILLER = D + 0x1000
GAME = D + 0x1400
EXT = D + 0x1800
PLAYER_OBJ = D + 0x1C00

STRUCT = 0x6F26DE50
PASSIVE = 0x6F3A3400
SHARED = 0x6F3A34B0
A_PLAYER, A_EXT, A_MASK = 0x6F3A1650, 0x6F41B420, 0x6F473170

st = {"mask": 0, "asked": []}


def rd32(a):
    return struct.unpack("<I", bytes(emu.mu.mem_read(a, 4)))[0]


def pop_ret(uc, eax, nbytes):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    uc.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + nbytes)
    uc.reg_write(UC_X86_REG_EIP, ret)


def hook(addr, fn):
    emu.mu.hook_add(UC_HOOK_CODE, lambda uc, a, s, u: fn(uc), begin=addr, end=addr)


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    out = sys.argv[1]
    cases = int(sys.argv[2]) if len(sys.argv) > 2 else 300
    rng = random.Random(int(sys.argv[3]) if len(sys.argv) > 3 else 1)
    hook(A_PLAYER, lambda uc: pop_ret(uc, PLAYER_OBJ, 4))
    hook(A_EXT, lambda uc: pop_ret(uc, EXT, 0))

    def on_mask(uc):
        st["asked"].append(uc.reg_read(UC_X86_REG_ECX))
        pop_ret(uc, st["mask"], 0)

    hook(A_MASK, on_mask)
    lines = []
    for _ in range(cases):
        flags = rng.getrandbits(32) if rng.random() < 0.5 else rng.choice([0, 0x10000, 0x40000000, 0x10100, 0xFFFF])
        emu.mu.mem_write(KILLER + 0x5C, struct.pack("<I", flags))
        r = emu.call(STRUCT, ecx=KILLER, stack_args=(0,))
        lines.append("FLAG %08x %d" % (flags, r & 0xFFFFFFFF))
        for name, fn, off in (("MASK0", PASSIVE, 0x38), ("MASK3", SHARED, 0x68)):
            mask = rng.getrandbits(16) if rng.random() < 0.8 else rng.getrandbits(32)
            a = rng.randrange(16)
            b = rng.randrange(16) if rng.random() < 0.9 else rng.randrange(32)
            st["mask"] = mask
            st["asked"].clear()
            r = emu.call(fn, ecx=GAME, stack_args=(a, b))
            assert st["asked"] == [EXT + off], (name, [hex(x) for x in st["asked"]])
            lines.append("%s %08x %d %d" % (name, mask, b, r & 0xFFFFFFFF))
    with open(out, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("gate cases written: %d -> %s" % (len(lines), out))


if __name__ == "__main__":
    main()
