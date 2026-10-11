"""Unicorn check of the build payment and pending-payment refund of Game.dll 1.26a (docs/spec/construction.md, section 2).

Runs the original:
  0x6F0EDA70  CAbilityBuild placement check (flag 0): pays the gold and lumber cost when both are covered,
              records the paid amounts in the ability (+0x94 gold, +0x98 lumber) and sets bit 31 of +0x20.
              Flag 1 (the restore path, event 0xD029C) is not run here.
  0x6F0D9400  pay: changer with the negated amounts (through the wrappers 0x6F416A00 / 0x6F416A40).
  0x6F0ED9F0  refund or clear: message, then (when an amount is non-zero) the player and the changers.
  0x6F0F7750  pending refund: zeroes +0x94/+0x98 and refunds them through 0x6F0ED9F0.

Stubbed (logged): the type costs 0x6F29B800 (gold, +0x20) and 0x6F29B830 (lumber, +0x24), the resource reads
0x6F473170 (called twice by the placement check; the first is the gold test, the second the lumber test; the
object it reads is not identified, LIKELY the stored gold and lumber), the changer wrappers 0x6F416A00/0x6F416A40
(the original's x10 scaling happens inside them and is not run), the player setter 0x6F3A1650, the message 0x6F03FEF0 /
0x6F03FF20, the post-pay hook 0x6F03FEF0, and the owner's vtable slot 25 (0x64).

Python model = transcription of the decompile. Checks: the logged calls in order, the ability fields after the call.
Usage: GAME_DLL=/path/Game.dll python3 -I tools/emu/construction_pay_emu.py [--cases N] [--seed S]
"""
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402
from capstone import Cs, CS_ARCH_X86, CS_MODE_32  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
u32 = emu.u32

F_PLACE = 0x6F0EDA70
F_PENDING_REFUND = 0x6F0F7750
F_GOLD_COST = 0x6F29B800
F_LUMBER_COST = 0x6F29B830
F_RES = 0x6F473170
F_CHANGE_GOLD = 0x6F416A00
F_CHANGE_LUMBER = 0x6F416A40
F_SET_PLAYER = 0x6F3A1650
F_MSG_A = 0x6F03FF20
F_MSG_B = 0x6F03FEF0

ABIL = emu.DATA + 0x4000        # the build ability (this)
OWNER = emu.DATA + 0x4400       # the owner unit (this+0x30)
OWNER_VT = emu.DATA + 0x4800
OWNER_STUB = emu.DATA + 0x4900  # hook target, never executed
OWNER_INDEX = 2
EVENT = emu.DATA + 0x4C00       # the event (first argument): +0xC is the building's unit object, +0x34 its type
BUILDING = emu.DATA + 0x4D00
OWN_SLOT = 25                    # vtable slot 0x64 of the owner

LOG = []
STATE = {"gold_cost": 0, "lumber_cost": 0, "res": [0, 0], "res_calls": 0}
md = Cs(CS_ARCH_X86, CS_MODE_32)


def stack_arg(uc, n):
    esp = uc.reg_read(UC_X86_REG_ESP)
    return struct.unpack("<i", bytes(uc.mem_read(esp + 4 + 4 * n, 4)))[0]


def cleanup_bytes(addr):
    off = addr - emu.BASE
    for ins in md.disasm(emu.img[off:off + 512], addr):
        if ins.mnemonic == "ret":
            return int(ins.op_str, 16) if ins.op_str else 0
    raise RuntimeError("no ret found at %#x" % addr)


def make_stub(addr, name, nargs, ret_of, pops):
    def cb(uc, address, size, user):
        LOG.append((name,) + tuple(stack_arg(uc, i) for i in range(nargs)))
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
        uc.reg_write(UC_X86_REG_EAX, u32(ret_of(uc)))
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + pops)
        uc.reg_write(UC_X86_REG_EIP, ret)
    mu.hook_add(UC_HOOK_CODE, cb, None, addr, addr)


def install_stubs():
    make_stub(F_GOLD_COST, "gold_cost", 0, lambda uc: STATE["gold_cost"], 0)
    make_stub(F_LUMBER_COST, "lumber_cost", 0, lambda uc: STATE["lumber_cost"], 0)
    mu.hook_add(UC_HOOK_CODE, _res_hook, None, F_RES, F_RES)
    make_stub(F_CHANGE_GOLD, "change_gold", 1, lambda uc: 0, cleanup_bytes(F_CHANGE_GOLD))
    make_stub(F_CHANGE_LUMBER, "change_lumber", 1, lambda uc: 0, cleanup_bytes(F_CHANGE_LUMBER))
    make_stub(F_SET_PLAYER, "set_player", 1, lambda uc: 0, cleanup_bytes(F_SET_PLAYER))
    make_stub(F_MSG_A, "msg", 0, lambda uc: 0, cleanup_bytes(F_MSG_A))
    make_stub(F_MSG_B, "post_pay", 0, lambda uc: 0, cleanup_bytes(F_MSG_B))
    # The owner's vtable slot 25 (thiscall, no arguments).
    make_stub(OWNER_STUB, "owner_index", 0, lambda uc: OWNER_INDEX, 0)


