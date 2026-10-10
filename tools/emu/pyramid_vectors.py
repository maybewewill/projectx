#!/usr/bin/env python3
"""Pathing pyramid labeller and lookup: differential test against the ORIGINAL Game.dll 1.26a.

Usage:
    GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn+pefile> \
        python3 tools/emu/pyramid_vectors.py [--cases 2000] [--rs crates/sim/tests/pathing/pyramid_data.rs]

What is compared, per operation (a lookup with a given level and cell), in every case:
  * the return value of 0x6F498A60 (lookup, thiscall: ecx = search object S, args level, x, y);
  * the list count S+0x6C, the generation S+0x2C and every pyramid record (stamp, word1);
  * every node-list entry (36 bytes, read from the list base at S+0x5C) up to the count.
The Python model below is a transcription of the decompile of 0x6F498A60 (lookup) and 0x6F498A00
(labeller), 0x6F4984E0 (append), 0x6F498010 (copy). The original is the reference: any difference
between the model and the emulated machine is reported as a mismatch. The allocator 0x6F4B39C0 is
replaced by a Python hook (it talks to Storm.dll, which is not in the upload); the hook keeps the
data pointer the caller reads (list base at +0x0C) and returns 1 like the original does.

Each case is generated from a 64-bit seed with the LCG below; the Rust test regenerates the same
cases from the seed and checks one digest per case (see `digest_*` and crates/sim/tests/pathing).

Object layout used (read from the decompile):
    S (search object):  +0x1C..+0x28 level pointers (level 0 first), +0x2C generation,
                        +0x50 list struct W (+0x5C base, +0x64 grow, +0x68 capacity, +0x6C count),
                        +0x90 mode, +0xD4 path type T, +0xD8 flag enable.
    level object:       +0x28 record array, +0x3C width, +0x40 height.
    record:             8 bytes: +0 stamp (u32), +4 word1 (low 16 bits = region id, bits 16..31 = state).

The state of a record for path type T is the pair of bits (30-T, 31-T) of word1; state 01 blocks
at every level, state 10 blocks at level 0 and asks for a finer look above it.
"""

import argparse
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402  (maps Game.dll into a Unicorn emulator at import time)
from emu import mu, call, u32  # noqa: E402
from unicorn import UC_HOOK_CODE, UC_PROT_ALL  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_ECX,
    UC_X86_REG_EAX,
    UC_X86_REG_EIP,
    UC_X86_REG_ESP,
)

LOOKUP = 0x6F498A60
ALLOC = 0x6F4B39C0
BASE = emu.BASE
STAGING = 0x6FA84FD0  # the labeller's 36-byte staging entry (global)
ENTRY = 36

M32 = 0xFFFFFFFF
M64 = 0xFFFFFFFFFFFFFFFF
DIGEST_INIT = 0xCBF29CE484222325
DIGEST_MUL = 0x100000001B3

S_ADDR = 0x00500000
LEVEL_ADDR = [0x00500200 + 0x40 * k for k in range(4)]
REC_ADDR = [0x00510000 + 0x8000 * k for k in range(4)]
HEAP_ARENA = 0x00600000
ARENA_END = 0x00800000
MAX_W = 4096  # node-list capacity is kept well below this in the generated cases

# ---------------------------------------------------------------- Python model of the original

class Lvl:
    def __init__(self, w, h):
        self.w = w
        self.h = h
        self.stamp = [0] * (w * h)
        self.word1 = [0] * (w * h)


class Pyr:
    """The state the original reads: four levels, S.gen, the list W, T, flag enable, grow, cap."""

    def __init__(self, levels, path_type, flag_on, grow):
        self.levels = levels  # list of 4 Lvl, level 0 first
        self.T = path_type
        self.flag_on = flag_on
        self.gen = 1
        self.grow = grow
        self.cap = 0
        self.entries = []  # bytearray of 36 each, index = node id (the list W)
        self.staging = None  # set from the image on first use

    def count(self):
        return len(self.entries)


def state_of(word1, T):
    return ((word1 << (T & 0x1F)) & 0xC0000000)


COV = {'ret_-1': 0, 'ret_-2': 0, 'ret_id': 0, 'label_L0': 0, 'label_L1': 0,
       'label_L2': 0, 'label_L3': 0, 'append_fail': 0, 'grow': 0}


def model_append(p, entry):
    """0x6F4984E0 with count 1: returns 1 on success."""
    n = 1
    if len(p.entries) + n > p.cap:
        if p.grow == 0:
            COV['append_fail'] += 1
            return 0
        COV['grow'] += 1
        need = len(p.entries) + n - p.cap
        p.cap += max(p.grow, need)
    p.entries.append(entry)
    return 1


