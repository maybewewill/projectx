#!/usr/bin/env python3
"""Differential vectors for the soft-float helpers of Game.dll 1.26a.

Calls each ORIGINAL helper inside Unicorn (the user's own Game.dll is mapped at its
image base; nothing from it is written into the repository) and writes the observed
input/output pairs as JSON, for crates/softfloat/tests to replay.

    GAME_DLL=/path/to/Game.dll python3 tools/emu/softfloat_vectors.py \
        --n 10000 --seed 1 --out /path/to/softfloat_vectors.json

Output schema (all numbers are hex strings of 32-bit patterns):

    {"format": "softfloat-vectors-1",
     "dll_sha256": "...",
     "cases": [{"fn": "add", "args": ["3f800000", "40000000"], "ret": "40400000"}, ...]}

Argument conventions (as in the original):
  * binary helpers: ECX = result pointer, EDX = pointer to operand A, [esp+4] = pointer to B
  * unary helpers:  ECX = result pointer, EDX = pointer to operand A
  * I2R:            ECX = result pointer, EDX = the integer itself
  * F2I / R2I:      ECX = pointer to operand, return value in EAX
  * compares:       ECX = pointer to A, [esp+4] = pointer to B, return 0/1 in EAX

Inputs for which the original does not return within the instruction budget (for
example an exponent so large that POW's integer loop never ends) are skipped and
not recorded (the printed per-function counts show how many were kept).

Constants: the original fills its constant pool at start-up with tiny initialiser
stubs (plain stores, I2R and soft division). They are executed here before any call,
so the reference values are the ones the real engine uses.
"""

import argparse
import hashlib
import json
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "pylib"))

import pefile  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EIP)

FORMAT = "softfloat-vectors-1"

# Entry points (image base 0x6F000000).
ADD = 0x6F6EF000
SUB = 0x6F6EEEF0
MUL = 0x6F6EEE20
DIV = 0x6F6EFE10
RECIP = 0x6F6EF100
I2R = 0x6F6EF350
F2I = 0x6F6EEC20
R2I = 0x6F6EEC70
SQRT = 0x6F6EF750
SIN = 0x6F6EF820
COS = 0x6F6EF8D0
TAN = 0x6F6F0140
ASIN = 0x6F6EFC70
ACOS = 0x6F6EFB00
ATAN = 0x6F6F0170
ATAN2 = 0x6F6F02D0
POW = 0x6F6F0810
EXP = 0x6F6F07B0
LN = 0x6F6F0060
TRUNC = 0x6F6EF290
FLOOR_B = 0x6F6EF170
CEIL_B = 0x6F6EF200
ROUND_B = 0x6F6EF330  # floor(x + 0.5)
CEIL_LIKE = 0x6F6EF200
CMP_GT = 0x6F01FD40   # a > b
CMP_LE = 0x6F01FD60   # a <= b
CMP_LT = 0x6F01FD80   # a < b (same as 0x6F00C110)
CMP_LT2 = 0x6F00C110  # a < b
CMP_EQ = 0x6F009AE0   # a == b (ordered)
CMP_APPROX = 0x6F00C3F0  # |a - b| < 0.001
# Fused squared distance of two points: ECX = result pointer, EDX = pointer to P (x, y),
# [esp+4] = pointer to Q (x, y). Result = (Q.x-P.x)^2 + (Q.y-P.y)^2 with the fused rounding.
DIST2 = 0x6F6EF3B0

# Scratch memory inside the emulator (outside the image).
SCR = 0x00200000
RES = SCR + 0x000
OPA = SCR + 0x010
OPB = SCR + 0x020
STACK = 0x00100000
TOP = STACK + 0xF000
SENT = 0x00300000


class Hang(Exception):
    """The original did not return within the instruction budget."""


