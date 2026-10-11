"""Unicorn check of the kill-experience candidate test of Game.dll 1.26a (docs/spec/heroes.md, section 8.3).

The real code of the per-candidate routine `0x6F28B990` runs on a synthetic killer record and a synthetic hero
candidate. Real code also runs below it: the position normaliser chain (`0x6F4743A0` -> `0x6F4742F0` -> `0x6F497460`
-> `0x6F4885A0` -> `0x6F483270` / `0x6F483E70`), the 2-D vector helper `0x6F38B0B0`, softfloat, the distance helper
`0x6F020000` and the x87 comparison. Only these callees answer from the synthetic configuration:

  0x6F03FA30  handle lookup of the candidate's position reference (returns the synthetic unit object)
  0x6F26EB10  "hero at maximum level" of the candidate (the level code is `0x6F207F10`, not run here)
  0x6F3A34E0  shared-experience mask test: killer owner, candidate owner (returns the bit)
  0x6F079770  the parent of a flag-0x10 unit (returns a synthetic parent with flag 0)
  0x6F286640  list append (returns a fresh slot; the appended pointer is written by the caller)
  vtable 0xB8 of the candidate (returns the position reference; `ret 0`, the caller's pointer stays on the stack)

Everything else is the binary. The expected values come from the gate and the comparison as read in the disassembly:

  proceed  = (MaxLevelHeroesDrainExp != 0 or not at_max) and (flags & 0x40000100) == 0 and shared_bit
  global   = GlobalExperience != 0 (appended before the range test, for every proceeding candidate)
  in range = not (V < D), with D the squared distance of the normaliser outputs (NaN is accepted: the x87
             `fcompp` + `test ah,5` + `jnp` sequence rejects only the strictly-less case, V < D)

The harness reads D from the real code (the `fld` after `0x6F020000`) and V from the record, so the comparison
itself is checked without a Python float model of the normaliser. Lines are written as text vectors; no Game.dll
data is written into them.

Run:  GAME_DLL=/path/to/Game.dll python3 tools/emu/hero_candidate_emu.py OUT.txt [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402  (maps Game.dll; mu; DATA; STACK)
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_ECX, UC_X86_REG_EAX  # noqa: E402

mu = emu.mu
DATA = emu.DATA

CANDIDATE_ROUTINE = 0x6F28B990
NORMALISER = 0x6F4743A0          # reached through the real code; listed for the report only
PARENT_FN = 0x6F079770
GATE_ALLY = 0x6F3A34E0
GATE_MAX = 0x6F26EB10
LOOKUP = 0x6F03FA30
LIST_APPEND = 0x6F286640
WORLD_PTR = 0x6FAB7368           # DAT_6fab7368: the world object (origins at +0x6C / +0x70)
DRAIN_FLAG = 0x6FA70E98          # DAT_6fa70e98 (MaxLevelHeroesDrainExp)
GLOBAL_FLAG = 0x6FA70E94         # DAT_6fa70e94 (GlobalExperience)
DIST_RESULT_SITE = 0x6F28BA71    # `fld dword ptr [eax]` right after the call to 0x6F020000 (D is at [eax])
VCALL_SLOT = 0xB8

STUB = 0x00380000
STUB_SIZE = 0x4000
VCALL_STUB = STUB + 0x100
PARENT_STUB = STUB + 0x200

RECORD = DATA + 0x8000           # killer record: +0 x, +4 y, +8 killer owner, +0xC V, +0x10 in-range list, +0xDC global
CAND = DATA + 0x9000             # candidate hero
PARENT = DATA + 0x9100           # parent of a flag-0x10 unit
VT = DATA + 0x9400               # vtable (slot 0xB8 = VCALL_STUB)
OBJ = DATA + 0x9800              # synthetic unit object returned by the lookup (positions, grid sub-object)
WORLD = DATA + 0xA000            # world object: +0x6C / +0x70 origins
POSREF = DATA + 0xB000           # the position reference struct returned by the vcall (+8 index, +0xC tag)
SLOTS = DATA + 0xC000            # append slots (one dword each)

# Unit fields read by the code (docs/spec/unit-core.md, groups-events-2, heroes.md)
U_FLAGS = 0x5C
U_OWNER = 0x58


def rd32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def f2b(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def b2f(v):
    return struct.unpack("<f", struct.pack("<I", v & 0xFFFFFFFF))[0]


def wrf(a, v):
    wr32(a, f2b(v))


def rdf(a):
    return b2f(rd32(a))


def stack_arg(esp, i):
    return rd32(esp + 4 + 4 * i)


_pop_of = {}
_counts = {}
_handlers = {}


def hook(name, target, pop, handler):
    _pop_of[name] = pop
    _counts[name] = 0
    _handlers[target] = (name, pop, handler)
    mu.hook_add(UC_HOOK_CODE, _dispatch, begin=target, end=target)


def _dispatch(uc, addr, size, user):
    name, pop, handler = _handlers[addr]
    _counts[name] += 1
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    value = handler(uc, esp)
    if value is not None:
        uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
    uc.reg_write(UC_X86_REG_EIP, ret)


class Case:
    drain = 0          # MaxLevelHeroesDrainExp (DAT_6fa70e98)
    at_max = 0         # 0x6F26EB10 answer
    shared = False     # 0x6F3A34E0 answer
    global_on = 0      # GlobalExperience (DAT_6fa70e94)
    cand_flags = 0
    slot_next = 0      # next append slot index
    list_of = {}       # list address -> list of appended pointers
    vcall_ecx = []     # the unit each vcall was made on


C = Case()


def h_lookup(uc, esp):
    return OBJ


def h_parent(uc, esp):
    return PARENT


def h_at_max(uc, esp):
    return C.at_max


def h_shared(uc, esp):
    return 1 if C.shared else 0


def h_append(uc, esp):
    lst = uc.reg_read(UC_X86_REG_ECX)
    slot = SLOTS + 4 * C.slot_next
    C.slot_next += 1
    C.list_of.setdefault(lst, []).append(slot)
    return slot


def h_vcall(uc, esp):
    C.vcall_ecx.append(uc.reg_read(UC_X86_REG_ECX))
    return POSREF


def h_dist_site(uc, esp):
    # `fld dword ptr [eax]` is about to read D: EAX holds its address. Log D and V for the reference check.
    eax = uc.reg_read(UC_X86_REG_EAX)
    C.d_read = rdf(eax)
    C.v_read = rdf(RECORD + 0xC)
    return None


def install():
    mu.mem_map(STUB, STUB_SIZE)
    mu.mem_write(STUB, b"\xcc" * STUB_SIZE)
    # The vtable of the synthetic units: slot 0xB8 is the position reference getter.
    wr32(VT + VCALL_SLOT, VCALL_STUB)
    wr32(WORLD_PTR, WORLD)
    hook("lookup", LOOKUP, 0, h_lookup)
    hook("parent", PARENT_FN, 0, h_parent)
    hook("at_max", GATE_MAX, 0, h_at_max)
    hook("shared", GATE_ALLY, 8, h_shared)
    hook("append", LIST_APPEND, 0, h_append)
    hook("vcall", VCALL_STUB, 0, h_vcall)
    mu.hook_add(UC_HOOK_CODE, _dist_hook, begin=DIST_RESULT_SITE, end=DIST_RESULT_SITE)


def _dist_hook(uc, addr, size, user):
    h_dist_site(uc, 0)


def random_world(rng):
    # origins and cell/position fields of the synthetic unit and grid (values only feed the normaliser)
    for off in (0x6C, 0x70):
        wrf(WORLD + off, rng.uniform(-64.0, 64.0))
    for off in range(0x70, 0x88, 4):
        wrf(OBJ + off, rng.uniform(-1.0, 1.0))
    wr32(OBJ + 0x14, rng.getrandbits(32) & 0xFFFFFFFF)
    for off in range(0x40, 0x50, 4):
        wrf(OBJ + 0x14 + off, rng.uniform(-64.0, 64.0))
        wrf(OBJ + 0x68 + off, rng.uniform(-64.0, 64.0))
    wrf(OBJ + 0x14 + 0x48, rng.uniform(0.5, 2.0))
    wrf(OBJ + 0x68 + 0x48, rng.uniform(0.5, 2.0))


def run_once():
    C.list_of = {}
    C.vcall_ecx = []
    C.slot_next = 0
    C.d_read = None
    C.v_read = None
    emu.call(CANDIDATE_ROUTINE, ecx=RECORD, stack_args=(CAND,))


def run_case(rng):
    C.list_of = {}
    C.vcall_ecx = []
    C.slot_next = 0
    C.d_read = None
    C.v_read = None
    C.drain = rng.randrange(2)
    C.at_max = rng.randrange(2)
    C.shared = rng.random() < 0.6
    C.global_on = rng.randrange(2)
    flag_choices = [0, 0, 0, 0x10, 0x100, 0x40000000, 0x20, 0x1000, 0x40000100, 0x10 | 0x100]
    C.cand_flags = rng.choice(flag_choices)
    wr32(DRAIN_FLAG, C.drain)
    wr32(GLOBAL_FLAG, C.global_on)
    random_world(rng)
    # candidate and its parent
    wr32(CAND + 0x00, VT)
    wr32(CAND + U_FLAGS, C.cand_flags)
    wr32(CAND + U_OWNER, rng.randrange(12))
    wr32(PARENT + 0x00, VT)
    wr32(PARENT + U_FLAGS, 0)
    # the position reference (+8 agent index, +0xC tag) used by the lookup hook
    wr32(POSREF + 8, rng.getrandbits(31))
    wr32(POSREF + 0xC, rng.getrandbits(32))
    # the killer record: normalised dying position, killer owner, V = R * R, the two lists
    wrf(RECORD + 0x0, rng.uniform(-200.0, 200.0))
    wrf(RECORD + 0x4, rng.uniform(-200.0, 200.0))
    wr32(RECORD + 0x8, rng.randrange(12))
    radius = rng.choice([10.0, 30.0, 100.0, 200.0, 400.0, 1000.0])
    wrf(RECORD + 0xC, radius * radius)
    # a flag-0x10 unit is replaced by its parent (flag 0), which the vcall then receives
    expect_unit = PARENT if (C.cand_flags & 0x10) else CAND
    run_once()
    # Boundary pass: when the candidate is on the path, the same inputs are run again with V = D (read from the
    # first run). The original accepts a tie (V < D is the only rejection), so the candidate must be listed.
    if C.d_read is not None and rng.random() < 0.3:
        if rng.random() < 0.25:
            # V = quiet NaN: the x87 compare is unordered, which the original does not reject (the reject needs V < D)
            wr32(RECORD + 0xC, 0x7FC00000)
        else:
            wrf(RECORD + 0xC, C.d_read)
        run_once()
        C.boundary = True
    else:
        C.boundary = False
    gate = (C.drain != 0 or C.at_max == 0) and (C.cand_flags & 0x40000100) == 0 and C.shared
    appended_global = len(C.list_of.get(RECORD + 0xDC, []))
    appended_in = len(C.list_of.get(RECORD + 0x10, []))
    expect_global = 1 if (gate and C.global_on) else 0
    if C.d_read is None:
        expect_in = 0
    else:
        d, v = C.d_read, C.v_read
        in_range = not (v < d)           # NaN accepted (unordered compare)
        expect_in = 1 if (gate and in_range) else 0
    ok = appended_global == expect_global and appended_in == expect_in
    if gate and C.d_read is not None:
        ok = ok and C.vcall_ecx == [expect_unit]
    elif not gate:
        ok = ok and C.vcall_ecx == []
    if not ok:
        raise SystemExit("mismatch: flags=%#x drain=%d at_max=%d shared=%d global=%d gl=%d in=%d expect=%d/%d vcall=%s"
                         % (C.cand_flags, C.drain, C.at_max, C.shared, C.global_on, appended_global,
                            appended_in, expect_global, expect_in, C.vcall_ecx))
    return {"boundary": C.boundary, "flags": C.cand_flags, "drain": C.drain, "at_max": C.at_max, "shared": int(C.shared),
            "global_on": C.global_on, "gate": gate, "appended_global": appended_global,
            "appended_in": appended_in, "d": C.d_read, "v": C.v_read}


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "hero_candidate_vectors.txt"
    cases = int(sys.argv[2]) if len(sys.argv) > 2 else 500
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    rng = random.Random(seed)
    install()
    lines = []
    stats = {"gate_pass": 0, "gate_fail": 0, "global_added": 0, "in_range": 0, "out_of_range": 0, "nan_d": 0,
             "flag_0x10_substituted": 0}
    for _ in range(cases):
        r = run_case(rng)
        stats["gate_pass" if r["gate"] else "gate_fail"] += 1
        stats["global_added"] += r["appended_global"]
        if r["gate"]:
            if r["v"] != r["v"]:
                stats["nan_v"] = stats.get("nan_v", 0) + 1
            stats["in_range" if r["appended_in"] else "out_of_range"] += 1
        if r["boundary"]:
            stats["boundary_tie_listed" if r["appended_in"] else "boundary_tie_missed"] = \
                stats.get("boundary_tie_listed" if r["appended_in"] else "boundary_tie_missed", 0) + 1
        if r["flags"] & 0x10:
            stats["flag_0x10_substituted"] += 1
        lines.append("%x %d %d %d %d %d %d %r %r" % (r["flags"], r["drain"], r["at_max"], r["shared"],
                                                     r["global_on"], r["appended_global"], r["appended_in"],
                                                     r["d"], r["v"]))
    with open(out_path, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("cases: %d -> %s" % (len(lines), out_path))
    print("counts: " + " ".join("%s=%d" % kv for kv in sorted(stats.items())))
    print("hook calls: " + " ".join("%s=%d" % kv for kv in sorted(_counts.items()) if kv[1]))


if __name__ == "__main__":
    main()
