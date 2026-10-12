"""Unicorn check of the miss gate 0x6F2BEAC0 with the REAL ability-list iterator (Game.dll 1.26a).

Earlier harnesses (execute_evade_emu.py) replaced the iterator 0x6F4786B0 / 0x6F03FA30 by a stub. This one runs them
for real, over a hand-built agent table (the table pointer at [0x6FAB7788]) and ability lists, and calls the gate
directly:   ECX = the attack ability's owner (attacker), EDX = target, stack (0, 0)   (the direct-path call of the
execute 0x6F0CF660 at 0x6F0CF899).

Real code that runs: gate 0x6F2BEAC0, helper 0x6F2BEA20, walks 0x6F077360 (slots 0x158/0x164) and 0x6F077420 (slots
0x15C/0x168), iterator 0x6F4786B0, lookup 0x6F03FA30, score 0x6F08BDA0, EvadeAttack 0x6F08BD20, chance 0x6F02E9E0.
Stubbed: smart-pointer builder 0x6F038450, RNG helper 0x6F2853D0 (queue of draws, records the stream), effect
0x6F052FB0, assert 0x6F537E80, definition lookup 0x6F265ED0, ability slot 0x268 (enabled flag per case).

The Python model (below) is written from the disassembly; every random case must match it: result, number of draws,
the stream, the entry on which the effect ran.

Walk A (slots 0x158 score, 0x164 evade) runs over the TARGET's list; walk B (0x15C/0x168) over the ATTACKER's list.
Vtable base: the real CAbilityEvasion vtable starts at 0x6F89D62C (COL at -4); earlier notes used 0x6F89D628 (4 bytes low).

Usage: GAME_DLL=/path/to/Game.dll py tools/emu/evasion_gate_emu.py [cases]
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
D = emu.DATA
GATE, GATE_A = 0x6F2BEAC0, 0x6F2BEA20
S8450, RNG, EFFECT, ASSERT, DEFLOOK = 0x6F038450, 0x6F2853D0, 0x6F052FB0, 0x6F537E80, 0x6F265ED0
EVADE, SCORE = 0x6F08BD20, 0x6F08BDA0
BASE158, BASE164 = 0x6F052440, 0x6F0524D0
TABLE_PTR = 0x6FAB7788
EVASION_VT = 0x6F89D62C

TBL = D + 0x4000
ARR_LO = D + 0x5000
ARR_HI = D + 0x6000
HOBJ = D + 0x7000      # handle objects 0x80 apart
ABIL = D + 0x8000      # ability objects 0x100 apart
DEFS = D + 0xB000      # definition (0x100) + level table (0x100) per ability, 0x200 apart
UNIT_A = D + 0xE000
UNIT_T = D + 0xE400
VT_ON = D + 0xF000
VT_OFF = D + 0xF400
STUB_ON = D + 0xF800
STUB_OFF = D + 0xF810
NLO = 16
NHI = 16

ST = {"draws": [], "queue": [], "effects": [], "ptrs": 0}
FS = []


def w32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def r32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def f32b(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def bf32(b):
    return struct.unpack("<f", struct.pack("<I", b))[0]


def ret_to(pop):
    esp = mu.reg_read(UC_X86_REG_ESP)
    mu.reg_write(UC_X86_REG_EIP, r32(esp))
    mu.reg_write(UC_X86_REG_ESP, esp + 4 + pop)


def fs_scan(lo, size):
    code = bytes(mu.mem_read(lo, size))
    out = []
    for i in range(len(code) - 7):
        if code[i] != 0x64:
            continue
        if code[i + 1] == 0xA1 and code[i + 2:i + 6] == b"\0\0\0\0":
            out.append((lo + i, 6, True))
        elif code[i + 1] == 0xA3 and code[i + 2:i + 6] == b"\0\0\0\0":
            out.append((lo + i, 6, False))
        elif code[i + 1] == 0x89 and code[i + 2] == 0x0D and code[i + 3:i + 7] == b"\0\0\0\0":
            out.append((lo + i, 7, False))
    return out


TRACE = []


def on_code(uc, eip, size, user):
    TRACE.append(eip)
    del TRACE[:-12]
    for addr, ln, reads in FS:
        if eip == addr:
            if reads:
                mu.reg_write(UC_X86_REG_EAX, 0)
            mu.reg_write(UC_X86_REG_EIP, addr + ln)
            return
    if eip == S8450:
        out = mu.reg_read(UC_X86_REG_ECX)
        arg = r32(mu.reg_read(UC_X86_REG_ESP) + 4)
        w32(out, arg)
        if arg:
            w32(arg + 4, r32(arg + 4) + 1)
        mu.reg_write(UC_X86_REG_EAX, out)
        ret_to(4)
    elif eip == RNG:
        buf = mu.reg_read(UC_X86_REG_ECX)
        ST["draws"].append(mu.reg_read(UC_X86_REG_EDX))
        w32(buf, ST["queue"].pop(0))
        mu.reg_write(UC_X86_REG_EAX, buf)
        ret_to(0)
    elif eip == EFFECT:
        ST["effects"].append(mu.reg_read(UC_X86_REG_ECX))
        mu.reg_write(UC_X86_REG_EAX, 0)
        ret_to(0)
    elif eip == ASSERT:
        ret_to(0)
    elif eip == DEFLOOK:
        mu.reg_write(UC_X86_REG_EAX, 0)
        ret_to(0)
    elif eip == STUB_ON or eip == STUB_OFF:
        mu.reg_write(UC_X86_REG_EAX, 1 if eip == STUB_ON else 0)
        ret_to(4)


mu.hook_add(UC_HOOK_CODE, on_code)
FS.extend(fs_scan(GATE_A, 0x300))
mu.mem_write(STUB_ON, b"\xc3")
mu.mem_write(STUB_OFF, b"\xc3")
for vt, stub in ((VT_ON, STUB_ON), (VT_OFF, STUB_OFF)):
    # the REAL CAbilityEvasion vtable (RTTI COL at 0x6F89D62C-4); only slot 0x268 (enabled) is replaced
    mu.mem_write(vt, bytes(mu.mem_read(EVASION_VT, 0x400)))
    w32(vt + 0x268, stub)


def build(rng, mode):
    """Random scene. Returns the model description. Lists are built with random validity defects."""
    mu.mem_write(ARR_LO, b"\0" * 0x1000)
    mu.mem_write(ARR_HI, b"\0" * 0x1000)
    mu.mem_write(HOBJ, b"\0" * 0x1000)
    mu.mem_write(ABIL, b"\0" * 0x3000)
    mu.mem_write(DEFS, b"\0" * 0x3000)
    w32(TABLE_PTR, TBL)
    w32(TBL + 0xC, ARR_LO)
    w32(TBL + 0x1C, NLO)
    w32(TBL + 0x2C, ARR_HI)
    w32(TBL + 0x3C, NHI)
    for u in (UNIT_A, UNIT_T):
        mu.mem_write(u, b"\0" * 0x400)
        w32(u + 4, 100)
        w32(u + 0x1DC, 0xFFFFFFFF)
        w32(u + 0x1E0, 0xFFFFFFFF)
    free = list(range(NLO + NHI))
    rng.shuffle(free)
    scene = {"A": [], "T": []}
    k = 0
    for side, unit in (("A", UNIT_A), ("T", UNIT_T)):
        n = rng.choice([0, 0, 1, 1, 2, 3, 4])
        prev_pair = unit + 0x1DC
        for _ in range(n):
            slot = free.pop()
            hi = slot >= NLO
            idx = (slot - NLO) | 0x80000000 if hi else slot
            key = rng.randrange(1, 1 << 30)
            h = HOBJ + 0x40 * k
            ab = ABIL + 0x100 * k
            df = DEFS + 0x200 * k
            tb = df + 0x100
            ent = {"k": k, "idx": idx, "key": key}
            ent["removed"] = rng.random() < 0.1
            ent["stale"] = rng.random() < 0.1  # key in the object differs from the link's key
            ent["table_ok"] = rng.random() < 0.85  # slot marker == -2
            ent["enabled"] = rng.random() < 0.85
            ent["count"] = rng.choice([0, 0, 0, 1, -1])
            ent["level"] = rng.choice([0, 0, 0, 1, 2, -1])
            ent["dflag"] = rng.random() < 0.9
            ent["dcount"] = rng.choice([1, 2, 3])
            choice = rng.random()
            if choice < 0.15:
                ent["chance"] = [bf32(rng.choice([0x7FC00000, 0xFFC00000, 0x7F800000, 0x80000000, 0x00000000]))
                                 for _ in range(3)]
            else:
                ent["chance"] = [rng.choice([0.0, 0.25, 0.5, 0.75, 1.0, -0.5, rng.random()]) for _ in range(3)]
            # handle object
            w32(h + 0x18, key ^ (1 if ent["stale"] else 0))
            w32(h + 0x20, 1 if ent["removed"] else 0)
            w32(h + 0x54, ab)
            arr = ARR_HI if hi else ARR_LO
            i = (slot - NLO) if hi else slot
            w32(arr + i * 8, 0xFFFFFFFE if ent["table_ok"] else 0)
            w32(arr + i * 8 + 4, h)
            # ability
            w32(ab, VT_ON if ent["enabled"] else VT_OFF)
            w32(ab + 0x24, 0xFFFFFFFF)
            w32(ab + 0x28, 0xFFFFFFFF)
            w32(ab + 0x3C, ent["count"])
            w32(ab + 0x50, ent["level"] & 0xFFFFFFFF)
            w32(ab + 0x54, df)
            w32(df + 0x2C, 1 if ent["dflag"] else 0)
            w32(df + 0x50, ent["dcount"])
            w32(df + 0x54, tb)
            for lv in range(3):
                w32(tb + 0x20 + lv * 0x68, f32b(ent["chance"][lv]))
            # link from the previous node / the unit head
            w32(prev_pair, idx)
            w32(prev_pair + 4, key)
            prev_pair = ab + 0x24
            ent["ab"] = ab
            scene[side].append(ent)
            k += 1
    return scene


def chance_of(e):
    lv = e["level"]
    if lv < 0:
        return 0.0
    if e["dflag"] and lv < e["dcount"]:
        return e["chance"][lv]
    return 0.0


def reachable(lst):
    """Nodes the iterator reaches: stops at the first link that does not resolve."""
    out = []
    for e in lst:
        if e["stale"] or e["removed"] or not e["table_ok"]:
            break
        out.append(e)
    return out


def model(scene, rvals):
    """Gate result for ECX = attacker list (walk 2) per the disassembly; the target list (walk 1) scores 0.0."""
    nodes = reachable(scene["T"])  # walk A = the TARGET's list (slots 0x158 score / 0x164 evade)
    best = 0.0
    sel = None
    for e in nodes:
        sc = 0.0 if (e["count"] > 0 or not e["enabled"]) else chance_of(e)
        if not (best >= sc):  # fcompp: C0 set when best < sc or unordered
            best = sc
            sel = e
    draws = []
    effects = []
    res = 0
    if sel is not None and not (sel["count"] > 0 or not sel["enabled"]):
        c = chance_of(sel)
        r = rvals.pop(0)
        draws.append(2)
        if not (c <= r):  # evades iff c > r or unordered
            effects.append(sel["ab"])
            res = 1
    return res, draws, effects


def run(scene, rvals):
    ST["draws"] = []
    ST["effects"] = []
    ST["queue"] = [f32b(x) for x in rvals]
    emu.call(GATE, ecx=UNIT_A, edx=UNIT_T, stack_args=(0, 0))
    return ST["draws"][:], ST["effects"][:]


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 600
    rng = random.Random(12345)
    bad = 0
    stats = {"evaded": 0, "draws": 0, "tgt_only": 0}
    for case in range(n):
        scene = build(rng, 0)
        rvals = [rng.choice([rng.random(), 0.5, 0.25, 0.75]) for _ in range(4)]
        rv_model = [bf32(f32b(x)) for x in rvals]
        exp = model(scene, rv_model[:])
        res_raw = None
        ST["draws"], ST["effects"] = [], []
        ST["queue"] = [f32b(x) for x in rvals]
        try:
            res_raw = emu.call(GATE, ecx=UNIT_A, edx=UNIT_T, stack_args=(0, 0)) & 0xFF
        except Exception as ex:
            print("ERR", [hex(t) for t in TRACE], ex, hex(mu.reg_read(UC_X86_REG_EIP)), hex(r32(mu.reg_read(UC_X86_REG_ESP))), [hex(x) for x in ST["draws"]]); raise
        got = (res_raw, ST["draws"][:], ST["effects"][:])
        if got != exp:
            bad += 1
            if bad <= 5:
                print("MISMATCH case", case, "got", got, "expected", exp)
        stats["evaded"] += exp[0]
        stats["draws"] += len(exp[1])
        if scene["A"] and not scene["T"]:
            stats["tgt_only"] += 1
    print("cases", n, "mismatches", bad, stats)


if __name__ == "__main__":
    main()
