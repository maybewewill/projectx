"""Differential test of the JASS hashtable's per-kind child map (TSHashTable<..., DWORD>) in Game.dll 1.26a.

The original map is run under Unicorn with its own code: constructor 0x6F3ADD60, insert 0x6F401810 (the
bucket-chain add with the growth check at 0x6F400800), unlink 0x6F3ADE10 (element base destructor, used for
removal) and bucket destroy 0x6F3ABF90 (param 0), which calls the element destructor (handle release, 0x6F3ADDF0)
for every element. Storm's allocator imports (IAT 0x6F86D540 free, 0x6F86D57C alloc, 0x6F86D58C realloc) are
the only stubs; the realloc stub copies to a fresh block.

Checked, per random sequence of add / remove / destroy:
  * the destroy order (the element indices in the order the original releases them), and
  * the growth trigger: the bucket count changes exactly at the add where the model predicts a grow.
Sequences in which the original grows are not written to the vectors: the post-growth (rehash) order is not
verified (see crates/sim/src/hashmap.rs). Writes the vectors to crates/sim/tests/hashmap/data.rs with --rs.

    GAME_DLL=/path/Game.dll PYTHONPATH=<dir with unicorn, pefile> \
        python3 tools/emu/hashmap_vectors.py --seqs 3000 --rs crates/sim/tests/hashmap/data.rs
"""
import argparse
import os
import random
import struct
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), 'pylib'))
import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_PROT_ALL
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EIP

DLL = os.environ.get('GAME_DLL', 'Game.dll')
pe = pefile.PE(DLL, fast_load=True)
BASE = pe.OPTIONAL_HEADER.ImageBase
SIZE = (pe.OPTIONAL_HEADER.SizeOfImage + 0xfff) & ~0xfff
IMG = bytes(pe.get_memory_mapped_image())

CTOR, INSERT, UNLINK, DESTROY = 0x6f3add60, 0x6f401810, 0x6f3ade10, 0x6f3abf90
VT_HANDLE = 0x6f941db4          # TSHashTable<GameHashTableHandleReg, DWORD>
IAT_FREE, IAT_ALLOC, IAT_REALLOC = 0x6f86d540, 0x6f86d57c, 0x6f86d58c
STUB_FREE, STUB_ALLOC, STUB_REALLOC = 0x00300010, 0x00300020, 0x00300030
SENT, TOP = 0x00300000, 0x0010f000
HEAP_BASE, HEAP_END = 0x00400000, 0x00900000
GROW_SCORE = 13        # growth when the chain score exceeds this (400800 / 400940: cmp [ecx+10],0xd; ja)
DECAY = 3              # score decays by 3 per add (6f400800: score-3 or 0)
MAX_MASK_GROW = 0x1fff # no growth once mask >= 0x1fff


mu = Uc(UC_ARCH_X86, UC_MODE_32)
mu.mem_map(0, 0x1000, UC_PROT_ALL)
mu.mem_map(BASE, SIZE, UC_PROT_ALL)
mu.mem_write(BASE, IMG[:SIZE].ljust(SIZE, b'\0'))
mu.mem_map(0x00100000, 0x10000, UC_PROT_ALL)
mu.mem_map(0x00300000, 0x1000, UC_PROT_ALL)
mu.mem_map(HEAP_BASE, HEAP_END - HEAP_BASE, UC_PROT_ALL)
HEAP = [HEAP_BASE]
SIZES = {}
FREED = []


def rd(a):
    return struct.unpack('<I', bytes(mu.mem_read(a, 4)))[0]


def wr(a, v):
    mu.mem_write(a, struct.pack('<I', v & 0xffffffff))


def halloc(n):
    p = HEAP[0]
    n2 = (max(n, 1) + 15) & ~15
    HEAP[0] += n2
    SIZES[p] = n2
    mu.mem_write(p, b'\0' * n2)
    return p


def reg(r):
    return mu.reg_read(r)


def arg(i):
    return rd(reg(UC_X86_REG_ESP) + 4 + 4 * i)


def stdcall_ret(pop):
    esp = reg(UC_X86_REG_ESP)
    ret = rd(esp)
    mu.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
    mu.reg_write(UC_X86_REG_EIP, ret)


def h_alloc(uc):
    p = halloc(arg(0))
    stdcall_ret(16)
    mu.reg_write(UC_X86_REG_EAX, p)


def h_free(uc):
    p = arg(0)
    if p:
        FREED.append(p)
    stdcall_ret(16)
    mu.reg_write(UC_X86_REG_EAX, 0)


def h_realloc(uc):
    old, n = arg(0), arg(1)
    p = halloc(n)
    if old:
        mu.mem_write(p, bytes(mu.mem_read(old, min(SIZES.get(old, 0), n))))
    stdcall_ret(20)
    mu.reg_write(UC_X86_REG_EAX, p)


STUBS = {STUB_FREE: h_free, STUB_ALLOC: h_alloc, STUB_REALLOC: h_realloc}


def _hook(uc, addr, size, user):
    f = STUBS.get(addr)
    if f:
        f(uc)


mu.hook_add(UC_HOOK_CODE, _hook, None, 0x00300000, 0x00300100)
for iat, stub in ((IAT_FREE, STUB_FREE), (IAT_ALLOC, STUB_ALLOC), (IAT_REALLOC, STUB_REALLOC)):
    wr(iat, stub)


