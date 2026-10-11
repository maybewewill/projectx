"""Unicorn check of the AI town helpers of Game.dll 1.26a (docs/spec/ai-natives.md section 6).

Runs the original helper bodies behind the town natives against a Python model of the same rules:
  0x6F2D11A0  GetMinesOwned body        (this = AI state)
  0x6F2D1210  GetGoldOwned body
  0x6F2D1310  TownHasHall body          (stack: slot)
  0x6F2D12B0  TownHasMine body          (stack: slot)
  0x6F2D5330  add a site to the AI's slots (stack: site index)
  0x6F2D5380  TownWithMine body
The state is synthetic: a map-wide site array (global 0x6FAB4E90), an AI state with four slots (+0x1B8), unit
objects with the type (+0x30) and flag word (+0x5C), and a player record with the race (+0x260). Stubbed:
  0x6F4786B0  reference resolver (answers from the model: live unit pointer or 0)
  0x6F0787D0  ability finder (returns a record whose +0x6C is the unit's gold)
Everything else, including 0x6F2DEC10, 0x6F2D0C00, 0x6F3A1650 and the code constant 0x6F082470, is the real code.

Usage: GAME_DLL=/path/Game.dll python3 -I tools/emu/ai_towns_emu.py [--vectors OUT.tsv] [--cases N] [--seed S]
The vector file holds synthetic states and the original's answers only (no game data); it is read by
crates/sim/tests/ai_towns_vectors.rs. Line format (tab separated):
  op, race, slots(4, comma), sites (';' list of 'hall,m1,m2'), units ('id:type:flags:gold' list), arg, expected
where a site field is a unit id or -1 (no reference).
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

F_MINES = 0x6F2D11A0
F_GOLD = 0x6F2D1210
F_HAS_HALL = 0x6F2D1310
F_HAS_MINE = 0x6F2D12B0
F_ADD_SLOT = 0x6F2D5330
F_WITH_MINE = 0x6F2D5380
F_RESOLVE = 0x6F4786B0
F_FIND_ABILITY = 0x6F0787D0

G_SITE_ARRAY = 0x6FAB4E90
G_SITE_COUNT = 0x6FAB4E74
G_PLAYER_TABLE = 0x6FAB65F4

REGION = 0x00500000
mu.mem_map(REGION, 0x40000)
AI = REGION + 0x0000          # AI state (0x400 bytes)
SITE_ARR = REGION + 0x1000    # array of site pointers
SITES = REGION + 0x2000       # site records, 0x80 each
UNITS = REGION + 0x8000       # unit objects, 0x100 each
ABIL = REGION + 0x20000       # ability records, 0x80 each
PLAYER_TABLE = REGION + 0x30000  # player table: [idx*4 + 0x58]
PLAYER_REC = REGION + 0x31000    # player record, +0x260 race
TAG_LIVE = 0x2B61676C

UNIT_MODEL = {}  # id -> dict(live, type, flags, gold)


def rd(addr):
    return struct.unpack("<I", bytes(mu.mem_read(addr, 4)))[0]


def wr(addr, v):
    mu.mem_write(addr, struct.pack("<I", u32(v)))


def unit_addr(uid):
    return UNITS + uid * 0x100


def ret_plain(uc, ret_val, pops_args=0):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
    uc.reg_write(UC_X86_REG_EAX, u32(ret_val))
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pops_args)
    uc.reg_write(UC_X86_REG_EIP, ret)


def hook(uc, address, size, user):
    ecx = uc.reg_read(UC_X86_REG_ECX)
    if address == F_RESOLVE:
        # ECX points at an (id, tag) pair. A unit reference stores id = unit id + 1 (0 would clash with nothing).
        uid, tag = rd(ecx), rd(ecx + 4)
        u = UNIT_MODEL.get(uid)
        if tag == TAG_LIVE and u is not None and u["live"]:
            ret_plain(uc, unit_addr(uid))
        else:
            ret_plain(uc, 0)
    elif address == F_FIND_ABILITY:
        # ECX = unit pointer; the record's +0x6C is the gold of that unit.
        uid = (ecx - UNITS) // 0x100
        a = ABIL + uid * 0x80
        wr(a + 0x6C, UNIT_MODEL[uid]["gold"])
        ret_plain(uc, a, pops_args=0x14)


mu.hook_add(UC_HOOK_CODE, hook, begin=F_RESOLVE, end=F_RESOLVE)
mu.hook_add(UC_HOOK_CODE, hook, begin=F_FIND_ABILITY, end=F_FIND_ABILITY)


def ref(uid):
    """(id, tag) of a reference: -1 -> none."""
    if uid < 0:
        return (0xFFFFFFFF, 0xFFFFFFFF)
    return (uid, TAG_LIVE)


def build(case):
    """Writes the case into emulator memory."""
    for off in range(0, 0x400, 4):
        wr(AI + off, 0)
    for i in range(4):
        wr(AI + 0x1B8 + 4 * i, case["slots"][i] & 0xFFFFFFFF)
    wr(AI + 0x24, 3)  # player index
    wr(G_PLAYER_TABLE, PLAYER_TABLE)
    wr(PLAYER_TABLE + 3 * 4 + 0x58, PLAYER_REC)
    wr(PLAYER_REC + 0x260, case["race"])
    wr(G_SITE_ARRAY, SITE_ARR)
    wr(G_SITE_COUNT, len(case["sites"]))
    UNIT_MODEL.clear()
    for u in case["units"]:
        UNIT_MODEL[u["id"]] = u
        a = unit_addr(u["id"])
        for off in range(0, 0x100, 4):
            wr(a + off, 0)
        wr(a + 0x30, u["type"])
        wr(a + 0x5C, u["flags"])
    for i, s in enumerate(case["sites"]):
        a = SITES + i * 0x80
        wr(SITE_ARR + 4 * i, a)
        for off in range(0, 0x80, 4):
            wr(a + off, 0)
        for off, uid in ((0x48, s[0]), (0x54, s[1]), (0x60, s[2])):
            r = ref(uid)
            wr(a + off, r[0])
            wr(a + off + 4, r[1])


def call_thiscall(fn, arg=None):
    stack = () if arg is None else (arg,)
    return emu.call(fn, ecx=AI, stack_args=stack)


def s32(x):
    return x - (1 << 32) if x & 0x80000000 else x


# ---- the model (the rules of ai_towns.rs) ----

def live(case, uid):
    if uid < 0:
        return False
    u = UNIT_MODEL.get(uid)
    return bool(u and u["live"])


def site_of_slot(case, slot):
    if not 0 <= slot < 4:
        return None
    idx = case["slots"][slot]
    if idx < 0 or idx >= len(case["sites"]):
        return None
    return case["sites"][idx]


def model(case, op, arg):
    if op == "mines":
        n = 0
        for slot in range(4):
            s = site_of_slot(case, slot)
            if s is None:
                continue
            n += int(live(case, s[1])) + int(live(case, s[2]))
        return n
    if op == "gold":
        total = 0
        for slot in range(4):
            s = site_of_slot(case, slot)
            if s is None:
                continue
            for m in (s[1], s[2]):
                if live(case, m):
                    total += UNIT_MODEL[m]["gold"]
        return s32(u32(total))
    if op == "hall":
        s = site_of_slot(case, arg)
        return int(bool(s and live(case, s[0])))
    if op == "mine":
        s = site_of_slot(case, arg)
        return int(bool(s and (live(case, s[1]) or live(case, s[2]))))
    if op == "withmine":
        for slot in range(4):
            s = site_of_slot(case, slot)
            if s is None or not live(case, s[1]):
                continue
            m = UNIT_MODEL[s[1]]
            if case["race"] in (1, 2):
                if not live(case, s[0]):
                    continue
                if UNIT_MODEL[s[0]]["flags"] & 0x2000000:
                    continue
                return slot
            if m["flags"] & 0x2000000:
                continue
            if case["race"] != 4:
                return slot
            if m["type"] == 0x65676F6C:
                return slot
        return -1
    raise ValueError(op)


def model_add_slot(slots, site):
    slots = list(slots)
    for i in range(4):
        if slots[i] == site:
            return i, slots
    for i in range(4):
        if slots[i] == -1:
            slots[i] = site
            return i, slots
    return 4, slots


FUNCS = {"mines": F_MINES, "gold": F_GOLD, "hall": F_HAS_HALL, "mine": F_HAS_MINE, "withmine": F_WITH_MINE}


def random_case(rng):
    nsites = rng.randint(1, 6)
    units = []
    nunits = 0

    def new_unit(kind):
        nonlocal nunits
        uid = nunits
        nunits += 1
        typ = {"hall": 0x686B6565, "mine": rng.choice([0x6E676F6C, 0x75676F6C, 0x65676F6C, 0x65676F6C])}[kind]
        flags = rng.choice([0, 0, 0, 0x2000000, 0x100, 0x2000100])
        u = {"id": uid, "type": typ, "flags": flags, "gold": rng.choice([0, 300, 600, 12500, 50000, 2**31 - 1, 7]),
             "live": rng.random() < 0.8}
        units.append(u)
        return uid

    sites = []
    for _ in range(nsites):
        s = []
        for kind in ("hall", "mine", "mine"):
            s.append(new_unit(kind) if rng.random() < 0.7 else -1)
        sites.append(tuple(s))
    slots = []
    for _ in range(4):
        slots.append(rng.randrange(nsites) if rng.random() < 0.7 else -1)
    return {"race": rng.choice([1, 2, 3, 4, 5, 7]), "slots": slots, "sites": sites, "units": units}


def fmt_case(case):
    return "\t".join([
        str(case["race"]),
        ",".join(str(x) for x in case["slots"]),
        ";".join(",".join(str(x) for x in s) for s in case["sites"]),
        " ".join("%d:%d:%d:%d:%d" % (u["id"], u["type"], u["flags"], u["gold"], int(u["live"])) for u in case["units"]),
    ])


def main():
    args = sys.argv[1:]
    out = None
    n = 400
    seed = 1
    if "--vectors" in args:
        out = args[args.index("--vectors") + 1]
    if "--cases" in args:
        n = int(args[args.index("--cases") + 1])
    if "--seed" in args:
        seed = int(args[args.index("--seed") + 1])
    rng = random.Random(seed)
    lines = []
    checks = 0
    bad = 0
    for _ in range(n):
        case = random_case(rng)
        build(case)
        for op in ("mines", "gold", "withmine"):
            got = s32(call_thiscall(FUNCS[op]))
            want = model(case, op, None)
            checks += 1
            if got != want:
                bad += 1
                print("MISMATCH", op, got, want, fmt_case(case))
            lines.append("\t".join([op, fmt_case(case), "-", str(got)]))
        for op in ("hall", "mine"):
            for slot in (-1, 0, 1, 2, 3, 4, 5):
                got = call_thiscall(FUNCS[op], u32(slot)) & 0xFF
                want = model(case, op, slot)
                checks += 1
                if got != want:
                    bad += 1
                    print("MISMATCH", op, slot, got, want, fmt_case(case))
                lines.append("\t".join([op, fmt_case(case), str(slot), str(got)]))
        # add_slot on fresh state, every site index of the case plus an extra one
        for site in list(range(len(case["sites"]))) + [9]:
            build(case)
            got = call_thiscall(F_ADD_SLOT, site)
            want, new_slots = model_add_slot(case["slots"], site)
            after = [s32(rd(AI + 0x1B8 + 4 * i)) for i in range(4)]
            checks += 1
            if got != want or after != new_slots:
                bad += 1
                print("MISMATCH add_slot", site, got, want, after, new_slots)
            lines.append("\t".join(["addslot", fmt_case(case), str(site), "%d|%s" % (got, ",".join(str(x) for x in after))]))
    print("cases %d, checks %d, mismatches %d" % (n, checks, bad))
    if out:
        with open(out, "w") as f:
            f.write("\n".join(lines) + "\n")
        print("wrote", len(lines), "vector lines to", out)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
