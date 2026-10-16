"""V6 verification wave: Unicorn cores of item and neutral abilities of Game.dll 1.26a (docs/spec/abilities-items.md s19,
docs/spec/abilities-neutral.md s9).

Same method as item_cores3_emu.py: the ORIGINAL code on fake objects, callees that need the object model stubbed and
recorded, the soft-float library and the level data getters run for real. Rows go to crates/sim/tests/v6_vectors.txt and
are recomputed by crates/sim/tests/v6_vectors.rs.

Row kinds (hex unless noted, `|` separates input from output):
  TN  cx cy ux uy best haveBest first excl | replaced best ret   Town Portal destination callback 0x6F0264B0
  TM  e0 e8 flagD illusion goldcost woodcost bounty_g bounty_w bounty_flag | called gold wood ...  Transmute buff action 0x6F1B59B0 -> 0x6F1B58B0

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


# ---------------------------------------------------------------- TM: Transmute buff action (inner body)
TM_INNER = 0x6F1B58B0
TM_TRAMP = D + 0xF000
TM_STRUCT = D + 0xF100
TM_OBJ = D + 0xE400
TM_S124 = D + 0xE800


def case_tm(r):
    s0, s1 = rand_real(r, small=True), rand_real(r, small=True)
    flag = r.choice([0, 1])
    ill = r.choice([0, 0, 0, 1])
    goldcost = r.choice([0, 50, 100, 135, 400, r.randint(0, 3000), r.randint(-5, 3000)])
    woodcost = r.choice([0, 0, 20, 60, 200, r.randint(0, 800)])
    bg, bw = r.randint(0, 200), r.randint(0, 100)
    bflag = r.choice([0, 1])
    T.setup(0x6F8C9F64)
    mu.mem_write(TM_OBJ, bytes(0x40))
    mu.mem_write(TM_STRUCT, struct.pack('<II', s0, s1))
    # trampoline: mov eax, 0x1234; mov ecx, inner; jmp ecx
    mu.mem_write(TM_TRAMP, bytes([0xB8]) + struct.pack('<I', TM_STRUCT + 4) + bytes([0xB9]) + struct.pack('<I', TM_INNER) + bytes([0xFF, 0xE1]))
    w32(T.UNIT + 0x5C, (0x40000000 if ill else 0) | r.choice([0, 0x1000]))
    w32(T.UVT + 0x124, TM_S124)
    got = {'calls': []}

    def rec(name, ret=0):
        def f(uc, c, d, st):
            got['calls'].append(name)
            if name == 'give':
                got['give'] = (signed(st[1]), signed(st[2]), st[3], st[4])
            return ret
        return f

    sf = {
        0x6F29B800: rec('gold', goldcost & 0xffffffff),
        0x6F29B830: rec('wood', woodcost & 0xffffffff),
        0x6F2834E0: rec('2834E0', TM_OBJ),
        0x6F473170: rec('473170', bflag),
        0x6F29DD30: rec('bgold', bg),
        0x6F29DDD0: rec('bwood', bw),
        0x6F28BAD0: rec('give'),
        0x6F2829D0: rec('setlife'),
        TM_S124: rec('slot124'),
    }
    # esi of the inner body is the unit: the real caller loads it from [buff + 0x30]; the trampoline sets ESI through a
    # code hook on the first instruction
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_ESI

    def seti(uc, a, sz, u):
        if a == TM_INNER:
            uc.reg_write(UC_X86_REG_ESI, T.UNIT)

    h = mu.hook_add(UC_HOOK_CODE, seti, begin=TM_INNER, end=TM_INNER)
    try:
        T.trace_call(TM_TRAMP, 0, (T.OWNER, 0x55, TM_STRUCT, flag, 0, 0, 0, 0, 0, 0), stub_fn=sf, into=(TM_INNER,), span=0x200,
                     cleanup={0x6F28BAD0: 20, 0x6F2829D0: 4, TM_S124: 4, 0x6F29B800: 0, 0x6F29B830: 0,
                              0x6F473170: 0, 0x6F29DD30: 0, 0x6F29DDD0: 0, 0x6F2834E0: 0})
    finally:
        mu.hook_del(h)
    out = 'TM %x %x %d %d %d %d %d %d %d |' % (s0, s1, flag, ill, goldcost & 0xffffffff, woodcost & 0xffffffff, bg, bw, bflag)
    if 'give' in got:
        g = got['give']
        out += ' 1 %d %d %x %x' % (g[0], g[1], g[2], g[3])
    else:
        out += ' 0 0 0 0 0'
    out += ' %d' % int('setlife' in got['calls'])
    return out


GENS = [('TN', case_tn), ('TM', case_tm)]


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
