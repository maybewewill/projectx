#!/usr/bin/env python3
"""Pathing vectors from the ORIGINAL Game.dll 1.26a, by Unicorn emulation.

Usage:
    GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn+pefile> \
        python3 tools/emu/pathing_vectors.py [--heap-seqs 3000] [--out PATH]

Writes crates/sim/tests/pathing/data.rs (digests and spot values only; no raw data). Every
expected value is an observation of the emulated machine code. The Python transcriptions
of the routines are used only as a second check: each emulated result is also compared with
them, and the mismatch counts are printed.

Covered original functions:
    isqrt        0x6F4BABB0  __fastcall: ecx = n, returns eax (approximate integer sqrt)
    heap push    0x6F498580  thiscall: ecx = heap object; stack = key, node, stamp
    heap pop     0x6F4985F0  thiscall: ecx = heap object; stack = out pointer (12 bytes),
                              returns the out pointer; the popped entry is copied there

Heap object fields used (read from the decompile): +0x0C array pointer, +0x14 growth step,
+0x18 capacity in entries, +0x1C size counter including the sentinel. An entry is 12 bytes
(key, node, stamp); entry 0 is the sentinel. Capacity is set large enough that the grow path
never calls the allocator, and the erase path never reaches memmove for pop (count 0).

Digests (all FNV-style multiply-add over 64 bits, words are u32):
    isqrt:  h = INIT; h = h * P + r over the results of inputs 0..=65535 then the random
            inputs (LCG below, seed ISQRT_SEED).
    heap:   per op, words = [kind, pop_key, pop_node, pop_stamp, size, entries...] where
            kind 0 = push, 1 = pop (pop words are 0 for a push), size counts the sentinel;
            op_hash = h over those words. Sequence digest = chain of op_hash values.
            Sequence s is generated from seed (s + 1) * 0x9E3779B97F4A7C15 (mod 2^64).
"""

import argparse
import hashlib
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402  (maps Game.dll into a Unicorn emulator at import time)
from emu import call, mu  # noqa: E402

M32 = 0xFFFFFFFF
M64 = 0xFFFFFFFFFFFFFFFF
DIGEST_INIT = 0xCBF29CE484222325
DIGEST_MUL = 0x100000001B3

ISQRT_FN = 0x6F4BABB0
HEAP_PUSH = 0x6F498580
HEAP_POP = 0x6F4985F0
ISQRT_RANDOM = 50_000
ISQRT_SEED = 0x5EED5EED

HEAP_BASE = 0x00400000  # private 64 KB area, outside everything emu.py maps
HOBJ = HEAP_BASE + 0x0000
ARR = HEAP_BASE + 0x1000
OUT = HEAP_BASE + 0xE000
CAP = 4096  # entries; sequences are at most 160 ops long, so the grow path never runs
mu.mem_map(HEAP_BASE, 0x10000)

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEFAULT_OUT = os.path.join(REPO, "crates", "sim", "tests", "pathing", "data.rs")


class Lcg:
    """64-bit LCG; u32() takes the top 32 bits. Same generator as the Rust tests."""

    def __init__(self, seed):
        self.x = seed & M64

    def u32(self):
        self.x = (self.x * 6364136223846793005 + 1442695040888963407) & M64
        return self.x >> 32


def digest_words(h, words):
    for w in words:
        h = (h * DIGEST_MUL + w) & M64
    return h


def isqrt_ref(n):
    """Transcription of the decompiled routine (guess, Newton step, stop rule)."""
    if n < 0x100:
        u = n // 12 + 1
    elif n < 0x10000:
        u = n // 200 + 0x15
    else:
        hi = ((n * 0x39ACC69D) >> 32) & M32
        u = ((((n - hi) >> 1) + hi) >> 14) + 0x1BC
    while True:
        q = n // u
        d = u - q  # signed in C
        u = (q + u) // 2  # q + u is non-negative here
        if tdiv(d, 2) == 0:
            return u


def tdiv(a, b):
    """C-style integer division (truncates toward zero)."""
    q = abs(a) // abs(b)
    return q if (a >= 0) == (b > 0) else -q


class HeapRef:
    """Transcription of the decompiled push and pop (slot 0 is the sentinel)."""

    def __init__(self):
        self.slots = [(0, 0, 0)]

    def push(self, key, node, stamp):
        self.slots.append((0, 0, 0))
        i = len(self.slots) - 1
        while i > 1:
            p = i >> 1
            if self.slots[p][0] < key:
                break
            self.slots[i] = self.slots[p]
            i = p
        self.slots[i] = (key, node, stamp)

    def pop(self):
        root = self.slots[1]
        last = self.slots.pop()
        if len(self.slots) > 1:
            last_index = len(self.slots) - 1
            half = last_index >> 1
            hole = 1
            if half != 0:
                while True:
                    child = hole << 1
                    if child < last_index and self.slots[child + 1][0] <= self.slots[child][0]:
                        child += 1
                    if last[0] <= self.slots[child][0]:
                        break
                    self.slots[hole] = self.slots[child]
                    hole = child
                    if child > half:
                        break
            self.slots[hole] = last
        return root

    def words(self, kind, popped):
        out = [kind] + list(popped) + [len(self.slots)]
        for e in self.slots:
            out.extend(e)
        return out


def emu_isqrt(n):
    return call(ISQRT_FN, ecx=n) & M32


