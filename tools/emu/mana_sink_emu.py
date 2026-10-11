"""Unicorn run of the hero mana sink `0x6F278F50` (Game.dll 1.26a, docs/spec/heroes.md section 10.5).

The function is `__thiscall(ECX = hero, [esp+4] = pointer to a real delta, [esp+8] = mode)`, `ret 8`. It is run for
real, with the helpers it calls: the mana stat wrapper at hero+0xB8 (id at +8, second word at +0xC), the handle lookup
`0x6F03FA30`, the pair setters of the stat object (`0x6F477350/0x6F477400/0x6F4A92B0/0x6F4A9390`), the ratio helper
`0x6F117320` and the softfloat helpers. The event dispatch `0x6F26F5D0` (an SEH frame raising 0xD01A9) is stubbed: it
is a notification and does not change the cells (checked by the cell trace of the run, as for the life sink).

The stat object of the mana wrapper is a separate object (handle id 2, here at DATA+0x4800). Its cells have the same
layout as the life pair: +0x78 current mana, +0x80 floor, +0x84 maximum. The hero itself is at DATA+0x4000 and is not
written by the sink (the life cells of the hero are kept as a check).

Start-up constants are the same as in life_sink_emu.py (0.0 at 0x6FAAE470, 0.5 at 0x6FAAE4D0, 1.0 at 0x6FAAE4F4).

Run:  GAME_DLL=/path/Game.dll python3 tools/emu/mana_sink_emu.py OUT.txt [CASES] [SEED]

Output rows: `flags cur0 floor0 max0 hero_life delta mode cur1 floor1 max1 hero_life1 ret`, all 32-bit hex.
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
import life_sink_emu as L  # noqa: E402

SINK = 0x6F278F50
HERO = emu.DATA + 0x4000          # the hero (the sink's ECX)
MANA = emu.DATA + 0x4800          # the mana stat object (handle id 2)
ID_MANA = 2
VAL = L.VAL
rd = L.rd
wr = L.wr
f2bits = L.f2bits


def setup():
    L.install_hooks()
    emu.mu.mem_write(L.WORLD_SLOT, struct.pack("<I", L.W))
    wr(L.W + 0x1C, 8)
    wr(L.W + 0x0C, L.T)
    wr(L.T + 1 * 8, 0xFFFFFFFE)
    wr(L.T + 1 * 8 + 4, HERO)
    wr(L.T + ID_MANA * 8, 0xFFFFFFFE)
    wr(L.T + ID_MANA * 8 + 4, MANA)
    wr(0x6FAAE4F4, f2bits(1.0))
    wr(0x6FAAE4D0, f2bits(0.5))
    wr(0x6FAAE4D4, f2bits(0.5))
    # the event dispatch of the hero (SEH frame): a recorder, no cell is written by it
    L.stub(0x6F26F5D0, 0, L.REFRESH_CALLS)


def build(cur, floor, mx, life=0.0):
    # the hero: its life pair is kept (checked unchanged), its mana wrapper points at MANA
    L.build_unit(life, 0.0, 1000.0, 0)
    wr(HERO + 0xB8 + 8, ID_MANA)
    wr(HERO + 0xB8 + 0xC, 0)
    # the mana object: vtable slot not used, flags 0, byte +0x4F 0 (events not suppressed)
    emu.mu.mem_write(MANA + 0x4F, bytes([0]))
    wr(MANA + 0x5C, 0)
    wr(MANA + 0x78, f2bits(cur))
    wr(MANA + 0x80, f2bits(floor))
    wr(MANA + 0x84, f2bits(mx))


def read_mana():
    return (rd(MANA + 0x78), rd(MANA + 0x80), rd(MANA + 0x84))


def run(cur, floor, mx, delta_bits, mode):
    build(cur, floor, mx)
    wr(VAL, delta_bits)
    before = read_mana()
    life_before = rd(HERO + 0x78)
    ret = emu.call(SINK, ecx=HERO, stack_args=(VAL, mode))
    after = read_mana()
    return before, after, life_before, rd(HERO + 0x78), ret


def draw_case(rng):
    """Valid mana states (floor <= cur <= max, zeros of either sign), about half aimed at the zero paths."""
    if rng.random() < 0.5:
        mx = rng.choice([0.0, 1.0, 50.0, 300.0, 600.0, rng.uniform(0.0, 2000.0)])
        floor = rng.choice([0.0, -0.0]) if mx < 1.0 else rng.choice([0.0, -0.0, 1.0])
        kind = rng.randrange(4)
        if kind == 0:
            cur = floor
        elif kind == 1:
            cur = rng.choice([0.0, -0.0])
        elif kind == 2:
            cur = mx
        else:
            cur = rng.uniform(floor, mx) if mx > floor else floor
        cur = min(max(cur, floor), mx)
        cut = rng.random()
        if cut < 0.35 and cur != 0.0:
            delta = -cur
        elif cut < 0.5:
            delta = rng.choice([0.0, -0.0, 0.25, -0.25, 0.4, -0.4, 0.5, -0.5])
        elif cut < 0.7:
            delta = -rng.choice([1.0, 2.0, 3.0, 5.0, 10.0, 1000.0])
        else:
            delta = rng.choice([-(cur + 0.5), -(cur + 2.5), cur, mx, -mx - 1.0, 0.5 - cur, 3.0, 40.0])
        return cur, floor, mx, delta, rng.choice([1, 2, 3, 2, 3, 0, 4])
    mx = rng.choice([1.0, 50.0, 300.0, 600.0, 1000.0, rng.uniform(0.0, 2000.0)])
    floor = rng.choice([0.0, 1.0]) if mx >= 1.0 else 0.0
    cur = rng.uniform(floor, mx) if rng.random() < 0.7 else rng.choice([floor, mx])
    gains = [0.0, 1.0, -1.0, 0.5, 2.5, -2.5, 10.0, 25.0, -3.0, 0.25, 100.0, 1.5]
    delta = rng.choice(gains) if rng.random() < 0.6 else rng.uniform(-200.0, 400.0)
    return cur, floor, mx, delta, rng.choice([1, 2, 1, 2, 3, 0])


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    out_path = sys.argv[1]
    cases = int(sys.argv[2]) if len(sys.argv) > 2 else 200
    rng = random.Random(int(sys.argv[3]) if len(sys.argv) > 3 else 5)
    setup()
    lines = []
    for _ in range(cases):
        cur, floor, mx, delta, mode = draw_case(rng)
        before, after, lb, la, ret = run(cur, floor, mx, f2bits(delta), mode)
        lines.append("%08x %08x %08x %08x %08x %08x %d %08x %08x %08x %08x %08x" % (
            0, *before, lb, f2bits(delta), mode, *after, la, ret & 0xFFFFFFFF))
    with open(out_path, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("mana sink cases written: %d -> %s" % (len(lines), out_path))


if __name__ == "__main__":
    main()
