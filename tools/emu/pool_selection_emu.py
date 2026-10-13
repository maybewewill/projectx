"""Original unit/item pool selection and mutation vectors for WC3 1.26a.

Only the final unit/item builder is stubbed. Selection, soft-float arithmetic,
RNG stepping, and bounded add/remove operations execute original Game.dll code.
GAME_DLL must point to the user's recorded 1.26a binary. --emit writes derived results.
"""
import argparse
import hashlib
import os
from pathlib import Path
import random
import struct
import sys

EXPECTED_SHA256 = "6d21bd9a0f9fbc8446f455c9e89ac994fed68174426fe608fcb9baefd4dec53c"
path = Path(os.environ["GAME_DLL"])
if hashlib.sha256(path.read_bytes()).hexdigest() != EXPECTED_SHA256:
    raise ValueError("Expected the recorded 1.26a Game.dll")
sys.path.insert(0, str(Path(__file__).resolve().parent))
import emu
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP

POOL, ENTRIES, RNG, WEIGHT, X, Y, FACE = [emu.DATA + value for value in
                                       (0x1000, 0x2000, 0x3000, 0x4000, 0x4010, 0x4014, 0x4018)]
BUILD_RESULT = 0x13572468
observed = {"draws": 0, "builds": []}


def wr(address, value):
    emu.mu.mem_write(address, struct.pack("<I", value & 0xFFFFFFFF))


def rd(address):
    return struct.unpack("<I", emu.mu.mem_read(address, 4))[0]


def observe(uc, address, size, user):
    if address == 0x6F011390:
        observed["draws"] += 1
    elif address in (0x6F24F1F0, 0x6F2B9940):
        ecx, edx, esp = [uc.reg_read(reg) for reg in (UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP)]
        if address == 0x6F24F1F0:
            assert ecx == 3
            assert tuple(rd(esp + 4 + 4 * i) for i in range(3)) == (X, Y, FACE)
            observed["builds"].append(edx)
        else:
            assert edx == X and rd(esp + 4) == Y
            observed["builds"].append(ecx)


for target in (0x6F011390, 0x6F24F1F0, 0x6F2B9940):
    emu.mu.hook_add(UC_HOOK_CODE, observe, begin=target, end=target)
emu.mu.mem_write(0x6F24F1F0, b"\xb8" + struct.pack("<I", BUILD_RESULT) + b"\xc2\x0c\x00")
emu.mu.mem_write(0x6F2B9940, b"\xb8" + struct.pack("<I", BUILD_RESULT) + b"\xc2\x04\x00")
wr(0x6FAB73D8, RNG)
wr(X, 0x41200000)
wr(Y, 0x41A00000)
wr(FACE, 0x42B40000)


def set_pool(entries):
    assert len(entries) <= 64
    wr(POOL + 0x20, 64)
    wr(POOL + 0x24, len(entries))
    wr(POOL + 0x28, ENTRIES)
    wr(POOL + 0x2C, 0)
    if entries:
        emu.mu.mem_write(ENTRIES, b"".join(struct.pack("<II", *entry) for entry in entries))


def select(entries, state, item):
    set_pool(entries)
    emu.write_state(RNG, *state)
    observed["draws"] = 0
    observed["builds"].clear()
    function, args = (0x6F3E0280, (X, Y)) if item else (0x6F3E17B0, (3, X, Y, FACE))
    result = emu.call(function, ecx=POOL, stack_args=args)
    expected_draws = 1 if entries else 0
    assert observed["draws"] == expected_draws
    assert len(observed["builds"]) == expected_draws
    assert result == (BUILD_RESULT if entries else 0)
    picked = observed["builds"][0] if entries else None
    return picked, emu.read_state(RNG), observed["draws"]


def h(value):
    return f"{value:08x}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cases", type=int, default=2000)
    parser.add_argument("--seed", type=int, default=126)
    parser.add_argument("--emit", type=Path, required=True)
    args = parser.parse_args()
    if args.cases < 1:
        raise ValueError("--cases must be positive")
    rng = random.Random(args.seed)
    special = [0, 0x80000000, 1, 0x007FFFFF, 0x00800000, 0x3F000000, 0x3F800000,
               0x40000000, 0x41200000, 0x4B800000, 0x7F7FFFFF, 0x7F800000,
               0xFF800000, 0x7FC00000, 0x7F800001, 0xBF800000]
    fixtures = [[]]
    fixtures += [[(11, w), (22, v)] for w in special for v in special]
    fixtures += [[(11, 0), (0xFFFFFFFF, 0x3F800000)],
                 [(0xFFFFFFFF, 0), (22, 0)], [(0, 0x3F800000)]]
    # Choose valid raw states whose next output is exactly U=0, U=1/2, or the largest U<1.
    packed = 0xABCDEF01
    emu.write_state(RNG, 0, packed)
    mix = emu.call(0x6F011390, ecx=RNG)
    boundaries = [(output, ((output - mix) & 0xFFFFFFFF, packed))
                  for output in (0, 0x00400000, 0x007FFFFF)]
    lines = []
    for n in range(args.cases):
        boundary = n - len(fixtures)
        if 0 <= boundary < len(boundaries):
            output, state = boundaries[boundary]
            entries = [(11, 0x3F800000), (22, 0x3F800000)]
        else:
            entries = fixtures[n] if n < len(fixtures) else [
                (rng.choice([0, 0xFFFFFFFF, rng.getrandbits(32)]),
                 rng.choice(special) if rng.random() < 0.7 else rng.getrandbits(32))
                for _ in range(rng.randrange(0, 17))]
            state = (rng.getrandbits(32), rng.getrandbits(32))
        unit = select(entries, state, False)
        item = select(entries, state, True)
        assert unit == item, (entries, state, unit, item)
        picked, after, draws = unit
        if 0 <= boundary < len(boundaries):
            assert after[0] == output
            assert picked == (11 if output == 0 else 22), "the half-range tie must skip entry 0"
        lines.append(" ".join(["P", h(state[0]), h(state[1]), str(len(entries)),
                               *(value for entry in entries for value in map(h, entry)),
                               "none" if picked is None else h(picked), h(after[0]), h(after[1]), str(draws)]))
    set_pool([])
    for n in range(args.cases):
        remove = rng.random() < 0.35
        rawcode = rng.choice([0, 0xFFFFFFFF, *range(1, 33)])
        weight = rng.choice(special) if rng.random() < 0.7 else rng.getrandbits(32)
        if remove:
            emu.call(0x6F3EFEB0, ecx=POOL, stack_args=(rawcode,))
        else:
            wr(WEIGHT, weight)
            emu.call(0x6F3EFE00, ecx=POOL, stack_args=(rawcode, WEIGHT))
        count = rd(POOL + 0x24)
        assert count <= 34 and rd(POOL + 0x28) == ENTRIES
        values = [rd(ENTRIES + 4 * i) for i in range(2 * count)]
        lines.append(" ".join(["M", "remove" if remove else "add", h(rawcode), h(weight),
                               str(count), *map(h, values)]))
    args.emit.parent.mkdir(parents=True, exist_ok=True)
    args.emit.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"{args.cases * 2} original selections; {args.cases} original mutations; unit/item results agree")


if __name__ == "__main__":
    main()
