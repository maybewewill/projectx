"""Unicorn probe of the timed-ability start `0x6F030920` (Game.dll 1.26a), docs/spec/construction.md section 13.

`0x6F030920(this, &D)` (ECX = this, one stack argument = pointer to a float D, ret 4) is called by the build,
train, research and revive classes right after they store the total time at this+0x78/+0x7C (`0x6F023A30`).
D is the speed factor of the object (`0x6F281530`: 1.0, or times a cheat constant), NOT the duration. It starts
the countdown of the agile property at this+0x7C: a modifier with slope -D and horizon R/D + 1.0, and a listener
that posts message 0xD01C4 (completion) when the property reaches 0.0.

The agile engine calls (property getter `0x6F4773A0`, modifier creator `0x6F4774E0`, listener creator `0x6F477550`)
and the vtable call on the object at this+0x6C are replaced by recorders. The soft-float arithmetic and
`0x6F021F80` (R / D) run as in the original.

Usage:  GAME_DLL=/path/Game.dll py tools/emu/timed_start_emu.py [--vectors out.tsv] [--n N]
Vector line: gate  D bits  R bits  ->  started(0/1)  slope bits  horizon bits  listener threshold bits  message
"""
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP, UC_X86_REG_EIP  # noqa: E402

mu = emu.mu
DATA = emu.DATA
START = 0x6F030920
OBJ = DATA + 0x4000
SUB = DATA + 0x4400       # object at this+0x6C: first dword = pointer to a function (ret 8)
SUBFN = DATA + 0x4410
ZERO = DATA + 0x4420      # cell whose content is 0 (returned by the creators)
ZPTR = DATA + 0x4430      # pointer to ZERO
DCELL = DATA + 0x4440
RCELL = DATA + 0x4450
LOG = []
ST = {}


def rd32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def stub(addr, name, retbytes, handler=None):
    def cb(uc, address, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        args = [rd32(esp + 4 + 4 * i) for i in range(6)]
        eax = handler(uc, args) if handler else 0
        LOG.append(name)
        ret = rd32(esp)
        uc.reg_write(UC_X86_REG_EAX, eax or 0)
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + retbytes)
        uc.reg_write(UC_X86_REG_EIP, ret)
    mu.hook_add(UC_HOOK_CODE, cb, None, addr, addr)


def h_get(uc, a):
    wr32(a[0], ST["R"])
    return a[0]


def h_mod(uc, a):
    # (out, &slope, &horizon)
    ST["slope"] = rd32(a[1])
    ST["horizon"] = rd32(a[2])
    return ZPTR


def h_lis(uc, a):
    # (out, flag, &threshold, msg, owner, flag2)
    ST["thr"] = rd32(a[2])
    ST["msg"] = a[3]
    return ZPTR


def setup():
    try:
        mu.mem_map(0, 0x1000)
    except Exception:
        pass
    stub(0x6F4773A0, "get", 4, h_get)
    stub(0x6F4774E0, "mod", 12, h_mod)
    stub(0x6F477550, "lis", 24, h_lis)
    mu.mem_write(SUBFN, b"\xC2\x08\x00")
    wr32(SUB, SUBFN)
    wr32(ZERO, 0)
    wr32(ZPTR, ZERO)


def f2b(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def b2f(b):
    return struct.unpack("<f", struct.pack("<I", b & 0xFFFFFFFF))[0]


def run(d_bits, r_bits, gate=0):
    del LOG[:]
    ST.clear()
    ST["R"] = r_bits
    mu.mem_write(OBJ, b"\0" * 0x200)
    wr32(OBJ + 0x6C, SUB)
    wr32(OBJ + 0x48, gate)
    wr32(DCELL, d_bits)
    emu.call(START, ecx=OBJ, stack_args=[DCELL])
    return ("mod" in LOG), ST.get("slope"), ST.get("horizon"), ST.get("thr"), ST.get("msg")


def cases(n, seed):
    rnd = random.Random(seed)
    fixed_d = [1.0, 2.0, 0.5, 0.25, 4.0, 10.0, 1e-6, 2e-7, 1.9999e-7, -1.0, 0.0, 3.0e38, 1e-30]
    fixed_r = [60.0, 0.0, 1.0, 30.0, 100.0, 5.5, 1e6]
    out = []
    for d in fixed_d:
        for r in fixed_r:
            out.append((f2b(d), f2b(r), 0))
    out.append((f2b(1.0), f2b(60.0), 1))
    out.append((f2b(1.0), f2b(60.0), 3))
    while len(out) < n:
        k = rnd.random()
        if k < 0.5:
            d = f2b(rnd.choice([0.1, 0.25, 0.5, 1.0, 1.5, 2.0, 4.0, 10.0]) * (1 + rnd.random() * 1e-3))
            r = f2b(rnd.uniform(0.0, 300.0))
        else:
            d = rnd.getrandbits(32)
            r = rnd.getrandbits(32)
        out.append((d, r, rnd.choice([0, 0, 0, 1, -1 & 0xFFFFFFFF, 2])))
    return out


def main():
    setup()
    n = 400
    vec = None
    for i, a in enumerate(sys.argv):
        if a == "--n":
            n = int(sys.argv[i + 1])
        if a == "--vectors":
            vec = sys.argv[i + 1]
    lines = []
    started = 0
    for d, r, g in cases(n, 11):
        s, sl, ho, thr, msg = run(d, r, g)
        started += int(s)
        lines.append("%d\t%08x\t%08x\t%d\t%s\t%s\t%s\t%s" % (g if g < 0x80000000 else g - (1 << 32), d, r, int(s),
                     "%08x" % sl if sl is not None else "-", "%08x" % ho if ho is not None else "-",
                     "%08x" % thr if thr is not None else "-", "%x" % msg if msg is not None else "-"))
    print("cases", len(lines), "started", started)
    if vec:
        with open(vec, "w") as f:
            f.write("\n".join(lines) + "\n")
    else:
        for l in lines[:12]:
            print(l)


if __name__ == "__main__":
    main()