class Game:
    def __init__(self, path):
        with open(path, "rb") as f:
            raw = f.read()
        self.sha256 = hashlib.sha256(raw).hexdigest()
        pe = pefile.PE(data=raw, fast_load=True)
        self.base = pe.OPTIONAL_HEADER.ImageBase
        size = pe.OPTIONAL_HEADER.SizeOfImage
        img = bytes(pe.get_memory_mapped_image())
        mapsz = (size + 0xFFF) & ~0xFFF
        self.img = img
        self.mu = Uc(UC_ARCH_X86, UC_MODE_32)
        self.mu.mem_map(self.base, mapsz)
        self.mu.mem_write(self.base, img[:mapsz].ljust(mapsz, b"\0"))
        self.mu.mem_map(STACK, 0x10000)
        self.mu.mem_map(0x00200000, 0x10000)
        self.mu.mem_map(SENT, 0x1000)
        # the RNG/global pointer slot used by some natives; harmless for these helpers
        self.mu.mem_write(0x6FAB73D8, struct.pack("<I", 0x00201000))

    # ---- low level -------------------------------------------------------
    def call(self, fn, ecx=None, edx=None, stack=(), count=200_000):
        mu = self.mu
        esp = TOP
        for a in reversed(stack):
            esp -= 4
            mu.mem_write(esp, struct.pack("<I", a & 0xFFFFFFFF))
        esp -= 4
        mu.mem_write(esp, struct.pack("<I", SENT))
        mu.reg_write(UC_X86_REG_ESP, esp)
        if ecx is not None:
            mu.reg_write(UC_X86_REG_ECX, ecx & 0xFFFFFFFF)
        if edx is not None:
            mu.reg_write(UC_X86_REG_EDX, edx & 0xFFFFFFFF)
        mu.emu_start(fn, SENT, timeout=20_000_000, count=count)
        if mu.reg_read(UC_X86_REG_EIP) != SENT:
            # the instruction budget ran out: the original does not return for these inputs
            raise Hang(fn)
        return mu.reg_read(UC_X86_REG_EAX)

    def rd(self, addr):
        return struct.unpack("<I", bytes(self.mu.mem_read(addr, 4)))[0]

    def wr(self, addr, v):
        self.mu.mem_write(addr, struct.pack("<I", v & 0xFFFFFFFF))

    def init_constants(self):
        """Run the constant-pool initialiser stubs (see module docstring)."""
        for lo, hi in ((0x6F84DC00, 0x6F84E340), (0x6F863A00, 0x6F864000)):
            for a in self._stub_starts(lo, hi):
                try:
                    self.call(a, count=4000)
                except Exception:  # noqa: BLE001 - a non-stub may fault; keep going
                    pass

    def _stub_starts(self, lo, hi):
        out = []
        for a in range(lo, hi):
            off = a - self.base
            b = self.img[off]
            prev = self.img[off - 1]
            if prev in (0xC3, 0xCC) and b not in (0xCC, 0xC3):
                out.append(a)
        return out

    # ---- helper wrappers (value in, value out; bit patterns) -------------
    def binop(self, fn, a, b):
        self.wr(OPA, a)
        self.wr(OPB, b)
        self.call(fn, ecx=RES, edx=OPA, stack=[OPB])
        return self.rd(RES)

    def unop(self, fn, a):
        self.wr(OPA, a)
        self.call(fn, ecx=RES, edx=OPA)
        return self.rd(RES)

    def i2r(self, n):
        self.call(I2R, ecx=RES, edx=n)
        return self.rd(RES)

    def f2i(self, a):
        self.wr(OPA, a)
        return self.call(F2I, ecx=OPA) & 0xFFFFFFFF

    def r2i(self, a):
        self.wr(OPA, a)
        return self.call(R2I, ecx=OPA) & 0xFFFFFFFF

    def dist2(self, ax, ay, bx, by):
        self.wr(OPA, ax)
        self.wr(OPA + 4, ay)
        self.wr(OPB, bx)
        self.wr(OPB + 4, by)
        self.call(DIST2, ecx=RES, edx=OPA, stack=[OPB])
        return self.rd(RES)

    def cmp(self, fn, a, b):
        self.wr(OPA, a)
        self.wr(OPB, b)
        if fn == CMP_APPROX:  # fastcall: ECX = A, EDX = B
            return self.call(fn, ecx=OPA, edx=OPB) & 0xFFFFFFFF
        return self.call(fn, ecx=OPA, stack=[OPB]) & 0xFFFFFFFF


