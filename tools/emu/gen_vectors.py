#!/usr/bin/env python3
"""Generate RNG test vectors by running the ORIGINAL Game.dll (1.26a) under Unicorn.

Usage:
    GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn+pefile> \
        python3 tools/emu/gen_vectors.py OUT.json [--n 300] [--seed 1]

Every value in the output is an observation of the original machine code:
inputs (generator state, arguments), the returned value and the generator
state(s) after the call. No Python model is used to produce expected values.

Covered original functions:
    seeder          0x6F4AB660  (__fastcall ecx = state ptr, stack = seed)
    step            0x6F011390  (ecx = state ptr, returns output A)
    GetRandomInt    0x6F3B3560  (stack = lo, hi)
    GetRandomReal   0x6F3B35A0  (stack = ptr a, ptr b; float32 operands)
    SetRandomSeed   0x6F3B36D0  (stack = seed)
    stream init     0x6F2852D0  (ecx = seed; fills the 45 streams)
    soft add        0x6F6EF000  (ecx = out ptr, edx = ptr x, stack = ptr y)
    soft sub        0x6F6EEEF0  (same convention, out = x - y)
    soft mul        0x6F6EEE20  (same convention, out = x * y)

Ordinary float operands are used for most real and soft-float cases; the
"*_uniform*" keys use uniformly random 32-bit patterns (NaN, inf, denormals).

Output JSON layout: {"meta": {...}, "seeder": [...], "step": [...], ...}.
Integers are unsigned 32-bit (floats are their IEEE bit patterns) unless the
key name says otherwise.
"""

import argparse
import hashlib
import json
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402  (maps Game.dll into a Unicorn emulator at import time)
from emu import call, mu, read_state, write_state, GADDR, DATA  # noqa: E402

M32 = 0xFFFFFFFF
GAME_DLL = os.environ.get("GAME_DLL", "Game.dll")

# Stream-array scaffolding (same layout the spec describes: subsystem 13 object
# at OBJ, field +0x10 -> P, field +0xC of P -> stream array base, header 4 bytes).
OBJ = DATA + 0x500
PTR = DATA + 0x520
STREAM_BASE = DATA + 0x1000
SLOT_LOOKUP = 0x6F4C34D0  # stub: mov eax, OBJ ; ret
mu.mem_write(OBJ + 0x10, struct.pack("<I", PTR))
mu.mem_write(PTR + 0xC, struct.pack("<I", STREAM_BASE))
mu.mem_write(SLOT_LOOKUP, b"\xb8" + struct.pack("<I", OBJ) + b"\xc3")

FA = DATA + 0x200  # float operand a
FB = DATA + 0x204  # float operand b
FX = DATA + 0x310  # soft-float operand x
FY = DATA + 0x314  # soft-float operand y
FOUT = DATA + 0x300
SEEDPTR = DATA + 0x400


def s32(x):
    return ((x + 2**31) % 2**32) - 2**31


def f2b(f):
    return struct.unpack("<I", struct.pack("<f", f))[0]


def b2f(b):
    return struct.unpack("<f", struct.pack("<I", b & M32))[0]


def stream_states():
    return [list(read_state(STREAM_BASE + 4 + 8 * k)) for k in range(45)]


def clear_streams():
    for k in range(45):
        write_state(STREAM_BASE + 4 + 8 * k, 0, 0)


def rand_any_bits(rng):
    """Uniformly random 32-bit patterns: every exponent, denormals, NaN and inf."""
    return rng.getrandbits(32)


def rand_float_bits(rng):
    """Random float32 bit patterns: mostly ordinary values, some edge cases."""
    r = rng.random()
    if r < 0.05:
        return rng.choice([0x00000000, 0x80000000, 0x00000001, 0x007FFFFF,
                           0x00800000, 0x3F800000, 0xBF800000, 0x7F7FFFFF])
    if r < 0.15:
        return f2b(rng.uniform(-1e-3, 1e-3))
    if r < 0.2:
        return f2b(rng.uniform(-1e6, 1e6))
    return f2b(rng.uniform(-600.0, 600.0))


def gen_seeder(rng, n):
    out = []
    for i in range(n):
        s = rng.getrandbits(32)
        if i < 4:
            s = [0, 1, 0x69707365, 0x77617233][i]
        write_state(GADDR, 0, 0)
        call(0x6F4AB660, ecx=GADDR, stack_args=[s])
        a, b = read_state(GADDR)
        out.append({"s": s, "a": a, "b": b})
    return out


def gen_step(rng, n):
    out = []
    for _ in range(n):
        a, b = rng.getrandbits(32), rng.getrandbits(32)
        write_state(GADDR, a, b)
        ret = call(0x6F011390, ecx=GADDR)
        a2, b2 = read_state(GADDR)
        out.append({"a": a, "b": b, "out": ret, "a2": a2, "b2": b2})
    return out


