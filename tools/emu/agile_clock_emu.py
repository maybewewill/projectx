"""Unicorn harness for the agile clock of Game.dll 1.26a (docs/spec/lockstep.md section 14).

Real code of Game.dll runs on a synthetic timer queue ("Agile/Timer Dispatch/Queue", constructed by 0x6F46C590):

  advance   0x6F46E520  clock := clock + inc (soft add); at the horizon: pump to the horizon, rebase, pump to the excess
  pump      0x6F46E370  pops every element with expiry <= clock; clock := that expiry during the callback
  rebase    0x6F469D40  every pending expiry -= horizon (soft sub), row += 1
  re-arm    0x6F46CC80  periodic: expiry := clock + period (soft add), pushed back
  start     0x6F477FF0 (-> 0x6F477F80 -> 0x6F477EF0 -> heap push 0x6F46A030)  the TimerStart / sleep path
  remaining 0x6F4779F0  expiry - queue clock;  elapsed 0x6F4775D0  timeout - remaining

Only the allocator of the element pool is replaced (a pre-filled free list), the owner callback (vtable slot 3, recorded) and
the base-class tail 0x6F022AA0 of Start. A Python model of the control flow (add/sub are the real soft-float routines
called in Unicorn) is compared with the real code over long tick sequences.

Run:  GAME_DLL=C:\\path\\Game.dll python tools/emu/agile_clock_emu.py probe | check [TICKS] [SEED] | vectors OUT.txt
No Game.dll bytes are written into the vectors.
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_ECX, UC_X86_REG_EDX  # noqa: E402

mu = emu.mu


def _call(fn, ecx=None, edx=None, stack_args=()):
    """emu.call without the wall-clock timeout (a timer thread per call costs about 1 ms)."""
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
    except Exception as exc:                       # an unmapped access or similar: report where
        raise RuntimeError("call %#x failed at %#x: %s" % (fn, mu.reg_read(UC_X86_REG_EIP), exc))
    if mu.reg_read(UC_X86_REG_EIP) != emu.SENT:
        raise RuntimeError("call %#x did not return (stopped at %#x)" % (fn, mu.reg_read(UC_X86_REG_EIP)))


ADD, SUB = 0x6F6EF000, 0x6F6EEEF0
ADVANCE, REMAINING, ELAPSED, START = 0x6F46E520, 0x6F4779F0, 0x6F4775D0, 0x6F477FF0
MGR_PTR = 0x6FAB7368

# synthetic memory
BASE = 0x00800000
mu.mem_map(BASE, 0x00200000)
MGR = BASE + 0x100
QA = BASE + 0x1000            # queue A
QL = BASE + 0x1200            # queue (l), unused but present
HEAPA = BASE + 0x10000
HEAP_CAP = 4096
POOL = BASE + 0x40000         # blocks of 0x40 bytes: +0 link, +4 element
POOL_N = 2048
TIMERS = BASE + 0x80000       # timer objects (CAgentTimer stand-ins), 0x40 bytes each
VT = BASE + 0x2000
STUB_FIRE = BASE + 0x2100
TMP = BASE + 0x3000           # scratch floats
MAX_TIMERS = 1024
H300 = 0x43960000
MIN_TIMEOUT = 0x38D1B717      # 1e-4  (float at 0x6FAAE478)
MIN_EXCESS = 0x3556BF95       # the float at 0x6FAAE61C
MILLI = 0x3A83126F            # the float at 0x6FAAE47C (0.001)


def rd32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def b2f(b):
    return struct.unpack("<f", struct.pack("<I", b & 0xFFFFFFFF))[0]


def f2b(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def soft(fn, a, b):
    wr32(TMP, a)
    wr32(TMP + 4, b)
    _call(fn, ecx=TMP + 8, edx=TMP, stack_args=(TMP + 4,))
    return rd32(TMP + 8)


def sadd(a, b):
    return soft(ADD, a, b)


def ssub(a, b):
    return soft(SUB, a, b)


fired = []          # (timer index, expiry bits, clock bits at the callback)


def _fire_hook(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    elem = rd32(esp + 4)
    t = uc.reg_read(UC_X86_REG_ECX)
    fired.append(((t - TIMERS) // 0x40, rd32(elem + 4), rd32(QA + 0x40)))
    uc.reg_write(UC_X86_REG_ESP, esp + 8)
    uc.reg_write(UC_X86_REG_EIP, ret)


def _base_tail_hook(uc, address, size, user):      # 0x6F022AA0: thiscall, one stack argument
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    uc.reg_write(UC_X86_REG_ESP, esp + 8)
    uc.reg_write(UC_X86_REG_EIP, ret)


_installed = [False]


def init_queue():
    if not _installed[0]:
        mu.hook_add(UC_HOOK_CODE, _fire_hook, begin=STUB_FIRE, end=STUB_FIRE)
        mu.hook_add(UC_HOOK_CODE, _base_tail_hook, begin=0x6F022AA0, end=0x6F022AA0)
        _installed[0] = True
    mu.mem_write(BASE, b"\0" * 0x8000)
    mu.mem_write(HEAPA, b"\0" * (4 * HEAP_CAP))
    mu.mem_write(POOL, b"\0" * (0x40 * POOL_N))
    mu.mem_write(TIMERS, b"\0" * (0x40 * MAX_TIMERS))
    wr32(MGR_PTR, MGR)
    wr32(MGR + 0x40, QA)
    wr32(MGR + 0x44, QL)
    for q in (QA, QL):
        wr32(q + 0x10, HEAPA)
        wr32(q + 0x18, 0)             # no growth: the harness never reallocates
        wr32(q + 0x1C, HEAP_CAP)
        wr32(q + 0x20, 1)             # count includes the unused slot 0
        wr32(q + 0x48, H300)
    for i in range(POOL_N):
        wr32(POOL + 0x40 * i, POOL + 0x40 * (i + 1) if i + 1 < POOL_N else 0)
    wr32(QA + 0x38, POOL)
    wr32(VT + 12, STUB_FIRE)
    wr32(VT + 16, 0x6F4776B0)
    wr32(VT + 24, 0x6F4779F0)
    for i in range(MAX_TIMERS):
        wr32(TIMERS + 0x40 * i, VT)
    del fired[:]


def real_start(idx, timeout_bits, repeat):
    t = TIMERS + 0x40 * idx
    wr32(TMP + 0x20, timeout_bits)
    wr32(TMP + 0x24, 0)
    _call(START, ecx=t, stack_args=(1, TMP + 0x20, TMP + 0x24, 1 if repeat else 0, 0))
    elem = rd32(t + 0xC)
    return dict(elem=elem, expiry=rd32(elem + 4), period=rd32(elem + 8), flags=rd32(elem + 0x10), seq=rd32(elem + 0x14))


def real_advance(inc_bits):
    wr32(TMP + 0x30, inc_bits)
    _call(ADVANCE, ecx=TMP + 0x30, edx=QA)


def real_remaining(idx):
    _call(REMAINING, ecx=TIMERS + 0x40 * idx, stack_args=(TMP + 0x34,))
    return rd32(TMP + 0x34)


def real_elapsed(idx):
    _call(ELAPSED, ecx=TIMERS + 0x40 * idx, stack_args=(TMP + 0x38,))
    return rd32(TMP + 0x38)


def real_state():
    return rd32(QA + 0x40), rd32(QA + 0x44), rd32(QA + 0x20) - 1


class Model:
    """Control flow of 0x6F46E520 / 0x6F46E370 / 0x6F469D40 / 0x6F46CC80 / 0x6F477FF0, soft add/sub from the real code."""

    def __init__(self):
        self.clock = 0
        self.row = 0
        self.seq = 0
        self.heap = []
        self.fired = []

    @staticmethod
    def lt(a, b):
        return b2f(a) < b2f(b)

    def start(self, idx, timeout, repeat):
        if b2f(timeout) == b2f(timeout) and not self.lt(timeout, H300):
            timeout = ssub(H300, MILLI)
        if self.lt(timeout, MIN_TIMEOUT):
            timeout = MIN_TIMEOUT
        self.seq += 1
        e = dict(expiry=sadd(self.clock, timeout), seq=self.seq, idx=idx, period=timeout, repeat=repeat)
        self.heap.append(e)
        return e

    def pump(self):
        v2 = self.clock
        while self.heap:
            top = min(self.heap, key=lambda e: (b2f(e["expiry"]), e["seq"]))
            if self.lt(v2, top["expiry"]):
                break
            self.heap.remove(top)
            self.clock = top["expiry"]
            self.fired.append((top["idx"], top["expiry"], self.clock))
            if top["repeat"]:
                top["expiry"] = sadd(self.clock, top["period"])
                self.heap.append(top)
        self.clock = v2

    def advance(self, inc):
        tmp = sadd(self.clock, inc)
        if self.lt(tmp, H300):
            self.clock = tmp
            self.pump()
            return
        excess = ssub(tmp, H300)
        if self.lt(ssub(excess, 0) & 0x7FFFFFFF, MIN_EXCESS):
            excess = 0
        self.clock = H300
        self.pump()
        for e in self.heap:
            e["expiry"] = ssub(e["expiry"], H300)
        self.row += 1
        self.clock = excess
        self.pump()

    def remaining(self, e):
        return ssub(e["expiry"], self.clock)


def probe():
    init_queue()
    for i, t in enumerate([0.0, 5e-5, 1e-4, 1.0, 299.0, 299.9995, 300.0, 600.0, 1e6]):
        d = real_start(i, f2b(t), False)
        print("%-10g -> expiry %-12.6g period %-12.6g flags %x seq %d" % (t, b2f(d["expiry"]), b2f(d["period"]), d["flags"], d["seq"]))


TICK = 0x3BA3D70A     # 0.005 f32, the per-tick increment (lockstep.md section 9)


def scenario(rng, ticks, pure=False):
    """Random action script: starts (tick, idx, timeout bits, repeat) and increment overrides (tick, bits)."""
    starts, incs = [], {}
    idx = 0
    pool = [0.0, 5e-5, 1e-4, 0.005, 0.01, 0.1, 0.5, 1.0, 29.999, 120.0, 299.0, 299.999, 300.0, 301.0, 600.0, 1800.0]
    for t in range(ticks):
        if rng.random() < 0.004 and idx < 400:
            r = rng.random()
            if r < 0.3:
                to = rng.uniform(0.001, 5.0)
            elif r < 0.5:
                to = rng.uniform(5.0, 299.0)
            elif r < 0.7:
                to = rng.choice(pool)
            elif r < 0.85:
                to = rng.uniform(250.0, 2000.0)
            else:
                to = rng.uniform(0.05, 1.0)
            rep = rng.random() < 0.45
            if rep:
                to = max(to, 0.02) if to < 300.0 else to      # keeps a periodic pump bounded
            starts.append((t, idx, f2b(to), rep))
            idx += 1
        if not pure and rng.random() < 0.002:
            incs[t] = f2b(rng.choice([rng.uniform(0.001, 0.02), rng.uniform(1.0, 20.0), 0.0, 0.0001, 0.1]))
    return starts, incs


def run(seed, ticks, pure=False, qevery=250):
    """Runs the scenario on the real code and the model; returns (mismatches, trace)."""
    rng = random.Random(seed)
    starts, incs = scenario(rng, ticks, pure)
    by_tick = {}
    for s in starts:
        by_tick.setdefault(s[0], []).append(s)
    init_queue()
    m = Model()
    elems = {}
    bad = 0
    trace = dict(starts=starts, incs=incs, checks=[], queries=[], fires=None, rollovers=0, seed=seed, ticks=ticks)
    qrng = random.Random(seed ^ 0x5EED)
    last_row = 0
    for t in range(ticks):
        for (_, idx, to, rep) in by_tick.get(t, ()):
            d = real_start(idx, to, rep)
            e = m.start(idx, to, rep)
            elems[idx] = e
            if (d["expiry"], d["period"], d["seq"]) != (e["expiry"], e["period"], e["seq"]) or bool(d["flags"] & 1) != rep:
                bad += 1
                print("START MISMATCH", t, idx, hex(to), d, e)
        inc = incs.get(t, TICK)
        real_advance(inc)
        m.advance(inc)
        rc, rr, rn = real_state()
        if (rc, rr, rn) != (m.clock, m.row, len(m.heap)):
            bad += 1
            print("STATE MISMATCH tick", t, hex(rc), rr, rn, hex(m.clock), m.row, len(m.heap))
            if bad > 5:
                break
        if rr != last_row:
            trace["rollovers"] += 1
            last_row = rr
        if t % 1000 == 0:
            trace["checks"].append((t, rc, rr, rn))
        if qevery and t % qevery == 0 and m.heap:
            for e in qrng.sample(m.heap, min(3, len(m.heap))):
                rrem = real_remaining(e["idx"])
                mrem = m.remaining(e)
                rel = real_elapsed(e["idx"])
                mel = ssub(e["period"], mrem)
                trace["queries"].append((t, e["idx"], rrem, rel))
                if rrem != mrem or rel != mel:
                    bad += 1
                    print("QUERY MISMATCH", t, e["idx"], hex(rrem), hex(mrem), hex(rel), hex(mel))
    if fired != m.fired:
        bad += 1
        n = next((i for i, (a, b) in enumerate(zip(fired, m.fired)) if a != b), min(len(fired), len(m.fired)))
        print("FIRED MISMATCH at", n, len(fired), len(m.fired), fired[n:n + 2], m.fired[n:n + 2])
    trace["fires"] = list(fired)
    return bad, trace


def fnv(fires):
    h = 0xCBF29CE484222325
    for (idx, exp, clk) in fires:
        for w in (idx, exp, clk):
            for b in struct.pack("<I", w):
                h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "probe":
        probe()
    elif cmd == "check":
        ticks = int(sys.argv[2]) if len(sys.argv) > 2 else 200000
        seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
        pure = len(sys.argv) > 4 and sys.argv[4] == "pure"
        bad, tr = run(seed, ticks, pure)
        print("ticks", ticks, "seed", seed, "rollovers", tr["rollovers"], "fires", len(tr["fires"]), "queries", len(tr["queries"]),
              "mismatches", bad)
        sys.exit(1 if bad else 0)
    elif cmd == "vectors":
        out = sys.argv[2]
        lines = ["# Agile-clock vectors (docs/spec/lockstep.md section 14): tools/emu/agile_clock_emu.py run on the real routines",
                 "# 0x6F46E520/0x6F46E370/0x6F469D40/0x6F46CC80/0x6F477FF0/0x6F4779F0/0x6F4775D0 over a synthetic queue.",
                 "# S tick idx timeout_bits repeat | I tick inc_bits | C tick clock_bits row pending | Q tick idx remaining elapsed",
                 "# FL idx expiry clock (first fires) | F count fnv64 | default increment 0x3BA3D70A"]
        for seed, ticks, pure in ((11, 150000, True), (12, 90000, False), (13, 150000, False)):
            bad, tr = run(seed, ticks, pure, qevery=1000)
            assert bad == 0, "mismatch while writing vectors"
            lines.append("SCENARIO %d %d" % (seed, ticks))
            for (t, idx, to, rep) in tr["starts"]:
                lines.append("S %d %d %08x %d" % (t, idx, to, 1 if rep else 0))
            for t in sorted(tr["incs"]):
                lines.append("I %d %08x" % (t, tr["incs"][t]))
            for (t, c, r, n) in tr["checks"]:
                lines.append("C %d %08x %d %d" % (t, c, r, n))
            for (t, idx, rem, el) in tr["queries"]:
                lines.append("Q %d %d %08x %08x" % (t, idx, rem, el))
            for (idx, exp, clk) in tr["fires"][:100]:
                lines.append("FL %d %08x %08x" % (idx, exp, clk))
            lines.append("F %d %016x" % (len(tr["fires"]), fnv(tr["fires"])))
            lines.append("ROLLOVERS %d" % tr["rollovers"])
            lines.append("END")
            print("scenario", seed, "rollovers", tr["rollovers"], "fires", len(tr["fires"]))
        with open(out, "w", newline=chr(10)) as fh:
            fh.write(chr(10).join(lines) + chr(10))