# ---- input generation ----------------------------------------------------
SPECIAL = [
    0x00000000, 0x80000000, 0x00000001, 0x80000001, 0x007FFFFF, 0x00400000,
    0x00800000, 0x80800000, 0x7F7FFFFF, 0xFF7FFFFF, 0x7F800000, 0xFF800000,
    0x7FC00000, 0xFFC00000, 0x3F800000, 0xBF800000, 0x40000000, 0xC0000000,
    0x3F000000, 0x3F800001, 0x3F7FFFFF, 0x4B000000, 0x4B800000, 0x4B7FFFFF,
    0x4B800001, 0xCB000000, 0x41200000, 0x42C80000, 0x3DCCCCCD, 0x40490FDB,
    0x3FC90FDB, 0x40C90FDB, 0x00000100, 0x01000000, 0x0000FFFF, 0x12345678,
]


def rand_bits(rng):
    """Random binary32 pattern, biased toward the ranges the game actually uses."""
    kind = rng.random()
    if kind < 0.20:
        return rng.getrandbits(32)
    if kind < 0.55:  # ordinary magnitudes
        e = rng.randint(127 - 24, 127 + 24)
        return (rng.getrandbits(1) << 31) | (e << 23) | rng.getrandbits(23)
    if kind < 0.70:  # any exponent
        return (rng.getrandbits(1) << 31) | (rng.randint(0, 255) << 23) | rng.getrandbits(23)
    if kind < 0.78:  # powers of two (fraction zero)
        return (rng.getrandbits(1) << 31) | (rng.randint(1, 254) << 23)
    if kind < 0.84:  # denormal / zero exponent
        return (rng.getrandbits(1) << 31) | rng.getrandbits(23)
    if kind < 0.90:  # integers near 2^24 and small integers
        n = rng.choice([rng.randint(-40000, 40000), rng.randint(-(1 << 25), 1 << 25)])
        return struct.unpack("<I", struct.pack("<f", float(n)))[0]
    return rng.choice(SPECIAL)


def near(rng, a):
    """A value close to `a`, to exercise cancellation and alignment shifts."""
    r = rng.random()
    if r < 0.4:
        return a ^ rng.getrandbits(rng.randint(1, 12))
    if r < 0.7:
        e = ((a >> 23) & 0xFF) + rng.randint(-3, 3)
        e = max(0, min(255, e))
        return (rng.getrandbits(1) << 31) | (e << 23) | (a & 0x7FFFFF)
    return (a & 0x80000000) ^ (0x80000000 if rng.random() < 0.5 else 0) | (a & 0x7FFFFFFF)


def binary_inputs(rng, n):
    out = []
    while len(out) < n:
        a = rand_bits(rng)
        r = rng.random()
        if r < 0.35:
            b = rand_bits(rng)
        elif r < 0.7:
            b = near(rng, a)
        else:
            b = rng.choice(SPECIAL) if rng.random() < 0.5 else rand_bits(rng)
        out.append((a, b))
    return out


def unary_inputs(rng, n):
    out = []
    while len(out) < n:
        if rng.random() < 0.6:
            out.append((rand_bits(rng),))
        else:
            out.append((rng.choice(SPECIAL),))
    return out


def trig_inputs(rng, n):
    """Arguments for sin/cos/tan/atan/... : mostly moderate magnitudes."""
    out = []
    while len(out) < n:
        r = rng.random()
        if r < 0.5:
            v = rng.uniform(-10.0, 10.0)
        elif r < 0.7:
            v = rng.uniform(-1.0, 1.0)
        elif r < 0.85:
            v = rng.uniform(-1e4, 1e4)
        else:
            out.append((rng.choice(SPECIAL),))
            continue
        out.append((struct.unpack("<I", struct.pack("<f", v))[0],))
    return out