def model_label(p, rec_lvl, idx, level, x, y, staging):
    lv = p.levels[rec_lvl]
    lv.stamp[idx] = p.gen & M32
    lv.word1[idx] = (lv.word1[idx] & 0xFFFF0000) | (len(p.entries) & 0xFFFF)
    flag = ((lv.word1[idx] >> 16) & 0xFF) if p.flag_on else 0
    s = bytearray(staging)
    struct.pack_into('<II', s, 0, x & M32, y & M32)
    s[0x20] = flag
    s[0x22] = level & 0xFF
    model_append(p, s)


def model_lookup(p, staging, L, x, y):
    """0x6F498A60. Returns the signed result. Records are addressed by (level, row-major index)."""
    lv = p.levels[L] if 0 <= L < 4 else None
    ux = x >> L
    uy = y >> L
    if lv is None or not (0 <= ux < lv.w and 0 <= uy < lv.h):
        return -1
    idx = uy * lv.w + ux
    st = state_of(lv.word1[idx], p.T)
    if st == 0x40000000:
        return -1
    if st == 0x80000000:
        return -1 if L == 0 else -2
    cur_level, cur_idx = L, idx
    if L < 3:
        for k in range(L + 1, 4):
            nk = p.levels[k]
            kx = x >> k
            ky = y >> k
            if not (0 <= kx < nk.w and 0 <= ky < nk.h):
                raise AssertionError('walk leaves the pyramid (not a valid case)')
            kidx = ky * nk.w + kx
            if state_of(nk.word1[kidx], p.T) != 0:
                break
            cur_level, cur_idx = k, kidx
    cur = p.levels[cur_level]
    if cur.stamp[cur_idx] != (p.gen & M32):
        COV['label_L%d' % cur_level] += 1
        model_label(p, cur_level, cur_idx, cur_level, x, y, staging)
    return cur.word1[cur_idx] & 0xFFFF


# ---------------------------------------------------------------- digests (shared with Rust)

def fnv_words(h, words):
    for w in words:
        h = ((h ^ (w & M32)) * DIGEST_MUL) & M64
    return h


def digest_state(p, ret):
    """One op's observable state: return, gen, count, all records, all entries (x, y, flag, level)."""
    words = [ret & M32, p.gen & M32, len(p.entries)]
    for lv in p.levels:
        for i in range(lv.w * lv.h):
            words.append(lv.stamp[i])
            words.append(lv.word1[i])
    for e in p.entries:
        x, y = struct.unpack_from('<II', e, 0)
        words += [x, y, e[0x20], e[0x22]]
    return words


# ---------------------------------------------------------------- deterministic generator

class Lcg:
    """64-bit LCG; `u32` is the top 32 bits. The Rust side uses the same constants."""

    def __init__(self, seed):
        self.s = ((seed + 1) * 0x9E3779B97F4A7C15) & M64

    def u32(self):
        self.s = (self.s * 6364136223846793005 + 1442695040888963407) & M64
        return self.s >> 32

    def below(self, n):
        return self.u32() % n


def make_case(seed):
    """Returns (w0, h0, T, flag_on, grow, cap0, levels_hi, requests) for a seed."""
    g = Lcg(seed)
    w0 = 8 * (1 + g.below(3))
    h0 = 8 * (1 + g.below(3))
    T = 2 * g.below(8)
    flag_on = g.below(2)
    grow = [0, 1, 2, 3, 4, 8][g.below(6)]
    cap0 = [0, 1, 2, 5][g.below(4)]
    dims = [(w0 >> k, h0 >> k) for k in range(4)]
    recs = []  # per level: list of (state, hi16)
    for (w, h) in dims:
        lvl = []
        for _ in range(w * h):
            r = g.below(100)
            state = 0 if r < 50 else (1 if r < 65 else (2 if r < 85 else 3))
            lvl.append((state, g.u32() & 0xFFFF))
        recs.append(lvl)
    nreq = 1 + g.below(4)
    requests = []
    for _ in range(nreq):
        bump = g.below(5) != 0
        nops = 1 + g.below(10)
        ops = []
        for _ in range(nops):
            L = g.below(4) if g.below(3) == 0 else 0
            x = g.below(w0 + 4) - 2
            y = g.below(h0 + 4) - 2
            ops.append((L, x, y))
        requests.append((bump, ops))
    return w0, h0, T, flag_on, grow, cap0, recs, requests


