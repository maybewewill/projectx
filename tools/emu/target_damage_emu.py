"""Unicorn check of the target-side damage routine CUnit::Damage (Game.dll 1.26a, VA 0x6F2A40D0).

What is run: the original routine, with its real code for the armor factor (0x6F26D270), the damage-type
lookup (0x6F26D300), the softfloat helpers, the status test (0x6F26DE50) and the record constructor, and
with the parts that are not part of the amount replaced by stubs (see below). It writes one line per random case:
all inputs and the output float, so that the Rust model (crates/sim/src/damage.rs, `target_side_amount`) can be
compared with the original bit for bit.

Replaced by stubs (they touch no amount; their effects are side effects on unit state):
- the ability lists of the target (masks at +0x1DC/+0x1E0 and +0x250/+0x254 are set to -1, so no list is walked:
  the attack-handler passes 1 and 2 are NOT covered here),
- 0x6F27BA70, 0x6F2A3DC0, 0x6F2ADA80, 0x6F3328E0, 0x6F0419C0, 0x6F29B730 (no-ops),
- the target's virtual 0x11C (invulnerability, a stub returning the case's value) and 0x128 (the HP change: the
  stub returns the value it was given, so the output is the amount that reached the HP change),
- Misc keys: `DefenseArmor` (per case), `MagicImmunesResistDamage` and `EtherealDamageBonusAlly` (per case),
- the owner test 0x6F285B00 (per case), the player object 0x6F2834E0 (+0x268 per case), the object 13 returned
  by 0x6F4C34D0 (per case; the relation branch is on or off; its meaning is UNSURE).

Per case (random): the record (source present or not, flags, raw amount, class word, category), the target (flags
at +0x5C and +0x20, armor and armor type, counters at +0x158 and +0x15C), the 7x8 damage-type table, the 7
ethereal slots, the 21-entry negative-armor table (the original's values, see neg_armor_vectors.py), the
DefenseArmor value, the relation constant, and the gate/ethereal rule flags. The two real masks of the
class tests (0x6FA6F2CC, 0x6FA6F2D0) and the real relation constant (0x6FAAE574 = 100.0) are the values the
start-up code leaves (runtime_consts.py).

Run (outside the repository for the output; the output holds numbers from the user's Game.dll):
    GAME_DLL=/path/to/Game.dll PYTHONPATH=<unicorn> python3 tools/emu/target_damage_emu.py \
        <neg_armor.tsv> <out.vec> [cases]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402  (maps Game.dll, provides call())
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_EAX,
    UC_X86_REG_ECX,
    UC_X86_REG_EDX,
    UC_X86_REG_EIP,
    UC_X86_REG_ESP,
)

TARGET = 0x6F2A40D0
ARMOR_BUILDER_FLAG = 0x6FA70E90
TABLE = 0x6FAB4358  # 7 rows x 8 columns, row-major, 0x20 bytes per row
ETHEREAL = 0x6FAB42E4  # slot k at ETHEREAL + 4k (k = attack row)
NEG_TABLE = 0x6FAB4300  # 21 entries
ONE = 0x6FAAE4F4
ZERO = 0x6FAAE470
REL_CONST = 0x6FAAE574
MASK_A = 0x6FA6F2CC
MASK_B = 0x6FA6F2D0

# Values the start-up code leaves (tools/emu/runtime_consts.py)
REL_CONST_BITS = 0x42C80000  # 100.0
MASK_A_BITS = 0x033EE700
MASK_B_BITS = 0x00C11830

# Hooked addresses
READER_MISC = 0x6F009EE0  # Misc value reader (section, key, element); ret 8
READER_BOOL = 0x6F009F90  # Misc boolean reader (ECX section, EDX key, element); ret 4
OWNER_TEST = 0x6F285B00  # ECX source, stack target; ret 4
PLAYER_OF = 0x6F2834E0  # ECX unit; returns the player object
RELATION_SETTING = 0x6F4C34D0  # ECX 0x0D; returns a block with a flag pointer
ABILITY_NEXT = 0x6F4786B0  # ability list walker: must never be reached here
NOOPS = {  # address: stack bytes popped
    0x6F27BA70: 8,
    0x6F2A3DC0: 8,
    0x6F2ADA80: 4,
    0x6F3328E0: 0,
    0x6F0419C0: 4,
    0x6F29B730: 4,
}

# Scratch layout inside the emulator's data page
DATA = emu.DATA
T = DATA + 0x1000  # target unit
VT = DATA + 0x2000  # target vtable (slots 0x11C, 0x128)
STUB_INV = DATA + 0x2100
STUB_HP = DATA + 0x2200
SRC = DATA + 0x1400  # source unit
PS = DATA + 0x1800  # player block of the source (+0x268)
PT = DATA + 0x1900  # player block of the target (+0x268)
RELX = DATA + 0x1A00
RELP = DATA + 0x1B00
RELQ = DATA + 0x1C00
REC = DATA + 0x4000  # damage record
OUT = DATA + 0x4100
HPBUF = DATA + 0x4200

CFG = {}
HP_SEEN = {}
CALLS = {"ability_walk": 0}


def rd32(a):
    return struct.unpack("<I", bytes(emu.mu.mem_read(a, 4)))[0]


def wr32(a, v):
    emu.mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def cstr(a):
    out = b""
    while True:
        c = bytes(emu.mu.mem_read(a + len(out), 1))
        if c == b"\0" or len(out) > 200:
            break
        out += c
    return out.decode("latin-1")


def f32_bits(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def bits_f32(b):
    return struct.unpack("<f", struct.pack("<I", b & 0xFFFFFFFF))[0]


def add_hook(addr, pop, handler):
    """Runs `handler(uc, args)` in place of the code at `addr` and returns to the caller.

    `pop` is the number of argument bytes the original routine removes on return. The handler's return value,
    when not None, is put in EAX."""

    def cb(uc, address, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = rd32(esp)
        args = [rd32(esp + 4 + 4 * i) for i in range(pop // 4)]
        value = handler(uc, args)
        if value is not None:
            uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
        uc.reg_write(UC_X86_REG_EIP, ret)

    emu.mu.hook_add(UC_HOOK_CODE, cb, begin=addr, end=addr)


def misc_reader(uc, args):
    """Misc value reader: ECX = output, stack = key, element. Writes an f32 and returns the output pointer."""
    out = uc.reg_read(UC_X86_REG_ECX)
    key = cstr(args[0])
    index = args[1]
    values = CFG["misc"].get(key)
    if values is None or index >= len(values):
        bits = f32_bits(0.0)  # the reader's default (DAT_6faae470 = 0.0)
    else:
        bits = values[index]
    wr32(out, bits)
    return out


def misc_bool(uc, args):
    key = cstr(uc.reg_read(UC_X86_REG_EDX))
    if key == "MagicImmunesResistDamage":
        return int(CFG["gate_rule"])
    if key == "EtherealDamageBonusAlly":
        return int(CFG["eth_ally_rule"])
    return 0


def owner_test(uc, args):
    return int(CFG["owner_test"])


def player_of(uc, args):
    unit = uc.reg_read(UC_X86_REG_ECX)
    return PS if unit == SRC else PT


def relation_setting(uc, args):
    return RELX


def ability_walk(uc, args):
    CALLS["ability_walk"] += 1
    raise RuntimeError("ability list walked: the masks should have skipped it")


def invulnerable(uc, args):
    return int(CFG["invuln"])


def hp_change(uc, args):
    # Two stack arguments: [a1] = pointer to the first real, [a2] = pointer to the amount sent as -V.
    sent = rd32(args[1])
    HP_SEEN["bits"] = sent
    wr32(HPBUF, sent)
    return HPBUF


def install_hooks():
    add_hook(READER_MISC, 8, misc_reader)
    add_hook(READER_BOOL, 4, misc_bool)
    add_hook(OWNER_TEST, 4, owner_test)
    add_hook(PLAYER_OF, 0, player_of)
    add_hook(RELATION_SETTING, 0, relation_setting)
    add_hook(ABILITY_NEXT, 0, ability_walk)
    for addr, pop in NOOPS.items():
        add_hook(addr, pop, lambda uc, args: None)
    add_hook(STUB_INV, 0, invulnerable)
    add_hook(STUB_HP, 8, hp_change)


def setup_static():
    emu.mu.mem_write(ONE, struct.pack("<I", f32_bits(1.0)))
    emu.mu.mem_write(ZERO, struct.pack("<I", 0))
    wr32(REL_CONST, REL_CONST_BITS)
    wr32(MASK_A, MASK_A_BITS)
    wr32(MASK_B, MASK_B_BITS)
    wr32(ARMOR_BUILDER_FLAG, 0)  # the table is set by the case, the builder must not run
    wr32(VT + 0x11C, STUB_INV)
    wr32(VT + 0x128, STUB_HP)
    wr32(T, VT)


def load_neg_table(path):
    bits = []
    with open(path) as fh:
        for line in fh:
            va, b = line.split()[:2]
            if int(va, 16) < NEG_TABLE or int(va, 16) >= NEG_TABLE + 4 * 21:
                continue
            bits.append(int(b, 16))
    assert len(bits) == 21, len(bits)
    return bits


def random_case(rng, neg_bits):
    case = {}
    raw_int = rng.randrange(0, 61)
    case["raw"] = f32_bits(float(raw_int) + (rng.random() * 0.5 if rng.random() < 0.2 else 0.0))
    case["class"] = rng.choice([0x10, 0x10, 0x10, 0x20, 0x400000, 0x4000000, 0x2, 0x8000, 0x10000000])
    case["category"] = rng.choice([0, 1, 2, 3, 4, 5, 6, 0, 4])
    case["src"] = 1 if rng.random() < 0.85 else 0
    flags = 0
    for bit in (0x2, 0x8, 0x20, 0x40, 0x80, 0x100, 0x200):
        if rng.random() < 0.15:
            flags |= bit
    if rng.random() < 0.03:
        flags |= 0x10
    if rng.random() < 0.03:
        flags |= 0x4
    case["rflags"] = flags
    tflags = 0
    for bit in (0x10000, 0x2000000, 0x80000000, 0x8000000, 0x4000, 0x2000, 0x20000, 0x40):
        if rng.random() < 0.5:
            tflags |= bit
    if rng.random() < 0.05:
        tflags |= 0x100
    case["tflags"] = tflags
    case["tflags20"] = 0x8 if rng.random() < 0.3 else 0
    if rng.random() < 0.3:
        case["tflags20"] |= 0x20
    case["armor_value"] = rng.choice(
        [float(rng.randrange(-25, 26)), rng.uniform(-30.0, 30.0), float(rng.randrange(0, 40))]
    )
    case["armor_type"] = rng.randrange(0, 8)
    case["cnt158"] = rng.choice([0, 0, 1, 2, -1])
    case["cnt15c"] = rng.choice([0, 0, 1])
    case["invuln"] = 1 if rng.random() < 0.05 else 0
    case["gate_rule"] = 1 if rng.random() < 0.2 else 0
    case["eth_ally_rule"] = 1 if rng.random() < 0.3 else 0
    case["owner_test"] = 1 if rng.random() < 0.5 else 0
    case["rel_on"] = 1 if rng.random() < 0.4 else 0
    case["src268"] = 1 if rng.random() < 0.5 else 0
    case["tgt268"] = 1 if rng.random() < 0.5 else 0
    case["k_defense"] = f32_bits(rng.choice([0.0, 0.03, 0.06, 0.1, 0.25, 0.5]))
    case["table"] = []
    for _ in range(7 * 8):
        if rng.random() < 0.3:
            case["table"].append(f32_bits(1.0))
        else:
            case["table"].append(f32_bits(rng.uniform(0.2, 1.8)))
    case["eth"] = []
    for _ in range(7):
        if rng.random() < 0.4:
            case["eth"].append(f32_bits(1.0))
        else:
            case["eth"].append(f32_bits(rng.uniform(0.5, 2.0)))
    case["param4"] = f32_bits(rng.uniform(-5.0, 5.0)) if rng.random() < 0.5 else 0
    case["param5"] = rng.choice([0, 1])
    case["neg"] = neg_bits
    return case


def apply_case(case):
    CFG["misc"] = {
        "DefenseArmor": [case["k_defense"]],
    }
    CFG["gate_rule"] = case["gate_rule"]
    CFG["eth_ally_rule"] = case["eth_ally_rule"]
    CFG["owner_test"] = case["owner_test"]
    CFG["invuln"] = case["invuln"]
    wr32(T + 0x5C, case["tflags"])
    wr32(T + 0x20, case["tflags20"])
    wr32(T + 0xE0, f32_bits(case["armor_value"]))
    wr32(T + 0xE4, case["armor_type"])
    wr32(T + 0x158, case["cnt158"] & 0xFFFFFFFF)
    wr32(T + 0x15C, case["cnt15c"] & 0xFFFFFFFF)
    for off in (0x1DC, 0x1E0, 0x250, 0x254):
        wr32(T + off, 0xFFFFFFFF)
    wr32(PS + 0x268, case["src268"])
    wr32(PT + 0x268, case["tgt268"])
    wr32(RELX + 0x10, RELP)
    wr32(RELP, RELQ)
    wr32(RELQ + 4, 2 if case["rel_on"] else 0)
    if not case["rel_on"]:
        wr32(RELP, 0)
    for i, b in enumerate(case["neg"]):
        wr32(NEG_TABLE + 4 * i, b)
    for r in range(7):
        for c in range(8):
            wr32(TABLE + 0x20 * r + 4 * c, case["table"][r * 8 + c])
    for k in range(7):
        wr32(ETHEREAL + 4 * k, case["eth"][k])
    wr32(SRC, 0)
    wr32(REC + 0x00, SRC if case["src"] else 0)
    wr32(REC + 0x04, 0)
    wr32(REC + 0x0C, case["rflags"])
    wr32(REC + 0x10, case["raw"])
    wr32(REC + 0x14, case["class"])
    wr32(REC + 0x18, 0)
    wr32(REC + 0x1C, 0)
    wr32(REC + 0x20, case["category"])
    wr32(OUT, 0xDEADBEEF)


def run_case(case):
    apply_case(case)
    HP_SEEN.clear()
    emu.call(TARGET, ecx=T, stack_args=(OUT, REC, case["param4"], case["param5"]))
    return rd32(OUT), HP_SEEN.get("bits")


FIELDS = [
    "raw", "class", "category", "src", "rflags", "tflags", "tflags20", "armor_value", "armor_type",
    "cnt158", "cnt15c", "invuln", "gate_rule", "eth_ally_rule", "owner_test", "rel_on", "src268", "tgt268",
    "k_defense", "param4", "param5",
]


def encode(case, out_bits, hp_bits):
    parts = []
    for name in FIELDS:
        v = f32_bits(case[name]) if name == "armor_value" else case[name]
        parts.append("%08x" % (v & 0xFFFFFFFF))
    parts += ["%08x" % v for v in case["table"]]
    parts += ["%08x" % v for v in case["eth"]]
    parts += ["%08x" % v for v in case["neg"]]
    parts.append("%08x" % out_bits)
    parts.append("%08x" % (hp_bits if hp_bits is not None else 0xFFFFFFFF))
    return " ".join(parts)


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    neg_bits = load_neg_table(sys.argv[1])
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 2000
    seed = int(os.environ.get("TARGET_SEED", "1"))
    rng = random.Random(seed)
    install_hooks()
    setup_static()
    with open(sys.argv[2], "w") as fh:
        fh.write("# fields: " + " ".join(FIELDS) + " table[56] eth[7] neg[21] out hp\n")
        fh.write("# neg " + " ".join("%08x" % b for b in neg_bits) + "\n")
        for _ in range(n):
            case = random_case(rng, neg_bits)
            out_bits, hp = run_case(case)
            fh.write(encode(case, out_bits, hp) + "\n")
    print(f"cases: {n}, ability walks: {CALLS['ability_walk']} (must be 0)")


if __name__ == "__main__":
    main()
