"""Unicorn checks of the Bash roll of Game.dll 1.26a (docs/spec/abilities-human-heroes.md, section 3).

ORIGINAL code of the user's Game.dll, run for real:
  1. The effective chance `0x6F1A3470` (ECX = the Bash ability, argument = out pointer): reads DataA through the level
     getter `0x6F02E9E0` (a fake ability with a real definition and level record), looks the distribution constant up
     in `0x6F1D21F0` (the 21-entry table, its start-up initialiser runs for real on the first call), and multiplies it
     by I2R(counter at +0x84). Rows: DataA bits, counter, result bits.
  2. The proc compare `0x6F01FD60` (ECX = pointer to the draw, argument = pointer to the chance): rows: draw bits,
     chance bits, result.
  3. The draw `0x6F2853D0` (ECX = out pointer, EDX = stream 3) on the stream table in the emulated thread context:
     rows: stream state (a, b), result bits. The Rust test steps `rng::Rng` from the same state and applies
     `add(0.0, mul(1.0, U))`.

Usage (the user's own Game.dll; output rows are derived values only):
  GAME_DLL=/path/Game.dll py tools/emu/human_heroes_emu.py [--dump OUT_DIR] [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402

mu = emu.mu
DATA = emu.DATA
wr, rd = A.wr, A.rd

F_CHANCE = 0x6F1A3470
F_LE = 0x6F01FD60
F_DRAW = 0x6F2853D0
STREAM_BASE = DATA + 0xE000
OUT = DATA + 0xD100
RV = DATA + 0xD110
CV = DATA + 0xD120
STREAM = 3

SPECIAL = [0x00000000, 0x80000000, 0x7F800000, 0xFF800000, 0x7FC00000, 0xFFC00001, 0x00000001, 0x007FFFFF,
           0x3F800000, 0xBF800000, 0x42C80000, 0x42480000]


def f2u(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def rnd_bits(r):
    k = r.random()
    if k < 0.2:
        return r.choice(SPECIAL)
    if k < 0.75:
        return f2u(r.choice([float(r.randint(0, 110)), r.uniform(0.0, 110.0), r.randint(0, 22) * 5.0 + r.choice([0.0, 0.5, 2.4, 2.5, -2.5])]))
    return r.getrandbits(32)


def install():
    A.setup_stubs()
    wr(A.TIB_INNER + 0xC, STREAM_BASE)


def check_chance(cases, r, rows):
    for _ in range(cases):
        chance = rnd_bits(r)
        count = r.choice([1, 1, 2, 3, r.randint(1, 40), r.randint(-3, 100000), r.getrandbits(32)])
        A.make_ability(A.VT_THUNDERBOLT, A.AHTB, 0)
        wr(A.LVL + 0x20, chance)
        wr(A.OBJ + 0x84, count & 0xFFFFFFFF)
        wr(OUT, 0xDEADBEEF)
        emu.call(F_CHANCE, ecx=A.OBJ, stack_args=(OUT,))
        rows.append((chance, count & 0xFFFFFFFF, rd(OUT)))


def check_le(cases, r, rows):
    for _ in range(cases):
        draw = rnd_bits(r)
        chance = draw if r.random() < 0.1 else rnd_bits(r)
        if r.random() < 0.3:
            draw = f2u(r.random())
        wr(RV, draw)
        wr(CV, chance)
        got = emu.call(F_LE, ecx=RV, stack_args=(CV,)) & 0xFF
        rows.append((draw, chance, got))


def check_draw(cases, r, rows):
    for _ in range(cases):
        a = r.getrandbits(32)
        b = r.getrandbits(32)
        base = STREAM_BASE + 4 + 8 * STREAM
        mu.mem_write(base, struct.pack("<II", a, b))
        wr(OUT, 0xDEADBEEF)
        emu.call(F_DRAW, ecx=OUT, edx=STREAM)
        rows.append((a, b, rd(OUT)))


CLASSES = [
    ("CAbilityThunderClap", 0x6F8F67C4, b"AHtc"),
    ("CAbilityBash", 0x6F90A414, b"AHbh"),
    ("CAbilityAvatar", 0x6F90998C, b"AHav"),
    ("CAbilityDivineShield", 0x6F894AEC, b"AHds"),
    ("CAbilityResurrection", 0x6F91C1CC, b"AHre"),
    ("CAbilityWaterElemental", 0x6F89BF84, b"AHwe"),
    ("CBuffThunderClap", 0x6F8E79B4, b"BHtc"),
    ("CBuffPause", 0x6F8B3BEC, b"BPSE"),
    ("CBuffAvatar", 0x6F9039A4, b"BHav"),
    ("CBuffDivineShield", 0x6F894794, b"BHds"),
    ("CBuffWaterElemental", 0x6F8EBDBC, b"BHwe"),
]


def check_classes():
    """Slot 7 (the code getter) of each class vtable returns its four-character code."""
    bad = 0
    for name, vt, tag in CLASSES:
        got = emu.call(rd(vt + 0x1C), ecx=A.OBJ)
        ok = got == int.from_bytes(tag, "big")
        bad += 0 if ok else 1
        print("class %-24s vtable %#x code %s %s" % (name, vt, tag.decode(), "ok" if ok else "MISMATCH %#x" % got))
    return bad


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump = args[1]
        args = args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 24
    r = random.Random(seed)
    install()
    bad_classes = check_classes()
    print("class identity mismatches:", bad_classes)
    chance_rows, le_rows, draw_rows = [], [], []
    check_chance(cases, r, chance_rows)
    check_le(cases, r, le_rows)
    check_draw(cases, r, draw_rows)
    print("effective chance rows:", len(chance_rows))
    print("proc compare rows:", len(le_rows))
    print("draw rows:", len(draw_rows))
    if dump:
        os.makedirs(dump, exist_ok=True)
        for name, rows, fmt in (
            ("chance.txt", chance_rows, "%08x %08x %08x\n"),
            ("compare.txt", le_rows, "%08x %08x %d\n"),
            ("draw.txt", draw_rows, "%08x %08x %08x\n"),
        ):
            with open(os.path.join(dump, name), "w") as fh:
                for row in rows:
                    fh.write(fmt % row)


if __name__ == "__main__":
    main()