def dist2_inputs(rng, n):
    """Two points (ax, ay, bx, by). Mixes ordinary magnitudes, cell-scale values (the
    world coordinates divided by 32), exact repeats, cancellation (near) and specials."""
    out = []
    while len(out) < n:
        r = rng.random()
        if r < 0.3:
            ax, ay = rand_bits(rng), rand_bits(rng)
            bx, by = rand_bits(rng), rand_bits(rng)
        elif r < 0.6:
            # cell-scale points: centre and candidate within a few hundred cells
            def cell():
                v = rng.uniform(-1200.0, 1200.0) / 32.0
                return struct.unpack("<I", struct.pack("<f", v))[0]
            ax, ay, bx, by = cell(), cell(), cell(), cell()
        elif r < 0.85:
            ax, ay = rand_bits(rng), rand_bits(rng)
            bx, by = near(rng, ax), near(rng, ay)
        elif r < 0.93:
            ax, ay = rand_bits(rng), rand_bits(rng)
            bx, by = ax, ay if rng.random() < 0.5 else rand_bits(rng)
        else:
            ax, ay = rng.choice(SPECIAL), rng.choice(SPECIAL)
            bx, by = rng.choice(SPECIAL), rng.choice(SPECIAL)
        out.append((ax, ay, bx, by))
    return out


def int_inputs(rng, n):
    out = []
    while len(out) < n:
        r = rng.random()
        if r < 0.4:
            v = rng.randint(-(1 << 31), (1 << 31) - 1)
        elif r < 0.8:
            v = rng.randint(-(1 << 25), 1 << 25)
        else:
            v = rng.choice([0, 1, -1, 2, -2, 255, 256, 65535, 1 << 30, -(1 << 31), (1 << 31) - 1,
                            16777216, 16777217, 16777219, -16777219])
        out.append(((v) & 0xFFFFFFFF,))
    return out


