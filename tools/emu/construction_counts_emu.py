"""Unicorn check of the unit registration transition of Game.dll 1.26a (docs/spec/construction.md, section 3).

`0x6F29F850` (`__thiscall` on the unit, stack args: flags, old type, new type) moves a unit between the player's
counters and applies its food. The original code runs; the calls it makes are replaced by hooks that log them:
the owner getter (unit vtable slot 59), the player setter `0x6F3A1650`, the counter wrappers
`0x6F40FD30/FD70/FDF0/FE30`, the food getters `0x6F29BB20/BAF0` and the food adders `0x6F03F970` / `0x6F40F5E0`.
The unit's registration word (`unit+0x64`) is the state under test.

The Python model is a transcription of the decompile. A case passes when the logged calls (name and arguments,
in order) and the registration word after the call equal the model's. The food getters read the global "current
type"; the harness sets their values per case (which type the original reads is UNSURE, see the spec).

Usage: GAME_DLL=/path/Game.dll PYTHONPATH=<pylib> python3 -I tools/emu/construction_counts_emu.py [--cases N] [--seed S]
"""
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402  (maps Game.dll and the stack)
from capstone import Cs, CS_ARCH_X86, CS_MODE_32  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
u32 = emu.u32

F_TRANSITION = 0x6F29F850
F_OWNER_SLOT = 59           # unit vtable slot 0xEC
F_SET_PLAYER = 0x6F3A1650
F_WRAP_ADD_COMPLETED = 0x6F40FD30
F_WRAP_REMOVE_INPROG = 0x6F40FD70
F_WRAP_REMOVE_COMPLETED = 0x6F40FDF0
F_WRAP_BEGIN_INPROG = 0x6F40FE30
F_FOOD_GET_MADE = 0x6F29BB20
F_FOOD_GET_USED = 0x6F29BAF0
F_FOOD_ADD_MADE = 0x6F03F970   # __thiscall (this, amount)
F_FOOD_ADD_USED = 0x6F40F5E0   # __thiscall (this, amount)

UNIT = emu.DATA + 0x3000
UNIT_VT = emu.DATA + 0x3800
OWNER_STUB = emu.DATA + 0x3900   # never executed: the hook answers the call
OWNER_INDEX = 3

LOG = []
STATE = {"cur_made": 0, "cur_used": 0}
md = Cs(CS_ARCH_X86, CS_MODE_32)


def stack_arg(uc, n):
    esp = uc.reg_read(UC_X86_REG_ESP)
    return struct.unpack("<i", bytes(uc.mem_read(esp + 4 + 4 * n, 4)))[0]


def cleanup_bytes(addr):
    """Bytes popped by the callee's first `ret` (its stack argument size), by a linear sweep of the image."""
    off = addr - emu.BASE
    for ins in md.disasm(emu.img[off:off + 512], addr):
        if ins.mnemonic == "ret":
            return int(ins.op_str, 16) if ins.op_str else 0
    raise RuntimeError("no ret found at %#x" % addr)


def make_stub(addr, name, args_of, ret_of, pops):
    def cb(uc, address, size, user):
        LOG.append((name,) + tuple(args_of(uc)))
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
        uc.reg_write(UC_X86_REG_EAX, u32(ret_of(uc)))
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + pops)
        uc.reg_write(UC_X86_REG_EIP, ret)
    mu.hook_add(UC_HOOK_CODE, cb, None, addr, addr)


def none_(uc):
    return ()


def one(uc):
    return (stack_arg(uc, 0),)


def two(uc):
    return (stack_arg(uc, 0), stack_arg(uc, 1))


def zero(uc):
    return 0


def install_stubs():
    make_stub(F_SET_PLAYER, "set_player", one, zero, cleanup_bytes(F_SET_PLAYER))
    make_stub(F_WRAP_ADD_COMPLETED, "add_completed", two, zero, cleanup_bytes(F_WRAP_ADD_COMPLETED))
    make_stub(F_WRAP_REMOVE_INPROG, "remove_inprog", two, zero, cleanup_bytes(F_WRAP_REMOVE_INPROG))
    make_stub(F_WRAP_REMOVE_COMPLETED, "remove_completed", one, zero, cleanup_bytes(F_WRAP_REMOVE_COMPLETED))
    make_stub(F_WRAP_BEGIN_INPROG, "begin_inprog", two, zero, cleanup_bytes(F_WRAP_BEGIN_INPROG))
    make_stub(F_FOOD_GET_MADE, "get_made", none_, lambda uc: STATE["cur_made"], cleanup_bytes(F_FOOD_GET_MADE))
    make_stub(F_FOOD_GET_USED, "get_used", none_, lambda uc: STATE["cur_used"], cleanup_bytes(F_FOOD_GET_USED))
    make_stub(F_FOOD_ADD_MADE, "food_made", one, zero, cleanup_bytes(F_FOOD_ADD_MADE))
    make_stub(F_FOOD_ADD_USED, "food_used", one, zero, cleanup_bytes(F_FOOD_ADD_USED))
    # The owner getter of the unit (vtable slot 59) is a thiscall without arguments.
    make_stub(OWNER_STUB, "owner", none_, lambda uc: OWNER_INDEX, 0)


