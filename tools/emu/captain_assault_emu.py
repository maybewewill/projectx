"""Unicorn check of the AddAssault request body of Game.dll 1.26a (docs/spec/ai-natives.md section 8).

Runs the original `0x6F2D6EA0` (the body behind AddAssault / AddDefenders: `0x6F2D96F0` / `0x6F2D96D0` fetch the
captain and tail-call it; stdcall-ish: ECX = AI state, stack = captain, quantity, type code, `retn 0xC`) against a
Python model of its control flow. The two stage routines are replaced by recording hooks:
  0x6F2D4F20  stage A "units already in the group"       (stack: captain, &qty, type; retn 0xC)
  0x6F2D50A0  stage B "recruit idle units"               (stack: captain, &qty, type, flag; retn 0x10)
A hook writes the call (stage, type, flag) to a log and lowers *qty by a scripted amount (including past zero).
What the body itself decides: the type pair (a type and its morphed form: ohun/otbk, edoc/edcm, edot/edtm,
ospw/ospm, ugar/ugrm; anything else has no partner), the stage order, the early exit when the quantity hits 0
(return 1), and at the end clearing bit 1 of the captain's flag word (+0x6C) and returning 0.

Usage: GAME_DLL=/path/Game.dll py -I tools/emu/captain_assault_emu.py [--vectors OUT.tsv] [--cases N] [--seed S]
Vector line (tab separated): type, qty0, flags0, scripted decrements (comma), expected: ret, flags, calls
  (calls = ';'-joined 'stage:type:flag' with stage A or B, types in hex).
"""
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
u32 = emu.u32

F_ADD = 0x6F2D6EA0
F_STAGE_A = 0x6F2D4F20
F_STAGE_B = 0x6F2D50A0

REGION = 0x00500000
mu.mem_map(REGION, 0x4000)
AI = REGION
CAPTAIN = REGION + 0x1000

PAIRS = {
    0x6F68756E: 0x6F74626B,  # ohun -> otbk
    0x65646F63: 0x6564636D,  # edoc -> edcm
    0x65646F74: 0x6564746D,  # edot -> edtm
    0x6F737077: 0x6F73706D,  # ospw -> ospm
    0x75676172: 0x7567726D,  # ugar -> ugrm
}

LOG = []
SCRIPT = []


def rd(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr(a, v):
    mu.mem_write(a, struct.pack("<I", u32(v)))


def hook(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd(esp)
    if address == F_STAGE_A:
        _cap, pq, typ = rd(esp + 4), rd(esp + 8), rd(esp + 12)
        LOG.append(("A", typ, 0))
        pop = 0xC
    else:
        _cap, pq, typ, flag = rd(esp + 4), rd(esp + 8), rd(esp + 12), rd(esp + 16)
        LOG.append(("B", typ, flag))
        pop = 0x10
    d = SCRIPT.pop(0) if SCRIPT else 0
    wr(pq, rd(pq) - d)
    uc.reg_write(UC_X86_REG_EAX, 0)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
    uc.reg_write(UC_X86_REG_EIP, ret)


mu.hook_add(UC_HOOK_CODE, hook, begin=F_STAGE_A, end=F_STAGE_A)
mu.hook_add(UC_HOOK_CODE, hook, begin=F_STAGE_B, end=F_STAGE_B)


def s32(x):
    return x - (1 << 32) if x & 0x80000000 else x


def run_original(typ, qty, flags, script):
    del LOG[:]
    SCRIPT[:] = list(script)
    wr(CAPTAIN + 0x6C, flags)
    ret = emu.call(F_ADD, ecx=AI, stack_args=(CAPTAIN, qty, typ))
    return ret, rd(CAPTAIN + 0x6C), list(LOG)


def model(typ, qty, flags, script):
    script = list(script)
    log = []
    alt = PAIRS.get(typ, 0)

    def stage(kind, t, flag):
        nonlocal qty
        log.append((kind, t, flag))
        d = script.pop(0) if script else 0
        qty = s32(u32(qty - d))

    seq = [("A", typ, 0)]
    if alt:
        seq.append(("A", alt, 0))
    seq.append(("B", typ, 1))
    if alt:
        seq.append(("B", alt, 1))
    seq.append(("B", typ, 0))
    if alt:
        seq.append(("B", alt, 0))
    for kind, t, flag in seq:
        stage(kind, t, flag)
        if qty == 0:
            return 1, flags, log
    return 0, flags & ~1, log


def fmt_calls(calls):
    return ";".join("%s:%X:%d" % c for c in calls)


def main():
    args = sys.argv[1:]
    out = args[args.index("--vectors") + 1] if "--vectors" in args else None
    n = int(args[args.index("--cases") + 1]) if "--cases" in args else 600
    seed = int(args[args.index("--seed") + 1]) if "--seed" in args else 1
    rng = random.Random(seed)
    types = list(PAIRS) + [PAIRS[k] for k in PAIRS] + [0x6F677275, 0x68666F6F, 0, 0x6F68756F]
    lines = []
    bad = 0
    for _ in range(n):
        typ = rng.choice(types)
        qty = rng.choice([0, 1, 1, 2, 3, 5, 9, 20, -1, -3])
        flags = rng.choice([0, 1, 3, 0x4001, 0x8001, 0x1])
        script = [rng.choice([0, 0, 1, 1, 2, 3, 5]) for _ in range(6)]
        got = run_original(typ, qty, flags, script)
        want = model(typ, qty, flags, script)
        ok = got[0] == want[0] and got[1] == want[1] and got[2] == want[2]
        if not ok:
            bad += 1
            print("MISMATCH", hex(typ), qty, hex(flags), script, got, want)
        lines.append("\t".join([
            "%X" % typ, str(qty), str(flags), ",".join(str(x) for x in script),
            "%d|%d|%s" % (got[0] & 0xFF, got[1], fmt_calls(got[2]))]))
    print("cases %d, mismatches %d" % (n, bad))
    if out:
        with open(out, "w") as f:
            f.write("\n".join(lines) + "\n")
        print("wrote", len(lines), "vector lines to", out)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
