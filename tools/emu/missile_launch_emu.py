"""Unicorn probe of the missile launch 0x6F2C3F70 (Game.dll 1.26a): the TARGET POINT handed to the flight setup.

CMissile::Launch(this = missile, rec = launch record). For a single target that does not miss, the target point is the
target's position led by its velocity (a quadratic intercept solved by 0x6F4B3000), raised by the target's impact
offset (target vtable slot 0x118) on top of its ground height (slot 0xE4), and clamped to the whole-map rectangle
(0x6F00F560). The point and the record are handed to 0x6F2C3CF0 (stubbed here: it captures the three floats).

Real code that runs: 0x6F2C3F70, 0x6F476390 (target velocity), 0x6F4B3000 (intercept), 0x6F1D2B10 (vector copy), the
soft-float library. Stubbed: the property setters of the missile (slots 0 of the objects at this+0x90/+0x98), the target's
vtable slots 0xB8 (position), 0xE4 (ground height, x87 return), 0x118 (impact offset), the normaliser 0x6F4743A0
(positions supplied as world floats), the handle lookup 0x6F03FA30 (returns an object carrying the step vector), the miss
gate 0x6F2BEAC0, the target binding 0x6F2C1B50, the world rectangle 0x6F00F560, 0x6F2C3CF0 (capture).

Usage: GAME_DLL=... py tools/emu/missile_launch_emu.py [cases] [--rs out.rs]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
EXTRA = 0x00400000
mu.mem_map(EXTRA, 0x10000)
MISSILE, REC, TARGET, SOURCE = EXTRA, EXTRA + 0x400, EXTRA + 0x800, EXTRA + 0xC00
VT_TARGET, VT_SETTER = EXTRA + 0x1000, EXTRA + 0x1400
POS, VELOBJ, RECT, OUTCAP = EXTRA + 0x1800, EXTRA + 0x1900, EXTRA + 0x1A00, EXTRA + 0x1B00
GROUNDF, IMPACTF = EXTRA + 0x1C00, EXTRA + 0x1C10
CODE = EXTRA + 0x2000

LAUNCH = 0x6F2C3F70
NORM, LOOKUP, GATE, BIND, TAIL, RECTFN, MISSTXT, MISC = 0x6F4743A0, 0x6F03FA30, 0x6F2BEAC0, 0x6F2C1B50, 0x6F2C3CF0, 0x6F00F560, 0x6F2BB970, 0x6F009EE0
CHECK = {"miss": 0}


def w32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def r32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def fb(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def bf(b):
    return struct.unpack("<f", struct.pack("<I", b))[0]


def ret_to(pop):
    esp = mu.reg_read(UC_X86_REG_ESP)
    mu.reg_write(UC_X86_REG_EIP, r32(esp))
    mu.reg_write(UC_X86_REG_ESP, esp + 4 + pop)


SOFT = {0x6F6EEE20: ("mul", True), 0x6F6EEEF0: ("sub", True), 0x6F6EF000: ("add", True), 0x6F6EF750: ("sqrt", False),
        0x6F6EF100: ("recip", False)}
TRACE = {"on": False, "log": [], "pend": {}}


def hook(uc, eip, size, user):
    if TRACE["on"]:
        if eip in TRACE["pend"]:
            nm, out, a, b = TRACE["pend"].pop(eip)
            TRACE["log"].append((nm, bf(a), bf(b) if b is not None else None, bf(r32(out))))
        if eip in SOFT:
            nm, three = SOFT[eip]
            esp = mu.reg_read(UC_X86_REG_ESP)
            out, a_ptr = mu.reg_read(UC_X86_REG_ECX), mu.reg_read(UC_X86_REG_EDX)
            b = r32(r32(esp + 4)) if three else None
            TRACE["pend"][r32(esp)] = (nm, out, r32(a_ptr), b)
    if eip == NORM:
        obj = mu.reg_read(UC_X86_REG_ECX)
        out = r32(mu.reg_read(UC_X86_REG_ESP) + 4)
        for i in range(3):
            w32(out + 4 * i, r32(obj + 4 * i))
        mu.reg_write(UC_X86_REG_EAX, out)
        ret_to(4)
    elif eip == LOOKUP:
        mu.reg_write(UC_X86_REG_EAX, VELOBJ)
        ret_to(0)
    elif eip == GATE:
        mu.reg_write(UC_X86_REG_EAX, CHECK["miss"])
        ret_to(8)
    elif eip == BIND:
        ret_to(4)
    elif eip == MISSTXT:
        ret_to(0)
    elif eip == MISC:
        out = mu.reg_read(UC_X86_REG_ECX)
        w32(out, fb(0.5))
        mu.reg_write(UC_X86_REG_EAX, out)
        ret_to(8)
    elif eip == RECTFN:
        out = mu.reg_read(UC_X86_REG_ECX)
        for i in range(4):
            w32(out + 4 * i, r32(RECT + 4 * i))
        mu.reg_write(UC_X86_REG_EAX, out)
        ret_to(0)
    elif eip == 0x6F2C41C7:
        CHECK["solve"] = mu.reg_read(UC_X86_REG_EAX) & 0xFF
    elif eip == TAIL:
        a3 = r32(mu.reg_read(UC_X86_REG_ESP) + 8)
        for i in range(3):
            w32(OUTCAP + 4 * i, r32(a3 + 4 * i))
        w32(OUTCAP + 12, a3)
        CHECK["dump"] = [bf(r32(a3 + 4 * i)) for i in range(-6, 6)]
        mu.reg_write(UC_X86_REG_EAX, 0)
        ret_to(8)


mu.hook_add(UC_HOOK_CODE, hook)

# code stubs
def put(addr, b):
    mu.mem_write(addr, b)


S_SETTER, S_POS, S_GROUND, S_IMPACT = CODE, CODE + 0x20, CODE + 0x40, CODE + 0x60
put(S_SETTER, b"\xC2\x08\x00")                                       # ret 8
put(S_POS, b"\xB8" + struct.pack("<I", POS) + b"\xC3")               # mov eax,POS ; ret
put(S_GROUND, b"\xD9\x05" + struct.pack("<I", GROUNDF) + b"\xC2\x10\x00")  # fld [g] ; ret 16 (four stack arguments)
put(S_IMPACT, b"\xB8" + struct.pack("<I", IMPACTF) + b"\xC2\x04\x00")      # mov eax,[impact] ; ret 4
w32(VT_SETTER, S_SETTER)
w32(VT_TARGET + 0xB8, S_POS)
w32(VT_TARGET + 0xE4, S_GROUND)
w32(VT_TARGET + 0x118, S_IMPACT)
w32(TARGET, VT_TARGET)
w32(MISSILE + 0x90, VT_SETTER)
w32(MISSILE + 0x98, VT_SETTER)


def setup(c):
    mu.mem_write(REC, b"\0" * 0x100)
    w32(REC + 0x00, fb(c["lx"]))
    w32(REC + 0x04, fb(c["ly"]))
    w32(REC + 0x08, fb(c["lz"]))
    w32(REC + 0x38, SOURCE)
    w32(REC + 0x4C, fb(c["speed"]))
    w32(REC + 0x6C, TARGET)
    w32(REC + 0x78, 1)
    w32(REC + 0x68, 0)
    for i, v in enumerate((c["tx"], c["ty"], 0.0)):
        w32(POS + 4 * i, fb(v))
    # position record handle pair (+8,+0xC) are passed to the lookup stub; velocity at object +0x80/+0x84 (cells/s)
    w32(VELOBJ + 0x80, fb(c["vx"]))
    w32(VELOBJ + 0x84, fb(c["vy"]))
    w32(GROUNDF, fb(c["ground"]))
    w32(IMPACTF, fb(c["impact"]))
    for i, v in enumerate(c["rect"]):
        w32(RECT + 4 * i, fb(v))
    CHECK["miss"] = c.get("miss", 0)


def run(c):
    setup(c)
    mu.mem_write(OUTCAP, b"\0" * 16)
    emu.call(LAUNCH, ecx=MISSILE, stack_args=(REC,))
    return [bf(r32(OUTCAP + 4 * i)) for i in range(3)], r32(OUTCAP + 12)



def run_bits(c):
    setup(c)
    mu.mem_write(OUTCAP, b"\0" * 16)
    CHECK["solve"] = None
    emu.call(LAUNCH, ecx=MISSILE, stack_args=(REC,))
    return [r32(OUTCAP + 4 * i) for i in range(3)], CHECK["solve"]


def gen_case(rng, i):
    mode = i % 6
    pos = lambda: rng.choice([0.0, 31.5, -32.0, 127.5, 500.25, -700.5, rng.randrange(-4000, 4000) / 4.0, rng.uniform(-3000, 3000)])
    c = dict(lx=pos(), ly=pos(), lz=rng.uniform(-50, 200), tx=pos(), ty=pos(),
             ground=rng.choice([0.0, 10.0, -35.5, rng.uniform(-300, 600)]),
             impact=rng.choice([0.0, 5.0, 60.0, rng.uniform(0, 120)]), rect=None)
    # speeds in world units per second; the target step vector is in cells per second (times 32 inside)
    c["speed"] = rng.choice([0.0, 900.0, 1200.0, 1500.0, rng.uniform(100, 2500), rng.uniform(-10, 40)])
    if mode == 0:
        c["vx"] = c["vy"] = 0.0
    elif mode == 1:
        c["vx"], c["vy"] = rng.uniform(-5, 5), rng.uniform(-5, 5)
    elif mode == 2:
        c["vx"], c["vy"] = rng.uniform(-40, 40), rng.uniform(-40, 40)
    elif mode == 3:
        c["vx"], c["vy"] = rng.choice([0.0, 1.0, -2.5, 10.0]), rng.choice([0.0, -1.0, 3.0, 8.0])
    elif mode == 4:
        c["vx"], c["vy"] = rng.uniform(-1, 1), rng.uniform(-1, 1)
        c["tx"], c["ty"] = c["lx"] + rng.uniform(-3, 3), c["ly"] + rng.uniform(-3, 3)
    else:
        c["vx"], c["vy"] = rng.uniform(-8, 8), rng.uniform(-8, 8)
        c["speed"] = abs(c["speed"]) + 50.0
    w = rng.choice([4000.0, 6000.0, 1000.0, 20000.0])
    x0, x1 = -rng.uniform(100, w), rng.uniform(100, w)
    y0, y1 = -rng.uniform(100, w), rng.uniform(100, w)
    if rng.random() < 0.3:
        x1 = c["tx"] + rng.uniform(-200, 200)
    c["rect"] = (y0, x0, y1, x1)
    f32 = lambda v: bf(fb(v))
    for k in ("lx", "ly", "lz", "tx", "ty", "ground", "impact", "speed", "vx", "vy"):
        c[k] = f32(c[k])
    c["rect"] = tuple(f32(v) for v in c["rect"])
    return c


def write_vectors(path, n=1800, seed=2026):
    rng = random.Random(seed)
    rows, solved = [], 0
    for i in range(n):
        c = gen_case(rng, i)
        out, s = run_bits(c)
        solved += 1 if s else 0
        rows.append((fb(c["lx"]), fb(c["ly"]), fb(c["tx"]), fb(c["ty"]), fb(c["vx"]), fb(c["vy"]), fb(c["speed"]),
                     fb(c["ground"]), fb(c["impact"]), *[fb(v) for v in c["rect"]], out[0], out[1], out[2],
                     1 if s else 0))
    lines = ["// Generated by tools/emu/missile_launch_emu.py from the original Game.dll (0x6F2C3F70); do not edit.",
             "// (launch x, launch y, target x, target y, step vx, step vy [cells/s raw], speed, ground z, impact offset,",
             "//  rect y0, x0, y1, x1, out x, out y, out z, intercept solved); all f32 bits.",
             "pub const ROWS: &[[u32; 17]] = &["]
    lines += ["    [" + ", ".join("0x%08X" % v for v in r) + "]," for r in rows]
    lines.append("];")
    with open(path, "w", newline="\n") as fh:
        fh.write("\n".join(lines) + "\n")
    print("rows", n, "solved", solved)


if __name__ == "__main__":
    if len(sys.argv) > 2 and sys.argv[1] == "--rs":
        write_vectors(sys.argv[2])
        sys.exit(0)
    base = dict(lx=0.0, ly=0.0, lz=0.0, speed=900.0, tx=1000.0, ty=0.0, vx=0.0, vy=0.0, ground=10.0, impact=5.0,
                rect=(-5000.0, -5000.0, 5000.0, 5000.0))
    for c in (base, dict(base, vx=2.0, vy=1.0), dict(base, tx=300.0, ty=700.0, vx=2.0, vy=1.0, lx=10.0, ly=20.0)):
        out, s = run_bits(c)
        print([bf(v) for v in out], s)