def model(reg, flags, old, new, made, used):
    """The transcription of 0x6F29F850: returns (new registration word, the calls in order)."""
    calls = [("owner",), ("set_player", OWNER_INDEX)]
    if (flags & 0x40) == 0:
        if reg & 0x8:
            calls.append(("remove_inprog", old, 0))
            reg &= 0xFFFFFFF6
        elif reg & 0x10:
            calls.append(("remove_completed", old))
            reg &= 0xFFFFFFED
        if (flags & 1) == 0:
            if flags & 2:
                calls.append(("add_completed", new, flags & 0x20))
                reg |= 0x12
        else:
            calls.append(("begin_inprog", new, flags & 0x20))
            reg |= 0x9
    if (flags & 0x1000) == 0:
        cur = reg
        m5 = 0
        m4 = 0
        if cur & 0x800:
            calls.append(("get_made",))
            calls.append(("food_made", -made))
            m5 = 0xC00
        if flags & 0x100:
            calls.append(("get_used",))
            calls.append(("food_used", used))
            m4 = 0x300
        if cur & 0x200:
            calls.append(("get_used",))
            calls.append(("food_used", -used))
            m5 |= 0x300
        if flags & 0x400:
            calls.append(("get_made",))
            calls.append(("food_made", made))
            m4 |= 0xC00
        reg = (reg & ~m5) | m4
    return reg & 0xFFFFFFFF, calls


def effect_text(c):
    return ",".join(str(x) for x in c)


def main():
    args = sys.argv[1:]
    cases = int(args[args.index("--cases") + 1]) if "--cases" in args else 3000
    seed = int(args[args.index("--seed") + 1]) if "--seed" in args else 1
    vec_path = args[args.index("--vectors") + 1] if "--vectors" in args else None
    rnd = random.Random(seed)
    rows = []
    install_stubs()
    mu.mem_write(UNIT_VT + 4 * F_OWNER_SLOT, struct.pack("<I", OWNER_STUB))
    mu.mem_write(UNIT, struct.pack("<I", UNIT_VT))
    mism = 0
    calls_total = 0
    for i in range(cases):
        if rnd.random() < 0.5:
            reg = rnd.getrandbits(12)
        else:
            reg = rnd.choice([0, 0x1, 0x8, 0x9, 0x10, 0x12, 0x200, 0x300, 0x800, 0xC00, 0x810])
        if rnd.random() < 0.5:
            flags = rnd.getrandbits(13)
        else:
            flags = rnd.choice([0, 1, 2, 3, 0x20, 0x21, 0x40, 0x100, 0x400, 0x1000, 0x101, 0x102, 0x502, 0x1040])
        old = rnd.randint(1, 6)
        new = rnd.randint(1, 6)
        STATE["cur_made"] = rnd.randint(0, 9)
        STATE["cur_used"] = rnd.randint(0, 9)
        mu.mem_write(UNIT + 0x64, struct.pack("<I", reg))
        del LOG[:]
        emu.call(F_TRANSITION, ecx=UNIT, stack_args=(flags, old, new))
        got_reg = struct.unpack("<I", bytes(mu.mem_read(UNIT + 0x64, 4)))[0]
        exp_reg, exp_calls = model(reg, flags, old, new, STATE["cur_made"], STATE["cur_used"])
        got_calls = [tuple(c) for c in LOG]
        calls_total += len(exp_calls)
        rows.append("%08x\t%08x\t%d\t%d\t%d\t%d\t%08x\t%s" % (
            reg, flags, old, new, STATE["cur_made"], STATE["cur_used"], got_reg,
            ";".join(effect_text(c) for c in exp_calls if c[0] not in ("owner", "set_player", "get_made", "get_used"))))
        if got_reg != exp_reg or got_calls != exp_calls:
            mism += 1
            if mism <= 10:
                print("MISMATCH case", i, "reg", hex(reg), "flags", hex(flags), "old", old, "new", new)
                print("  got  ", hex(got_reg), got_calls)
                print("  model", hex(exp_reg), exp_calls)
    if vec_path:
        with open(vec_path, "w") as f:
            f.write("# Generated by tools/emu/construction_counts_emu.py from Game.dll 1.26a (Unicorn). Do not edit.\n")
            f.write("# reg flags old new food_made food_used reg_out effects (name,args separated by ',' ; ';' between effects)\n")
            f.write("\n".join(rows) + "\n")
    print("cases", cases, "logged calls (model)", calls_total, "mismatches", mism)
    sys.exit(1 if mism else 0)


if __name__ == "__main__":
    main()
