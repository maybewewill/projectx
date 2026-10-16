"""V6 verification wave: Unicorn cores of item and neutral abilities of Game.dll 1.26a (docs/spec/abilities-items.md s19,
docs/spec/abilities-neutral.md s9).

Same method as item_cores3_emu.py: the ORIGINAL code on fake objects, callees that need the object model stubbed and
recorded, the soft-float library and the level data getters run for real. Rows go to crates/sim/tests/v6_vectors.txt and
are recomputed by crates/sim/tests/v6_vectors.rs.

Row kinds (hex unless noted, `|` separates input from output):
  TN  cx cy ux uy best haveBest first excl | replaced best ret   Town Portal destination callback 0x6F0264B0
  (more kinds are appended below as they are verified)

Usage: GAME_DLL=/path/Game.dll py tools/emu/v6_emu.py OUT.txt [CASES] [SEED] [KIND,KIND]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import item_cores_emu as C  # noqa: E402
import item_cores2_emu as C2  # noqa: E402
import item_cores3_emu as C3  # noqa: E402

T = C.T
mu = C.mu
D = C.D
w32 = C.w32
r32 = C.r32
rand_real = C.rand_real
fbits = C.fbits
T.ALLOW.append((0x6F01FD00, 0x6F01FD60))
T.ALLOW.append((0x6F6EE000, 0x6F6F1000))
setrec = C3.setrec


def signed(v):
    return v - (1 << 32) if v >= 1 << 31 else v


# ---------------------------------------------------------------- TN: Town Portal destination callback
TN_CB = 0x6F0264B0
TN_CTX = D + 0xC000
TN_CAND = D + 0xC400
TN_CVT = D + 0xC800
TN_OBJ = D + 0xCC00
TN_OVT = D + 0xD000
TN_S_POS = D + 0xD400
TN_S_EXCL = D + 0xD420
TN_NORM = 0x6F4743A0


def case_tn(r):
    first = r.choice([0, 0, 1])
    have = r.choice([0, 1, 1])
    excl = r.choice([0, 0, 0, 1])
    cx, cy, ux, uy = (rand_real(r, small=True) for _ in range(4))
    best = rand_real(r, small=True)
    mu.mem_write(TN_CTX, bytes(0x40))
    mu.mem_write(TN_CAND, bytes(0x40))
    mu.mem_write(TN_OBJ, bytes(0x40))
    for i in range(0x200):
        w32(TN_CVT + 4 * i, T.RETSTUB)
        w32(TN_OVT + 4 * i, T.RETSTUB)
    w32(TN_CAND, TN_CVT)
    w32(TN_OBJ, TN_OVT)
    w32(TN_CVT + 0xB8, TN_S_POS)
    w32(TN_OVT + 0x3EC, TN_S_EXCL)
    w32(TN_CTX, TN_OBJ)
    w32(TN_CTX + 4, first)
    w32(TN_CTX + 8, cx)
    w32(TN_CTX + 0xC, cy)
    w32(TN_CTX + 0x10, 0x1234 if have else 0)
    w32(TN_CTX + 0x14, best)
    got = {}

    def pos(uc, c, d, st):
        got['buf'] = st[0]
        return st[0]

    def normalise(uc, c, d, st):
        # the callback reads the position at [esp+0x2c] (its own frame) - the buffer handed to slot 0xB8
        w32(got['buf'], ux)
        w32(got['buf'] + 4, uy)
        w32(got['buf'] + 8, 0)
        return 0

    ret, _, _ = T.trace_call(TN_CB, TN_CAND, (), edx=TN_CTX,
                 stub_fn={TN_S_POS: pos, TN_S_EXCL: lambda *x: excl, TN_NORM: normalise},
                 span=0x200, cleanup={TN_S_POS: 4, TN_S_EXCL: 4, TN_NORM: 0})
    replaced = 1 if r32(TN_CTX + 0x10) == TN_CAND else 0
    return 'TN %x %x %x %x %x %d %d %d | %d %x %x' % (cx, cy, ux, uy, best, have, first, excl, replaced, r32(TN_CTX + 0x14), ret)


GENS = [('TN', case_tn)]


def main():
    out = sys.argv[1]
    cases = int(sys.argv[2]) if len(sys.argv) > 2 else 300
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    only = sys.argv[4].split(',') if len(sys.argv) > 4 else None
    r = random.Random(seed)
    rows = []
    for name, fn in GENS:
        if only and name not in only:
            continue
        for _ in range(cases):
            rows.append(fn(r))
    with open(out, 'w') as fh:
        fh.write('\n'.join(rows) + '\n')
    print(len(rows), 'rows')


if __name__ == '__main__':
    main()
