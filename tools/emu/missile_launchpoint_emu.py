"""Unicorn check of the missile LAUNCH POINT 0x6F29E6A0 (Game.dll 1.26a): where a ranged attack's projectile starts.

0x6F29E6A0 (thiscall on the unit, one stack argument = the output vector) returns
    unit position (x, y from the normaliser 0x6F4743A0, z from the unit's vtable slot 0xE4: ground + fly height)
  + R * O,
with O the weapon's launch offset (0x6F29E5F0: (launchX', launchY', launchZ) taken from the unit, see docs) and R the
rotation about the z axis by the unit's facing (0x6F4AFCD0 axis-angle matrix with the (0,0,1) axis, 0x6F6EF990 sin/cos,
0x6F07B950 matrix times vector), added by 0x6F07B8F0; when the unit flag +0x20 & 0x1000 is set, 0x6F2835A0 also adds the
global vector at 0x6FAB44AC to the position (0x6F1D2B10).

Real code that runs: 0x6F29E6A0, 0x6F4AFCD0, 0x6F6EF990, 0x6F07B950, 0x6F07B8F0, 0x6F2835A0, 0x6F00C9F0, 0x6F1D2B10 and
the soft-float library. Stubbed: 0x6F29E5F0 (offset supplied), the normaliser 0x6F4743A0 (position supplied), the unit's
vtable slots 0xB8 (position object), 0xE4 (height, x87 return, four stack arguments) and the position object's slot 0x1C
(facing).

Usage: GAME_DLL=... py tools/emu/missile_launchpoint_emu.py [--rs out.rs]
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
UNIT, VT_UNIT, POSOBJ, VT_POS, OFFS, OUT, ZF, ANG = (EXTRA + 0x100 * k for k in range(1, 9))
CODE = EXTRA + 0x2000
LAUNCHPT, OFFSET_FN, NORM = 0x6F29E6A0, 0x6F29E5F0, 0x6F4743A0
GLOBAL_ADD = 0x6FAB44AC


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


def hook(uc, eip, size, user):
    if eip == OFFSET_FN:
        out = r32(mu.reg_read(UC_X86_REG_ESP) + 4)
        for i in range(3):
            w32(out + 4 * i, r32(OFFS + 4 * i))
        mu.reg_write(UC_X86_REG_EAX, out)
        ret_to(4)
    elif eip == NORM:
        obj = mu.reg_read(UC_X86_REG_ECX)
        out = r32(mu.reg_read(UC_X86_REG_ESP) + 4)
        for i in range(3):
            w32(out + 4 * i, r32(obj + 8 + 4 * i))
        mu.reg_write(UC_X86_REG_EAX, out)
        ret_to(4)


mu.hook_add(UC_HOOK_CODE, hook)


def put(addr, b):
    mu.mem_write(addr, b)


S_POS, S_ANG, S_Z = CODE, CODE + 0x20, CODE + 0x40
put(S_POS, b"\xB8" + struct.pack("<I", POSOBJ) + b"\xC3")                                   # mov eax,POSOBJ ; ret
put(S_ANG, b"\x8B\x44\x24\x04\x8B\x15" + struct.pack("<I", ANG) + b"\x89\x10\xC2\x04\x00")  # *[esp+4] = [ANG] ; ret 4
put(S_Z, b"\xD9\x05" + struct.pack("<I", ZF) + b"\xC2\x10\x00")                             # fld [z] ; ret 16
w32(VT_UNIT + 0xB8, S_POS)
w32(VT_UNIT + 0xE4, S_Z)
w32(VT_POS + 0x1C, S_ANG)
w32(UNIT, VT_UNIT)
w32(POSOBJ, VT_POS)


def setup(c):
    for i in range(3):
        w32(OFFS + 4 * i, fb(c["o"][i]))
        w32(POSOBJ + 8 + 4 * i, fb(c["p"][i]))   # position floats at +8 (POSOBJ+0 is the vtable pointer)
        w32(GLOBAL_ADD + 4 * i, fb(c["g"][i]))
    w32(ANG, fb(c["angle"]))
    w32(ZF, fb(c["z"]))
    w32(UNIT + 0x20, 0x1000 if c["flag"] else 0)


def run_bits(c):
    setup(c)
    emu.call(LAUNCHPT, ecx=UNIT, stack_args=(OUT,))
    return [r32(OUT + 4 * i) for i in range(3)]


# the NORM stub copies three floats from the object it is given; the position object holds them from +8
def hook_norm_fix():
    pass


def gen_case(rng, i):
    f32 = lambda v: bf(fb(v))
    ang = rng.choice([0.0, 0.5, 1.5707964, 3.1415927, -1.0, 6.2831855, rng.uniform(-7, 7), rng.uniform(0, 6.2832)])
    return dict(
        o=[f32(v) for v in (rng.choice([0.0, 30.0, 60.0, -12.5, rng.uniform(-80, 80)]), rng.choice([0.0, 20.0, -8.0, rng.uniform(-60, 60)]),
                            rng.choice([0.0, 50.0, 90.0, rng.uniform(-20, 150)]))],
        p=[f32(v) for v in (rng.uniform(-4000, 4000), rng.uniform(-4000, 4000), 0.0)],
        z=f32(rng.choice([0.0, 128.0, rng.uniform(-100, 600)])),
        angle=f32(ang), flag=rng.random() < 0.2,
        g=[f32(v) for v in (rng.uniform(-10, 10), rng.uniform(-10, 10), rng.uniform(-10, 10))],
    )


def write_vectors(path, n=1500, seed=5):
    rng = random.Random(seed)
    rows = []
    for i in range(n):
        c = gen_case(rng, i)
        out = run_bits(c)
        rows.append((fb(c["o"][0]), fb(c["o"][1]), fb(c["o"][2]), fb(c["p"][0]), fb(c["p"][1]), fb(c["z"]),
                     fb(c["angle"]), 1 if c["flag"] else 0, fb(c["g"][0]), fb(c["g"][1]), fb(c["g"][2]), *out))
    lines = ["// Generated by tools/emu/missile_launchpoint_emu.py from the original Game.dll (0x6F29E6A0); do not edit.",
             "// (offset x, y, z as 0x6F29E5F0 returns them, unit x, y, unit z, facing, flag 0x1000, global add x, y, z,",
             "//  out x, y, z); all f32 bits except the flag.",
             "pub const ROWS: &[[u32; 14]] = &["]
    lines += ["    [" + ", ".join("0x%08X" % v for v in r) + "]," for r in rows]
    lines.append("];")
    with open(path, "w", newline="\n") as fh:
        fh.write("\n".join(lines) + "\n")
    print("rows", n)


if __name__ == "__main__":
    if len(sys.argv) > 2 and sys.argv[1] == "--rs":
        write_vectors(sys.argv[2])
        sys.exit(0)
    c = dict(o=[30.0, 0.0, 50.0], p=[100.0, 200.0, 0.0], z=10.0, angle=1.5707964, flag=False, g=[0.0, 0.0, 0.0])
    print([bf(v) for v in run_bits(c)])
