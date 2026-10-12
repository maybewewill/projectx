"""Unicorn probe of the construction tick CAbilityBuildInProgress `0x6F045600` (Game.dll 1.26a), docs/spec/construction.md 13.

The tick arms the agile life modifier of the unit under construction. The agile engine (property getters,
modifier constructors, listener registration) is replaced by recorders; the soft-float arithmetic and the
small helpers `0x6F022340` / `0x6F022530` run as in the original.

Inputs (all float bits): total = this+0x78, remaining R (the value the property getter of this+0x7C
returns), k = this+0x70 (with this+0x6C != 0 as the "has factor" tag), owner max life A (the cell the
owner's slot 0x130 returns).

Recorded: arguments of the modifier creator `0x6F02DC60` (owner life property): the two float values
pointed to by its 2nd and 3rd stack arguments, and which other helpers ran.

Usage:  GAME_DLL=/path/Game.dll py tools/emu/construction_tick_emu.py [--vectors out.tsv] [--n N]
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

TICK = 0x6F045600
OBJ = DATA + 0x4000
OWN = DATA + 0x4800
VT_OBJ = DATA + 0x5000
VT_OWN = DATA + 0x5400
VT_LIS = DATA + 0x5800
ACELL = DATA + 0x5C00
RCELL = DATA + 0x5C10
RET = DATA + 0x5F00

LOG = []
STATE = {}


def rd32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def stub(addr, name, retbytes, handler=None):
    """Replace the function at addr by a recorder. Handler(uc, ecx, edx, args) may return eax."""
    def cb(uc, address, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ecx = uc.reg_read(UC_X86_REG_ECX)
        edx = uc.reg_read(UC_X86_REG_EDX)
        args = [rd32(esp + 4 + 4 * i) for i in range(6)]
        eax = 0
        if handler:
            eax = handler(uc, ecx, edx, args) or 0
        LOG.append((name, ecx, edx, args[: max(retbytes // 4, 0)]))
        ret = rd32(esp)
        uc.reg_write(UC_X86_REG_EAX, eax)
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + retbytes)
        uc.reg_write(UC_X86_REG_EIP, ret)
    mu.hook_add(UC_HOOK_CODE, cb, None, addr, addr)


def f2b(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def b2f(b):
    return struct.unpack("<f", struct.pack("<I", b & 0xFFFFFFFF))[0]


def h_getprop(uc, ecx, edx, args):
    # 0x6F4773A0 (ecx = property, [esp+4] = out pointer): returns R for this+0x7C.
    wr32(args[0], STATE["R"])
    return args[0]


def h_calc(uc, ecx, edx, args):
    return ACELL  # owner slot 0x130: pointer to the cell holding A


def h_mod(uc, ecx, edx, args):
    # 0x6F02DC60(ecx = owner, out, &a, &b)
    STATE["mod"] = (rd32(args[1]), rd32(args[2]))
    return args[0]


def setup():
    try:
        mu.mem_map(0, 0x1000)
    except Exception:
        pass
    stub(0x6F4773A0, "getprop", 4, h_getprop)
    stub(0x6F4773E0, "agilecopy", 4)
    stub(0x6F02DC60, "mkmod", 12, h_mod)
    stub(0x6F02C960, "relmod", 0)
    stub(0x6F477D30, "cancel", 0)
    stub(0x6F4778F0, "listen", 20)
    stub(0x6F045110, "pauseres", 8)
    mu.mem_write(RET, b"\xC3")
    mu.mem_write(VT_OWN, struct.pack("<I", RET) * 256)
    mu.mem_write(VT_OBJ, struct.pack("<I", RET) * 256)
    mu.mem_write(VT_LIS, struct.pack("<I", RET) * 16)
    stub(RET, "vcall", 0)
    # owner slot 0x130
    calc = DATA + 0x5E00
    mu.mem_write(calc, b"\xB8" + struct.pack("<I", ACELL) + b"\xC2\x04\x00")  # mov eax, ACELL; ret 4
    mu.mem_write(VT_OWN + 0x130, struct.pack("<I", calc))
    # listener objects at +0xF8 and +0x100: first dword is a vtable-like pointer to a function pointer
    lis = DATA + 0x5E40
    mu.mem_write(lis, b"\xC2\x08\x00")  # ret 8
    mu.mem_write(VT_LIS, struct.pack("<I", lis))


def run(total, R, A, k, tag=1, extra=None):
    del LOG[:]
    STATE.clear()
    STATE["R"] = R
    mu.mem_write(OBJ, b"\0" * 0x200)
    mu.mem_write(OWN, b"\0" * 0x200)
    wr32(OBJ, VT_OBJ)
    wr32(OWN, VT_OWN)
    wr32(OBJ + 0x30, OWN)
    wr32(OBJ + 0x78, total)
    wr32(OBJ + 0x6C, tag)
    wr32(OBJ + 0x70, k)
    wr32(OBJ + 0xF8, VT_LIS)
    wr32(OBJ + 0x100, VT_LIS)
    wr32(ACELL, A)
    if extra:
        for off, v in extra.items():
            wr32(OBJ + off, v)
    emu.call(TICK, ecx=OBJ)
    return list(LOG), STATE.get("mod")


def cases(n, seed):
    rnd = random.Random(seed)
    out = []
    for total in (60.0, 0.0, 1.0, 100.0, 250.5):
        for r in (60.0, 0.0, 30.0, 1.0):
            for a in (1000.0, 0.0, 550.0):
                for k in (1.0, 2.0, 0.5, 0.0, 4.0):
                    out.append((f2b(total), f2b(r), f2b(a), f2b(k)))
    while len(out) < n:
        if rnd.random() < 0.6:
            out.append((f2b(rnd.uniform(1, 400)), f2b(rnd.uniform(0, 400)), f2b(rnd.uniform(1, 5000)),
                        f2b(rnd.choice([0.25, 0.5, 1.0, 2.0, 3.0, 10.0]))))
        else:
            out.append(tuple(rnd.getrandbits(32) for _ in range(4)))
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
    for t, r, a, k in cases(n, 5):
        lg, mod = run(t, r, a, k)
        started += int(mod is not None)
        lines.append("%08x\t%08x\t%08x\t%08x\t%s\t%s" % (t, r, a, k,
                     "%08x" % mod[0] if mod else "-", "%08x" % mod[1] if mod else "-"))
    print("cases", len(lines), "started", started)
    if vec:
        with open(vec, "w") as f:
            f.write("\n".join(lines) + "\n")
    else:
        for l in lines[:6]:
            print(l)


if __name__ == "__main__":
    main()