def run_isqrt():
    inputs = list(range(65536))
    lcg = Lcg(ISQRT_SEED)
    inputs += [lcg.u32() for _ in range(ISQRT_RANDOM)]
    digest = DIGEST_INIT
    mismatches = 0
    for n in inputs:
        r = emu_isqrt(n)
        if r != isqrt_ref(n):
            mismatches += 1
            if mismatches <= 5:
                print(f"  isqrt mismatch: n={n:#x} emu={r} ref={isqrt_ref(n)}")
        digest = (digest * DIGEST_MUL + r) & M64
    print(f"isqrt: {len(inputs)} inputs ({65536} sweep + {ISQRT_RANDOM} random), "
          f"mismatches vs transcription: {mismatches}")
    spot_inputs = [0, 1, 2, 3, 4, 11, 12, 15, 16, 199, 200, 255, 256, 65535, 65536, 0x6876,
                   0x6877, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF, 1000000]
    spot = [(n, emu_isqrt(n)) for n in spot_inputs]
    if isqrt_mismatch_in(spot):
        sys.exit("spot value mismatch")
    return digest, spot


def isqrt_mismatch_in(pairs):
    return any(r != isqrt_ref(n) for n, r in pairs)


def gen_ops(s):
    rng = Lcg((s + 1) * 0x9E3779B97F4A7C15)
    mask = [0x3, 0xF, 0xFF, 0xFFFFFFFF][s % 4]
    length = 1 + rng.u32() % 160
    ops = []
    size = 0
    for _ in range(length):
        r = rng.u32() % 100
        if r < 60 or size == 0:
            key = rng.u32() & mask
            node = rng.u32() % 1000
            stamp = rng.u32()
            ops.append(("push", key, node, stamp))
            size += 1
        else:
            ops.append(("pop",))
            size -= 1
    return ops


def emu_reset_heap():
    obj = bytearray(0x20)
    struct.pack_into("<I", obj, 0x0C, ARR)
    struct.pack_into("<I", obj, 0x14, 0x40)
    struct.pack_into("<I", obj, 0x18, CAP)
    struct.pack_into("<I", obj, 0x1C, 1)
    mu.mem_write(HOBJ, bytes(obj))
    mu.mem_write(ARR, bytes(12))


def run_heap(seq_count):
    seqs = []
    total_ops = 0
    total_pops = 0
    mismatches = 0
    for s in range(seq_count):
        ops = gen_ops(s)
        emu_reset_heap()
        ref = HeapRef()
        digest = DIGEST_INIT
        for op in ops:
            if op[0] == "push":
                _, key, node, stamp = op
                call(HEAP_PUSH, ecx=HOBJ, stack_args=[key, node, stamp])
                ref.push(key, node, stamp)
                popped = (0, 0, 0)
                kind = 0
            else:
                call(HEAP_POP, ecx=HOBJ, stack_args=[OUT])
                popped = struct.unpack("<3I", bytes(mu.mem_read(OUT, 12)))
                ref_popped = ref.pop()
                kind = 1
                total_pops += 1
                if tuple(ref_popped) != tuple(popped):
                    mismatches += 1
            size = struct.unpack("<I", bytes(mu.mem_read(HOBJ + 0x1C, 4)))[0]
            raw = bytes(mu.mem_read(ARR, size * 12))
            words = [kind, *popped, size, *struct.unpack("<%dI" % (3 * size), raw)]
            if words != ref.words(kind, popped):
                mismatches += 1
            op_hash = digest_words(DIGEST_INIT, words)
            digest = (digest * DIGEST_MUL + op_hash) & M64
            total_ops += 1
        seqs.append((len(ops), digest))
    print(f"heap: {seq_count} sequences, {total_ops} operations ({total_pops} pops), "
          f"mismatches vs transcription: {mismatches}")
    return seqs, mismatches


def game_dll_sha256():
    path = os.environ.get("GAME_DLL", "Game.dll")
    with open(path, "rb") as fh:
        return hashlib.sha256(fh.read()).hexdigest()


def write_rust(path, isqrt_digest, spot, seqs):
    lines = []
    lines.append("//! Observed outputs of the ORIGINAL Game.dll 1.26a, produced by")
    lines.append("//! tools/emu/pathing_vectors.py (Unicorn emulation of the binary). Numbers only.")
    lines.append(f"//! Game.dll sha256: {game_dll_sha256()}")
    lines.append("//!")
    lines.append("//! ISQRT_DIGEST: multiply-add digest (see the script header) of the results for")
    lines.append("//! inputs 0..=65535 and then ISQRT_RANDOM values from the generator in main.rs.")
    lines.append("//! HEAP_SEQUENCES: (operation count, chained digest) for sequence s, generated")
    lines.append("//! from seed (s + 1) * 0x9E37_79B9_7F4A_7C15 in main.rs.")
    lines.append("")
    lines.append(f"pub const ISQRT_RANDOM: usize = {ISQRT_RANDOM};")
    lines.append(f"pub const ISQRT_SEED: u64 = 0x{ISQRT_SEED:016X};")
    lines.append(f"pub const ISQRT_DIGEST: u64 = 0x{isqrt_digest:016X};")
    lines.append("/// (input, output) pairs checked one by one.")
    lines.append("pub const ISQRT_SPOT: &[(u32, u32)] = &[")
    for n, r in spot:
        lines.append(f"    (0x{n:08X}, {r}),")
    lines.append("];")
    lines.append("/// (operation count, digest) per heap sequence.")
    lines.append("pub const HEAP_SEQUENCES: &[(u32, u64)] = &[")
    for n_ops, d in seqs:
        lines.append(f"    ({n_ops}, 0x{d:016X}),")
    lines.append("];")
    lines.append("")
    with open(path, "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--heap-seqs", type=int, default=3000)
    ap.add_argument("--out", default=DEFAULT_OUT)
    args = ap.parse_args()

    isqrt_digest, spot = run_isqrt()
    seqs, heap_mismatches = run_heap(args.heap_seqs)
    write_rust(args.out, isqrt_digest, spot, seqs)
    print(f"wrote {args.out}")
    if heap_mismatches:
        print("HEAP MISMATCHES FOUND", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
