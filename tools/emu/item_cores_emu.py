"""Unicorn numeric cores of the item abilities of Game.dll 1.26a (docs/spec/abilities-items.md, task S36).

Every row is the observed behaviour of ORIGINAL code on fake objects; the callees that need the object model are
stubbed and recorded (the tracer of item_trace_emu.py), the soft-float library and the level data getters run for real.
The Rust side (crates/sim/tests/item_ability_vectors.rs) recomputes each row with the model of item_abilities.rs.

Row kinds (all numbers hex unless noted, `|` separates input from output):
  CH  flags charges key           | new_charges calls     the charge routine 0x6F2B8390 (key: 0 none, 1 live, 2 dead)
  LM  level xp n m t0..t(m-1)     | amount_bits           CAbilityLevelMod effect 0x6F0DE200 (need table t, clamp rule)
  FX  code A B ethereal flag      | k (sink amount)*k      item heal / mana / restore effect bodies (code: 0 AIhe,
                                                          1 AIma, 2 AIre); the sink: 1 life adder, 2 mana adder
  CB  flags5c misc immune A eth   | called amount          the area heal callback 0x6F0F0470
  AT  A B C                       | agi int str adders*3   attribute tome init + effect (AIxm 0x6F0E5D50, 0x6F0DDDF0)
  AB  A B C                       | adders*3               attribute bonus init 0x6F0E57E0 (AIab)
  PB  kind value                  | observed args          persistent bonus apply / remove (kind: dmg, arm, asp, life,
                                                          mana)
  SP  hero dur herodur extra A B  | dur A B             item speed effect 0x6F1FC4C0: the buff apply arguments
  TM  kind A                      | observed args          tomes: 0 life (AImi 0x6F0DE2F0), 1 mana, 2 xp (AIem), 3 attack
                                                          (AIaa 0x6F0DE190, with an attack ability)

Usage: GAME_DLL=/path/Game.dll py tools/emu/item_cores_emu.py OUT.txt [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import item_trace_emu as T  # noqa: E402

emu = T.emu
mu = T.mu
D = emu.DATA
ITEM = D + 0x5000
KEYOBJ = D + 0x5400
VOBJ = D + 0x5800
VTAB = D + 0x5900
HERO = D + 0x5A00
ATK = D + 0x5B00
OUTP = D + 0x6000

U32 = struct.Struct('<I')


def w32(a, v):
    mu.mem_write(a, U32.pack(v & 0xffffffff))


def r32(a):
    return U32.unpack(bytes(mu.mem_read(a, 4)))[0]


def fbits(x):
    return struct.unpack('<I', struct.pack('<f', x))[0]


SPECIAL = [0, 0x80000000, 0x3F800000, 0xBF800000, 0x7F800000, 0x7FC00000, 0x00000001, 0x4B000000, 0xCF000000, 0x42C80000]


def rand_real(r, small=False):
    k = r.random()
    if k < 0.12:
        return r.choice(SPECIAL)
    if k < 0.75:
        v = r.choice([float(r.randint(-300, 300)), r.uniform(-500.0, 500.0), r.uniform(0, 50), r.randint(0, 40) + 0.5])
        return fbits(v)
    if small:
        return fbits(r.uniform(-1000.0, 1000.0))
    return r.getrandbits(32)


# ---------------------------------------------------------------- CH: the charge routine
CHARGE = 0x6F2B8390
RESOLVE = 0x6F4786B0
NOTIFY = 0x6F299750
DESTROY = 0x6F2B8330
UI = 0x6F3327F0
SETLIFE = 0x6F2B89E0


def case_ch(r):
    flags = r.choice([0, 0x1000, 0x40, 0x1040, 0x80, 0x1000 | 0x40 | 0x400, 0x240])
    charges = r.choice([-2, -1, 0, 1, 2, 3, 7, r.randint(-5, 9)])
    key = r.choice([0, 1, 2])
    T.setup(0x6F8C9F64)
    mu.mem_write(ITEM, bytes(0x100))
    w32(ITEM + 0x20, flags)
    w32(ITEM + 0x84, charges)
    if key == 0:
        w32(ITEM + 0x8C, 0xFFFFFFFF)
        w32(ITEM + 0x90, 0xFFFFFFFF)
    else:
        w32(ITEM + 0x8C, 0x11)
        w32(ITEM + 0x90, 0x22)
    w32(VOBJ, VTAB)
    w32(VTAB, SETLIFE)
    w32(ITEM + 0x54, VTAB)
    calls = []

    def resolve(uc, c, d, st):
        calls.append('R')
        return KEYOBJ if key == 1 else 0

    def rec(letter):
        def f(uc, c, d, st):
            calls.append(letter)
            return 0
        return f

    _, trace, cl = T.trace_call(CHARGE, ITEM, (), stub_fn={RESOLVE: resolve, NOTIFY: rec('N'), DESTROY: rec('D'),
                                                           UI: rec('U'), SETLIFE: rec('S')}, span=0x100,
                                  cleanup={SETLIFE: 8})
    return 'CH %x %x %x | %x %s' % (flags, charges & 0xffffffff, key, r32(ITEM + 0x84), ''.join(c for c in calls if c != 'R') or '-')


# ---------------------------------------------------------------- LM: level mod effect
LEVELMOD = 0x6F0DE200


def case_lm(r):
    m = r.randint(3, 12)
    t = sorted(r.randint(50, 6000) for _ in range(m))
    if r.random() < 0.1:
        t[r.randrange(m)] = t[0]
    level = r.randint(1, m)
    n = r.choice([1, 1, 2, 3, 5, r.randint(-2, 8)])
    lo = t[level - 2] if level >= 2 else 0
    hi = t[level - 1]
    xp = r.choice([lo, hi - 1, r.randint(min(lo, hi), max(lo, hi)), r.randint(0, hi + 100)])
    T.setup(0x6F8C70BC)
    w32(T.ABIL + 0x124, n)
    got = {}

    def need(uc, c, d, st):
        a = st[0]
        sa = a - (1 << 32) if a >= 1 << 31 else a
        idx = min(max(sa, 1), m) - 1
        return t[idx]

    def addxp(uc, c, d, st):
        got['amount'] = r32(st[0])
        return 0

    _, trace, cl = T.trace_call(LEVELMOD, T.ABIL, (), stub_fn={0x6F2A0210: lambda *a: level,
                                                               0x6F26EB30: lambda *a: xp & 0xffffffff,
                                                               0x6F26EB50: need, 0x6F26EB70: addxp}, span=0x200)
    return 'LM %x %x %x %x %s | %x' % (level, xp & 0xffffffff, n & 0xffffffff, m, ' '.join('%x' % v for v in t), got.get('amount', 0xdead))


# ---------------------------------------------------------------- FX: heal / mana / restore effect bodies
FX_FN = {0: (0x6F8C9F64, 0x6F0E3990), 1: (0x6F8DF81C, 0x6F119F10), 2: (0x6F91A1A4, 0x6F1DBB10)}
LIFE_ADD = 0x6F277C20
MANA_ADD = 0x6F279140
MISC_REAL = 0x6F009EE0


def case_fx(r):
    code = r.choice([0, 1, 2])
    vt, fn = FX_FN[code]
    T.setup(vt)
    a, b = rand_real(r), rand_real(r)
    eth = r.choice([0, 0, 1, 2, -1])
    misc = rand_real(r)
    # data A and B of the level record
    rec = bytearray(bytes(mu.mem_read(T.REC, 0x68)))
    struct.pack_into('<I', rec, 0x20, a)
    struct.pack_into('<I', rec, 0x24, b)
    mu.mem_write(T.REC, bytes(rec))
    w32(T.UNIT + 0x158, eth)
    sinks = []

    def life(uc, c, d, st):
        sinks.append((1, r32(st[1])))
        return 0

    def mana(uc, c, d, st):
        sinks.append((2, r32(st[1])))
        return 0

    def miscf(uc, c, d, st):
        w32(c, misc)
        return c

    # the class slot 0x42C runs for real (it is the only virtual call, answered by the original code)
    slot42c = struct.unpack('<I', bytes(mu.mem_read(vt + 0x42C, 4)))[0]
    _, trace, cl = T.trace_call(fn, T.ABIL, (), stub_fn={LIFE_ADD: life, MANA_ADD: mana, MISC_REAL: miscf},
                                into=(slot42c,), span=0x200)
    return 'FX %x %x %x %x %x | %x %s' % (code, a, b, eth & 0xffffffff, misc, len(sinks),
                                          ' '.join('%x %x' % s for s in sinks) or '-')


# ---------------------------------------------------------------- CB: area heal callback
CBFN = 0x6F0F0470
IMMUNE = 0x6F28D750
MISC_BOOL = 0x6F009F90
EFFECT_ENTRY = 0x6F052EC0


def case_cb(r):
    T.setup(0x6F8C9F64)
    flags5c = r.choice([0, 0x40000000, 0x40000000 | 0x1, 0x1, r.getrandbits(32)])
    misc = r.choice([0, 1, 1, 0])
    immune = r.choice([0, 0, 1])
    a = rand_real(r)
    eth = r.choice([0, 0, 1, 3])
    mu.mem_write(ITEM, bytes(0x400))
    cand = ITEM
    w32(cand, T.UVT)
    w32(cand + 0x5C, flags5c)
    w32(cand + 0x158, eth)
    rec = bytearray(bytes(mu.mem_read(T.REC, 0x68)))
    struct.pack_into('<I', rec, 0x20, a)
    mu.mem_write(T.REC, bytes(rec))
    sinks = []
    mm = rand_real(r)
    slot42c = struct.unpack('<I', bytes(mu.mem_read(0x6F8C9F64 + 0x42C, 4)))[0]
    slot308 = struct.unpack('<I', bytes(mu.mem_read(0x6F8C9F64 + 0x308, 4)))[0]

    def life(uc, c, d, st):
        sinks.append((c, r32(st[1])))
        return 0

    r_, trace, cl = T.trace_call(CBFN, cand, (), edx=T.ABIL,
                                 stub_fn={LIFE_ADD: life, IMMUNE: lambda *a_: immune, MISC_BOOL: lambda *a_: misc,
                                          MISC_REAL: lambda uc, c, d, st: (w32(c, mm) or c)},
                                 into=(slot42c, slot308), span=0x200)
    called = 1 if sinks else 0
    return 'CB %x %x %x %x %x | %x %x %x' % (flags5c, misc, immune, a, eth & 0xffffffff, called,
                                              sinks[0][1] if sinks else 0, r_)


# ---------------------------------------------------------------- AT / AB: attribute tome and bonus
ADD_AGI, ADD_INT, ADD_STR = 0x6F20A1E0, 0x6F20A260, 0x6F20A160
PROF_A, PROF_B, PROF_C = 0x6F01FA50, 0x6F01FAD0, 0x6F01FB50


def prof(bits):
    def f(uc, c, d, st):
        w32(st[0], bits)
        return st[0]
    return f


def case_at(r):
    a, b, c = rand_real(r), rand_real(r), rand_real(r)
    T.setup(0x6F8C5BCC)
    w32(T.UNIT + 0x1F0, HERO)
    adders = []

    def add(name):
        def f(uc, ecx, edx, st):
            adders.append((name, st[0], st[1]))
            return 0
        return f

    _, tr, _ = T.trace_call(0x6F0E5D50, T.ABIL, (T.D + 0x4000, T.D + 0x4000),
                            stub_fn={PROF_A: prof(a), PROF_B: prof(b), PROF_C: prof(c)}, span=0x100)
    stored = (r32(T.ABIL + 0x124), r32(T.ABIL + 0x128), r32(T.ABIL + 0x12C))
    _, tr, _ = T.trace_call(0x6F0DDDF0, T.ABIL, (), stub_fn={ADD_AGI: add('agi'), ADD_INT: add('int'), ADD_STR: add('str')},
                            span=0x100)
    order = ' '.join('%s %x %x' % x for x in adders)
    return 'AT %x %x %x | %x %x %x %s' % (a, b, c, stored[0], stored[1], stored[2], order)


def case_ab(r):
    a, b, c = rand_real(r), rand_real(r), rand_real(r)
    T.setup(0x6F8B9834)
    w32(T.UNIT + 0x1F0, HERO)
    adders = []

    def add(name):
        def f(uc, ecx, edx, st):
            adders.append((name, st[0], st[1]))
            return 0
        return f

    _, tr, _ = T.trace_call(0x6F0E57E0, T.ABIL, (T.D + 0x4000, T.D + 0x4000),
                            stub_fn={PROF_A: prof(a), PROF_B: prof(b), PROF_C: prof(c), 0x6F0DDC70: lambda *x: 0,
                                     ADD_AGI: add('agi'), ADD_INT: add('int'), ADD_STR: add('str')}, span=0x100)
    return 'AB %x %x %x | %s' % (a, b, c, ' '.join('%s %x %x' % x for x in adders))


# ---------------------------------------------------------------- PB: persistent bonus apply / remove
PB_CASES = {
    # kind: (vtable, apply fn, remove fn, sink target, needs attack ability, what to read)
    'dmg': (0x6F8B8EBC, 0x6F0F4100, 0x6F0F4170, 0x6F0C6C10),
    'arm': (0x6F8B950C, 0x6F0F4340, 0x6F0F4390, 0x6F2769C0),
    'asp': (0x6F8B91E4, 0x6F0F4230, 0x6F0F4290, 0x6F0C9090),
}
COUNTER = 0x6F477D30
CHK40C0 = 0x6F0F40C0
CHK41F0 = 0x6F0F41F0
MASKFN = 0x6F0C7000
OWNER_FN = 0x6F472890


def case_pb(r):
    kind = r.choice(list(PB_CASES))
    vt, ap, rm, sink = PB_CASES[kind]
    value = rand_real(r)
    T.setup(vt)
    w32(T.UNIT + 0x1E8, ATK)
    w32(ATK, T.UVT)
    w32(T.ABIL + 0x88, value)
    out = []

    def grab(uc, c, d, st):
        # damage bonus: (amount, mask); armor: (&value); attack speed: (&value, 0)
        if kind == 'dmg':
            out.append((st[0], st[1]))
        else:
            out.append((r32(st[0]), 0))
        return 0

    stubs = {sink: grab, COUNTER: lambda *a: 0, CHK40C0: lambda *a: 0, CHK41F0: lambda *a: 0, MASKFN: lambda *a: 3}
    w32(T.ABIL + 0x20, 0)
    T.trace_call(ap, T.ABIL, (), stub_fn=stubs, span=0x100)
    applied = list(out)
    out.clear()
    w32(T.ABIL + 0x20, 0x80)
    T.trace_call(rm, T.ABIL, (), stub_fn=stubs, span=0x100)
    removed = list(out)
    f = lambda L: ' '.join('%x %x' % x for x in L) or '-'
    return 'PB %s %x | %s | %s' % (kind, value, f(applied), f(removed))


LIFE_SINK = 0x6F2AC450
MANA_SINK = 0x6F278F50


def case_pl(r):
    """AIml / AImm (max life / mana bonus): init applies +value, slot 0xA4 removes -value (both mode 1)."""
    which = r.choice(['life', 'mana'])
    vt = 0x6F8B9E84 if which == 'life' else 0x6F8BA1AC
    init = 0x6F0DDF70 if which == 'life' else 0x6F0DDFF0
    remove = 0x6F0DDFA0 if which == 'life' else 0x6F0DE020
    sink = LIFE_SINK if which == 'life' else MANA_SINK
    value = rand_real(r)
    T.setup(vt)
    w32(T.ABIL + 0x88, value)
    out = []

    def grab(uc, c, d, st):
        out.append((r32(st[0]), st[1]))
        return 0

    stubs = {sink: grab, 0x6F0DDC70: lambda *a: 0, 0x6F0D1CD0: lambda *a: 0}
    T.trace_call(init, T.ABIL, (T.UNIT, 0), stub_fn=stubs, span=0x100)
    applied = list(out)
    out.clear()
    T.trace_call(remove, T.ABIL, (), stub_fn=stubs, span=0x100)
    removed = list(out)
    f = lambda L: ' '.join('%x %x' % x for x in L) or '-'
    return 'PL %s %x | %s | %s' % (which, value, f(applied), f(removed))


# ---------------------------------------------------------------- TM: tomes
TOMES = {
    'life': (0x6F8C74EC, 0x6F0DE2F0, LIFE_SINK),
    'mana': (None, 0x6F0DE320, MANA_SINK),
}


def case_tm(r):
    kind = r.choice(['life', 'mana', 'xp', 'attack'])
    n = r.choice([0, 1, 5, 50, 100, 250, r.randint(-100, 5000), r.getrandbits(32)]) & 0xffffffff
    out = []
    if kind in ('life', 'mana'):
        vt = 0x6F8C74EC
        fn = 0x6F0DE2F0 if kind == 'life' else 0x6F0DE320
        sink = LIFE_SINK if kind == 'life' else MANA_SINK
        T.setup(vt)
        w32(T.ABIL + 0x124, n)

        def grab(uc, c, d, st):
            out.append((r32(st[0]), st[1]))
            return 0

        T.trace_call(fn, T.ABIL, (), stub_fn={sink: grab}, span=0x100)
        return 'TM %s %x | %s' % (kind, n, ' '.join('%x %x' % x for x in out))
    if kind == 'xp':
        T.setup(0x6F8C7D4C)
        w32(T.ABIL + 0x124, n)

        def grab(uc, c, d, st):
            out.append((r32(st[0]), st[1], st[2], st[3], st[4], st[5]))
            return 0

        T.trace_call(0x6F0DE390, T.ABIL, (), stub_fn={0x6F26EB70: grab}, span=0x100)
        return 'TM xp %x | %s' % (n, ' '.join('%x %x %x %x %x %x' % x for x in out))
    T.setup(0x6F8C6C8C)
    w32(T.ABIL + 0x124, n)
    w32(T.UNIT + 0x1E8, ATK)

    def grab(uc, c, d, st):
        out.append((st[0], st[1]))
        return 0

    T.trace_call(0x6F0DE190, T.ABIL, (), stub_fn={0x6F0C6BD0: grab}, span=0x100)
    return 'TM attack %x | %s' % (n, ' '.join('%x %x' % x for x in out) or '-')



# ---------------------------------------------------------------- SP: item speed effect (AIsp 0x6F1FC4C0)
BUFF = D + 0x7000
BVT = D + 0x7400
HERO_TEST = 0x6F1D2660
FIND_BUFF = 0x6F0787D0
BUFF_APPLY = 0x6F0D6500


def case_sp(r):
    hero = r.choice([0, 1])
    dur, herodur, extra, a, b = (rand_real(r) for _ in range(5))
    T.setup(0x6F92376C)
    rec = bytearray(bytes(mu.mem_read(T.REC, 0x68)))
    struct.pack_into('<I', rec, 0x08, dur)
    struct.pack_into('<I', rec, 0x0C, herodur)
    struct.pack_into('<I', rec, 0x20, a)
    struct.pack_into('<I', rec, 0x24, b)
    mu.mem_write(T.REC, bytes(rec))
    w32(T.ABIL + 0x9C, extra)
    mu.mem_write(BUFF, bytes(0x200))
    mu.mem_write(BVT, bytes(0x800))
    w32(BUFF, BVT)
    w32(BVT + 0x358, BUFF_APPLY)
    got = {}

    def apply(uc, c, d, st):
        got['args'] = (r32(st[1]), r32(st[2]), r32(st[3]))
        return 0

    T.trace_call(0x6F1FC4C0, T.ABIL, (T.UNIT,), stub_fn={HERO_TEST: lambda *x: hero, FIND_BUFF: lambda *x: BUFF,
                                                         BUFF_APPLY: apply}, span=0x200)
    g = got.get('args', (0xdead, 0xdead, 0xdead))
    return 'SP %x %x %x %x %x %x | %x %x %x' % (hero, dur, herodur, extra, a, b, g[0], g[1], g[2])

GENS = [('CH', case_ch), ('LM', case_lm), ('FX', case_fx), ('CB', case_cb), ('AT', case_at), ('AB', case_ab),
        ('PB', case_pb), ('PL', case_pl), ('TM', case_tm), ('SP', case_sp)]


def main():
    out = sys.argv[1]
    cases = int(sys.argv[2]) if len(sys.argv) > 2 else 300
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    r = random.Random(seed)
    rows = []
    for name, fn in GENS:
        for _ in range(cases):
            rows.append(fn(r))
    with open(out, 'w') as fh:
        fh.write('\n'.join(rows) + '\n')
    print(len(rows), 'rows')


if __name__ == '__main__':
    main()