def _res_hook(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
    LOG.append(("res",))
    v = STATE["res"][STATE["res_calls"]]
    STATE["res_calls"] += 1
    uc.reg_write(UC_X86_REG_EAX, u32(v))
    uc.reg_write(UC_X86_REG_ESP, esp + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


def rd(addr):
    return struct.unpack("<I", bytes(mu.mem_read(addr, 4)))[0]


def wr(addr, v):
    mu.mem_write(addr, struct.pack("<I", u32(v)))


def model_place(flags, gold, lumber, r_gold, r_lumber):
    """0x6F0EDA70 flag 0. Returns (flags, paid_gold, paid_lumber, calls)."""
    calls = [("gold_cost",), ("lumber_cost",)]
    # The owner is non-null in this harness, so the fallback 0x6F472890 is not called.
    calls.append(("res",))
    if gold <= r_gold // 10:
        calls.append(("res",))
        if lumber <= r_lumber // 10:
            calls.append(("change_gold", -gold))
            calls.append(("change_lumber", -lumber))
            calls.append(("post_pay",))
            return (flags | 0x80000000) & 0xFFFFFFFF, gold, lumber, calls
    calls.append(("msg",))
    return flags & 0x7FFFFFFF, 0, 0, calls


def model_refund_or_clear(flags, player, gold, lumber):
    calls = [("msg",)]
    if gold or lumber:
        if player == -1:
            calls.append(("owner_index",))
            player = OWNER_INDEX
        calls.append(("set_player", player))
        calls.append(("change_gold", gold))
        calls.append(("change_lumber", lumber))
    return flags & 0x7FFFFFFF, calls


def model_pending_refund(flags, paid_gold, paid_lumber):
    flags2, calls = model_refund_or_clear(flags, -1, paid_gold, paid_lumber)
    return flags2, 0, 0, calls


def main():
    args = sys.argv[1:]
    cases = int(args[args.index("--cases") + 1]) if "--cases" in args else 3000
    seed = int(args[args.index("--seed") + 1]) if "--seed" in args else 1
    rnd = random.Random(seed)
    install_stubs()
    mu.mem_write(OWNER_VT + 4 * OWN_SLOT, struct.pack("<I", OWNER_STUB))
    mu.mem_write(OWNER, struct.pack("<I", OWNER_VT))
    wr(ABIL + 0x30, OWNER)
    wr(EVENT + 0xC, BUILDING)
    wr(BUILDING + 0x34, 0x1234)
    mism = 0
    n_place = n_pay = n_refund = 0
    for i in range(cases):
        kind = rnd.choice(["place", "place", "pending"])
        flags = rnd.getrandbits(32) & rnd.choice([0xFFFFFFFF, 0x7FFFFFFF, 0x0000FFFF])
        gold = rnd.randint(0, 600)
        lumber = rnd.randint(0, 400)
        r_gold = rnd.randint(0, 99999)
        r_lumber = rnd.randint(0, 99999)
        # Sometimes make the check borderline on purpose.
        if rnd.random() < 0.3:
            r_gold = gold * 10 + rnd.choice([-1, 0, 9, 10])
            r_gold = max(0, r_gold)
        if rnd.random() < 0.3:
            r_lumber = lumber * 10 + rnd.choice([-1, 0, 9, 10])
            r_lumber = max(0, r_lumber)
        STATE["gold_cost"] = gold
        STATE["lumber_cost"] = lumber
        STATE["res"] = [r_gold, r_lumber]
        STATE["res_calls"] = 0
        wr(ABIL + 0x20, flags)
        wr(ABIL + 0x94, 0)
        wr(ABIL + 0x98, 0)
        del LOG[:]
        if kind == "place":
            n_place += 1
            # (event pointer, flag): the dispatcher passes the event and flag 0 for the placement check.
            emu.call(F_PLACE, ecx=ABIL, stack_args=(EVENT, 0))
            exp_flags, pg, pl, exp_calls = model_place(flags, gold, lumber, r_gold, r_lumber)
            if pg or pl:
                n_pay += 1
        else:
            n_refund += 1
            paid_g = rnd.randint(0, 600)
            paid_l = rnd.randint(0, 400)
            wr(ABIL + 0x94, paid_g)
            wr(ABIL + 0x98, paid_l)
            emu.call(F_PENDING_REFUND, ecx=ABIL, stack_args=(0xFFFFFFFF,))
            exp_flags, pg, pl, exp_calls = model_pending_refund(flags, paid_g, paid_l)
        got = (rd(ABIL + 0x20), rd(ABIL + 0x94), rd(ABIL + 0x98))
        got_calls = [tuple(c) for c in LOG]
        exp = (u32(exp_flags), pg, pl)
        if got != exp or got_calls != exp_calls:
            mism += 1
            if mism <= 10:
                print("MISMATCH case", i, kind, "flags", hex(flags), "gold", gold, "lumber", lumber,
                      "res", r_gold, r_lumber)
                print("  got  ", [hex(x) for x in got], got_calls)
                print("  model", [hex(x) for x in exp], exp_calls)
    print("cases", cases, "placements", n_place, "paid", n_pay, "pending refunds", n_refund, "mismatches", mism)
    sys.exit(1 if mism else 0)


if __name__ == "__main__":
    main()
