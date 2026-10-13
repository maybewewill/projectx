"""Unicorn checks for the undead / night elf hero spells (docs/spec/abilities-undead-ne-heroes.md).

ORIGINAL code of the user's Game.dll, run for real:
  * the heal / nothing / damage relation selector `0x6F08EAC0` (Death Coil `0x6F88E1F4` and Holy Light `0x6F88E1D4`
    tables, read from the DLL): ability in EAX, target unit in ESI, table pointer on the stack. The three helpers it
    calls (enemy test `0x6F285B00`, second relation test `0x6F285B80`, race getter `0x6F29C1B0`) are stubs that
    return chosen values; the unit class word at +0x248 is set per case.

Usage (user's own Game.dll):  GAME_DLL=/path/Game.dll py tools/emu/undead_ne_emu.py [--dump OUT_DIR] [CASES] [SEED]
Prints one summary line; with --dump writes rows `table enemy ally classbit race result` (hex) for the Rust test.
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESI  # noqa: E402

mu = emu.mu
DATA = emu.DATA
wr, rd = A.wr, A.rd

UNIT = DATA + 0x2000
ABIL = DATA + 0x3000
C_ENEMY = DATA + 0xD000
C_ALLY = DATA + 0xD004
C_RACE = DATA + 0xD008

F_SELECT = 0x6F08EAC0
F_ENEMY = 0x6F285B00
F_ALLY = 0x6F285B80
F_RACE = 0x6F29C1B0
TABLE_DEATH_COIL = 0x6F88E1F4
TABLE_HOLY = 0x6F88E1D4


def stub_cell(fn, cell, nbytes):
    code = b"\xA1" + struct.pack("<I", cell) + (b"\xC2" + struct.pack("<H", nbytes) if nbytes else b"\xC3")
    a = A._stub_next[0]
    mu.mem_write(a, code)
    A._stub_next[0] += 0x20
    A.patch_jump(fn, a)


def install():
    A.setup_stubs()
    stub_cell(F_ENEMY, C_ENEMY, 4)
    stub_cell(F_ALLY, C_ALLY, 4)
    stub_cell(F_RACE, C_RACE, 0)


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump = args[1]
        args = args[2:]
    cases = int(args[0]) if args else 800
    seed = int(args[1]) if len(args) > 1 else 11
    r = random.Random(seed)
    install()
    rows = []
    bad = 0
    for _ in range(cases):
        table = r.choice((TABLE_DEATH_COIL, TABLE_HOLY))
        enemy = r.randint(0, 1)
        ally = r.randint(0, 1)
        classbit = r.randint(0, 1)
        race = r.choice((0, 1, 2, 3, 4, 5, 6, 7))
        mu.mem_write(UNIT, b"\0" * 0x400)
        wr(UNIT + 0x248, 2 if classbit else 0)
        wr(C_ENEMY, enemy)
        wr(C_ALLY, ally)
        wr(C_RACE, race)
        mu.reg_write(UC_X86_REG_EAX, ABIL)
        mu.reg_write(UC_X86_REG_ESI, UNIT)
        got = emu.call(F_SELECT, stack_args=(table,))
        got = got - (1 << 32) if got >= 1 << 31 else got
        undead = 1 if (classbit or race == 5) else 0
        base = (rd(table + 8 * ((ally) | (2 * enemy)) + 4 * undead))
        want = base - (1 << 32) if base >= 1 << 31 else base
        if got != want:
            bad += 1
            print("MISMATCH", hex(table), enemy, ally, classbit, race, got, want)
        rows.append((table, enemy, ally, classbit, race, got & 0xFFFFFFFF))
    print("relation selector cases=%d mismatches=%d" % (len(rows), bad))
    if dump:
        os.makedirs(dump, exist_ok=True)
        with open(os.path.join(dump, "relation.txt"), "w") as f:
            for row in rows:
                f.write(" ".join("%08x" % v for v in row) + "\n")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
