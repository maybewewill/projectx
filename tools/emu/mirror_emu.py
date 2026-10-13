"""Unicorn check of the numeric cores of Mirror Image (`AOmi`, `CAbilityMirrorImage`) of Game.dll 1.26a
(docs/spec/abilities-orc-heroes.md, section 7).

The delayed handler of the timer message 0xD01C1 (`0x6F13D990`) does, with the real helpers run here:
  R  the slot pick: the roll wrapper `0x6F285350` on combat stream 12 with count 1, sides N+1, bonus 0 and the
     multiplier 1.0; the slot of the real hero is `R2I(result) - 1`. (`0x6F13DA70`)
  P  the polygon offset `0x6F1127D0` (ECX = slot k, EDX = slot count, stack = pointer to the radius, two output
     pointers): out1 = r * sin(2 pi k / N), out2 = r * cos(2 pi k / N), all through the soft float.
  Q  the rotation matrix about the vertical axis `0x6F4AFCD0` (ECX = out 9 floats, EDX = pointer to the angle, stack =
     axis pointer, flag 1) for the axis (0, 0, 1) and the caster's facing.
  T  the transform `0x6F07B950` (ECX = out 3 floats, EDX = pointer to the vector, stack = pointer to the matrix).

Output (text, derived numbers only, one case per line, float values as hex bits):
  R <a> <b> <n> <result int> <a'> <b'>
  P <k> <N> <r> <out1> <out2>
  Q <angle> <m0..m8>
  T <m0..m8> <v0 v1 v2> <o0 o1 o2>

Usage: GAME_DLL=/path/Game.dll py tools/emu/mirror_emu.py OUT.txt COUNT SEED
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from emu import *  # noqa: E402,F401  (maps Game.dll, defines call/read_state/write_state, mu, DATA)
from test_emu import py_seed  # noqa: E402

M = 0xFFFFFFFF
ROLL = 0x6F285350
F_POS = 0x6F1127D0
F_MAT = 0x6F4AFCD0
F_XFORM = 0x6F07B950
F_F2I = 0x6F6EEC20
ONE_PTR = 0x6FAAE4F4
STREAM_PICK = 12

# The stream table of the roll harness (same layout as test_damage_roll.py).
OBJ = DATA + 0x500
P = DATA + 0x520
BASE = DATA + 0x1000
mu.mem_write(OBJ + 0x10, struct.pack('<I', P))
mu.mem_write(P + 0xC, struct.pack('<I', BASE))
mu.mem_write(0x6F4C34D0, b'\xb8' + struct.pack('<I', OBJ) + b'\xc3')  # mov eax, OBJ; ret
mu.mem_write(0x6F4CC1E0, b'\xc3')  # log helper on the count <= 0 path

OUT = DATA + 0x800
W = DATA + 0x2000  # scratch cells for the float helpers


def w32(a, v):
    mu.mem_write(a, struct.pack('<I', v & M))


def r32(a):
    return struct.unpack('<I', bytes(mu.mem_read(a, 4)))[0]


def stream_ptr(k):
    return BASE + 4 + 8 * k


def fbits(x):
    return struct.unpack('<I', struct.pack('<f', x))[0]


SPECIAL = [0x00000000, 0x80000000, 0x3F800000, 0xBF800000, 0x7F800000, 0xFF800000, 0x7FC00000, 0x00000001,
           0x43000000, 0x43800000, 0x40490FDB, 0x40C90FDB, 0x3FC90FDB]


def rnd_bits(r, lo=-1000.0, hi=1000.0):
    k = r.random()
    if k < 0.1:
        return r.choice(SPECIAL)
    if k < 0.8:
        return fbits(r.uniform(lo, hi))
    return r.getrandbits(32)


def case_roll(r, lines):
    n = r.randint(1, 9)
    for k in range(45):
        write_state(stream_ptr(k), *py_seed(r.getrandbits(32)))
    a, b = read_state(stream_ptr(STREAM_PICK))
    emu_ret = call(ROLL, ecx=OUT, edx=STREAM_PICK, stack_args=[1, n + 1, 0, ONE_PTR])
    assert emu_ret == OUT
    fb = r32(OUT)
    w32(W, fb)
    res = call(F_F2I, ecx=W)
    a2, b2 = read_state(stream_ptr(STREAM_PICK))
    lines.append(f"R {a:08x} {b:08x} {n} {res & M:08x} {a2:08x} {b2:08x}")


def case_pos(r, lines):
    n = r.randint(1, 10)
    k = r.randint(0, n)
    rb = rnd_bits(r, 0.0, 600.0) if r.random() < 0.8 else 0x43000000
    w32(W, rb)
    w32(W + 4, 0)
    w32(W + 8, 0)
    call(F_POS, ecx=k, edx=n, stack_args=(W, W + 4, W + 8))
    lines.append(f"P {k} {n} {rb:08x} {r32(W + 4):08x} {r32(W + 8):08x}")


def case_mat(r, lines):
    # The caster's facing: finite, mostly in [0, 2 pi), some negative or larger values and the two zeros.
    k = r.random()
    if k < 0.05:
        ang = r.choice([0x00000000, 0x80000000])
    elif k < 0.3:
        ang = fbits(r.uniform(-7.0, 7.0))
    else:
        ang = fbits(r.uniform(0.0, 6.2831855))
    w32(W, 0)
    w32(W + 4, 0)
    w32(W + 8, 0x3F800000)
    w32(W + 0x20, ang)
    mu.mem_write(W + 0x40, b'\0' * 0x40)
    call(F_MAT, ecx=W + 0x40, edx=W + 0x20, stack_args=(W, 1))
    ms = [r32(W + 0x40 + 4 * i) for i in range(9)]
    lines.append("Q %08x " % ang + " ".join("%08x" % m for m in ms))


def case_xform(r, lines):
    ms = [rnd_bits(r, -2.0, 2.0) for _ in range(9)]
    vs = [rnd_bits(r, -600.0, 600.0) for _ in range(3)]
    for i, m in enumerate(ms):
        w32(W + 0x40 + 4 * i, m)
    for i, v in enumerate(vs):
        w32(W + 0x20 + 4 * i, v)
    mu.mem_write(W + 0x80, b'\0' * 0x10)
    call(F_XFORM, ecx=W + 0x80, edx=W + 0x20, stack_args=(W + 0x40,))
    os_ = [r32(W + 0x80 + 4 * i) for i in range(3)]
    lines.append("T " + " ".join("%08x" % x for x in ms + vs + os_))


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    lines = []
    for _ in range(count):
        case_roll(r, lines)
    for _ in range(count):
        case_pos(r, lines)
    for _ in range(count):
        case_mat(r, lines)
    for _ in range(count):
        case_xform(r, lines)
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print(f"wrote {len(lines)} lines")


if __name__ == "__main__":
    main()
