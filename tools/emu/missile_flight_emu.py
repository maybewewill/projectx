"""Unicorn check of the missile FLIGHT SETUP (Game.dll 1.26a): from the launch record's points to the completion timer.

Chain (all real code): 0x6F47A4A0 (stores the start cell: world minus origin, exponent 5 down, with the underflow mask),
0x6F47A650 (move issue: cell conversion of the target, the speed and the launch height) -> 0x6F497080 (move-target
setup: start cell back from the position object via 0x6F38B0B0, move vector via 0x6F020210, path request 0x6F496D10,
duration T' = min(T, horizon - 1.0, 60.0)) -> 0x6F4AA6A0 -> 0x6F483290 -> 0x6F4AA630 (clamp the duration to 1e-4, expiry =
clock + duration) -> 0x6F4AA5A0 (stub: records its arguments).

Stubbed (decisions unaffected): the handle lookup 0x6F03FA30 and the position-object getters 0x6F47A540 / 0x6F4AB150 (one
fake object), the playable-area clamp 0x6F469490 (identity), the store 0x6F4737D0 (writes the cell pair into the fake object
at +0x78/+0x7C; the object starts at zero, so the stored value is the converted point), the cell term 0x6F497460 (+0.0, a
fresh object on grid A with a zero step vector), 0x6F471AB0, 0x6F496980, 0x6F4AA560, 0x6F4969F0, 0x6F4B2DE0, 0x6F483450.

Usage: GAME_DLL=... py tools/emu/missile_flight_emu.py [--rs out.rs]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
EXTRA = 0x00400000
mu.mem_map(EXTRA, 0x10000)
ISSUE, SETPOS, GETMOVER, CLAMP, NOTIFY = 0x6F47A650, 0x6F47A4A0, 0x6F47A540, 0x6F469490, 0x6F471AB0
LOOKUP, STORE = 0x6F03FA30, 0x6F4737D0
SKIP = {0x6F496980: 0, 0x6F4AA560: 0xC, 0x6F4969F0: 4, 0x6F483450: 0xC}
GETOBJ, CELLTERM, HEADING, REC5A0 = 0x6F4AB150, 0x6F497460, 0x6F4B2DE0, 0x6F4AA5A0
WORLD_PTR = 0x6FAB7368
G_PTR = 0x6FAB73D8
POS, VT, MOVER, WORLD, FOBJ, SCRATCH, VEC = (EXTRA + 0x100 * k for k in range(1, 8))
TOBJ, HOME = EXTRA + 0x1200, EXTRA + 0x1400     # the followed target's position object, its handle struct
F_SPEED, F_Z, F_ARC = EXTRA + 0x900, EXTRA + 0x910, EXTRA + 0x920
STUBRET = EXTRA + 0x1000
GCLOCK = emu.GADDR + 0xBC     # the clock object the missile's grid uses (G + 0xBC)
LOG = []


def r32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def w32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def fb(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def bf(b):
    return struct.unpack("<f", struct.pack("<I", b))[0]


def ret_to(pop, eax=None):
    esp = mu.reg_read(UC_X86_REG_ESP)
    if eax is not None:
        mu.reg_write(UC_X86_REG_EAX, eax)
    mu.reg_write(UC_X86_REG_EIP, r32(esp))
    mu.reg_write(UC_X86_REG_ESP, esp + 4 + pop)


def hook(uc, eip, size, user):
    if eip == GETMOVER:
        ret_to(0, MOVER)
    elif eip == LOOKUP:
        ret_to(0, TOBJ if mu.reg_read(UC_X86_REG_ECX) == 0x77 else FOBJ)
    elif eip == CLAMP:
        ret_to(0, 0)
    elif eip == NOTIFY:
        ret_to(8, 0)
    elif eip in SKIP:
        ret_to(SKIP[eip], 0)
    elif eip == GETOBJ:
        ret_to(4, FOBJ)
    elif eip == STORE:
        # ecx = the object, [esp+4] = pointer to (cell x, cell y), [esp+8] = 1
        p = r32(mu.reg_read(UC_X86_REG_ESP) + 4)
        w32(FOBJ + 0x78, r32(p))
        w32(FOBJ + 0x7C, r32(p + 4))
        ret_to(8, 0)
    elif eip == CELLTERM:
        out = r32(mu.reg_read(UC_X86_REG_ESP) + 4)
        w32(out, 0)
        ret_to(4, out)
    elif eip == HEADING:
        ret_to(8, SCRATCH)
    elif eip == REC5A0:
        esp = mu.reg_read(UC_X86_REG_ESP)
        a = [r32(esp + 4 + 4 * i) for i in range(5)]

        def deref(p):
            try:
                return r32(p)
            except Exception:
                return None
        LOG.append([(x, deref(x)) for x in a])
        ret_to(0x14, 0)
    elif eip == STUBRET:
        ret_to(0, 0x1234)


mu.hook_add(UC_HOOK_CODE, hook)
mu.mem_write(STUBRET, b"\xc3")
w32(VT + 0x14, STUBRET)
w32(POS, VT)
w32(SCRATCH, 0)


def run(c):
    """c: launch (lx, ly, lz) and target (tx, ty, tz) in world units, speed (world/s), arc, origin (ox, oy), the
    clock (now, horizon). Returns (T' bits, expiry bits, duration given to the record bits) or None when no record."""
    del LOG[:]
    w32(WORLD_PTR, WORLD)
    w32(WORLD + 0x6C, fb(c["ox"]))
    w32(WORLD + 0x70, fb(c["oy"]))
    mu.mem_write(MOVER, b"\0" * 0x100)
    mu.mem_write(FOBJ, b"\0" * 0x100)
    w32(GCLOCK + 0x40, fb(c["now"]))
    w32(GCLOCK + 0x48, fb(c["horizon"]))
    w32(GCLOCK + 0x50, 0)
    w32(G_PTR, emu.GADDR)
    w32(F_SPEED, fb(c["speed"]))
    w32(F_Z, fb(c["lz"]))
    w32(F_ARC, fb(c["arc"]))
    for i, v in enumerate((c["lx"], c["ly"], c["lz"])):
        w32(VEC + 4 * i, fb(v))
    emu.call(SETPOS, ecx=POS, stack_args=(VEC,))
    # 0x6F47A650(this=POS; a2 = &launch z, a3..a5 = target x, y, z, a6 = &speed (world units/s), a7 = message,
    # a8 = notified object, a9 = &arc, a10 = 0 -> point move, otherwise the followed object)
    home = 0
    if c.get("homing"):
        # the homing branch follows the target object: its current cell position is 0x6F38B0B0 of its stored cells
        mu.mem_write(TOBJ, b"\0" * 0x100)
        mu.mem_write(HOME, b"\0" * 0x20)
        w32(HOME + 8, 0x77)
        w32(TOBJ + 0x78, fb(c["tcx"]))
        w32(TOBJ + 0x7C, fb(c["tcy"]))
        w32(TOBJ + 0x80, fb(c["tvx"]))
        w32(TOBJ + 0x84, fb(c["tvy"]))
        home = HOME
    args = [F_Z, fb(c["tx"]), fb(c["ty"]), fb(c["tz"]), F_SPEED, 0xD019C, 0xAAAA0000, F_ARC, home]
    emu.call(ISSUE, ecx=POS, stack_args=args)
    if not LOG:
        return None
    a = LOG[0]
    # args of 0x6F4AA5A0 (see the printout of the probe): a[3] points at the expiry float, a[2] is the duration by value
    return a[3][1], a[2][0]


def gen_case(rng, i):
    f32 = lambda v: struct.unpack("<f", struct.pack("<f", v))[0]
    pos = lambda: rng.choice([0.0, 31.5, -32.0, 127.5, 500.25, -700.5, rng.randrange(-4000, 4000) / 4.0, rng.uniform(-6000, 6000)])
    mode = i % 5
    c = dict(lx=pos(), ly=pos(), lz=rng.uniform(-100, 400), tx=pos(), ty=pos(), tz=rng.uniform(-100, 400))
    if mode == 1:        # short flights around the 0.1 s refusal
        c["tx"], c["ty"] = c["lx"] + rng.uniform(-80, 80), c["ly"] + rng.uniform(-80, 80)
    elif mode == 2:      # very long flights (cap 60 s)
        c["tx"], c["ty"] = c["lx"] + rng.uniform(-9000, 9000), c["ly"] + rng.uniform(-9000, 9000)
        c["speed"] = rng.uniform(5, 40)
    elif mode == 3:      # target on the launch point
        c["tx"], c["ty"] = c["lx"], c["ly"]
    c.setdefault("speed", rng.choice([900.0, 1200.0, 1500.0, 700.0, rng.uniform(50, 2500), 0.0, -5.0]))
    c["arc"] = rng.choice([0.0, 0.25, 0.5, 1.0])
    c["ox"] = rng.choice([-8192.0, -5248.0, 0.0, rng.uniform(-9000, 0)])
    c["oy"] = rng.choice([-8192.0, -5504.0, 0.0, rng.uniform(-9000, 0)])
    c["now"] = rng.choice([0.0, 0.005, 1.5, 12.34, rng.uniform(0, 298)])
    c["horizon"] = rng.choice([300.0, 300.0, 300.0, 100.0, 61.0])
    c["homing"] = i % 3 == 0
    c["tcx"], c["tcy"] = rng.uniform(-300, 300), rng.uniform(-300, 300)
    if mode == 1:
        c["tcx"], c["tcy"] = (c["lx"] - c["ox"]) / 32.0 + rng.uniform(-2, 2), (c["ly"] - c["oy"]) / 32.0 + rng.uniform(-2, 2)
    c["tvx"], c["tvy"] = rng.choice([0.0, rng.uniform(-10, 10)]), rng.choice([0.0, rng.uniform(-10, 10)])
    for k in list(c):
        if k != "homing":
            c[k] = f32(c[k])
    return c


def write_vectors(path, n=1500, seed=11):
    rng = random.Random(seed)
    rows, none = [], 0
    for i in range(n):
        c = gen_case(rng, i)
        res = run(c)
        if res is None:
            none += 1
            continue
        rows.append((fb(c["lx"]), fb(c["ly"]), fb(c["lz"]), fb(c["tx"]), fb(c["ty"]), fb(c["tz"]), fb(c["speed"]),
                     fb(c["arc"]), fb(c["ox"]), fb(c["oy"]), fb(c["now"]), fb(c["horizon"]), 1 if c["homing"] else 0,
                     fb(c["tcx"]), fb(c["tcy"]), fb(c["tvx"]), fb(c["tvy"]), res[0], res[1]))
    lines = ["// Generated by tools/emu/missile_flight_emu.py from the original Game.dll (0x6F47A650 -> 0x6F497080 -> 0x6F4AA6A0); do not edit.",
             "// (launch x, y, z, target x, y, z, speed [world/s], arc, origin x, y, clock now, clock horizon, homing flag,",
             "//  followed target cell x, y, step vx, vy, expiry, duration recorded); all f32 bits except the flag.",
             "pub const ROWS: &[[u32; 19]] = &["]
    lines += ["    [" + ", ".join("0x%08X" % v for v in r) + "]," for r in rows]
    lines.append("];")
    with open(path, "w", newline="\n") as fh:
        fh.write("\n".join(lines) + "\n")
    print("rows", len(rows), "no record", none)


if __name__ == "__main__":
    if len(sys.argv) > 2 and sys.argv[1] == "--rs":
        write_vectors(sys.argv[2])
        sys.exit(0)
    c = dict(lx=100.0, ly=200.0, lz=50.0, tx=1100.0, ty=200.0, tz=20.0, speed=900.0, arc=0.0, ox=-8192.0, oy=-8192.0,
             now=1.5, horizon=300.0)
    res = run(c)
    print(res, [[(hex(p), v if v is None else (hex(v), bf(v))) for p, v in row] for row in LOG])
