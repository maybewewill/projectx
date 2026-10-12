"""Unicorn harness for the agile property engine of Game.dll 1.26a (docs/spec/agile.md).

Real code runs on synthetic objects (CPoReal property, CPoReContinuousMod modifier, CPoReThresholdLis listener, and the
wrapper objects FloatModifier / FloatListener / Float) and a synthetic world:

  * the world global [0x6FAB73D8] points at a fake world G. G+0x10 holds the object factory (replaced by a recorder that
    allocates the right class by the 4-character tag of the creation message); G+0x14 is the property queue (same
    layout as the timer queue of tools/emu/agile_clock_emu.py: heap +0x10, count +0x20, pool +0x38, clock +0x40,
    row +0x44, horizon +0x48, sequence +0x50);
  * the handle table [0x6FAB7788] maps wrapper handles (index, generation) to objects;
  * the only message sink that is not real code is the owner of a listener (its vtable slot 8): it records the message.

Functions exercised: modifier creator 0x6F4774E0 (-> 0x6F478CA0), listener creator 0x6F477550 (-> 0x6F480FB0), property
getter 0x6F4773A0, value evaluation 0x6F477180, slope add 0x6F4A9320, listener decision 0x6F4A9C70 / 0x6F4A9B70, crossing time
0x6F4A8FD0, and the queue pump 0x6F46E9E0 with the timer callback 0x6F4A69B0.

Usage:  GAME_DLL=/path/Game.dll py -I tools/emu/agile_engine_emu.py probe
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ_UNMAPPED, UC_HOOK_MEM_WRITE_UNMAPPED  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EAX  # noqa: E402

mu = emu.mu
ADD, SUB, MUL, DIV = 0x6F6EF000, 0x6F6EEEF0, 0x6F6EEE20, 0x6F6EFE10

BASE = 0x00A00000
mu.mem_map(BASE, 0x00200000)

W_PTR = 0x6FAB73D8          # world global
H_PTR = 0x6FAB7788          # handle table global
G = BASE + 0x1000           # world
Q = G + 0x14                # property queue (normal objects)
Q2 = G + 0x68               # second queue (bit31 objects), present but unused
HEAP = BASE + 0x10000
HEAP_CAP = 2048
POOL = BASE + 0x20000
POOL_N = 1024
HT = BASE + 0x3000          # handle table
HT_ARR = BASE + 0x4000
OBJ_AREA = BASE + 0x40000
OBJ_SZ = 0x100
FACTORY = BASE + 0x2000     # code address hooked as the object factory
REC_SINK = BASE + 0x2100    # code address hooked as the recorder of messages to a listener owner
REC_VT = BASE + 0x2200      # vtable of the recorder object: slot 8 (+0x20) = REC_SINK
REC_OBJ = BASE + 0x2300
TMP = BASE + 0x6000

VT_PROP, VT_MOD, VT_LIS = 0x6F952DEC, 0x6F952A4C, 0x6F952B6C
VT_W_MOD, VT_W_LIS, VT_W_FLOAT = 0x6F878528, 0x6F878E40, 0x6F877BA8

H300 = 0x43960000
LOG = []
STATE = dict(next_obj=0, next_idx=1, objs={})


def rd32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def b2f(b):
    return struct.unpack("<f", struct.pack("<I", b & 0xFFFFFFFF))[0]


def f2b(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def call(fn, ecx=None, edx=None, stack_args=()):
    esp = emu.TOP
    for a in reversed(stack_args):
        esp -= 4
        mu.mem_write(esp, struct.pack("<I", a & 0xFFFFFFFF))
    esp -= 4
    mu.mem_write(esp, struct.pack("<I", emu.SENT))
    mu.reg_write(UC_X86_REG_ESP, esp)
    if ecx is not None:
        mu.reg_write(UC_X86_REG_ECX, ecx & 0xFFFFFFFF)
    if edx is not None:
        mu.reg_write(UC_X86_REG_EDX, edx & 0xFFFFFFFF)
    try:
        mu.emu_start(fn, emu.SENT, count=emu.MAX_INSNS)
    except Exception as exc:
        raise RuntimeError("call %#x failed at %#x: %s" % (fn, mu.reg_read(UC_X86_REG_EIP), exc))
    if mu.reg_read(UC_X86_REG_EIP) != emu.SENT:
        raise RuntimeError("call %#x did not return (stopped at %#x)" % (fn, mu.reg_read(UC_X86_REG_EIP)))
    return mu.reg_read(UC_X86_REG_EAX)


def soft(fn, a, b):
    wr32(TMP, a)
    wr32(TMP + 4, b)
    call(fn, ecx=TMP + 8, edx=TMP, stack_args=(TMP + 4,))
    return rd32(TMP + 8)


def sadd(a, b):
    return soft(ADD, a, b)


def ssub(a, b):
    return soft(SUB, a, b)


def smul(a, b):
    return soft(MUL, a, b)


def sdiv(a, b):
    return soft(DIV, a, b)


# ---------------------------------------------------------------------------------------------------------------- world
def alloc_obj(vt):
    a = OBJ_AREA + STATE["next_obj"] * OBJ_SZ
    STATE["next_obj"] += 1
    mu.mem_write(a, b"\0" * OBJ_SZ)
    wr32(a, vt)
    idx = STATE["next_idx"]
    STATE["next_idx"] += 1
    wr32(a + 0x14, idx)
    wr32(a + 0x18, 7)                         # generation
    wr32(HT_ARR + idx * 8, 0xFFFFFFFE)        # entry tag -2
    wr32(HT_ARR + idx * 8 + 4, a)
    STATE["objs"][a] = idx
    return a


def wrapper(vt, target):
    """A wrapper (FloatModifier / FloatListener / Float) pointing at object `target` by handle."""
    a = alloc_obj(vt)
    wr32(a + 8, rd32(target + 0x14))
    wr32(a + 0xC, rd32(target + 0x18))
    return a


def factory_hook(uc, address, size, user):
    ecx = uc.reg_read(UC_X86_REG_ECX)
    tag0, tag1 = rd32(ecx), rd32(ecx + 4)
    if tag0 == 0x5E6D6F64:
        vt = VT_MOD
    elif tag0 == 0x5E6C6973:
        vt = VT_LIS
    else:
        raise RuntimeError("factory: unknown tag %#x %#x" % (tag0, tag1))
    # raw object + the constructors 0x6F48AE90 (modifier) / 0x6F48B420 (listener): ids are assigned by 0x6F4AEFF0
    o = OBJ_AREA + STATE["next_obj"] * OBJ_SZ
    STATE["next_obj"] += 1
    mu.mem_write(o, bytes(OBJ_SZ))
    wr32(o, vt)
    wr32(o + 0x14, 0xFFFFFFFF)
    wr32(o + 0x18, 0xFFFFFFFF)
    wr32(o + 0x2C, o)
    wr32(o + 0x3C, o)
    wr32(o + 0x48, 0xFFFFFFFF)
    LOG.append(("create", vt, o))
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    uc.reg_write(UC_X86_REG_EAX, o)
    uc.reg_write(UC_X86_REG_ESP, esp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


def sink_hook(uc, address, size, user):
    """A message to the recorder object (vtable slot 8, thiscall with one stack argument)."""
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    msg = rd32(esp + 4)
    LOG.append(("sink", rd32(msg), rd32(msg + 4), rd32(msg + 8), rd32(msg + 0xC), rd32(Q + 0x44), rd32(Q + 0x40)))
    uc.reg_write(UC_X86_REG_ESP, esp + 8)
    uc.reg_write(UC_X86_REG_EIP, ret)


def init_world():
    mu.mem_write(BASE, b"\0" * 0x1000)
    mu.mem_write(G, b"\0" * 0x1000)
    mu.mem_write(HEAP, b"\0" * (4 * HEAP_CAP))
    mu.mem_write(POOL, b"\0" * (0x40 * POOL_N))
    mu.mem_write(HT, b"\0" * 0x100)
    mu.mem_write(HT_ARR, b"\0" * 0x4000)
    mu.mem_write(OBJ_AREA, b"\0" * (OBJ_SZ * 64))
    STATE["next_obj"] = 0
    ARENA[0] = 0
    STATE["next_idx"] = 1
    STATE["objs"].clear()
    del LOG[:]
    wr32(W_PTR, G)
    wr32(G + 0x10, FACTORY)
    wr32(H_PTR, HT)
    wr32(HT + 0x1C, 4000)
    wr32(HT + 0xC, HT_ARR)
    wr32(HT + 0x2C, HT_ARR2)
    wr32(HT + 0x3C, 4000)
    wr32(HT + 0x50, 1)
    wr32(HT + 0x54, 1)
    IDS["pos"] = 100
    IDS["neg"] = 100
    for q in (Q, Q2):
        wr32(q + 0x10, HEAP)
        wr32(q + 0x18, 0)
        wr32(q + 0x1C, HEAP_CAP)
        wr32(q + 0x20, 1)
        wr32(q + 0x48, H300)
    for i in range(POOL_N):
        wr32(POOL + 0x40 * i, POOL + 0x40 * (i + 1) if i + 1 < POOL_N else 0)
    wr32(Q + 0x38, POOL)
    # recorder object
    wr32(REC_VT + 0x20, REC_SINK)
    wr32(REC_OBJ, REC_VT)


HEAP_ARENA = BASE + 0x80000
ARENA = [0]


def new_hook(uc, address, size, user):
    """0x6F6EB5B2: operator new(size, file, line, flag), stdcall with 4 arguments."""
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    n = rd32(esp + 4)
    a = HEAP_ARENA + ARENA[0]
    ARENA[0] += (n + 15) & ~15
    mu.mem_write(a, bytes(n))
    uc.reg_write(UC_X86_REG_EAX, a)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + 16)
    uc.reg_write(UC_X86_REG_EIP, ret)


HT_ARR2 = BASE + 0x5000
IDS = dict(pos=100, neg=100)


def alloc_handle_hook(uc, address, size, user):
    """0x6F4BC310(this = handle table, obj, class, preferred): thiscall with three stack arguments; returns the table index
    (bit 31 set for the second table) and registers the object."""
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    obj, cls, pref = rd32(esp + 4), rd32(esp + 8), rd32(esp + 12)
    if pref != 0xFFFFFFFF and pref & 0x80000000:
        i = IDS["neg"]
        IDS["neg"] += 1
        wr32(HT_ARR2 + i * 8, 0xFFFFFFFE)
        wr32(HT_ARR2 + i * 8 + 4, obj)
        r = i | 0x80000000
    else:
        i = IDS["pos"]
        IDS["pos"] += 1
        wr32(HT_ARR + i * 8, 0xFFFFFFFE)
        wr32(HT_ARR + i * 8 + 4, obj)
        r = i
    LOG.append(("handle", obj, r))
    uc.reg_write(UC_X86_REG_EAX, r)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + 12)
    uc.reg_write(UC_X86_REG_EIP, ret)


_hooks = [False]


def install_hooks():
    if _hooks[0]:
        return
    mu.mem_write(FACTORY, b"\xC3")
    mu.mem_write(REC_SINK, b"\xC3")
    mu.hook_add(UC_HOOK_CODE, factory_hook, begin=FACTORY, end=FACTORY)
    mu.hook_add(UC_HOOK_CODE, sink_hook, begin=REC_SINK, end=REC_SINK)
    mu.mem_write(OBS_REG, bytes([0xC3]))
    wr32(OBS_VT + 8, OBS_REG)
    mu.hook_add(UC_HOOK_CODE, obs_hook, begin=OBS_REG, end=OBS_REG)
    mu.hook_add(UC_HOOK_CODE, alloc_handle_hook, begin=0x6F4BC310, end=0x6F4BC310)
    mu.hook_add(UC_HOOK_CODE, new_hook, begin=0x6F6EB5B2, end=0x6F6EB5B2)
    _hooks[0] = True


OBS_VT = BASE + 0x2400
OBS_REG = BASE + 0x2500


def obs_hook(uc, address, size, user):
    """Observer registration (vtable slot 2 of the embedded observer at agent+0x14), thiscall, three stack arguments."""
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    LOG.append(("observe", rd32(esp + 4), rd32(esp + 8), rd32(esp + 12)))
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + 12)
    uc.reg_write(UC_X86_REG_EIP, ret)


def make_owner():
    """The agent that owns a property (an ability / unit object): tag 'lga+' at +0xC, its handle pair struct at +0x54."""
    o = alloc_obj(REC_VT)
    wr32(o + 0xC, 0x2B61676C)
    s = OBJ_AREA + 60 * OBJ_SZ + (len(STATE["objs"]) % 3) * 0x40
    wr32(s + 0xC, rd32(o + 0x14))
    wr32(s + 0x10, rd32(o + 0x18))
    wr32(o + 0x54, s)
    wr32(s + 0x14, OBS_VT)
    return o


def make_prop(base_bits, lo_bits, hi_bits, owner=None):
    p = alloc_obj(VT_PROP)
    wr32(p + 0x30, owner if owner else make_owner())
    wr32(p + 0x4C, 0)
    wr32(p + 0x78, base_bits)
    wr32(p + 0x7C, 0)
    wr32(p + 0x80, lo_bits)
    wr32(p + 0x84, hi_bits)
    set_t0(p)
    return p


def set_t0(p):
    wr32(p + 0x70, rd32(Q + 0x40))
    wr32(p + 0x74, rd32(Q + 0x44))


def prop_value(p):
    """Getter of the Float wrapper 0x6F4773A0 (ecx = wrapper)."""
    w = wrapper(VT_W_FLOAT, p)
    call(0x6F4773A0, ecx=w, stack_args=(TMP + 0x20,))
    return rd32(TMP + 0x20)


def pump_to(target_bits):
    """The tick pump of the property queue: 0x6F46E9E0 (ecx = increment pointer, edx = queue) adds the increment to the
    clock, so the harness sets the clock to just below and adds the difference... Instead the harness calls the add itself."""
    raise NotImplementedError


def advance(inc_bits):
    wr32(TMP + 0x30, inc_bits)
    call(0x6F46E9E0, ecx=TMP + 0x30, edx=Q)


def dump_heap():
    n = rd32(Q + 0x20)
    arr = rd32(Q + 0x10)
    for i in range(1, n):
        e = rd32(arr + 4 * i)
        print("  heap[%d] expiry %08x (%g) period %08x flags %x seq %d obj %x ptr %x" % (
            i, rd32(e + 4), b2f(rd32(e + 4)), rd32(e + 8), rd32(e + 0x10), rd32(e + 0x14), rd32(e + 0x18), e))


def clock():
    return rd32(Q + 0x40), rd32(Q + 0x44)


TICK = 0x3BA3D70A


class Scn:
    """One scenario on the real code: a property with modifiers / listeners created at given ticks."""

    def __init__(self, clock_bits=0, row=0, base=0, lo=0xFF7FFFFF, hi=0x7F7FFFFF, owner_sink=True):
        install_hooks()
        init_world()
        wr32(Q + 0x40, clock_bits)
        wr32(Q + 0x44, row)
        self.prop = make_prop(base, lo, hi)
        self.pw = wrapper(VT_W_FLOAT, self.prop)
        self.tick = 0
        self.events = []
        self.nlog = 0
        self.mods = []
        self.lis = []
        self.lis_objs = []

    def value(self):
        call(0x6F4773A0, ecx=self.pw, stack_args=(TMP + 0x20,))
        return rd32(TMP + 0x20)

    def add_mod(self, rate, horizon):
        wr32(TMP + 0x40, rate)
        wr32(TMP + 0x44, horizon)
        out = TMP + 0x50 + 4 * len(self.mods)
        call(0x6F4774E0, ecx=self.pw, stack_args=(out, TMP + 0x40, TMP + 0x44))
        self.mods.append(rd32(out))
        self._collect()

    def add_lis(self, mode, thr, msg, owner=REC_OBJ):
        wr32(TMP + 0x60, thr)
        out = TMP + 0x90 + 4 * len(self.lis)
        call(0x6F477550, ecx=self.pw, stack_args=(out, mode, TMP + 0x60, msg, owner, 0))
        self.lis.append(rd32(out))
        self._collect()

    def set_value(self, x):
        wr32(TMP + 0x10, x)
        call(0x6F477350, ecx=self.pw, stack_args=(TMP + 0x10,))
        self._collect()

    def step(self, inc=TICK):
        advance(inc)
        self._collect()

    def _collect(self):
        for l in LOG[self.nlog:]:
            if l[0] == "sink":
                self.events.append((self.tick, l[6], l[5], self.lis_objs.index(l[4])))
            elif l[0] == "create" and l[1] == VT_LIS:
                self.lis_objs.append(l[2])
        self.nlog = len(LOG)

    def heap_entries(self):
        n = rd32(Q + 0x20)
        arr = rd32(Q + 0x10)
        out = []
        for i in range(1, n):
            e = rd32(arr + 4 * i)
            out.append((rd32(e + 4), rd32(e + 0x14), rd32(e + 0x10)))
        return out


def probe():
    sc = Scn(base=f2b(60.0))
    print("value", b2f(sc.value()))
    sc.add_mod(f2b(-1.0), f2b(61.0))
    sc.add_lis(0, f2b(0.0), 0xD01C4)
    sc.add_lis(0, f2b(10.0), 0xD01C5)
    print("heap", [(hex(a), b, hex(c)) for a, b, c in sc.heap_entries()])
    # subscriber list of the property
    n = rd32(sc.prop + 0x64)
    print("sub head", hex(n))
    k = 0
    while n and k < 5:
        print("  node", hex(n), "next", hex(rd32(n + 4)), "obj", hex(rd32(n + 8)))
        n = rd32(n + 4)
        k += 1
    for i in range(14000):
        sc.step()
        if sc.events and i > 12010:
            break
    print("events", sc.events)
    print("value", b2f(sc.value()), "tick", sc.tick)


FLT_MAX = 0x7F7FFFFF
NEG_FLT_MAX = 0xFF7FFFFF


# ----------------------------------------------------------------------------------------------- function vectors
def fn_state(rnd):
    """A random property state: clock, t0, base, slope, limits."""
    r = rnd.random()
    if r < 0.4:
        now, row = f2b(rnd.uniform(0.0, 299.0)), rnd.randrange(0, 3)
    elif r < 0.7:
        now, row = f2b(rnd.choice([0.0, 0.005, 0.1, 1.0, 100.0, 299.995, 299.9999])), rnd.randrange(0, 2)
    else:
        now, row = rnd.getrandbits(32) & 0x7FFFFFFF, rnd.randrange(0, 4)
    t0row = row if rnd.random() < 0.6 else max(0, row - rnd.randrange(0, 3))
    if t0row == row and rnd.random() < 0.5:
        t0 = now if rnd.random() < 0.3 else f2b(max(0.0, b2f(now) - rnd.choice([0.00005, 0.0001, 0.005, 0.1, rnd.uniform(0, 30)])))
    else:
        t0 = f2b(rnd.uniform(0.0, 299.0))
    base = f2b(rnd.choice([0.0, 60.0, 1.0, -3.0, rnd.uniform(-10, 200)]))
    slope = f2b(rnd.choice([0.0, -1.0, 1.0, 0.5, -0.5, 1e-7, -2e-7, 8e-7, -9e-7, rnd.uniform(-3, 3), 24 / 240.0]))
    if rnd.random() < 0.05:
        slope = rnd.getrandbits(32)
    lo, hi = rnd.choice([(NEG_FLT_MAX, FLT_MAX), (f2b(0.0), FLT_MAX), (f2b(0.0), f2b(100.0)), (f2b(-5.0), f2b(5.0)),
                         (f2b(0.0), f2b(24.0))])
    return now, row, t0, t0row, base, slope, lo, hi


def fn_setup(now, row, t0, t0row, base, slope, lo, hi):
    sc = Scn(now, row, base, lo, hi)
    wr32(sc.prop + 0x7C, slope)
    wr32(sc.prop + 0x70, t0)
    wr32(sc.prop + 0x74, t0row)
    return sc


def fn_eval_vectors(rnd, n):
    out = []
    for _ in range(n):
        st = fn_state(rnd)
        sc = fn_setup(*st)
        out.append("\t".join("%08x" % x if i != 1 and i != 3 else str(x) for i, x in enumerate(st)) + "\t%08x" % sc.value())
    return out


def fn_slope_vectors(rnd, n):
    out = []
    for _ in range(n):
        st = fn_state(rnd)
        delta = f2b(rnd.choice([0.0, -1.0, 1.0, 0.5, 1e-7, -8e-7, 24 / 240.0, rnd.uniform(-3, 3)]))
        sc = fn_setup(*st)
        wr32(TMP + 0x70, delta)
        call(0x6F4A9320, ecx=sc.prop, stack_args=(TMP + 0x70,))
        p = sc.prop
        res = (rd32(p + 0x78), rd32(p + 0x7C), rd32(p + 0x70), rd32(p + 0x74))
        out.append("\t".join("%08x" % x if i != 1 and i != 3 else str(x) for i, x in enumerate(st)) + "\t%08x" % delta +
                   "\t%08x\t%08x\t%08x\t%d" % res)
    return out


def fn_listener_setup(rnd, with_timer):
    st = fn_state(rnd)
    sc = fn_setup(*st)
    p = sc.prop
    thr = f2b(rnd.choice([0.0, 0.0, 24.0, 10.0, rnd.uniform(-5, 30), -5.0, 500.0, b2f(st[4])]))
    mode = rnd.choice([0, 0, 1, 1, 2])
    l = alloc_obj(VT_LIS)
    wr32(l + 0x2C, l)
    wr32(l + 0x3C, l)
    wr32(l + 0x40, p)
    wr32(l + 0x30, REC_OBJ)
    wr32(l + 0x4C, 0x90400)
    wr32(l + 0x50, thr)
    wr32(l + 0x54, mode)
    dummy = OBJ_AREA + 62 * OBJ_SZ
    mu.mem_write(dummy, bytes(0x40))
    if with_timer:
        wr32(l + 0x1C, dummy)
    return st, sc, p, l, thr, mode, dummy


def fn_listener_result(l, dummy):
    fired = [e for e in LOG if e[0] == "sink"]
    t = rd32(l + 0x1C)
    if t and t != dummy:
        timer = "%08x\t%d\t%x\t%08x" % (rd32(t + 4), rd32(t + 0x14), rd32(t + 0x10), rd32(t + 8))
    else:
        timer = "-\t-\t-\t-"
    cancelled = 1 if rd32(dummy + 0x10) & 0x10000 else 0
    return "%d\t%08x\t%s\t%d" % (len(fired), fired[0][6] if fired else 0, timer, cancelled)


def fn_decide_vectors(rnd, n):
    out = []
    for _ in range(n):
        st, sc, p, l, thr, mode, dummy = fn_listener_setup(rnd, rnd.random() < 0.5)
        near = rnd.random() < 0.3
        v = f2b(b2f(thr) + rnd.choice([0.0, 0.001, -0.004, 0.005, -0.005, 0.0049, 0.006])) if near else \
            f2b(rnd.choice([0.0, 60.0, 24.0, -3.0, rnd.uniform(-5, 100)]))
        wr32(TMP + 0x80, v)
        has_timer = rd32(l + 0x1C) != 0
        call(0x6F4A9C70, ecx=l, stack_args=(TMP + 0x80,))
        out.append("\t".join("%08x" % x if i != 1 and i != 3 else str(x) for i, x in enumerate(st)) +
                   "	%08x	%d	%08x	%d	" % (thr, mode, v, has_timer) + fn_listener_result(l, dummy))
    return out


def fn_jump_vectors(rnd, n):
    out = []
    for _ in range(n):
        st, sc, p, l, thr, mode, dummy = fn_listener_setup(rnd, rnd.random() < 0.5)
        t = b2f(thr)

        def pick():
            r = rnd.random()
            if r < 0.3:
                return f2b(t + rnd.choice([0.0, 0.001, -0.004, 0.005, -0.005, 0.0049, 0.006]))
            if r < 0.6:
                return f2b(t + rnd.choice([-1.0, 1.0, -10.0, 10.0, 0.1, -0.1]))
            return f2b(rnd.uniform(-5, 100))
        old, new = pick(), pick()
        wr32(TMP + 0x80, new)
        wr32(TMP + 0x84, old)
        has_timer = rd32(l + 0x1C) != 0
        call(0x6F4A9CF0, ecx=l, stack_args=(TMP + 0x80, TMP + 0x84))
        out.append("\t".join("%08x" % x if i != 1 and i != 3 else str(x) for i, x in enumerate(st)) +
                   "\t%08x\t%d\t%08x\t%08x\t%d\t" % (thr, mode, new, old, has_timer) + fn_listener_result(l, dummy))
    return out


def fn_cross_vectors(rnd, n):
    out = []
    for _ in range(n):
        st, sc, p, l, thr, mode, dummy = fn_listener_setup(rnd, False)
        wr32(TMP + 0x80, thr)
        call(0x6F4A8FD0, ecx=p, stack_args=(TMP + 0x90, TMP + 0x80))
        out.append("\t".join("%08x" % x if i != 1 and i != 3 else str(x) for i, x in enumerate(st)) +
                   "\t%08x\t%08x" % (thr, rd32(TMP + 0x90)))
    return out


def fn_mod_vectors(rnd, n):
    out = []
    for _ in range(n):
        st = fn_state(rnd)
        sc = fn_setup(*st)
        rate = f2b(rnd.choice([-1.0, 1.0, 0.5, -0.5, 1e-7, -1.9e-7, 2e-7, 0.0, 24 / 240.0, rnd.uniform(-3, 3)]))
        hor = f2b(rnd.choice([0.0, 1e-7, 1.9e-7, 2e-7, 1e-4, 1.0, 61.0, 300.0, 1000.0, rnd.uniform(0.0, 120.0)]))
        if rnd.random() < 0.05:
            hor = FLT_MAX
        sc.add_mod(rate, hor)
        p = sc.prop
        ents = sc.heap_entries()
        live = [e for e in ents if not (e[2] & 0x10000)]
        t = live[0] if live else None
        ent = "%08x	%d	%x" % (t[0], t[1], t[2]) if t else "-	-	-"
        # period of the entry
        n_h = rd32(Q + 0x20)
        per = "-"
        if t:
            arr = rd32(Q + 0x10)
            for i in range(1, n_h):
                e = rd32(arr + 4 * i)
                if rd32(e + 0x14) == t[1]:
                    per = "%08x" % rd32(e + 8)
        out.append("	".join("%08x" % x if i != 1 and i != 3 else str(x) for i, x in enumerate(st)) +
                   "	%08x	%08x	%08x	%08x	%08x	%d	%s	%s" % (rate, hor, rd32(p + 0x78), rd32(p + 0x7C), rd32(p + 0x70), rd32(p + 0x74), ent, per))
    return out


def fnvec(outdir, n, seed):
    rnd = random.Random(seed)
    for name, fn in (("eval", fn_eval_vectors), ("slope", fn_slope_vectors), ("decide", fn_decide_vectors),
                     ("jump", fn_jump_vectors), ("cross", fn_cross_vectors), ("mod", fn_mod_vectors)):
        lines = fn(rnd, n)
        path = os.path.join(outdir, "agile_fn_%s.tsv" % name)
        with open(path, "w", newline="\n") as f:
            f.write("\n".join(lines) + "\n")
        print(name, len(lines), path)


def gen_scenario(rnd):
    """A random scenario description: start state, operations by tick, and the number of ticks to run."""
    r = rnd.random()
    if r < 0.45:
        now, row = 0, 0
    elif r < 0.75:
        now, row = f2b(rnd.uniform(0.0, 299.0)), rnd.randrange(0, 3)
    else:
        now, row = f2b(rnd.uniform(296.0, 299.99)), rnd.randrange(0, 3)
    lo, hi = rnd.choice([(NEG_FLT_MAX, FLT_MAX), (f2b(0.0), FLT_MAX), (f2b(0.0), f2b(1e4)), (f2b(-1e4), f2b(1e4)),
                         (f2b(0.0), f2b(200.0))])
    base = f2b(rnd.choice([0.0, 1.0, 10.0, 60.0, 100.0, rnd.uniform(0.0, 120.0), rnd.uniform(0.0, 40.0)]))
    ops = []
    nmod = rnd.choice([1, 1, 1, 2, 3])
    t = 0
    kinds = rnd.choice(["count", "count", "count", "rise", "set", "mixed"])
    for k in range(nmod):
        if kinds == "count" or kinds == "mixed":
            d = rnd.choice([1.0, 1.0, 1.0, 0.5, 2.0, 1.5, 4.0, rnd.uniform(0.25, 3.0)])
            rate = f2b(-d)
            hor = rnd.choice([0.0, 0.0, 1.0, 1e-7, 1e-4, 5.0, 30.0, 200.0, rnd.uniform(1.0, 60.0)])
        else:
            d = rnd.choice([1.0, 0.5, 2.0, 0.1, rnd.uniform(0.05, 3.0)])
            rate = f2b(d) if rnd.random() < 0.85 else f2b(-d)
            hor = rnd.choice([0.0, 0.0, 10.0, rnd.uniform(1.0, 60.0)])
        ops.append((t, "M", rate, f2b(hor)))
        t += rnd.choice([0, 0, 1, 3, 40, 211])
    nl = rnd.choice([1, 1, 1, 2, 3])
    for k in range(nl):
        if kinds in ("count", "mixed"):
            ops.append((t, "L", 0, f2b(rnd.choice([0.0, 0.0, 0.0, 5.0, 10.0, rnd.uniform(0.0, 40.0), b2f(base), -5.0, 500.0, 0.004, -0.004])), 0xD01C4 + k))
        else:
            ops.append((t, "L", 1, f2b(rnd.choice([24.0, 12.0, 100.0, 20.0, rnd.uniform(5.0, 60.0)])), 0xD01C4 + k))
        t += rnd.choice([0, 1, 50])
    if kinds in ("set", "mixed") and rnd.random() < 0.8:
        ops.append((t + rnd.choice([100, 500, 1500]), "S", f2b(rnd.choice([0.0, 5.0, 25.0, rnd.uniform(-5.0, 80.0)]))))
    ops.sort(key=lambda o: o[0])
    ticks = rnd.choice([600, 1500, 3000, 4500, 6000])
    return dict(now=now, row=row, base=base, lo=lo, hi=hi, ops=ops, ticks=ticks)


def run_scenario(d):
    sc = Scn(d["now"], d["row"], d["base"], d["lo"], d["hi"])
    byt = {}
    for o in d["ops"]:
        byt.setdefault(o[0], []).append(o)
    vals = []
    for t in range(d["ticks"]):
        sc.tick = t
        for o in byt.get(t, ()):
            if o[1] == "M":
                sc.add_mod(o[2], o[3])
            elif o[1] == "L":
                sc.add_lis(o[2], o[3], o[4])
            elif o[1] == "S":
                sc.set_value(o[2])
        sc.step()
        sc.tick = t
        if t % 250 == 0 or t == d["ticks"] - 1:
            vals.append((t, sc.value()))
    return sc.events, vals


def fmt(n, d, events, vals):
    out = ["SCN %d %08x %d %08x %08x %08x %d" % (n, d["now"], d["row"], d["base"], d["lo"], d["hi"], d["ticks"])]
    for o in d["ops"]:
        if o[1] == "M":
            out.append("OP %d M %08x %08x" % (o[0], o[2], o[3]))
        elif o[1] == "L":
            out.append("OP %d L %d %08x %x" % (o[0], o[2], o[3], o[4]))
        else:
            out.append("OP %d S %08x" % (o[0], o[2]))
    for (t, q, row, li) in events:
        out.append("E %d %08x %d %d" % (t, q, row, li))
    for (t, v) in vals:
        out.append("V %d %08x" % (t, v))
    out.append("END")
    return out


def gen(path, count, seed):
    rnd = random.Random(seed)
    lines = ["# Agile engine scenarios (docs/spec/agile.md): tools/emu/agile_engine_emu.py gen on the real routines.",
             "# SCN n clock row base lo hi ticks | OP tick M rate horizon | OP tick L mode thr msg | OP tick S value",
             "# E tick queue_now row listener | V tick value (after the tick) | END"]
    fired = 0
    for n in range(count):
        d = gen_scenario(rnd)
        events, vals = run_scenario(d)
        fired += len(events)
        lines += fmt(n, d, events, vals)
        if n % 25 == 0:
            print("scenario", n, "events so far", fired, flush=True)
    with open(path, "w", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote", path, "scenarios", count, "events", fired)


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "probe"
    if cmd == "probe":
        probe()
    elif cmd == "fnvec":
        fnvec(sys.argv[2], int(sys.argv[3]), int(sys.argv[4]) if len(sys.argv) > 4 else 1)
    elif cmd == "gen":
        gen(sys.argv[2], int(sys.argv[3]), int(sys.argv[4]) if len(sys.argv) > 4 else 1)
