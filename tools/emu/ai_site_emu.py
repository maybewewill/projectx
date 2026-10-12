"""Unicorn vectors for the computer AI's build-spot search (docs/spec/ai-natives.md section 8).

Runs the real code of Game.dll 1.26a:

  0x6F2E1630  work buffer builder (101 x 101 window around the town)
  0x6F2DF4A0  structure marking callback (buffer tier and radius from buffType / buffRadius), with the real footprint
              walk 0x6F3E0D30, cell callback 0x6F2DF420 and cell marker 0x6F2DF380
  0x6F2E4810  work buffer copy (the retry keeps the copy taken before the first search)
  0x6F2DF6F0  the search, with the real snap 0x6F3E0B50 (clamp 0x6F469490), neighbour push 0x6F2DF680 and the
              softfloat library

The engine predicates around it are replaced by deterministic models (stubs at the function entries), and the Rust port
(`crates/sim/src/ai_site.rs`, test `crates/sim/tests/ai_site_vectors.rs`) uses the same models:

  0x6F0128C0  cell level          level(x, y)      = 1 if H(s1, x, y) % m1 == 0 else 0
  0x6F468F90  unwalkable          blocked(fx, fy)  = H(s2, bits(fx), bits(fy)) % m2 == 0
  0x6F475560  reachability        ok(tx, ty)       = H(s3, bits(tx), bits(ty)) % m3 != 0   (-1 when not ok)
  0x6F2A50C0  placement           ok(x, y)         = H(s4, bits(x), bits(y)) % m4 == 0     (0 when ok)
  0x6F32D0B0  pathing footprint   type -> synthetic texture object (w, h)
  0x6F29C300 / 0x6F29C330  buffType enum / buffRadius of a type
  0x6F020D30  BuildingAngle       the angle of the case
  vtable slot 0xB8 and 0x6F4743A0 (unit position and its normaliser): the unit's (x, y)

H(s, a, b): h = (a * 0x9E3779B1) ^ (b * 0x85EBCA77) ^ (s * 0xC2B2AE3D) (mod 2^32); h ^= h >> 15; h *= 0x2C1B3C6D;
h ^= h >> 12; h *= 0x297A2D39; h ^= h >> 15.

Line format (tab separated, hex bit patterns for floats):
  C  seed  cx cy  type  count_ge2  player  minx miny maxx maxy  angle  w h  s1 m1 s2 m2 s3 m3 s4 m4  nunits
  U  buff_type  radius  x y  w h  type
  V  hash_value_grid_after_marking  hash_state_grid_after_marking
  R  status  x y  retried  reach_failed_first

Run:  GAME_DLL=/path/to/Game.dll py -I tools/emu/ai_site_emu.py OUT.tsv [N_CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EIP  # noqa: E402

mu = emu.mu

F_BUILD = 0x6F2E1630
F_MARK_UNIT = 0x6F2DF4A0
F_COPY = 0x6F2E4810
F_SEARCH = 0x6F2DF6F0
F_LEVEL = 0x6F0128C0
F_UNWALK = 0x6F468F90
F_REACH = 0x6F475560
F_PLACE = 0x6F2A50C0
F_TEXTURE = 0x6F32D0B0
F_BUFF_TYPE = 0x6F29C300
F_BUFF_RADIUS = 0x6F29C330
F_ANGLE = 0x6F020D30
F_NORMALISE = 0x6F4743A0
WORLD_PTR = 0x6FAB7368

REGION = 0x00500000
mu.mem_map(REGION, 0x200000)
W = REGION + 0x00000          # work buffer (0x19000 bytes)
W2 = REGION + 0x20000         # copy
WORLD = REGION + 0x50000
OUT = REGION + 0x51000        # status / x / y / coordinates
TEXS = REGION + 0x60000       # texture objects, 0x100 apart
TEXDATA = REGION + 0x70000    # texel bytes, 0x400 apart
UNITS = REGION + 0x80000      # unit objects, 0x100 apart
VT = REGION + 0x90000         # unit vtable
VSTUB = REGION + 0x91000      # target of vtable slot 0xB8 (hooked)
BUILDER = REGION + 0x92000

MODEL = {}
LAST = {}
UNIT_INFO = {}      # unit address -> dict(x, y)
TYPE_INFO = {}      # type -> dict(buff, radius, tex)
TEX_BY_TYPE = {}


def call(fn, ecx=None, edx=None, stack_args=()):
    """emu.call with a long time limit (the search walks up to 10,201 cells)."""
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
    mu.emu_start(fn, emu.SENT, timeout=900_000_000, count=2_000_000_000)
    if mu.reg_read(UC_X86_REG_EIP) != emu.SENT:
        raise RuntimeError("call %#x did not return" % fn)
    return mu.reg_read(UC_X86_REG_EAX)


def f2b(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def b2f(b):
    return struct.unpack("<f", struct.pack("<I", b))[0]


def wr(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def rd(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def H(s, a, b):
    m = 0xFFFFFFFF
    h = ((a * 0x9E3779B1) ^ (b * 0x85EBCA77) ^ (s * 0xC2B2AE3D)) & m
    h ^= h >> 15
    h = (h * 0x2C1B3C6D) & m
    h ^= h >> 12
    h = (h * 0x297A2D39) & m
    h ^= h >> 15
    return h


def fnv(data):
    h = 0xCBF29CE484222325
    for byte in data:
        h ^= byte
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def ret(uc, value, pops=0):
    esp = uc.reg_read(UC_X86_REG_ESP)
    r = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
    uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pops)
    uc.reg_write(UC_X86_REG_EIP, r)


def hook(uc, address, size, user):
    ecx = uc.reg_read(UC_X86_REG_ECX)
    edx = uc.reg_read(UC_X86_REG_EDX)
    m = MODEL
    if address == F_LEVEL:
        x, y = ecx & 0xFFFFFFFF, edx & 0xFFFFFFFF
        ret(uc, 1 if H(m["s1"], x, y) % m["m1"] == 0 else 0, 4)
    elif address == F_UNWALK:
        ret(uc, 1 if H(m["s2"], rd(ecx), rd(edx)) % m["m2"] == 0 else 0, 8)
    elif address == F_REACH:
        ok = H(m["s3"], rd(edx), rd(edx + 4)) % m["m3"] != 0
        m["reach_calls"] = m.get("reach_calls", 0) + 1
        ret(uc, 0 if ok else 0xFFFFFFFF, 0x20)
    elif address == F_PLACE:
        ok = H(m["s4"], rd(edx), rd(rd_stack(uc, 1))) % m["m4"] == 0
        ret(uc, 0 if ok else 68, 0x38)
    elif address == F_TEXTURE:
        ret(uc, TEX_BY_TYPE[ecx])
    elif address == F_BUFF_TYPE:
        ret(uc, TYPE_INFO[ecx]["buff"])
    elif address == F_BUFF_RADIUS:
        wr(ecx, TYPE_INFO[edx]["radius"])
        ret(uc, ecx)
    elif address == F_ANGLE:
        wr(ecx, m["angle"])
        ret(uc, ecx)
    elif address == VSTUB:
        out = rd_stack(uc, 1)
        info = UNIT_INFO[ecx]
        wr(out, info["x"])
        wr(out + 4, info["y"])
        ret(uc, out, 4)
    elif address == F_NORMALISE:
        ret(uc, ecx)


def rd_stack(uc, n):
    """The n-th stack argument at function entry (1 = first)."""
    esp = uc.reg_read(UC_X86_REG_ESP)
    return struct.unpack("<I", bytes(uc.mem_read(esp + 4 * n, 4)))[0]


for fn in (F_LEVEL, F_UNWALK, F_REACH, F_PLACE, F_TEXTURE, F_BUFF_TYPE, F_BUFF_RADIUS, F_ANGLE, VSTUB, F_NORMALISE):
    mu.hook_add(UC_HOOK_CODE, hook, begin=fn, end=fn)


def make_texture(index, w, h):
    t = TEXS + index * 0x100
    for off in range(0, 0x100, 4):
        wr(t + off, 0)
    wr(t + 8, w)
    wr(t + 0xC, h)
    wr(t + 0x20, TEXDATA + index * 0x400)
    return t


def pick_float(rng, lo, hi):
    return f2b(float(rng.randint(int(lo * 4), int(hi * 4))) / 4.0)


def gen_case(rng, seed, hard=False, dense=False):
    c = _gen_case(rng, seed)
    if dense:
        # Buffer radii that cover the whole window: no free cell is left, so the tier buckets decide.
        c["m4"] = rng.choice([2, 3, 12])
        c["m3"] = rng.choice([1000000, 1000000, 3])
        cx, cy = b2f(c["cx"]), b2f(c["cy"])
        c["units"] = [
            dict(
                buff=rng.choice([1, 2, 4, 8, 8, 0]),
                radius=rng.choice([f2b(15.0), f2b(20.0), f2b(25.0), f2b(12.0)]),
                x=f2b(cx + rng.uniform(-1500, 1500)), y=f2b(cy + rng.uniform(-1500, 1500)),
                w=rng.randint(2, 16), h=rng.randint(2, 16), type=0x1000 + k,
            )
            for k in range(rng.randint(14, 30))
        ]
        return c
    if hard:
        # Cases that reach the tier buckets, the reachability retry and the failure exit: placement rarely or never
        # passes, reachability often fails, crowded centre with large buffer radii.
        c["m4"] = rng.choice([1000000, 1000000, 400, 60, 12])
        c["m3"] = rng.choice([1, 2, 3])
        cx, cy = b2f(c["cx"]), b2f(c["cy"])
        c["units"] = [
            dict(
                buff=rng.choice([1, 2, 4, 8, 8, 0]),
                radius=rng.choice([f2b(7.0), f2b(8.0), f2b(9.0), f2b(15.0), f2b(5.0), f2b(11.0)]),
                x=f2b(cx + rng.uniform(-500, 500)), y=f2b(cy + rng.uniform(-500, 500)),
                w=rng.randint(2, 16), h=rng.randint(2, 16), type=0x1000 + k,
            )
            for k in range(rng.randint(6, 18))
        ]
    return c


def _gen_case(rng, seed):
    min_x = float(rng.randint(-100, 0) * 32)
    min_y = float(rng.randint(-100, 0) * 32)
    width = float(rng.randint(40, 260) * 32)
    height = float(rng.randint(40, 260) * 32)
    cx = rng.uniform(min_x, min_x + width)
    cy = rng.uniform(min_y, min_y + height)
    if rng.random() < 0.5:
        cx = float(int(cx / 64) * 64)
        cy = float(int(cy / 64) * 64)
    typ = rng.choice([0x68777477, 0x757A6967, 0x6F747262, 0x68626172, 0x68746F77, rng.getrandbits(32)])
    angle = rng.choice([0.0, 90.0, 180.0, 270.0, 270.0, 270.0, 45.0, 89.0])
    return dict(
        seed=seed, cx=f2b(cx), cy=f2b(cy), type=typ, ge2=rng.randint(0, 1), player=rng.randint(0, 15),
        minx=f2b(min_x), miny=f2b(min_y), maxx=f2b(min_x + width), maxy=f2b(min_y + height),
        angle=f2b(angle), w=rng.randint(1, 16), h=rng.randint(1, 16),
        s1=rng.getrandbits(32), m1=rng.choice([5, 9, 23, 1000000]),
        s2=rng.getrandbits(32), m2=rng.choice([6, 11, 40, 1000000]),
        s3=rng.getrandbits(32), m3=rng.choice([3, 5, 1000000]),
        s4=rng.getrandbits(32), m4=rng.choice([1, 1, 2, 3, 6, 20]),
        units=[
            dict(
                buff=rng.choice([0, 1, 2, 4, 8, 8, 4, 3, 5, 16]),
                radius=rng.choice([f2b(-1.0), 0, f2b(0.0), f2b(3.0), f2b(7.0), f2b(9.0), f2b(8.5), f2b(15.0), f2b(2.75), f2b(12.0)]),
                x=f2b(cx + rng.uniform(-1700, 1700)), y=f2b(cy + rng.uniform(-1700, 1700)),
                w=rng.randint(1, 16), h=rng.randint(1, 16), type=0x1000 + k,
            )
            for k in range(rng.choice([0, 0, 1, 3, 6, 12]))
        ],
    )


def run_case(c):
    global MODEL
    MODEL = dict(s1=c["s1"], m1=c["m1"], s2=c["s2"], m2=c["m2"], s3=c["s3"], m3=c["m3"], s4=c["s4"], m4=c["m4"],
                 angle=c["angle"])
    TEX_BY_TYPE.clear()
    TYPE_INFO.clear()
    UNIT_INFO.clear()
    for off in (0x6C, 0x70, 0x74, 0x78):
        pass
    wr(WORLD + 0x6C, c["minx"])
    wr(WORLD + 0x70, c["miny"])
    wr(WORLD + 0x74, c["maxx"])
    wr(WORLD + 0x78, c["maxy"])
    wr(WORLD_PTR, WORLD)
    TEX_BY_TYPE[c["type"]] = make_texture(0, c["w"], c["h"])
    # centre point in memory
    wr(OUT + 0x10, c["cx"])
    wr(OUT + 0x14, c["cy"])
    wr(BUILDER + 0x34, 7)
    # the build
    for off in range(0, 0x19100, 4):
        mu.mem_write(W + off, b"\0\0\0\0")
    call(F_BUILD, ecx=W, stack_args=(c["player"], OUT + 0x10, OUT + 0x14, c["type"], c["ge2"], BUILDER))
    # the units
    wr(VT + 0xB8, VSTUB)
    for k, u in enumerate(c["units"]):
        ua = UNITS + k * 0x100
        for off in range(0, 0x100, 4):
            wr(ua + off, 0)
        wr(ua, VT)
        wr(ua + 0x30, u["type"])
        UNIT_INFO[ua] = dict(x=u["x"], y=u["y"])
        TYPE_INFO[u["type"]] = dict(buff=u["buff"], radius=u["radius"])
        TEX_BY_TYPE[u["type"]] = make_texture(1 + k, u["w"], u["h"])
        call(F_MARK_UNIT, ecx=ua, edx=W)
    vgrid = b"".join(bytes(mu.mem_read(W + 0x2C + 108 * j, 101)) for j in range(101))
    sgrid = b"".join(bytes(mu.mem_read(W + 0x2ACC + 108 * j, 101)) for j in range(101))
    LAST["v"], LAST["s"] = vgrid, sgrid
    # the copy and the search, as 0x6F2E50E0 does them
    call(F_COPY, ecx=W2, stack_args=(W,))
    wr(OUT, 0)
    call(F_SEARCH, ecx=W, stack_args=(OUT, OUT + 4, 1))
    status = rd(OUT)
    retried = 0
    failed_first = rd(W + 0x20)
    if status == 0xBF800000 and failed_first:
        retried = 1
        call(F_SEARCH, ecx=W2, stack_args=(OUT, OUT + 4, 0))
        status = rd(OUT)
    x, y = (rd(OUT + 4), rd(OUT + 8)) if status == 0x3F800000 else (0, 0)
    return fnv(vgrid), fnv(sgrid), status, x, y, retried, failed_first


def main():
    out_path = sys.argv[1]
    cases = int(sys.argv[2]) if len(sys.argv) > 2 else 40
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    rng = random.Random(seed)
    stats = dict(found=0, retried=0, none=0)
    with open(out_path, "w") as out:
        for n in range(cases):
            c = gen_case(rng, n, hard=len(sys.argv) > 4 and sys.argv[4] == "hard",
                         dense=len(sys.argv) > 4 and sys.argv[4] == "dense")
            vh, sh, status, x, y, retried, ff = run_case(c)
            if status == 0x3F800000:
                stats["found"] += 1
            else:
                stats["none"] += 1
            stats["retried"] += retried
            out.write("C\t%d\t%08x\t%08x\t%08x\t%d\t%d\t%08x\t%08x\t%08x\t%08x\t%08x\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\n" % (
                c["seed"], c["cx"], c["cy"], c["type"], c["ge2"], c["player"], c["minx"], c["miny"], c["maxx"], c["maxy"],
                c["angle"], c["w"], c["h"], c["s1"], c["m1"], c["s2"], c["m2"], c["s3"], c["m3"], c["s4"], c["m4"],
                len(c["units"])))
            for u in c["units"]:
                out.write("U\t%d\t%08x\t%08x\t%08x\t%d\t%d\t%d\n" % (u["buff"], u["radius"], u["x"], u["y"], u["w"], u["h"], u["type"]))
            out.write("V\t%016x\t%016x\n" % (vh, sh))
            out.write("R\t%08x\t%08x\t%08x\t%d\t%d\n" % (status, x, y, retried, ff))
            out.flush()
            print("case %d: status %08x pos %08x %08x retried %d units %d" % (n, status, x, y, retried, len(c["units"])), flush=True)
    print("done: %s -> %s" % (stats, out_path))


if __name__ == "__main__":
    main()
