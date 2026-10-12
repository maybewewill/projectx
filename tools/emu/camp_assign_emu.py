"""Unicorn check of the creep-camp assignment of Game.dll 1.26a (docs/spec/ai-natives.md section 10).

Runs the original `0x6F2E38A0` (stdcall, stack = the creep object, `retn 4`; returns the index of the camp the creep
joins, or -1 for a new camp). The camp list is the global array `0x6FAB4EC8` with count `0x6FAB4E78`; a camp's member
count is its `+0x24`. The cached squared limit `0x6FAB4EBC` is computed on first use as I2R(d * d) with
d = Misc `CreepCampPathingCellDistance` (reader `0x6F009F90`, stubbed to return d). Stubbed:
  0x6F009F90  Misc integer reader                      -> d
  0x6F2E1B70  first living member of the camp (ECX)    -> a member object or 0
  0x6F477070  squared distance creep <-> member         (stack: out, creep+0x164, 1; retn 0xC) -> scripted float
The routine itself decides: the lazy limit, the skip of full camps (12) and camps without a living member, the strict
"nearer than the best so far" test, and the result index (first wins on ties).

Usage: GAME_DLL=/path/Game.dll py -I tools/emu/camp_assign_emu.py [--vectors OUT.tsv] [--cases N] [--seed S]
Vector line: d, cache (0 = empty, else f32 bits), camps ('count/alive/dist_bits' joined by ';'), result index.
"""
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
u32 = emu.u32
F_ASSIGN = 0x6F2E38A0
H_MISC = 0x6F009F90
H_MEMBER = 0x6F2E1B70
H_DIST = 0x6F477070
G_ARRAY = 0x6FAB4EC8
G_COUNT = 0x6FAB4E78
G_CACHE = 0x6FAB4EBC

REGION = 0x00500000
mu.mem_map(REGION, 0x20000)
ARR = REGION
CAMPS = REGION + 0x1000      # 0x100 each
MEMBERS = REGION + 0x10000   # 0x400 each
CREEP = REGION + 0x1F000
CASE = {}


def rd(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr(a, v):
    mu.mem_write(a, struct.pack("<I", u32(v)))


def ret(uc, val, pop):
    esp = uc.reg_read(UC_X86_REG_ESP)
    r = rd(esp)
    uc.reg_write(UC_X86_REG_EAX, u32(val))
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
    uc.reg_write(UC_X86_REG_EIP, r)


def hook(uc, address, size, user):
    ecx = uc.reg_read(UC_X86_REG_ECX)
    esp = uc.reg_read(UC_X86_REG_ESP)
    if address == H_MISC:
        ret(uc, CASE["d"], 4)
    elif address == H_MEMBER:
        i = (ecx - CAMPS) // 0x100
        ret(uc, MEMBERS + i * 0x400 if CASE["camps"][i]["alive"] else 0, 0)
    elif address == H_DIST:
        out = rd(esp + 4)
        i = (ecx - 0x164 - MEMBERS) // 0x400
        wr(out, CASE["camps"][i]["dist"])
        ret(uc, out, 0xC)


for a in (H_MISC, H_MEMBER, H_DIST):
    mu.hook_add(UC_HOOK_CODE, hook, begin=a, end=a)


def fb(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def run(case):
    CASE.clear()
    CASE.update(case)
    wr(G_ARRAY, ARR)
    wr(G_COUNT, len(case["camps"]))
    wr(G_CACHE, case["cache"])
    for i, c in enumerate(case["camps"]):
        wr(ARR + 4 * i, CAMPS + i * 0x100)
        wr(CAMPS + i * 0x100 + 0x24, c["count"])
    got = emu.call(F_ASSIGN, stack_args=(CREEP,)) & 0xFFFFFFFF
    return got - (1 << 32) if got >= 1 << 31 else got


def random_case(rng):
    d = rng.choice([0, 1, 5, 26, 26, 26, 40])
    n = rng.randint(0, 9)
    limit = d * d
    camps = []
    for _ in range(n):
        dist = rng.choice([0.0, 1.0, limit * 0.5, float(limit), float(limit) - 0.5, float(limit) + 1.0, rng.uniform(0, 2.0 * limit + 4)])
        camps.append({
            "count": rng.choice([0, 1, 3, 5, 11, 12]),
            "alive": rng.choice([1, 1, 1, 0]),
            "dist": fb(dist),
        })
    cache = rng.choice([0, 0, 0, fb(float(rng.choice([100, 400, 676])))])
    return {"d": d, "cache": cache, "camps": camps}


def main():
    args = sys.argv[1:]
    out = args[args.index("--vectors") + 1] if "--vectors" in args else None
    n = int(args[args.index("--cases") + 1]) if "--cases" in args else 600
    seed = int(args[args.index("--seed") + 1]) if "--seed" in args else 1
    rng = random.Random(seed)
    lines = []
    for _ in range(n):
        case = random_case(rng)
        got = run(case)
        camps = ";".join("%d/%d/%08x" % (c["count"], c["alive"], c["dist"]) for c in case["camps"])
        lines.append("\t".join([str(case["d"]), "%08x" % case["cache"], camps or "-", str(got)]))
    print("cases", n)
    if out:
        open(out, "w").write("\n".join(lines) + "\n")
        print("wrote", out)


if __name__ == "__main__":
    main()
