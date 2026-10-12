"""Unicorn check of the creep-camp chooser of Game.dll 1.26a (GetCreepCamp, docs/spec/ai-natives.md section 8).

Runs the original `0x6F2E3980` (the selection loop behind the native `GetCreepCamp(min, max, flyers_ok)` at
`0x6F2E3D50`; ECX = attack captain, EDX = min level, stack = max level, flyers_ok, `retn 8`; returns the first
living member of the chosen camp, or 0) against a Python model. The camp list is the global array `0x6FAB4EC8`
with count `0x6FAB4E78`; a camp's member count is its `+0x24`. Stubbed, answering from the case:
  0x6F2E1D50  camp not usable for the player      (ECX = camp, stack: player; retn 4) -> nonzero skips the camp
  0x6F2E1B70  first living member of the camp      (ECX = camp)
  0x6F2E1C50  camp level                           (ECX = camp)
  0x6F2E1E80  camp has flyers                      (ECX = camp)
  0x6F4743A0  position of a unit (writes nothing the chooser reads back)
  0x6F4758D0  path cost to the camp                (8 stack arguments, retn 0x20) -> scripted per call
  0x6F481460  world sub-object whose +0x9C bounds the search limit (retn 0)
The chooser's own logic is the real code: the limit lowering (min(limit, W+0x9C+100), start 5000), the cost
horizon (a too-strong or flyer camp lowers the horizon to cost+200), the discard of candidates beyond the horizon,
and the two result slots (best level-in-range camp, nearest camp below the minimum level).

Usage: GAME_DLL=/path/Game.dll py -I tools/emu/creep_camp_emu.py [--vectors OUT.tsv] [--cases N] [--seed S]
Vector line (tab separated): min, max, flyers_ok, bound, camps ('count/skip/level/flyers/cost' joined by ';'),
expected camp index or -1.
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

F_CHOOSE = 0x6F2E3980
H_SKIP = 0x6F2E1D50
H_MEMBER = 0x6F2E1B70
H_LEVEL = 0x6F2E1C50
H_FLY = 0x6F2E1E80
H_POS = 0x6F4743A0
H_COST = 0x6F4758D0
H_WORLD = 0x6F481460
G_CAMP_ARRAY = 0x6FAB4EC8
G_CAMP_COUNT = 0x6FAB4E78
G_WORLD_SLOT = 0x6FAB73D8

REGION = 0x00500000
mu.mem_map(REGION, 0x20000)
ARR = REGION + 0x0000
CAMPS = REGION + 0x1000     # 0x100 each
MEMBERS = REGION + 0x10000  # 0x400 each
CAPT = REGION + 0x1F000
WORLD = REGION + 0x1E000

CASE = {}
COST_I = [0]


def rd(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr(a, v):
    mu.mem_write(a, struct.pack("<I", u32(v)))


def camp_index(ptr):
    return (ptr - CAMPS) // 0x100


def ret(uc, val, pop):
    esp = uc.reg_read(UC_X86_REG_ESP)
    r = rd(esp)
    uc.reg_write(UC_X86_REG_EAX, u32(val))
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
    uc.reg_write(UC_X86_REG_EIP, r)


def hook(uc, address, size, user):
    ecx = uc.reg_read(UC_X86_REG_ECX)
    if address == H_SKIP:
        ret(uc, CASE["camps"][camp_index(ecx)]["skip"], 4)
    elif address == H_MEMBER:
        ret(uc, MEMBERS + camp_index(ecx) * 0x400, 0)
    elif address == H_LEVEL:
        ret(uc, CASE["camps"][camp_index(ecx)]["level"], 0)
    elif address == H_FLY:
        ret(uc, CASE["camps"][camp_index(ecx)]["fly"], 0)
    elif address == H_POS:
        ret(uc, ecx, 4)
    elif address == H_COST:
        # the cost belongs to the camp being visited: calls come in camp order, one per visited camp
        ret(uc, CASE["costs"][COST_I[0]], 0x20)
        COST_I[0] += 1
    elif address == H_WORLD:
        ret(uc, WORLD, 0)


for a in (H_SKIP, H_MEMBER, H_LEVEL, H_FLY, H_POS, H_COST, H_WORLD):
    mu.hook_add(UC_HOOK_CODE, hook, begin=a, end=a)


def build(case):
    CASE.clear()
    CASE.update(case)
    COST_I[0] = 0
    wr(G_CAMP_ARRAY, ARR)
    wr(G_CAMP_COUNT, len(case["camps"]))
    wr(G_WORLD_SLOT, emu.GADDR)
    wr(WORLD + 0x9C, case["bound"])
    wr(CAPT + 0x68, 3)
    for i, c in enumerate(case["camps"]):
        wr(ARR + 4 * i, CAMPS + i * 0x100)
        wr(CAMPS + i * 0x100 + 0x24, c["count"])


def visits(case):
    """Which camps the chooser asks a cost for (model of the skip conditions), in order: used to script costs."""
    return [i for i, c in enumerate(case["camps"]) if c["count"] != 0 and c["skip"] == 0]


def model(case, mn, mx, fly_ok):
    camps = case["camps"]
    horizon = 999999
    best_d, best = 999999, -1
    fb_d, fb = 999999, -1
    limit = 5000
    costs = list(case["costs"])
    for i, c in enumerate(camps):
        if c["count"] == 0 or c["skip"] != 0:
            continue
        cost = costs.pop(0)
        lim2 = u32(case["bound"] + 100)
        if lim2 < limit:
            limit = lim2
        if cost > horizon:
            continue
        lvl = c["level"]
        too_hard = lvl > mx or (not fly_ok and c["fly"] != 0)
        if not too_hard:
            if lvl < mn:
                if cost < fb_d:
                    fb_d, fb = cost, i
            else:
                if cost < best_d:
                    best_d, best = cost, i
            continue
        if cost < horizon - 200:
            horizon = cost + 200
        if best_d > horizon:
            best, best_d = -1, 999999
        if fb_d > horizon:
            fb, fb_d = -1, 999999
    return best if best >= 0 else fb


def random_case(rng):
    n = rng.randint(0, 9)
    camps = []
    for _ in range(n):
        camps.append({
            "count": rng.choice([0, 1, 2, 3, 5, 12]),
            "skip": rng.choice([0, 0, 0, 1]),
            "level": rng.choice([0, 2, 4, 6, 8, 10, 12, 16, 20, 30]),
            "fly": rng.choice([0, 0, 0, 1]),
        })
    nvis = len(visits({"camps": camps}))
    costs = [rng.choice([0, 30, 120, 300, 700, 1500, 2400, 4800, 99999]) for _ in range(nvis)]
    return {"camps": camps, "costs": costs, "bound": rng.choice([0, 400, 4000, 9000])}


def main():
    args = sys.argv[1:]
    out = args[args.index("--vectors") + 1] if "--vectors" in args else None
    n = int(args[args.index("--cases") + 1]) if "--cases" in args else 600
    seed = int(args[args.index("--seed") + 1]) if "--seed" in args else 1
    rng = random.Random(seed)
    lines = []
    bad = 0
    for _ in range(n):
        case = random_case(rng)
        mn = rng.choice([0, 0, 2, 5, 10])
        mx = mn + rng.choice([0, 3, 9, 20, 90])
        fly = rng.choice([0, 1])
        build(case)
        got_ptr = emu.call(F_CHOOSE, ecx=CAPT, edx=mn, stack_args=(mx, fly))
        got = (got_ptr - MEMBERS) // 0x400 if got_ptr else -1
        want = model(case, mn, mx, fly)
        if got != want:
            bad += 1
            print("MISMATCH", mn, mx, fly, case, got, want)
        camps = ";".join("%d/%d/%d/%d" % (c["count"], c["skip"], c["level"], c["fly"]) for c in case["camps"])
        lines.append("\t".join([str(mn), str(mx), str(fly), str(case["bound"]), camps or "-",
                                ",".join(str(x) for x in case["costs"]) or "-", str(got)]))
    print("cases %d, mismatches %d" % (n, bad))
    if out:
        with open(out, "w") as f:
            f.write("\n".join(lines) + "\n")
        print("wrote", len(lines), "vector lines to", out)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