def gen_int(rng, n):
    out = []
    for i in range(n):
        a, b = rng.getrandbits(32), rng.getrandbits(32)
        lo = rng.randint(-1000, 1000)
        hi = rng.randint(-1000, 1000)
        if i % 7 == 0:
            hi = lo  # equal bounds: must return lo without drawing
        if i % 11 == 0:
            lo, hi = -(2**31), 2**31 - 1  # full range
        write_state(GADDR, a, b)
        ret = s32(call(0x6F3B3560, stack_args=[lo, hi]))
        a2, b2 = read_state(GADDR)
        out.append({"a": a, "b": b, "lo": lo, "hi": hi, "ret": ret,
                    "a2": a2, "b2": b2})
    return out


def gen_real(rng, n, pick=rand_float_bits):
    out = []
    for i in range(n):
        a, b = rng.getrandbits(32), rng.getrandbits(32)
        fa = pick(rng)
        fb = pick(rng)
        if i % 9 == 0:
            fb = fa  # equal operands: still one draw
        mu.mem_write(FA, struct.pack("<I", fa))
        mu.mem_write(FB, struct.pack("<I", fb))
        write_state(GADDR, a, b)
        ret = call(0x6F3B35A0, stack_args=[FA, FB]) & M32
        a2, b2 = read_state(GADDR)
        out.append({"a": a, "b": b, "fa": fa, "fb": fb, "ret": ret,
                    "a2": a2, "b2": b2})
    return out


def gen_set_random_seed(rng, n):
    out = []
    for _ in range(n):
        s = rng.getrandbits(32)
        write_state(GADDR, 0, 0)
        clear_streams()
        call(0x6F3B36D0, stack_args=[s])
        g = read_state(GADDR)
        out.append({"s": s, "g_a": g[0], "g_b": g[1],
                    "streams": stream_states()})
    return out


def gen_stream_init(rng, n):
    out = []
    for _ in range(n):
        x = rng.getrandbits(32)
        clear_streams()
        call(0x6F2852D0, ecx=x)
        out.append({"x": x, "streams": stream_states()})
    return out


def gen_soft(rng, n, fn, pick=rand_float_bits):
    out = []
    for _ in range(n):
        fx = pick(rng)
        fy = pick(rng)
        mu.mem_write(FX, struct.pack("<I", fx))
        mu.mem_write(FY, struct.pack("<I", fy))
        mu.mem_write(FOUT, struct.pack("<I", 0))
        call(fn, ecx=FOUT, edx=FX, stack_args=[FY])
        r = struct.unpack("<I", bytes(mu.mem_read(FOUT, 4)))[0]
        out.append({"x": fx, "y": fy, "r": r})
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[1])
    ap.add_argument("out", help="output JSON path")
    ap.add_argument("--n", type=int, default=300, help="cases per function")
    ap.add_argument("--seed", type=int, default=1, help="PRNG seed for inputs")
    args = ap.parse_args()

    rng = random.Random(args.seed)
    with open(GAME_DLL, "rb") as f:
        dll_sha = hashlib.sha256(f.read()).hexdigest()
    data = {
        "meta": {
            "game_dll_sha256": dll_sha,
            "input_seed": args.seed,
            "cases_per_function": args.n,
            "note": "observations of the original Game.dll under Unicorn; "
                    "float fields are IEEE float32 bit patterns",
        },
        "seeder": gen_seeder(rng, args.n),
        "step": gen_step(rng, args.n),
        "get_random_int": gen_int(rng, args.n),
        "get_random_real": gen_real(rng, args.n),
        "set_random_seed": gen_set_random_seed(rng, max(10, args.n // 10)),
        "stream_init": gen_stream_init(rng, max(10, args.n // 10)),
        "soft_add": gen_soft(rng, args.n, 0x6F6EF000),
        "soft_sub": gen_soft(rng, args.n, 0x6F6EEEF0),
        "soft_mul": gen_soft(rng, args.n, 0x6F6EEE20),
        # Uniform bit patterns cover every exponent, denormals, NaN and inf.
        "get_random_real_uniform": gen_real(rng, args.n, rand_any_bits),
        "soft_uniform_add": gen_soft(rng, args.n, 0x6F6EF000, rand_any_bits),
        "soft_uniform_sub": gen_soft(rng, args.n, 0x6F6EEEF0, rand_any_bits),
        "soft_uniform_mul": gen_soft(rng, args.n, 0x6F6EEE20, rand_any_bits),
    }
    with open(args.out, "w") as f:
        json.dump(data, f, indent=1)
    counts = {k: len(v) for k, v in data.items() if isinstance(v, list)}
    print("wrote", args.out, counts)


if __name__ == "__main__":
    main()
