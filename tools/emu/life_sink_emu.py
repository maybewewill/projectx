"""Unicorn run of the unit life sink `0x6F2AC450` (Game.dll 1.26a, docs/spec/heroes.md section 10).

The function is `__thiscall(ECX = unit, [esp+4] = pointer to a real delta, [esp+8] = mode)`, `ret 8`. It is run for
real, together with the CUnit vtable slots it calls (`0x6F28B1F0` set life, `0x6F28B230` add, `0x6F28B080` get,
`0x6F28B2F0` flag bit 8, `0x6F28B0D0` set max), the stat helpers (`0x6F4773E0/0x6F477350/0x6F477400`), the handle
lookup (`0x6F03FA30`), the clamp `0x6F4A92B0` and the softfloat helpers. The only synthetic parts are the world
handle table (one entry, id 1, which maps to the unit object) and the start-up constants 0.0 / 0.5 / 1.0.

Unit object fields used by the stat helpers (addresses in the unit object U):
  +0x5C  flags (bit 8 is read by vt+0x13C)
  +0x4F  byte, bit 0 suppresses the event dispatch in `0x6F4A9230`
  +0x78  current life (written by `0x6F4A92B0`, clamped to [+0x80, +0x84])
  +0x80  lower clamp of the pair (floor; kept by SetMax)
  +0x84  maximum life (read by vt+0x130 `0x6F28B080`, written by vt+0x144 through `0x6F4A9390`)
  +0xA0  handle id of the stat block (stat block = U+0x98, id at +8, second word at +0xC)

Each output row: flags, [life, floor, max] before, delta bits, mode, [life, floor, max] after, return value.

Run:  GAME_DLL=/path/Game.dll python3 tools/emu/life_sink_emu.py OUT.txt [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402

SINK = 0x6F2AC450
WORLD_SLOT = 0x6FAB7788          # DAT_6fab7788: pointer to the handle world
ONE = 0x6FAAE4F4                 # 1.0 (start-up)
HALF = 0x6FAAE4D0                # 0.5 (start-up)
ZERO = 0x6FAAE470                # 0.0 (file)

U = emu.DATA + 0x4000            # the unit object
W = emu.DATA + 0x5000            # the world: +0xC table, +0x1C count
T = emu.DATA + 0x5100            # handle table, entries of 8 bytes: [flag, object]
VAL = emu.DATA + 0x6000          # the delta real
UNIT_VTABLE = 0x6F931934         # CUnit vtable in .rdata (real slots)
ID = 1


def f2bits(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def bits2f(v):
    return struct.unpack("<f", struct.pack("<I", v & 0xFFFFFFFF))[0]


def wr(a, v):
    emu.mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def rd(a):
    return struct.unpack("<I", bytes(emu.mu.mem_read(a, 4)))[0]


def stub(addr, stack_bytes, log):
    """Replaces a function by a recorder: pops the return address and `stack_bytes` of arguments (`ret N`)."""
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_ECX

    def hook(uc, a, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = rd(esp)
        args = [rd(esp + 4 + 4 * i) for i in range(stack_bytes // 4)]
        log.append((addr, tuple(args), uc.reg_read(UC_X86_REG_ECX)))
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + stack_bytes)
        uc.reg_write(UC_X86_REG_EIP, ret)

    emu.mu.hook_add(UC_HOOK_CODE, hook, begin=addr, end=addr)


# Refresh helpers (`0x6F2AB460` raises the unit event 0xD01E6 through SEH and needs FS, `0x6F2AB5E0` is the same
# family). They are notifications: they do not change the life cells (checked by the cell trace in the run).
REFRESH_CALLS = []


def install_hooks():
    stub(0x6F2AB460, 0, REFRESH_CALLS)
    stub(0x6F2AB5E0, 0, REFRESH_CALLS)


def setup():
    install_hooks()
    emu.mu.mem_write(WORLD_SLOT, struct.pack("<I", W))
    wr(W + 0x1C, 8)
    wr(W + 0x0C, T)
    wr(T + ID * 8, 0xFFFFFFFE)   # -2: the entry is in use (the lookup accepts only -2)
    wr(T + ID * 8 + 4, U)
    wr(0x6FAAE4F4, f2bits(1.0))  # 1.0 (start-up value, see tools/emu/runtime_consts.py)
    wr(0x6FAAE4D0, f2bits(0.5))  # 0.5 (start-up value)
    wr(0x6FAAE4D4, f2bits(0.5))


def build_unit(life, floor, mx, flags):
    wr(U + 0x00, UNIT_VTABLE)
    emu.mu.mem_write(U + 0x4F, bytes([0]))
    wr(U + 0x5C, flags)
    wr(U + 0x78, f2bits(life))   # current life
    wr(U + 0x80, f2bits(floor))  # lower clamp of the pair setter 0x6F4A9390 (the floor)
    wr(U + 0x84, f2bits(mx))     # maximum life (vt+0x130 reads it, vt+0x144 writes it)
    wr(U + 0x98 + 8, ID)         # stat block id (U+0xA0)
    wr(U + 0x98 + 0xC, 0)


def read_cells():
    return (rd(U + 0x78), rd(U + 0x80), rd(U + 0x84))


def run(life, floor, mx, flags, delta_bits, mode):
    build_unit(life, floor, mx, flags)
    wr(VAL, delta_bits)
    before = read_cells()
    ret = emu.call(SINK, ecx=U, stack_args=(VAL, mode))  # the real function, see the module notes
    after = read_cells()
    return before, after, ret


def draw_case(rng):
    """One random case: (life, floor, max, flags, delta, mode).

    Valid states only (floor <= life <= max, zeros of either sign allowed). About half of the draws are aimed at
    the zero paths: a life at or near the floor, a delta that takes the life exactly to zero or below it, a maximum
    of zero, and +-0 values as inputs (the zero signs are what the sink's exact bits depend on, heroes.md section 10).
    """
    if rng.random() < 0.5:
        mx = rng.choice([0.0, 1.0, 50.0, 300.0, 600.0, rng.uniform(0.0, 2000.0)])
        floor = rng.choice([0.0, -0.0, 1.0]) if mx >= 1.0 else rng.choice([0.0, -0.0])
        if mx == 0.0:
            floor = rng.choice([0.0, -0.0])
        kind = rng.randrange(5)
        if kind == 0:
            life = floor                                  # life at the floor
        elif kind == 1:
            life = rng.choice([0.0, -0.0]) if floor == 0.0 else floor
            life = min(max(life, floor), mx)
        elif kind == 2:
            life = min(mx, floor + rng.choice([0.25, 0.5, 1.0, 2.5]))
        elif kind == 3:
            life = mx                                     # life at the maximum (may be zero when mx is zero)
        else:
            life = rng.choice([floor, mx, rng.uniform(floor, mx)])
        life = min(max(life, floor), mx)
        cut = rng.random()
        if cut < 0.35 and life != 0.0:
            delta = -life                                 # takes the life exactly to zero
        elif cut < 0.5:
            delta = rng.choice([0.0, -0.0, 0.25, -0.25, 0.4, -0.4, 0.5, -0.5])   # rounds to a zero delta
        elif cut < 0.7:
            delta = -rng.choice([1.0, 2.0, 3.0, 5.0, 10.0, 1000.0])  # below the floor
        else:
            delta = rng.choice([-(life + 0.5), -(life + 2.5), life, mx, -mx - 1.0, 0.5 - life])
        mode = rng.choice([1, 2, 3, 2, 3, 0])
        flags = rng.choice([0, 0x100])
        return life, floor, mx, flags, delta, mode
    mx = rng.choice([1.0, 50.0, 300.0, 600.0, 1000.0, rng.uniform(0.0, 2000.0)])
    floor = rng.choice([0.0, 1.0])
    # valid states only: floor <= life <= max (the original keeps life inside the pair; states outside it are not
    # modelled, see docs/spec/heroes.md section 10)
    life = rng.uniform(floor, mx) if rng.random() < 0.7 else rng.choice([floor, mx])
    gains = [0.0, 1.0, -1.0, 0.5, 2.5, -2.5, 10.0, 25.0, -3.0, 0.25, 100.0, 1.5]
    delta = rng.choice(gains) if rng.random() < 0.6 else rng.uniform(-200.0, 400.0)
    mode = rng.choice([1, 2, 1, 2, 3, 0])
    flags = rng.choice([0, 0x100])
    return life, floor, mx, flags, delta, mode


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
        life, floor, mx, flags, delta, mode = draw_case(rng)
        before, after, ret = run(life, floor, mx, flags, f2bits(delta), mode)
        lines.append("%08x %08x %08x %08x %08x %d %08x %08x %08x %08x" % (
            flags, *before, f2bits(delta), mode, *after, ret & 0xFFFFFFFF))
    with open(out_path, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("life sink cases written: %d -> %s" % (len(lines), out_path))


if __name__ == "__main__":
    main()