# ---- driver ----------------------------------------------------------------
def build_cases(game, rng, n, which):
    cases = []

    def add(fn, args, ret_fn):
        try:
            ret = ret_fn()
        except Hang:
            return  # skipped: the original does not terminate on these inputs
        cases.append({"fn": fn, "args": ["%08x" % x for x in args], "ret": "%08x" % ret})

    if "add" in which:
        for a, b in binary_inputs(rng, n):
            add("add", (a, b), lambda: game.binop(ADD, a, b))
    if "sub" in which:
        for a, b in binary_inputs(rng, n):
            add("sub", (a, b), lambda: game.binop(SUB, a, b))
    if "mul" in which:
        for a, b in binary_inputs(rng, n):
            add("mul", (a, b), lambda: game.binop(MUL, a, b))
    if "div" in which:
        for a, b in binary_inputs(rng, n):
            add("div", (a, b), lambda: game.binop(DIV, a, b))
        for a, _ in binary_inputs(rng, max(1, n // 20)):
            add("div", (a, a), lambda: game.binop(DIV, a, a))
    if "i2r" in which:
        for (v,) in int_inputs(rng, n):
            add("i2r", (v,), lambda: game.i2r(v))
    if "f2i" in which:
        for (a,) in unary_inputs(rng, n):
            add("f2i", (a,), lambda: game.f2i(a))
    if "r2i" in which:
        for (a,) in unary_inputs(rng, n):
            add("r2i", (a,), lambda: game.r2i(a))
    for name, fn in (("lt", CMP_LT2), ("le", CMP_LE), ("gt", CMP_GT), ("eq", CMP_EQ), ("approx_eq", CMP_APPROX)):
        if name in which:
            for a, b in binary_inputs(rng, n):
                add(name, (a, b), lambda: game.cmp(fn, a, b))
    for name, fn in (("sqrt", SQRT), ("sin", SIN), ("cos", COS), ("tan", TAN),
                     ("asin", ASIN), ("acos", ACOS), ("atan", ATAN), ("exp", EXP),
                     ("ln", LN), ("trunc", TRUNC)):
        if name in which:
            gen = unary_inputs if name in ("sqrt", "exp", "ln", "trunc") else trig_inputs
            for (a,) in gen(rng, n):
                add(name, (a,), lambda: game.unop(fn, a))
    if "dist2" in which:
        for a, b, c, d in dist2_inputs(rng, n):
            add("dist2", (a, b, c, d), lambda: game.dist2(a, b, c, d))
    if "atan2" in which:
        for a, b in binary_inputs(rng, n):
            add("atan2", (a, b), lambda: game.binop(ATAN2, a, b))
    for name, fn in (("floor_b", FLOOR_B), ("ceil_b", CEIL_B), ("round_b", ROUND_B)):
        if name in which:
            for (a,) in unary_inputs(rng, n):
                add(name, (a,), lambda: game.unop(fn, a))
    if "pow" in which:
        for a, b in binary_inputs(rng, n):
            add("pow", (a, b), lambda: game.binop(POW, a, b))
    return cases


ALL = ["add", "sub", "mul", "div", "i2r", "f2i", "r2i", "lt", "le", "gt", "eq", "approx_eq",
       "sqrt", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "exp", "ln", "pow", "trunc",
       "floor_b", "ceil_b", "round_b", "dist2"]


def write_embedded(cases, path, per_fn):
    """Writes a small, evenly spaced sample of the recorded cases as Rust constants
    (numbers only) for crates/softfloat/tests/embedded.rs."""
    by_fn = {}
    for c in cases:
        by_fn.setdefault(c["fn"], []).append(c)
    lines = [
        "//! Embedded observed input/output pairs (numbers only, recorded from the original",
        "//! `Game.dll` 1.26a). Generated by `tools/emu/softfloat_vectors.py --embed-rs`.",
        "",
        "/// One recorded call: argument bit patterns and the result bits.",
        "pub type Case = (&'static [u32], u32);",
        "",
        "/// `(function, cases)`.",
        "pub const CASES: &[(&str, &[Case])] = &[",
    ]
    for fn in sorted(by_fn):
        group = by_fn[fn]
        step = max(1, len(group) // per_fn)
        picked = group[::step][:per_fn]
        lines.append(f'    ("{fn}", &[')
        for c in picked:
            args = ", ".join("0x" + a for a in c["args"])
            lines.append(f"        (&[{args}], 0x{c['ret']}),")
        lines.append("    ]),")
    lines.append("];")
    lines.append("")
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--dll", default=os.environ.get("GAME_DLL"), help="path to Game.dll 1.26a")
    ap.add_argument("--out", required=True, help="output JSON path")
    ap.add_argument("--n", type=int, default=10000, help="cases per function")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--only", default=",".join(ALL), help="comma separated function names")
    ap.add_argument("--embed-rs", help="also write the embedded Rust test set to this path")
    ap.add_argument("--embed-per-fn", type=int, default=32, help="cases per function in the embedded set")
    args = ap.parse_args(argv)
    if not args.dll:
        ap.error("set GAME_DLL or pass --dll")
    game = Game(args.dll)
    game.init_constants()
    rng = random.Random(args.seed)
    which = set(args.only.split(","))
    cases = build_cases(game, rng, args.n, which)
    if args.embed_rs:
        write_embedded(cases, args.embed_rs, args.embed_per_fn)
    doc ={"format": FORMAT, "dll_sha256": game.sha256, "seed": args.seed, "cases": cases}
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(doc, f, separators=(",", ":"))
        f.write("\n")
    counts = {}
    for c in cases:
        counts[c["fn"]] = counts.get(c["fn"], 0) + 1
    print(json.dumps(counts, sort_keys=True))


if __name__ == "__main__":
    main()
