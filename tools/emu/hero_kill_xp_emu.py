"""Unicorn check of the kill-experience split of Game.dll 1.26a (docs/spec/heroes.md, section 8).

The real code of these entries runs on synthetic heroes, units and players:

  A  XP engine 0x6F20B080 (hero, value*, show, p4, handicap, level-walk, range): plain and kill branches.
  B  amount 0x6F2A0230 (victim -> K) and the per-candidate loop 0x6F2A3190 (split F = K / N, then the wrapper
     0x6F26EB70 and the XP engine for every candidate of the chosen list).

Below these entries everything runs as in the binary (softfloat, the hero and unit fields). Only the callees in
`install()` answer from the synthetic configuration: the Misc readers, the level-need and factor tables, the
player accessors, the upgrade-based bonus lookups, the ability lookup of the victim, and side effects (level-up
effects, refreshes) that carry no numbers. The vector lines hold the inputs and the outputs only; no Game.dll data
goes into them. They are compared by crates/sim/tests/kill_xp_vectors.rs.

Run:  GAME_DLL=/path/to/Game.dll python3 tools/emu/hero_kill_xp_emu.py OUT.txt MODE [CASES] [SEED]
      MODE is A (engine) or B (kill split).
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402  (maps Game.dll; call(); mu; DATA)
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EAX  # noqa: E402
from capstone import Cs, CS_ARCH_X86, CS_MODE_32  # noqa: E402

mu = emu.mu
DATA = emu.DATA

# ---- Game.dll 1.26a addresses ---------------------------------------------------------------------------------------
XP_ENGINE = 0x6F20B080     # thiscall(hero); stack: value*, show, p4, handicap, levelwalk, range; ret 0x18
PER_CAND = 0x6F2A3190      # thiscall(record); stack: victim; ret 4
AMOUNT = 0x6F2A0230        # thiscall(victim) -> K (int), plain ret
UNIT_LEVEL = 0x6F29DCA0    # level of a non-hero unit (tail-called by 0x6F2A0210)
OWNER_VF = 0x6F28B1D0      # CUnit::vf59 = return [ecx+0x58] (real code, reached through the vtable)
COUNTER_GET = 0x6F473170   # return [ecx] in this harness (the hero level counter is at hero+0x6C)
COUNTER_SET = 0x6F473140   # [ecx] = arg
MISC_LIST = 0x6F009F90     # ECX Misc, EDX key, arg index; ret 4
MISC_REAL = 0x6F009EE0     # ECX out, arg key, arg index; ret 8
GRANT_HERO = 0x6F00B100    # ECX index -> GrantHeroXP
GRANT_UNIT = 0x6F00B120    # ECX index -> GrantNormalXP
FACTOR = 0x6F00B150        # ECX out, EDX index -> factor (f32)
NEED = 0x6F207760          # arg level -> need XP (clamped)
PLAYER = 0x6F2834E0        # ECX unit -> owner player object
PLAYER_B1 = 0x6F40B340     # ECX player; args (tag, 11) -> B1
SLOT_EXISTS = 0x6F40B380   # args (W, index, 0, 0) -> nonzero when slot exists
LEVELS = 0x6F32E020        # ECX code -> levels
VALUE = 0x6F32E040         # ECX out, EDX code, arg index -> value (f32)
CAP_MIN = 0x6F207C80       # ECX hero, arg handle -> min
CAP_MAX = 0x6F207CC0       # ECX hero, arg handle -> max
ABILITY = 0x6F0787D0       # ECX unit, args (code, 1, 0, 1, 1) -> ability or 0
CODE = 0x6F113CC0          # returns the rawcode of the amount lookup
ABIL_EFFECT_CODE = 0x414F6D69  # the effect lookup of the XP engine (no numeric effect)
KEY_MAX_HERO = 0x6F876208
KEY_MAX_UNIT = 0x6F876218
KEY_SUMMON = 0x6F92F288
CONST_ONE = 0x6FAAE4F4
CONST_HALF = 0x6FAAE4D0
CONST_ZERO = 0x6FAAE470
CODE_TABLE = 0x6FA6F044    # nine rawcodes (the upgrades that scale the kill experience)
SIDE_EFFECTS = [(0x6F472890, 0), (0x6F296E50, 0), (0x6F332810, 0), (0x6F26FE50, 4), (0x6F26FE20, 0),
                (0x6F20A350, 0), (0x6F021270, 0), (0x6F052FB0, 0), (0x6F2DE140, 0), (0x6F332ED0, 0),
                (0x6F332700, 0), (0x6F11B020, 4)]

# ---- synthetic layout (inside the mapped DATA region) ----------------------------------------------------------------
VBUF = DATA + 0x000
HERO0 = DATA + 0x1000      # 0x200 bytes per hero, up to 8
UNIT0 = DATA + 0x2400      # 0x200 bytes per unit, up to 8
DYING = DATA + 0x3800      # the victim unit
VHERO = DATA + 0x3C00      # the victim's hero object (when it has one; the victim block is 0x400 bytes)
PLAYER0 = DATA + 0x4000    # 0x400 bytes per owner index, 16 owners
RECORD = DATA + 0x8000
LIST_IN = DATA + 0x8400
LIST_GL = DATA + 0x8500
VT_UNIT = DATA + 0x9000
VT_ABIL = DATA + 0x9400
ABIL_OBJ = DATA + 0x9800
STUB = 0x00380000
STUB_SIZE = 0x4000
ABIL_219 = STUB + 0x3000   # the ability's vtable slot 0x36C (flag)

# hero fields (docs/spec/heroes.md)
H_UNIT, H_COUNTER, H_XP, H_SP, H_SUSP, H_ABIL = 0x30, 0x6C, 0x8C, 0x90, 0xEC, 0xF4
# unit fields
U_VT, U_W, U_TYPE, U_OWNER, U_FLAGS, U_HERO, U_SUMMON = 0x00, 0x30, 0x33, 0x58, 0x5C, 0x1F0, 0x248
P_HANDICAP = 0x2A4
R_CNT_IN, R_ARR_IN, R_CNT_GL, R_ARR_GL = 0x14, 0x18, 0xE0, 0xE4


def rd32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def f2b(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def cstr(p):
    return bytes(mu.mem_read(p, 40)).split(b"\0")[0].decode()


_md = Cs(CS_ARCH_X86, CS_MODE_32)


def detect_pop(addr):
    """Bytes popped by the last `ret` before the int3 padding of a plain function (None when there is none)."""
    code = bytes(mu.mem_read(addr, 0x400))
    last = None
    seen = False
    for ins in _md.disasm(code, addr):
        if ins.mnemonic == "int3" and seen:
            break
        if ins.mnemonic == "ret":
            seen = True
            last = int(ins.op_str, 16) if ins.op_str else 0
    return last


HOOKS = {}
COUNTS = {}
POPS = {}


def hook(name, target, pop, handler):
    """Hook `target`. `pop` is the argument bytes the callee pops (checked against the last `ret` of the code)."""
    found = detect_pop(target)
    if pop is None:
        pop = found
        if pop is None:
            raise RuntimeError("no ret found for %s at %#x" % (name, target))
    elif found is not None and found != pop:
        print("note: %s at %#x: declared pop %d, last ret pops %d" % (name, target, pop, found))
    POPS[name] = pop
    HOOKS[target] = (name, pop, handler)
    COUNTS[name] = 0
    mu.hook_add(UC_HOOK_CODE, _on_code, begin=target, end=target)


def _on_code(uc, addr, size, user):
    name, pop, handler = HOOKS[addr]
    COUNTS[name] += 1
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    value = handler(uc, esp)
    if value is not None:
        uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
    uc.reg_write(UC_X86_REG_EIP, ret)


def arg(esp, i):
    return rd32(esp + 4 + 4 * i)


def signed(v):
    return v - 0x100000000 if v & 0x80000000 else v


class World:
    def __init__(self):
        self.max_hero = 1
        self.max_unit = 1
        self.need = []
        self.factor = []
        self.grant_hero = []
        self.grant_unit = []
        self.summon = 1.0
        self.b1 = {}
        self.handicap = {}
        self.slots = 0
        self.levels = {}
        self.values = {}
        self.caps = {}
        self.victim_level = 1
        self.ability = None    # None or (flag, L) for the victim's ability found by 0x6F0787D0


W = World()


def h_misc_list(uc, esp):
    key = cstr(uc.reg_read(UC_X86_REG_EDX))
    if arg(esp, 0) != 0:
        return 0
    if key == "MaxHeroLevel":
        return W.max_hero
    if key == "MaxUnitLevel":
        return W.max_unit
    return 0


def h_misc_real(uc, esp):
    out = uc.reg_read(UC_X86_REG_ECX)
    key = cstr(arg(esp, 0))
    wr32(out, f2b(W.summon) if key == "SummonedKillFactor" else 0)
    return out


def h_counter_get(uc, esp):
    return rd32(uc.reg_read(UC_X86_REG_ECX))


def h_counter_set(uc, esp):
    wr32(uc.reg_read(UC_X86_REG_ECX), arg(esp, 0))
    return None


def h_grant_hero(uc, esp):
    i = uc.reg_read(UC_X86_REG_ECX)
    return W.grant_hero[i] if i < len(W.grant_hero) else 0


def h_grant_unit(uc, esp):
    i = uc.reg_read(UC_X86_REG_ECX)
    return W.grant_unit[i] if i < len(W.grant_unit) else 0


def h_factor(uc, esp):
    out = uc.reg_read(UC_X86_REG_ECX)
    wr32(out, W.factor[uc.reg_read(UC_X86_REG_EDX)])
    return out


def h_need(uc, esp):
    level = signed(arg(esp, 0))
    n = W.max_hero
    if not W.need:
        return 0
    lv = min(max(level, 1), n if n >= 1 else 1)
    return W.need[lv - 1]


def h_player(uc, esp):
    unit = uc.reg_read(UC_X86_REG_ECX)
    return PLAYER0 + 0x400 * rd32(unit + U_OWNER)


def h_b1(uc, esp):
    return W.b1.get((uc.reg_read(UC_X86_REG_ECX) - PLAYER0) // 0x400, 0)


def h_slot(uc, esp):
    return 1 if arg(esp, 1) < W.slots else 0


def h_levels(uc, esp):
    return W.levels.get(uc.reg_read(UC_X86_REG_ECX), 0)


def h_value(uc, esp):
    out = uc.reg_read(UC_X86_REG_ECX)
    code = uc.reg_read(UC_X86_REG_EDX)
    k = W.levels.get(code, 0)
    if k == 0:
        wr32(out, f2b(0.0))
    else:
        wr32(out, W.values[code][min(arg(esp, 0), k - 1)])
    return out


def h_cap_min(uc, esp):
    return W.caps.get(arg(esp, 0), (0, 0))[0]


def h_cap_max(uc, esp):
    return W.caps.get(arg(esp, 0), (0, 0))[1]


def h_code(uc, esp):
    return 0x4142434D


def h_ability(uc, esp):
    if arg(esp, 0) == ABIL_EFFECT_CODE or W.ability is None:
        return 0
    return ABIL_OBJ


def h_abil_219(uc, esp):
    return 1 if (W.ability is not None and W.ability[0]) else 0


def h_unit_level(uc, esp):
    return W.victim_level


def h_noop(uc, esp):
    return None


def install():
    mu.mem_map(STUB, STUB_SIZE)
    mu.mem_write(STUB, b"\xcc" * STUB_SIZE)
    mu.mem_write(CONST_ONE, struct.pack("<I", f2b(1.0)))
    mu.mem_write(CONST_HALF, struct.pack("<I", f2b(0.5)))
    mu.mem_write(CONST_ZERO, struct.pack("<I", 0))
    # argument bytes popped by each callee (ret imm16 of the original; checked by hook() against its code)
    hook("counter_get", COUNTER_GET, 0, h_counter_get)
    hook("counter_set", COUNTER_SET, 4, h_counter_set)
    hook("misc_list", MISC_LIST, 4, h_misc_list)
    hook("misc_real", MISC_REAL, 8, h_misc_real)
    hook("grant_hero", GRANT_HERO, 0, h_grant_hero)
    hook("grant_unit", GRANT_UNIT, 0, h_grant_unit)
    hook("factor", FACTOR, 0, h_factor)
    hook("need", NEED, 4, h_need)
    hook("player", PLAYER, 0, h_player)
    hook("player_b1", PLAYER_B1, 8, h_b1)
    hook("slot_exists", SLOT_EXISTS, 0x10, h_slot)
    hook("levels", LEVELS, 0, h_levels)
    hook("value", VALUE, 4, h_value)
    hook("cap_min", CAP_MIN, 4, h_cap_min)
    hook("cap_max", CAP_MAX, 4, h_cap_max)
    hook("ability", ABILITY, 0x14, h_ability)
    hook("rawcode", CODE, 0, h_code)
    hook("unit_level", UNIT_LEVEL, 0, h_unit_level)
    hook("abil_219", ABIL_219, 0, h_abil_219)
    # Pops from the argument count of each call site in the XP engine body (callee cleans, MSVC convention).
    for addr, pop in SIDE_EFFECTS:
        hook("fx_%x" % addr, addr, pop, h_noop)
    # The owner is read through the unit vtable, slot 0xEC (CUnit::vf59, real code).
    wr32(VT_UNIT + 0xEC, OWNER_VF)
    wr32(VT_ABIL + 0x36C, ABIL_219)


def write_unit(addr, owner, flags, typ, hero):
    mu.mem_write(addr, b"\0" * 0x200)
    wr32(addr + U_VT, VT_UNIT)
    wr32(addr + U_W, addr + 0x300)
    mu.mem_write(addr + U_TYPE, bytes([typ]))
    wr32(addr + U_OWNER, owner)
    wr32(addr + U_FLAGS, flags)
    wr32(addr + U_HERO, hero)


def write_hero(addr, unit, counter, xp, sp, susp):
    mu.mem_write(addr, b"\0" * 0x200)
    wr32(addr + H_UNIT, unit)
    wr32(addr + H_COUNTER, counter)
    wr32(addr + H_XP, xp)
    wr32(addr + H_SP, sp)
    wr32(addr + H_SUSP, susp)
    for i in range(5):
        wr32(addr + H_ABIL + 4 * i, 0x1000 + i)


def write_world_players(rng):
    for p in range(16):
        addr = PLAYER0 + 0x400 * p
        mu.mem_write(addr, b"\0" * 0x400)
        wr32(addr + P_HANDICAP, W.handicap[p])  # already f32 bits (see random_world)
    # the upgrade-based bonus (6F40B340 answers from W.b1 keyed by the owner index)


def write_abilities():
    mu.mem_write(ABIL_OBJ, b"\0" * 0x200)
    wr32(ABIL_OBJ, VT_ABIL)
    if W.ability is not None:
        wr32(ABIL_OBJ + 0xE4, W.ability[1])


def write_codes(codes):
    for i, c in enumerate(codes):
        wr32(CODE_TABLE + 4 * i, c)


def fl(bits):
    return "%08x" % bits


def pick_float(rng):
    return rng.choice([0.0, 0.1, 0.25, 0.5, 1.0, 1.25, 1.5, 2.0, 0.333, 12.5, 40.0, 100.0, 250.5, 777.7,
                       rng.uniform(0.0, 300.0), rng.uniform(-30.0, 30.0)])


def random_world(rng):
    W.max_hero = rng.randint(1, 10)
    W.max_unit = rng.randint(1, 12)
    W.need = [rng.randint(0, 3000) for _ in range(W.max_hero)]
    W.factor = [f2b(rng.choice([0.0, 0.5, 1.0, 1.25, 0.8, 2.0, rng.uniform(0.0, 3.0)])) for _ in range(W.max_hero)]
    W.grant_hero = [rng.randint(0, 900) for _ in range(W.max_hero)]
    W.grant_unit = [rng.randint(0, 60) for _ in range(W.max_unit)]
    W.summon = rng.choice([0.5, 1.0, 1.5, 0.0, rng.uniform(0.0, 2.0)])
    W.handicap = {p: f2b(rng.choice([0.0, 0.5, 1.0, 1.5, 2.0, rng.uniform(0.0, 3.0)])) for p in range(16)}
    W.b1 = {p: rng.randint(0, 3) for p in range(16)}
    W.slots = rng.randint(0, 9)
    codes = [rng.choice([0, 0x54574E31 + i, 0x54574E31 + i]) for i in range(9)]
    W.levels = {}
    W.values = {}
    for c in codes:
        if c and c not in W.levels:
            k = rng.randint(0, 4)
            W.levels[c] = k
            W.values[c] = [f2b(rng.choice([0.0, 0.5, 1.0, 0.25, 2.0, rng.uniform(-0.5, 2.0)])) for _ in range(k)]
    write_codes(codes)
    W.caps = {}
    for i in range(5):
        lo = rng.randint(0, 3)
        W.caps[0x1000 + i] = (lo, rng.randint(max(lo - 1, 0), 6))
    W.victim_level = rng.randint(1, 12)
    W.ability = rng.choice([None, None, (0, rng.randint(0, 8)), (1, rng.randint(0, 8))])
    write_abilities()
    write_world_players(rng)
    return codes


def run_engine_case(rng, n):
    """Mode A: the XP engine on one hero. Returns the vector line."""
    random_world(rng)
    hero = HERO0
    unit = UNIT0
    owner = rng.randint(0, 15)
    write_unit(unit, owner, 0, ord("H"), hero)
    counter = rng.choice([rng.randint(0, W.max_hero - 1), rng.randint(0, W.max_hero), 0])
    xp = rng.randint(0, 3000)
    sp = rng.randint(0, 5)
    susp = 1 if rng.random() < 0.05 else 0
    write_hero(hero, unit, counter, xp & 0xFFFFFFFF, sp, susp)
    v = pick_float(rng)
    v_bits = f2b(v)
    wr32(VBUF, v_bits)
    flags = [rng.randint(0, 1) for _ in range(5)]  # p3, p4, p5 (handicap), p6 (walk), p7 (range)
    p3, p4, p5, p6, p7 = flags
    emu.call(XP_ENGINE, ecx=hero, stack_args=(VBUF, p3, p4, p5, p6, p7))
    out = (rd32(hero + H_XP), rd32(hero + H_SP), rd32(hero + H_COUNTER))
    return (
        "A V=%08x P3=%d P4=%d P5=%d P6=%d P7=%d OWNER=%d M=%d C=%d XP=%d SP=%d SUSP=%d "
        "NEED=%s FACT=%s HCAP=%s B1=%d SLOTS=%d CODES=%s LEV=%s VALS=%s CAPS=%s ABIL=%s "
        "OUT_XP=%d OUT_SP=%d OUT_C=%d"
        % (v_bits, p3, p4, p5, p6, p7, owner, W.max_hero, counter, xp, sp, susp,
           ",".join(str(x) for x in W.need), ",".join(fl(x) for x in W.factor),
           fl(W.handicap[owner]), W.b1[owner], W.slots, ",".join(str(c) for c in current_codes()),
           levels_str(), values_str(), caps_str(), ability_str(),
           out[0], out[1], out[2]))


def current_codes():
    return [rd32(CODE_TABLE + 4 * i) for i in range(9)]


def levels_str():
    return ",".join("%x:%d" % (c, k) for c, k in sorted(W.levels.items())) or "-"


def values_str():
    parts = []
    for c in sorted(W.values):
        parts.append("%x:%s" % (c, ",".join(fl(x) for x in W.values[c]) or "-"))
    return ";".join(parts) or "-"


def caps_str():
    return ",".join("%x:%d:%d" % (h, lo, hi) for h, (lo, hi) in sorted(W.caps.items())) or "-"


def ability_str():
    if W.ability is None:
        return "-"
    return "%d:%d" % W.ability


def run_split_case(rng, n):
    """Mode B: amount, split and the candidate engine calls. Returns the vector line."""
    random_world(rng)
    # victim
    hero_type = rng.random() < 0.35
    typ = ord("H") if hero_type else ord("h")
    victim_owner = rng.randint(0, 15)
    victim_flags = 0x40000000 if rng.random() < 0.1 else 0
    summoned = 1 if rng.random() < 0.2 else 0
    write_unit(DYING, victim_owner, victim_flags, typ, VHERO if hero_type else 0)
    wr32(DYING + U_SUMMON, 4 if summoned else 0)
    if hero_type:
        write_hero(VHERO, DYING, rng.randint(0, W.max_hero - 1), 0, 0, 0)
    amount = signed(run_call_amount())
    # candidates
    ncand = rng.randint(1, 6)
    cands = []
    for i in range(ncand):
        unit = UNIT0 + 0x200 * i
        hero = HERO0 + 0x200 * i
        owner = rng.randint(0, 15)
        write_unit(unit, owner, 0, ord("H"), hero)
        write_hero(hero, unit, rng.randint(0, W.max_hero), rng.randint(0, 3000), rng.randint(0, 4),
                   1 if rng.random() < 0.08 else 0)
        cands.append({"unit": unit, "hero": hero, "owner": owner, "in": rng.random() < 0.5,
                      "elig": rng.random() < 0.85})
    gl_flag = rng.randint(0, 1)
    rng.shuffle(cands)
    lst_in = [c for c in cands if c["elig"] and c["in"]]
    lst_gl = [c for c in cands if c["elig"]] if gl_flag else []
    for i, c in enumerate(lst_in):
        wr32(LIST_IN + 4 * i, c["unit"])
    for i, c in enumerate(lst_gl):
        wr32(LIST_GL + 4 * i, c["unit"])
    wr32(RECORD + R_CNT_IN, len(lst_in))
    wr32(RECORD + R_ARR_IN, LIST_IN)
    wr32(RECORD + R_CNT_GL, len(lst_gl))
    wr32(RECORD + R_ARR_GL, LIST_GL)
    before = [(rd32(c["hero"] + H_XP), rd32(c["hero"] + H_SP), rd32(c["hero"] + H_COUNTER)) for c in cands]
    emu.call(PER_CAND, ecx=RECORD, stack_args=(DYING,))
    after = [(rd32(c["hero"] + H_XP), rd32(c["hero"] + H_SP), rd32(c["hero"] + H_COUNTER)) for c in cands]
    cand_txt = []
    for c, b, a in zip(cands, before, after):
        cand_txt.append("%d.%d.%d.%d.%d.%d.%d.%d.%d.%d.%d" % (
            c["owner"], int(c["in"]), int(c["elig"]), b[0], b[1], b[2], int(rd32(c["hero"] + H_SUSP) != 0),
            a[0], a[1], a[2], 0))
    return (
        "B K=%d VTYPE=%d VOWN=%d VFLAGS=%d VSUM=%d VHERO=%d VCOUNT=%d VLEV=%d GL=%d ABIL=%s M=%d MU=%d "
        "NEED=%s FACT=%s GH=%s GU=%s SUMFACT=%s B1=%s HCAP=%s SLOTS=%d CODES=%s LEV=%s VALS=%s CAPS=%s CAND=%s"
        % (amount, typ, victim_owner, victim_flags, summoned, int(hero_type),
           rd32(VHERO + H_COUNTER) if hero_type else 0, W.victim_level, gl_flag, ability_str(), W.max_hero,
           W.max_unit,
           ",".join(str(x) for x in W.need), ",".join(fl(x) for x in W.factor),
           ",".join(str(x) for x in W.grant_hero), ",".join(str(x) for x in W.grant_unit),
           fl(f2b(W.summon)),
           ",".join("%d" % W.b1[p] for p in range(16)),
           ",".join(fl(W.handicap[p]) for p in range(16)), W.slots,
           ",".join(str(c) for c in current_codes()), levels_str(), values_str(), caps_str(),
           ";".join(cand_txt) or "-"))


def run_call_amount():
    return emu.call(AMOUNT, ecx=DYING)


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        sys.exit(2)
    out_path, mode = sys.argv[1], sys.argv[2].upper()
    cases = int(sys.argv[3]) if len(sys.argv) > 3 else 200
    seed = int(sys.argv[4]) if len(sys.argv) > 4 else 1
    rng = random.Random(seed)
    install()
    lines = []
    for n in range(cases):
        if mode == "A":
            lines.append(run_engine_case(rng, n))
        elif mode == "B":
            lines.append(run_split_case(rng, n))
        else:
            raise SystemExit("unknown mode %s" % mode)
    with open(out_path, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("mode %s: %d cases -> %s" % (mode, len(lines), out_path))
    print("callee pops: " + " ".join("%s=%s" % (k, v) for k, v in sorted(POPS.items())))
    print("hook calls: " + " ".join("%s=%d" % (k, v) for k, v in sorted(COUNTS.items()) if v))


if __name__ == "__main__":
    main()