def build_model(case, staging):
    w0, h0, T, flag_on, grow, cap0, recs, _ = case
    levels = []
    for k in range(4):
        w, h = w0 >> k, h0 >> k
        lv = Lvl(w, h)
        for i, (state, hi16) in enumerate(recs[k]):
            word1 = hi16 << 16
            # place the 2-bit state of type T at (31-T, 30-T)
            word1 &= ~((1 << (31 - T)) | (1 << (30 - T))) & M32
            word1 |= ((state >> 1) << (31 - T)) | ((state & 1) << (30 - T))
            lv.word1[i] = word1 & M32
        levels.append(lv)
    p = Pyr(levels, T, flag_on, grow)
    p.cap = cap0
    p.staging = staging
    return p


# ---------------------------------------------------------------- emulator harness

class Harness:
    def __init__(self):
        mu.mem_map(0x00500000, 0x00300000, UC_PROT_ALL)
        self.arena = HEAP_ARENA
        self.sizes = {}
        self.calls_alloc = 0
        mu.hook_add(UC_HOOK_CODE, self._alloc_hook, None, ALLOC, ALLOC)

    def reset_arena(self):
        self.arena = HEAP_ARENA
        self.sizes = {}

    def _alloc_hook(self, uc, addr, size, user):
        # __thiscall, callee cleans two stack arguments: (new byte size, flag). ECX = descriptor D.
        d = uc.reg_read(UC_X86_REG_ECX)
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = struct.unpack('<I', bytes(uc.mem_read(esp, 4)))[0]
        n = struct.unpack('<I', bytes(uc.mem_read(esp + 4, 4)))[0]
        old_base = struct.unpack('<I', bytes(uc.mem_read(d + 0xC, 4)))[0]
        old_size = self.sizes.get(d, 0)
        new = (self.arena + 15) & ~15
        self.arena = new + max(n, 16)
        if self.arena > ARENA_END:
            raise MemoryError('arena exhausted')
        if old_base and old_size:
            keep = min(old_size, n)
            uc.mem_write(new, bytes(uc.mem_read(old_base, keep)))
        uc.mem_write(d + 0xC, struct.pack('<I', new))
        self.sizes[d] = n
        self.calls_alloc += 1
        uc.reg_write(UC_X86_REG_EAX, 1)
        uc.reg_write(UC_X86_REG_ESP, esp + 12)
        uc.reg_write(UC_X86_REG_EIP, ret)

    def write_state(self, p, cap0):
        """Writes the model's initial state into the emulator's memory."""
        self.reset_arena()
        S = S_ADDR
        mu.mem_write(S, b'\0' * 0x200)
        for k in range(4):
            lv = p.levels[k]
            lo = LEVEL_ADDR[k]
            mu.mem_write(S + 0x1C + 4 * k, struct.pack('<I', lo))
            mu.mem_write(lo + 0x28, struct.pack('<I', REC_ADDR[k]))
            mu.mem_write(lo + 0x3C, struct.pack('<I', lv.w))
            mu.mem_write(lo + 0x40, struct.pack('<I', lv.h))
            buf = bytearray()
            for i in range(lv.w * lv.h):
                buf += struct.pack('<II', lv.stamp[i], lv.word1[i])
            mu.mem_write(REC_ADDR[k], bytes(buf))
        mu.mem_write(S + 0x2C, struct.pack('<I', p.gen))
        mu.mem_write(S + 0x64, struct.pack('<I', p.grow))
        mu.mem_write(S + 0x68, struct.pack('<I', 0))
        mu.mem_write(S + 0x6C, struct.pack('<I', 0))
        mu.mem_write(S + 0x90, struct.pack('<I', 1))
        mu.mem_write(S + 0xD4, struct.pack('<I', p.T))
        mu.mem_write(S + 0xD8, struct.pack('<I', p.flag_on))
        # initial capacity: a block of cap0 entries, so the list starts with a base pointer
        base = 0
        if cap0:
            base = self.arena
            self.arena += cap0 * ENTRY + 16
            self.sizes[S + 0x50] = cap0 * ENTRY
        mu.mem_write(S + 0x5C, struct.pack('<I', base))
        mu.mem_write(S + 0x68, struct.pack('<I', cap0))

    def start_request(self, p, bump):
        """Request start as far as the lookup tests need it: count = 0 (erase), generation bump."""
        if bump:
            p.gen = (p.gen + 1) & M32
        p.entries = []
        mu.mem_write(S_ADDR + 0x6C, struct.pack('<I', 0))
        mu.mem_write(S_ADDR + 0x2C, struct.pack('<I', p.gen))

    def lookup(self, L, x, y):
        ret = call(LOOKUP, ecx=S_ADDR, stack_args=(L, x, y))
        return ret - (1 << 32) if ret & 0x80000000 else ret

    def read_state(self, p):
        """Reads back what the original holds, in the model's shape (for comparison)."""
        S = S_ADDR
        count = struct.unpack('<I', bytes(mu.mem_read(S + 0x6C, 4)))[0]
        gen = struct.unpack('<I', bytes(mu.mem_read(S + 0x2C, 4)))[0]
        cap = struct.unpack('<I', bytes(mu.mem_read(S + 0x68, 4)))[0]
        base = struct.unpack('<I', bytes(mu.mem_read(S + 0x5C, 4)))[0]
        entries = []
        for i in range(count):
            entries.append(bytearray(mu.mem_read(base + i * ENTRY, ENTRY)))
        recs = []
        for k in range(4):
            lv = p.levels[k]
            raw = bytes(mu.mem_read(REC_ADDR[k], lv.w * lv.h * 8))
            st = [struct.unpack_from('<II', raw, 8 * i) for i in range(lv.w * lv.h)]
            recs.append(st)
        return count, gen, cap, entries, recs


