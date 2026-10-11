#!/usr/bin/env python3
"""Growth probe for the unit-type table (0x6FAB445C): the creep walk order after the bucket array grows.

Extends unit_order_probe.py (same stand-ins) with a realloc stand-in for the Storm realloc wrapper 0x6F6EB5CA
(stdcall: ptr, size, file, line, flags; the old block is copied, the new block is zeroed beyond the old size) and
uses the REAL bucket initialiser 0x6F2A0BF0 instead of hand-built buckets. It inserts N records in a known order
(enough for several growths), then runs the real creep walk 0x6F29FD90 and compares the visit order with the
insertion order. Exit status 1 on any mismatch.

Usage: GAME_DLL=/path/to/Game.dll python3 tools/emu/unit_order_growth.py [--records 300] [--seed 1]
"""
import argparse
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import softfloat_vectors as sv  # noqa: E402
import unit_order_probe as P  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

REALLOC = 0x6F6EB5CA


def install_realloc(g, st):
    sizes = {}

    base_alloc = st["allocs"]

    def realloc_hook(uc, address, size, user):
        ptr, n, _f, _l, _fl = P.stack_args(uc, 5)
        old = 0
        for p, s in base_alloc:
            if p == ptr:
                old = s
        if ptr in sizes:
            old = sizes[ptr]
        new = st["next"]
        st["next"] = (new + max(n, 1) + 15) & ~15
        if st["next"] > P.HEAP + P.HEAP_SIZE:
            raise RuntimeError("probe heap exhausted")
        uc.mem_write(new, b"\0" * max(n, 1))
        if ptr and old:
            uc.mem_write(new, bytes(uc.mem_read(ptr, min(old, n))))
        sizes[new] = n
        P.ret_from(uc, new, 20)

    g.mu.hook_add(UC_HOOK_CODE, realloc_hook, begin=REALLOC, end=REALLOC)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--records", type=int, default=300)
    ap.add_argument("--seed", type=int, default=1)
    args = ap.parse_args()
    path = os.environ.get("GAME_DLL")
    if not path:
        print("set GAME_DLL", file=sys.stderr)
        return 2
    g = sv.Game(path)
    P.patch_fs(g)
    st = P.install(g)
    install_realloc(g, st)
    P.make_table(g)
    g.call(P.BUCKET_INIT, ecx=P.T)
    rnd = random.Random(args.seed)
    codes = set()
    while len(codes) < args.records:
        c = "".join(chr(rnd.randrange(0x61, 0x7B)) for _ in range(4))
        codes.add(c)
    order = sorted(codes)
    rnd.shuffle(order)
    masks = set()
    nodes = []
    for c in order:
        nodes.append(P.add_record(g, P.pack_code(c)))
        masks.add(P.rd(g, P.T + 0x24))
    print("records", len(order), "bucket masks seen", sorted(masks))
    visits = P.run_creep(g, st)
    expect = [P.pack_code(c) for c in order]
    ok = visits == expect
    print("walk visits", len(visits), "== insertion order:", ok)
    if not ok:
        for i, (a, b) in enumerate(zip(visits, expect)):
            if a != b:
                print("first difference at", i, hex(a), hex(b))
                break
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
