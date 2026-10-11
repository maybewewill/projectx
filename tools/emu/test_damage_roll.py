"""Differential test of the damage roll of the original (docs/spec/attack-damage.md, section 3.2).

Two entry points are run in Unicorn on the user's Game.dll (GAME_DLL):
  * the roll wrapper 0x6F285350 (ECX = out pointer, EDX = RNG stream index, stack = count, sides, bonus,
    pointer to a real multiplier). It calls the dice sampler 0x6F26CAD0 for count > 0 and returns
    out = MUL(multiplier, I2R(sum + count + bonus)); for count <= 0 it stores 0.0 and draws nothing.
  * CAbilityAttack::GetDamageAmount 0x6F0C6940 (ECX = this, stack = out pointer). It reads count from this+0x88,
    sides from this+0x94, bonus from this+0xA0 + this+0xAC, calls the wrapper with stream 2 and multiplier 1.0,
    and stores max(0, result) to out.

The expected values are the Python model below: one stream step per die, die = floor(r * sides / 2^32), plus one
per die, plus the bonus. Values are kept below 2^24, so I2R and the multiplication by 1.0 are exact and the
expected result is the plain integer.

    GAME_DLL=/path/to/Game.dll python3 tools/emu/test_damage_roll.py
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from emu import *  # noqa: E402,F401  (maps Game.dll, defines call/read_state/write_state, mu, DATA)
from test_emu import py_seed, py_step  # noqa: E402

M = 0xFFFFFFFF
ROLL = 0x6F285350
GET_DAMAGE = 0x6F0C6940
ONE_PTR = 0x6FAAE4F4  # 1.0f, written by the CRT start-up (see tools/emu/runtime_consts.py)
ZERO_PTR = 0x6FAAE470  # 0.0f
# 0x6FAAE4F4 is zero in the file image and becomes 1.0 at start-up; the harness does not run the initialisers,
# so it is written here (value checked with tools/emu/runtime_consts.py).
mu.mem_write(ONE_PTR, struct.pack('<f', 1.0))

# Stream table of the harness (same layout as test_streams.py): the wrapper reads
# [[ [G]+0x10 ]+0xC] + stream*8 + 4, where G comes from 0x6F4C34D0.
OBJ = DATA + 0x500
P = DATA + 0x520
BASE = DATA + 0x1000
mu.mem_write(OBJ + 0x10, struct.pack('<I', P))
mu.mem_write(P + 0xC, struct.pack('<I', BASE))
mu.mem_write(0x6F4C34D0, b'\xb8' + struct.pack('<I', OBJ) + b'\xc3')  # mov eax, OBJ; ret
# Debug/log helpers on the non-draw paths: cdecl, the caller cleans up. Stubbed as "ret".
mu.mem_write(0x6F537E80, b'\xc3')  # assert-like call at the top of GetDamageAmount
mu.mem_write(0x6F4CC1E0, b'\xc3')  # log/assert on count <= 0 in the wrapper

OUT = DATA + 0x800
THIS = DATA + 0x900
STREAM_COMBAT = 2


def stream_ptr(k):
    return BASE + 4 + 8 * k


def model_roll(state, count, sides, bonus):
    """Python model of the wrapper: returns (value, new_state_of_stream)."""
    if count <= 0:
        return 0, state
    cur = state
    tot = 0
    for _ in range(count):
        cur = py_step(*cur)
        tot = (tot + ((cur[0] * sides) >> 32)) & M
    total = (tot + count + bonus) & M
    if total >= 0x80000000:
        total -= 1 << 32
    return total, cur


def run_roll(stream, count, sides, bonus):
    out = OUT
    got = call(ROLL, ecx=out, edx=stream, stack_args=[count, sides, bonus, ONE_PTR])
    val = struct.unpack('<f', struct.pack('<I', read_state(out)[0]))[0]
    return got, val, read_state(stream_ptr(stream))


def run_get_damage(count, sides, bonus_a, bonus_b):
    # this+0x2B8 = 0 (level index), arrays at +0x88 (count), +0x94 (sides), +0xA0 (base), +0xAC (bonus)
    idx_off = 0
    mu.mem_write(THIS + 0x2B8, struct.pack('<I', idx_off))
    mu.mem_write(THIS + 0x88, struct.pack('<I', count & M))
    mu.mem_write(THIS + 0x94, struct.pack('<I', sides & M))
    mu.mem_write(THIS + 0xA0, struct.pack('<I', bonus_a & M))
    mu.mem_write(THIS + 0xAC, struct.pack('<I', bonus_b & M))
    mu.mem_write(OUT, struct.pack('<I', 0xDEADBEEF))
    call(GET_DAMAGE, ecx=THIS, stack_args=[OUT])
    return struct.unpack('<f', struct.pack('<I', read_state(OUT)[0]))[0]


def main():
    random.seed(20261011)
    bad_roll = 0
    n_roll = 0
    bad_get = 0
    n_get = 0
    samples = []
    for trial in range(400):
        stream_state = py_seed(random.getrandbits(32))
        # All 45 streams hold a random state; only the used stream moves.
        for k in range(45):
            write_state(stream_ptr(k), *py_seed(random.getrandbits(32)))
        write_state(stream_ptr(STREAM_COMBAT), *stream_state)
        count = random.randint(-1, 16)
        sides = random.randint(1, 100)
        bonus = random.randint(-30, 60)
        before = read_state(stream_ptr(STREAM_COMBAT))
        val_exp, st_exp = model_roll(before, count, sides, bonus)
        got, val, st_got = run_roll(STREAM_COMBAT, count, sides, bonus)
        ok = (got == OUT) and (val == float(val_exp)) and (st_got == st_exp)
        n_roll += 1
        if not ok:
            bad_roll += 1
            if bad_roll <= 5:
                print('ROLL MISMATCH count', count, 'sides', sides, 'bonus', bonus, 'got', val, 'exp', val_exp)
        # GetDamageAmount on the same stream state: sum + count + (bonus_a + bonus_b), clamped at zero.
        for k in range(45):
            write_state(stream_ptr(k), *py_seed(random.getrandbits(32)))
        write_state(stream_ptr(STREAM_COMBAT), *stream_state)
        ba = random.randint(-20, 40)
        bb = random.randint(-10, 20)
        val_exp, _ = model_roll(before, count, sides, ba + bb)
        exp_clamped = float(max(0, val_exp))
        got_val = run_get_damage(count, sides, ba, bb)
        n_get += 1
        if got_val != exp_clamped:
            bad_get += 1
            if bad_get <= 5:
                print('GET MISMATCH count', count, 'sides', sides, 'bonus', ba + bb, 'got', got_val, 'exp', exp_clamped)
        if len(samples) < 3 and count > 0:
            samples.append((count, sides, ba + bb, got_val))
    print('roll wrapper cases', n_roll, 'mismatches', bad_roll)
    print('GetDamageAmount cases', n_get, 'mismatches', bad_get)
    for s in samples:
        print('sample count=%d sides=%d bonus=%d -> %r' % s)


if __name__ == '__main__':
    main()