def compare_state(p, ret_model, ret_emu, h, mism, where):
    count, gen, cap, entries, recs = h.read_state(p)
    ok = ret_model == ret_emu and count == len(p.entries) and gen == p.gen and cap == p.cap
    if ok:
        ok = all(entries[i] == p.entries[i] for i in range(count))
    if ok:
        for k in range(4):
            lv = p.levels[k]
            for i in range(lv.w * lv.h):
                if recs[k][i] != (lv.stamp[i], lv.word1[i]):
                    ok = False
                    break
            if not ok:
                break
    if not ok:
        mism.append((where, ret_model, ret_emu, count, len(p.entries)))
    return ok


def run_case(h, seed, staging_holder, mism):
    case = make_case(seed)
    w0, h0, T, flag_on, grow, cap0, recs, requests = case
    p = build_model(case, None)
    p.gen = 1
    h.write_state(p, cap0)
    dig = DIGEST_INIT
    nops = 0
    for ri, (bump, ops) in enumerate(requests):
        # the list is erased, not freed, between requests: the capacity carries over
        h.start_request(p, bump)
        for oi, (L, x, y) in enumerate(ops):
            rm = model_lookup(p, staging_holder[0], L, x, y)
            re = h.lookup(L, x, y)
            compare_state(p, rm, re, h, mism, (seed, ri, oi, L, x, y))
            COV['ret_id' if re >= 0 else 'ret_%d' % re] += 1
            dig = fnv_words(dig, digest_state(p, re))
            nops += 1
    return dig, nops


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--cases', type=int, default=2000)
    ap.add_argument('--seed0', type=int, default=0)
    ap.add_argument('--rs', help='Rust test file whose generated block is replaced')
    a = ap.parse_args()

    h = Harness()
    staging_holder = [bytes(mu.mem_read(STAGING, ENTRY))]
    mism = []
    digests = []
    total_ops = 0
    for c in range(a.cases):
        seed = a.seed0 + c
        dig, nops = run_case(h, seed, staging_holder, mism)
        digests.append((seed, dig))
        total_ops += nops
    print('cases %d, lookups %d, mismatches %d, allocator calls %d' %
          (a.cases, total_ops, len(mism), h.calls_alloc))
    print('coverage', COV)
    for m in mism[:20]:
        print('  MISMATCH', m)
    if mism:
        sys.exit(1)
    if a.rs:
        splice_rs(a.rs, digests, a)
        print('wrote %d case digests to %s' % (len(digests), a.rs))


BEGIN = '// BEGIN GENERATED (tools/emu/pyramid_vectors.py)'
END = '// END GENERATED'


def splice_rs(path, digests, a):
    lines = [BEGIN,
             '// Recorded from the original Game.dll 1.26a by tools/emu/pyramid_vectors.py.',
             '// (seed, digest) per case; the case is regenerated from the seed (see main.rs).',
             'pub const PYRAMID_CASES: &[(u64, u64)] = &[']
    for seed, dig in digests:
        lines.append('    (%d, 0x%016X),' % (seed, dig))
    lines.append('];')
    lines.append(END)
    block = '\n'.join(lines) + '\n'
    src = open(path).read()
    if BEGIN in src:
        i = src.index(BEGIN)
        j = src.index(END) + len(END) + 1
        src = src[:i] + block + src[j:]
    else:
        src = src + '\n' + block
    open(path, 'w').write(src)


if __name__ == '__main__':
    main()
