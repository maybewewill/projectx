"""Unicorn check of the hero experience tables (Game.dll 1.26a, builder VA 0x6F00ABD0, docs/spec/heroes.md).

The builder reads the Misc keys through the reader functions (list reader 0x6F009F90, count 0x6F009D10,
real reader 0x6F009EE0). Those readers are replaced by hooks that answer from synthetic random Misc
content, and the Storm allocator (SMemAlloc / SMemReAlloc, ordinals 401 / 405) by a bump heap. The builder
then runs in the emulator and its two tables are read back:

- the hero table at [0x6FAAE420], 12-byte records: +0 need XP (int), +4 grant XP (int), +8 factor (f32 bits);
- the unit table at [0x6FAAE430], one u32 per unit level (grant XP of a normal unit).

Every case is written as one line of a text file (the Rust side reads it, see crates/sim/tests/hero_xp_vectors.rs),
so no Game.dll data goes into the repository. The file is written to the path given on the command line.

Run:  GAME_DLL=/path/to/Game.dll python3 tools/emu/hero_xp_emu.py OUT.txt [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402  (maps Game.dll and provides call())
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_ECX,  # noqa: E402
                               UC_X86_REG_EDX, UC_X86_REG_EAX, UC_X86_REG_EDI)

BUILDER = 0x6F00ABD0
READ_LIST = 0x6F009F90   # ECX = Misc object, EDX = key, [esp+4] = index; returns int in EAX; ret 4
READ_REAL = 0x6F009EE0   # ECX = out, [esp+4] = key, [esp+8] = index; returns out in EAX; ret 8
COUNT = 0x6F009D10       # EDI = key; returns the element count in EAX; no stack arguments
LOOKUP_NEED = 0x6F207760  # stack = level; returns need XP of the clamped level (level-1 index)
TABLE_PTR = 0x6FAAE420   # hero table (12-byte records)
UNIT_PTR = 0x6FAAE430    # unit table (u32 per level)
ONE_HUNDRED = 0x6FAAE574  # 100.0 at start-up (attack-damage.md); divisor of the factor column
HALF = 0x6FAAE4D0        # 0.5 at start-up (XP rounding constant)
STORM_ALLOC = 0x6F86D57C  # IAT slot of Storm ordinal 401 (SMemAlloc)
STORM_REALLOC = 0x6F86D58C  # IAT slot of Storm ordinal 405 (SMemReAlloc)
STUB = 0x00380000
HEAP = 0x00400000
HEAP_SIZE = 0x400000
ZERO_RANGE = (0x6FAAE418, 0x6FAAE434)  # builder state (pointers and sizes), zero at start-up

KEYS_INT = {}   # key -> list of ints for the current case
KEYS_REAL = {}  # key -> list of f32 bit patterns for the current case
heap = [HEAP + 0x10]
stats = {"list_reads": 0, "real_reads": 0, "count_reads": 0}


def rd32(a):
    return struct.unpack("<I", bytes(emu.mu.mem_read(a, 4)))[0]


def wr32(a, v):
    emu.mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def cstr(p):
    b = bytes(emu.mu.mem_read(p, 64)).split(b"\0")[0]
    return b.decode()


def f32_bits(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def bits_f32(b):
    return struct.unpack("<f", struct.pack("<I", b & 0xFFFFFFFF))[0]


def alloc(n):
    p = heap[0]
    heap[0] = (p + max(n, 1) + 15) & ~15
    return p


def install_hooks():
    emu.mu.mem_map(STUB, 0x1000)
    emu.mu.mem_write(STUB, b"\xcc" * 0x1000)
    emu.mu.mem_map(HEAP, HEAP_SIZE)
    wr32(STORM_ALLOC, STUB)
    wr32(STORM_REALLOC, STUB + 0x10)

    def on_code(uc, addr, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = rd32(esp)
        if addr == STUB:  # SMemAlloc(size, file, line, flags), stdcall
            uc.reg_write(UC_X86_REG_EAX, alloc(rd32(esp + 4)))
            uc.reg_write(UC_X86_REG_ESP, esp + 4 + 16)
            uc.reg_write(UC_X86_REG_EIP, ret)
        elif addr == STUB + 0x10:  # SMemReAlloc(ptr, size, file, line, flags), stdcall
            uc.reg_write(UC_X86_REG_EAX, alloc(rd32(esp + 8)))
            uc.reg_write(UC_X86_REG_ESP, esp + 4 + 20)
            uc.reg_write(UC_X86_REG_EIP, ret)
        elif addr == READ_LIST:
            stats["list_reads"] += 1
            key = cstr(uc.reg_read(UC_X86_REG_EDX))
            index = rd32(esp + 4)
            vals = KEYS_INT.get(key, [])
            v = vals[index] if index < len(vals) else 0
            uc.reg_write(UC_X86_REG_EAX, v & 0xFFFFFFFF)
            uc.reg_write(UC_X86_REG_ESP, esp + 8)
            uc.reg_write(UC_X86_REG_EIP, ret)
        elif addr == READ_REAL:
            stats["real_reads"] += 1
            out = uc.reg_read(UC_X86_REG_ECX)
            key = cstr(rd32(esp + 4))
            index = rd32(esp + 8)
            vals = KEYS_REAL.get(key, [])
            bits = vals[index] if index < len(vals) else 0  # the reader's default is 0.0
            wr32(out, bits)
            uc.reg_write(UC_X86_REG_EAX, out)
            uc.reg_write(UC_X86_REG_ESP, esp + 4 + 8)
            uc.reg_write(UC_X86_REG_EIP, ret)
        elif addr == COUNT:
            stats["count_reads"] += 1
            key = cstr(uc.reg_read(UC_X86_REG_EDI))
            uc.reg_write(UC_X86_REG_EAX, len(KEYS_INT.get(key, [])))
            uc.reg_write(UC_X86_REG_ESP, esp + 4)
            uc.reg_write(UC_X86_REG_EIP, ret)

    for a in (STUB, STUB + 0x10, READ_LIST, READ_REAL, COUNT):
        emu.mu.hook_add(UC_HOOK_CODE, on_code, begin=a, end=a)
    emu.mu.mem_write(ONE_HUNDRED, struct.pack("<I", f32_bits(100.0)))
    emu.mu.mem_write(HALF, struct.pack("<I", f32_bits(0.5)))


def reset_builder_state():
    emu.mu.mem_write(ZERO_RANGE[0], b"\0" * (ZERO_RANGE[1] - ZERO_RANGE[0] + 4))
    heap[0] = HEAP + 0x10


def random_real(rng):
    return rng.choice([0.0, 0.1, 0.25, 0.5, 1.0, 1.5, 2.0, 0.333, 3.7, -0.5, 7.0, 0.02, 12.5])


def run_case(rng):
    """Random synthetic Misc content, the builder run, and the readback. Returns the case line."""
    # Every list has at least one element: with an empty list the builder reads the word before its table
    # (the previous record or the allocator header), which is not modelled (see docs/spec/heroes.md).
    n = rng.randint(1, 12)
    n2 = rng.randint(1, 10)
    need = [rng.randint(0, 5000) for _ in range(rng.randint(1, n + 2))]
    grant = [rng.randint(0, 900) for _ in range(rng.randint(1, n + 2))]
    factor = [rng.randint(-20, 200) for _ in range(rng.randint(1, n + 2))]
    normal = [rng.randint(0, 60) for _ in range(rng.randint(1, n2 + 2))]
    real = {
        "NeedHeroXPFormulaA": random_real(rng), "NeedHeroXPFormulaB": random_real(rng),
        "NeedHeroXPFormulaC": random_real(rng),
        "GrantHeroXPFormulaA": random_real(rng), "GrantHeroXPFormulaB": random_real(rng),
        "GrantHeroXPFormulaC": random_real(rng),
        "GrantNormalXPFormulaA": random_real(rng), "GrantNormalXPFormulaB": random_real(rng),
        "GrantNormalXPFormulaC": random_real(rng),
    }
    KEYS_INT.clear()
    KEYS_REAL.clear()
    KEYS_INT.update({"MaxHeroLevel": [n], "NeedHeroXP": need, "GrantHeroXP": grant,
                     "HeroFactorXP": factor, "MaxUnitLevel": [n2], "GrantNormalXP": normal})
    for k, v in real.items():
        KEYS_REAL[k] = [f32_bits(v)]
    reset_builder_state()
    out_buf = emu.DATA + 0x800
    emu.call(BUILDER, ecx=out_buf, edx=0)

    table = rd32(TABLE_PTR)
    units = rd32(UNIT_PTR)
    hero = [[rd32(table + 12 * i + 4 * k) for k in range(3)] for i in range(n)]
    normal_out = [rd32(units + 4 * i) for i in range(n2)]

    def fields(values):
        return " ".join(str(v) for v in values) if values else "-"

    need_f = [real["NeedHeroXPFormulaA"], real["NeedHeroXPFormulaB"], real["NeedHeroXPFormulaC"]]
    grant_f = [real["GrantHeroXPFormulaA"], real["GrantHeroXPFormulaB"], real["GrantHeroXPFormulaC"]]
    norm_f = [real["GrantNormalXPFormulaA"], real["GrantNormalXPFormulaB"], real["GrantNormalXPFormulaC"]]
    parts = [
        "N %d" % n,
        "NEED %s" % fields(need),
        "NEEDF %s" % " ".join("%08x" % f32_bits(v) for v in need_f),
        "GRANT %s" % fields(grant),
        "GRANTF %s" % " ".join("%08x" % f32_bits(v) for v in grant_f),
        "FACTOR %s" % fields(factor),
        "N2 %d" % n2,
        "NORMAL %s" % fields(normal),
        "NORMALF %s" % " ".join("%08x" % f32_bits(v) for v in norm_f),
        "OUTNEED %s" % fields([h[0] & 0xFFFFFFFF for h in hero]),
        "OUTGRANT %s" % fields([h[1] & 0xFFFFFFFF for h in hero]),
        "OUTFACTOR %s" % fields([h[2] for h in hero]),
        "OUTNORMAL %s" % fields(normal_out),
    ]
    # The need lookup (0x6F207760, the level clamp) for levels below 1, 1..N and above N.
    clamp = []
    for level in range(-2, n + 4):
        clamp.append("%d:%d" % (level, emu.call(LOOKUP_NEED, stack_args=(level & 0xFFFFFFFF,))))
    parts.append("CLAMP %s" % (" ".join(clamp)))
    return " | ".join(parts), hero, normal_out


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    out_path = sys.argv[1]
    cases = int(sys.argv[2]) if len(sys.argv) > 2 else 200
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    rng = random.Random(seed)
    install_hooks()
    lines = []
    for _ in range(cases):
        line, _, _ = run_case(rng)
        lines.append(line)
    with open(out_path, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("cases written: %d -> %s" % (len(lines), out_path))
    print("reader calls: list=%(list_reads)d real=%(real_reads)d count=%(count_reads)d" % stats)


if __name__ == "__main__":
    main()
