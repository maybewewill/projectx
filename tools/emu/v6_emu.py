"""V6 verification wave: Unicorn cores of item and neutral abilities of Game.dll 1.26a (docs/spec/abilities-items.md s19,
docs/spec/abilities-neutral.md s9).

Same method as item_cores3_emu.py: the ORIGINAL code on fake objects, callees that need the object model stubbed and
recorded, the soft-float library and the level data getters run for real. Rows go to crates/sim/tests/v6_vectors.txt and
are recomputed by crates/sim/tests/v6_vectors.rs.

Row kinds (hex unless noted, `|` separates input from output):
  TN  cx cy ux uy best haveBest first excl | replaced best ret   Town Portal destination callback 0x6F0264B0
  CL  race tlevel buff blevel upper ill mhero munit max | refuses   creep level rule 0x6F1D2630 over the unit level 0x6F29DCA0
  CE  ill hasowner | kind    Charm effect 0x6F1B13E0: 1 = vtable 0x124(0.0) on an illusion target, 2 = owner change 0x6F1B0C80
  PA  srcill hero existing flag200 dur herodur a b c d | path dur a b c d8 dfull   poison attack apply 0x6F13F2F0 (Envenomed Spears); path 1 new buff, 2 refresh of the existing one, 3 stacking buff, 0 stacking skipped
  PS  (same fields as PA)      poison attack apply 0x6F13F500 (Slow Poison)
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


# ---------------------------------------------------------------- CL: creep level rule (Charm, Doom, Transmute targets)
CL_RULE = 0x6F1D2630
CL_LEVEL = 0x6F29DCA0
CL_BUFFVT = D + 0xA000
CL_BUFF = D + 0xA400
CL_S36C = D + 0xA800


def case_cl(r):
    race = r.choice([8, 8, 8, 7, 1, 2, 3, 4, 0, 9])
    tlevel = r.choice([0, 1, 2, 3, 5, 6, 9, 10, r.randint(-2, 40)])
    hasbuff = r.choice([0, 0, 1])
    bflag = r.choice([0, 1])
    blevel = r.choice([0, 1, 5, r.randint(-3, 60)])
    upper = r.choice([0, 0, 1])
    ill = r.choice([0, 0, 1])
    mhero = r.choice([10, 10, 100, 0, r.randint(0, 12)])
    munit = r.choice([100, 100, 5, 0, r.randint(0, 30)])
    mx = r.choice([0, 1, 2, 3, 5, 6, r.randint(-1, 12)])
    return run_cl(race, tlevel, hasbuff, bflag, blevel, upper, ill, mhero, munit, mx)


def run_cl(race, tlevel, hasbuff, bflag, blevel, upper, ill, mhero, munit, mx):
    T.setup(0x6F8C9F64)
    mu.mem_write(CL_BUFF, bytes(0x200))
    mu.mem_write(CL_BUFFVT, bytes(0x800))
    w32(CL_BUFF, CL_BUFFVT)
    w32(CL_BUFFVT + 0x36C, CL_S36C)
    w32(CL_BUFF + 0xE4, blevel & 0xffffffff)
    w32(T.UNIT + 0x5C, 0x40000000 if ill else 0)
    w32(T.UNIT + 0x30, ((0x41 if upper else 0x6E) << 24) | 0x444444)
    sf = {
        0x6F29C1B0: lambda *x: race,
        0x6F29BB80: lambda *x: tlevel & 0xffffffff,
        0x6F0787D0: lambda *x: CL_BUFF if hasbuff else 0,
        CL_S36C: lambda *x: bflag,
        0x6F009F90: lambda uc, c, d, st: mhero if d == 0x6F876208 else munit,
    }
    ret, _, _ = T.trace_call(CL_RULE, T.UNIT, (), stub_fn=sf, edx=mx & 0xffffffff, into=(CL_LEVEL,), span=0x200,
                             cleanup={0x6F29C1B0: 0, 0x6F29BB80: 0, CL_S36C: 0, 0x6F009F90: 4})
    return 'CL %d %d %d %d %d %d %d %d %d | %d' % (race, tlevel, hasbuff * (1 + bflag), blevel, upper, ill, mhero, munit, mx, ret & 1)


# ---------------------------------------------------------------- CE: Charm effect
CE_S124 = D + 0xA900


def case_ce(r):
    ill = r.choice([0, 1])
    hasown = r.choice([0, 1])
    T.setup(0x6F912E44)
    mu.mem_write(T.OWNER, bytes(0x40))
    w32(T.ABIL + 0x30, T.OWNER if hasown else 0)
    victim = D + 0xA980
    mu.mem_write(victim, bytes(0x100))
    w32(victim, T.UVT)
    w32(victim + 0x5C, (0x40000000 if ill else 0) | r.choice([0, 0x10]))
    w32(T.UVT + 0x124, CE_S124)
    got = []

    def rec(name):
        def f(uc, c, d, st):
            got.append((name, st[0]))
            return 0
        return f

    T.trace_call(0x6F1B13E0, T.ABIL, (victim,), stub_fn={CE_S124: rec('s124'), 0x6F1B0C80: rec('own'), 0x6F472890: lambda *x: T.OWNER},
                 span=0x100, cleanup={CE_S124: 4})
    kind = 0
    arg = 0
    for name, a in got:
        if name == 's124':
            kind, arg = 1, a
        elif name == 'own':
            kind = 2
    return 'CE %d %d | %d %x' % (ill, hasown, kind, arg)


# ---------------------------------------------------------------- PA: poison attack apply (Envenomed Spears)
PA_VT = 0x6F8DA24C
PA_BUFFA = D + 0xB000
PA_BUFFAVT = D + 0xB200
PA_BUFFB = D + 0xB600
PA_BUFFBVT = D + 0xB800
PA_VICT = D + 0xBC00
PA_SRC = D + 0xBE00
PA_S380 = D + 0xA000
PA_S378 = D + 0xA020
PA_S324 = D + 0xA040
PA_S35C = D + 0xA060


def case_pa(r):
    return poison_case(r, 'PA', PA_VT, 0x6F13F2F0, 0x6F13ADC0, 0x6F13AE90)


def case_ps(r):
    return poison_case(r, 'PS', 0x6F8DA5AC, 0x6F13F500, 0x6F13AF60, 0x6F13B030)


def poison_case(r, tag, vt_code, fn, new_creator, stack_creator):
    srcill = r.choice([0, 0, 1])
    hero = r.choice([0, 1])
    existing = r.choice([0, 0, 1])
    flag200 = r.choice([0, 0, 1])
    dur, herodur = rand_real(r), rand_real(r)
    a, b, c = rand_real(r), rand_real(r), rand_real(r)
    d = r.choice([0, fbits(1.0), fbits(2.0), fbits(8.0), fbits(9.0), fbits(3.0), fbits(7.0), fbits(15.0), fbits(0.5), fbits(-1.0),
                  rand_real(r, small=True)])
    T.setup(vt_code, flags=0x200 if flag200 else 0)
    setrec(dur=dur, herodur=herodur, a=a, b=b, c=c, d=d)
    for obj in (PA_VICT, PA_SRC):
        mu.mem_write(obj, bytes(0x300))
        w32(obj, T.UVT)
    w32(PA_SRC + 0x5C, 0x40000000 if srcill else 0)
    for buf, vt in ((PA_BUFFA, PA_BUFFAVT), (PA_BUFFB, PA_BUFFBVT)):
        mu.mem_write(buf, bytes(0x200))
        mu.mem_write(vt, bytes(0x400))
        w32(buf, vt)
    w32(PA_BUFFAVT + 0x380, PA_S380)
    w32(PA_BUFFAVT + 0x324, PA_S324)
    w32(PA_BUFFBVT + 0x378, PA_S378)
    w32(PA_BUFFBVT + 0x324, PA_S324)
    w32(PA_BUFFBVT + 0x35C, PA_S35C)
    got = {}

    def cap(name, n):
        def f(uc, c_, d_, st):
            from unicorn.x86_const import UC_X86_REG_ESP
            esp = uc.reg_read(UC_X86_REG_ESP)
            words = struct.unpack('<10I', bytes(uc.mem_read(esp, 40)))
            vals = []
            for w in words[:n]:
                vals.append(r32(w) if 0x100000 <= w < 0x110000 else w)
            got[name] = (words, vals)
            return 0
        return f

    sf = {
        0x6F1D2660: lambda *x: hero,
        0x6F0787D0: lambda *x: PA_BUFFA if existing else 0,
        new_creator: lambda *x: PA_BUFFB,
        stack_creator: lambda *x: PA_BUFFB,
        PA_S380: cap('r', 7), PA_S378: cap('n', 7), PA_S324: lambda *x: 0, PA_S35C: cap('s', 8),
    }
    T.trace_call(fn, T.ABIL, (PA_VICT, PA_SRC), stub_fn=sf, span=0x300,
                 cleanup={PA_S380: 28, PA_S378: 28, PA_S324: 4, PA_S35C: 32, 0x6F042B20: 8, 0x6F076700: 8, 0x6F079CA0: 4})
    out = tag + ' %d %d %d %d %x %x %x %x %x %x |' % (srcill, hero, existing, flag200, dur, herodur, a, b, c, d)
    if 'r' in got:
        path, v = 2, got['r'][1]
        out += ' 2 %x %x %x %x %x -' % (v[2], v[3], v[4], v[5], v[6])
    elif 'n' in got:
        v = got['n'][1]
        out += ' 1 %x %x %x %x %x -' % (v[2], v[3], v[4], v[5], v[6])
    elif 's' in got:
        v = got['s'][1]
        out += ' 3 %x %x %x %x - %x' % (v[2], v[3], v[4], v[5], v[6])
    else:
        out += ' 0 0 0 0 0 0 0'
    return out


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


GENS = [('TN', case_tn), ('TM', case_tm), ('CL', case_cl), ('CE', case_ce), ('PA', case_pa), ('PS', case_ps)]


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