def call(fn, ecx=None, args=()):
    esp = TOP
    for a in reversed(args):
        esp -= 4
        wr(esp, a)
    esp -= 4
    wr(esp, SENT)
    mu.reg_write(UC_X86_REG_ESP, esp)
    if ecx is not None:
        mu.reg_write(UC_X86_REG_ECX, ecx & 0xffffffff)
    mu.emu_start(fn, SENT, timeout=20_000_000, count=5_000_000)
    return reg(UC_X86_REG_EAX)


class Model:
    """Verified no-growth model: buckets in insertion order (tail = last), chain score for growth."""

    def __init__(self):
        self.mask = -1
        self.b = []
        self.score = 0

    def add(self, key, eid):
        """Returns True when the add grows the table (the growth step itself is not modelled here)."""
        if self.mask == -1:
            self.mask = 3
            self.b = [[] for _ in range(4)]
        idx = key & self.mask
        if self.mask < MAX_MASK_GROW:
            self.score = self.score - DECAY if self.score > DECAY else 0
            for _ in self.b[idx]:
                self.score += 1
                if self.score > GROW_SCORE:
                    return True
        self.b[idx].append((key, eid))
        return False

    def remove(self, eid):
        for bucket in self.b:
            for i, (_, e) in enumerate(bucket):
                if e == eid:
                    del bucket[i]
                    return

    def destroy_order(self):
        out = []
        for bucket in self.b:
            out.extend(e for _, e in reversed(bucket))
        return out


def run_sequence(ops):
    """Runs ops on the original. Returns (destroy order, growth_at) where growth_at is the op index of the
    first add that grew the table, or None. Elements are numbered by their add order."""
    HEAP[0] = HEAP_BASE
    SIZES.clear()
    FREED.clear()
    H = halloc(0x40)
    call(CTOR, ecx=H)
    elems = []
    model = Model()
    growth_at = None
    for oi, (kind, val) in enumerate(ops):
        if kind == 'a':
            key = val
            expect_grow = model.add(key, len(elems))
            mask_before = rd(H + 0x24)
            N = call(INSERT, ecx=H, args=(key, 0, 0))
            wr(N + 0x00, key)
            wr(N + 0x14, key)
            wr(N + 0x1c, 0)           # owned flag 0: no handle release at destroy
            elems.append(N)
            grew = rd(H + 0x24) != mask_before and mask_before != 0xffffffff
            if grew != expect_grow:
                raise AssertionError("growth trigger mismatch at op %d: model %s, original %s"
                                     % (oi, expect_grow, grew))
            if grew:
                growth_at = oi
                break
        elif kind == 'r':
            if val < len(elems) and elems[val] is not None:
                model.remove(val)
                call(UNLINK, ecx=elems[val])
                elems[val] = None
    if growth_at is not None:
        return None, growth_at
    FREED.clear()
    call(DESTROY, ecx=H, args=(0,))
    order = [elems.index(p) for p in FREED if p in elems]
    expect = model.destroy_order()
    if order != expect:
        raise AssertionError("destroy order mismatch: model %s, original %s" % (expect, order))
    return order, None


def gen_sequence(rnd, n_ops):
    ops = []
    adds = 0
    for _ in range(n_ops):
        if adds and rnd.random() < 0.25:
            ops.append(('r', rnd.randrange(adds)))
        else:
            style = rnd.random()
            if style < 0.6:
                key = 4 * rnd.randrange(12)                  # heavy same-bucket collisions
            elif style < 0.9:
                key = rnd.randrange(64)
            else:
                key = rnd.randrange(0x7fffffff)
            ops.append(('a', key))
            adds += 1
    return ops


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--seqs', type=int, default=3000)
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--rs', default=None)
    a = ap.parse_args()
    rnd = random.Random(a.seed)
    vectors = []
    growth_seen = 0
    grow_points = []
    for s in range(a.seqs):
        ops = gen_sequence(rnd, rnd.randint(1, 40))
        order, grew_at = run_sequence(ops)
        if grew_at is not None:
            growth_seen += 1
            grow_points.append(grew_at)
            # the grow step is covered by the trigger check; the sequence stops there (not a vector)
            continue
        vectors.append((ops, order))
    print("sequences %d, verified vectors %d, sequences that grew (trigger checked, order not) %d"
          % (a.seqs, len(vectors), growth_seen))
    print("destroy orders match the model: 0 mismatches")
    if grow_points:
        print("growth trigger matched the model at every grow point (%d events)" % len(grow_points))
    if a.rs:
        write_rs(a.rs, vectors)
        print("wrote", a.rs)


def write_rs(path, vectors):
    lines = ["//! Generated by tools/emu/hashmap_vectors.py from Game.dll 1.26a (do not edit).",
             "//! Each vector is a sequence of ops and the element add-indices in the original's destroy order.",
             "//! Op encoding: (0, key) = add, (1, index) = remove the element added at that index.",
             "", "pub type Vector = (&'static [(u8, i32)], &'static [u32]);", "",
             "pub const VECTORS: &[Vector] = &["]
    for ops, order in vectors:
        enc = ", ".join("(%d, %d)" % (0 if k == 'a' else 1, v) for k, v in ops)
        lines.append("    (&[%s], &[%s])," % (enc, ", ".join(str(x) for x in order)))
    lines.append("];")
    with open(path, 'w') as f:
        f.write("\n".join(lines) + "\n")


if __name__ == '__main__':
    main()
