"""Unicorn check of the evasion decision and its chance table (Game.dll 1.26a).

Routines run for real:
  - CAbilityEvasion::EvadeAttack   0x6F08BD20 (vtable slot 0x168 of the evasion class, 0x6F89D628)
  - the score slot                 0x6F08BDA0 (vtable slot 0x15C)
  - the chance getter              0x6F02E9E0 (level -> chance)
Stubbed (their effects are not part of the decision):
  - the RNG stream helper          0x6F2853D0: returns a pointer to a float that the case sets (r); the stream index
    it is called with (EDX) is recorded, so the stream is checked too (expected 2, combat);
  - the effect on evasion          0x6F052FB0; the assert/log call 0x6F537E80; the Misc/def lookup 0x6F265ED0;
  - the enabled test is the object's own slot 0x268 (returns a value the case sets).

Part 1 (decision): for random (c, r) the result must be 1 iff c > r (strict), with the effect routine called once
exactly on a hit. Cases: finite values, equal values, NaN for r and for c.
Part 2 (chance table): the getter returns table[level] for 0 <= level < count with a loaded table, 0.0 for a
negative level, for level >= count, and for a definition without the flag. Random float tables.
Part 3 (gates): the count gate (+0x3C > 0 gives 0) and the enabled gate (slot 0x268 == 0 gives 0 without a draw).

Usage (output kept outside the repository; numbers come from the user's Game.dll):
    GAME_DLL=/path/to/Game.dll PYTHONPATH=<unicorn> python3 tools/emu/evasion_emu.py [cases]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402  (maps Game.dll)
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_EAX,
    UC_X86_REG_ECX,
    UC_X86_REG_EDX,
    UC_X86_REG_EIP,
    UC_X86_REG_ESP,
)

mu = emu.mu
EVADE = 0x6F08BD20
SCORE = 0x6F08BDA0
CHANCE = 0x6F02E9E0
RNG = 0x6F2853D0
EFFECT = 0x6F052FB0
ASSERT = 0x6F537E80
DEFLOOK = 0x6F265ED0

BASE = emu.DATA + 0x8000
EV = BASE + 0x000  # evasion ability object
DEF = BASE + 0x100  # definition object
TBL = BASE + 0x200  # level table
VT = BASE + 0x400  # evasion vtable
VT_FLAG = BASE + 0x500  # slot 0x268 result
RVAL = BASE + 0x600  # the r value returned by the RNG stub
RVPTR = BASE + 0x604
STUB_RET = BASE + 0x700  # plain 'ret' stub
STUB_268 = BASE + 0x720
CALLS = {}

STATE = {"rng_stream": None, "effect": 0, "flag268": 1, "rng_calls": 0, "def_calls": 0}


def wr32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def rd32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def arg(n):
    return rd32(mu.reg_read(UC_X86_REG_ESP) + 4 * n)


def ret_to(n_pop):
    esp = mu.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    mu.reg_write(UC_X86_REG_ESP, esp + 4 + n_pop)
    mu.reg_write(UC_X86_REG_EIP, ret)


def on_code(uc, eip, size, user):
    if eip == RNG:
        # __fastcall(ECX = output buffer, EDX = stream): writes the draw r into the buffer and returns the buffer
        # (the caller reads [eax]). The stream index is recorded; no stack arguments (ret 0).
        buf = mu.reg_read(UC_X86_REG_ECX)
        STATE["rng_stream"] = mu.reg_read(UC_X86_REG_EDX)
        STATE["rng_calls"] += 1
        wr32(buf, rd32(RVAL))
        mu.reg_write(UC_X86_REG_EAX, buf)
        ret_to(0)
        return
    if eip == EFFECT:
        STATE["effect"] += 1
        mu.reg_write(UC_X86_REG_EAX, 0)
        ret_to(0)  # __thiscall (ECX = ability), no stack arguments
        return
    if eip == ASSERT:
        mu.reg_write(UC_X86_REG_EAX, 0)
        ret_to(0)
        return
    if eip == DEFLOOK:
        STATE["def_calls"] += 1
        mu.reg_write(UC_X86_REG_EAX, 0)
        ret_to(0)
        return
    if eip == STUB_268:
        # vtable slot 0x268 of the evasion object: __thiscall with one stack argument (0)
        mu.reg_write(UC_X86_REG_EAX, STATE["flag268"])
        ret_to(4)
        return


def setup_objects(count, level_table, flag_def=1):
    for a, n in ((EV, 0x200), (DEF, 0x100), (TBL, 0x200), (VT, 0x100), (VT_FLAG, 0x40)):
        mu.mem_write(a, b"\0" * n)
    mu.mem_write(STUB_RET, b"\xc3")
    mu.mem_write(STUB_268, b"\xc3")
    wr32(VT + 0x268, STUB_268)
    wr32(EV, VT)
    wr32(EV + 0x3C, 0)  # count gate (signed <= 0 passes)
    wr32(EV + 0x50, 0)  # level index, set per case
    wr32(EV + 0x54, DEF)
    wr32(DEF + 0x2C, flag_def)  # the definition has data
    wr32(DEF + 0x50, count)  # number of levels
    wr32(DEF + 0x54, TBL)  # level table pointer
    for i, v in enumerate(level_table):
        wr32(TBL + 0x20 + i * 0x68, v)


def set_r(r_bits):
    wr32(RVAL, r_bits)


def call_fastcall_ecx(fn, ecx, stack_args, edx=None, stop_at=None):
    """thiscall/fastcall helper: pushes stack args (last first) and a return address to SENT."""
    esp = emu.TOP - 0x800
    for a in reversed(stack_args):
        esp -= 4
        mu.mem_write(esp, struct.pack("<I", a & 0xFFFFFFFF))
    esp -= 4
    mu.mem_write(esp, struct.pack("<I", emu.SENT))
    mu.reg_write(UC_X86_REG_ESP, esp)
    mu.reg_write(UC_X86_REG_ECX, ecx & 0xFFFFFFFF)
    if edx is not None:
        mu.reg_write(UC_X86_REG_EDX, edx & 0xFFFFFFFF)
    mu.emu_start(fn, emu.SENT, timeout=5_000_000, count=200_000)
    return mu.reg_read(UC_X86_REG_EAX)


def f32(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def bits_to_f(b):
    return struct.unpack("<f", struct.pack("<I", b & 0xFFFFFFFF))[0]


def is_nan_bits(b):
    return (b >> 23) & 0xFF == 0xFF and (b & 0x7FFFFF) != 0


def model_evade(c_bits, r_bits):
    """Decision of EvadeAttack: evades iff c > r. The original's x87 compare is 'fcom' with an unordered result
    (NaN) setting the parity flag, which the routine also tests: unordered counts as evading (LIKELY, see spec)."""
    if is_nan_bits(c_bits) or is_nan_bits(r_bits):
        return 1
    return 1 if bits_to_f(c_bits) > bits_to_f(r_bits) else 0


def run_evade(c_bits, r_bits, count=8, level=0, flag268=1):
    """One EvadeAttack call. Level table: the chance of `level` is c_bits."""
    table = [0] * count
    if 0 <= level < count:
        table[level] = c_bits
    setup_objects(count, table)
    wr32(EV + 0x50, level)
    STATE.update({"rng_stream": None, "effect": 0, "flag268": flag268, "rng_calls": 0})
    set_r(r_bits)
    ret = call_fastcall_ecx(EVADE, EV, [0, 0])
    return ret, dict(STATE)


def run_chance(table, count, level, flag_def=1):
    setup_objects(count, table, flag_def)
    wr32(EV + 0x50, level)
    out = BASE + 0x800
    mu.mem_write(out, b"\0" * 8)
    call_fastcall_ecx(CHANCE, EV, [out, level])
    return rd32(out)


def main():
    cases = int(sys.argv[1]) if len(sys.argv) > 1 else 300
    rng = random.Random(20261011)
    mu.hook_add(UC_HOOK_CODE, on_code)
    fails = 0
    # Part 1: decision
    n1 = 0
    for k in range(cases):
        c = bits_to_f(rng.getrandbits(32))
        r = bits_to_f(rng.getrandbits(32))
        if k % 5 == 0:
            r = rng.random()
            c = r  # equal values: strict compare must not evade
        if k % 7 == 0:
            c = rng.uniform(0.0, 1.0)
            r = rng.uniform(0.0, 1.0)
        cb = f32(c) if not (c != c) else 0x7FC00000
        rb = f32(r) if not (r != r) else 0x7FC00000
        # keep only finite or NaN values the model covers (infinities are fine too)
        ret, st = run_evade(cb, rb)
        want = model_evade(cb, rb)
        ok = (ret == want) and (st["effect"] == want) and (st["rng_stream"] == 2) and (st["rng_calls"] == 1)
        n1 += 1
        if not ok:
            fails += 1
            if fails <= 10:
                print("MISMATCH decision c=%08x r=%08x ret=%s want=%s effect=%s stream=%s" % (
                    cb, rb, ret, want, st["effect"], st["rng_stream"]))
    print("part1 decision cases: %d, mismatches so far: %d" % (n1, fails))
    # Part 3: gates, a count above zero and a disabled object draw nothing
    setup_objects(4, [f32(0.5)] * 4)
    wr32(EV + 0x3C, 1)
    set_r(0)
    STATE.update({"rng_calls": 0, "effect": 0})
    ret = call_fastcall_ecx(EVADE, EV, [0, 0])
    gate_count_ok = ret == 0 and STATE["rng_calls"] == 0
    wr32(EV + 0x3C, 0)
    ret2, st2 = run_evade(f32(1.0), f32(0.0), flag268=0)
    gate_flag_ok = ret2 == 0 and st2["rng_calls"] == 0
    print("part3 gates: count>0 gives 0 without a draw: %s; disabled gives 0 without a draw: %s" % (
        gate_count_ok, gate_flag_ok))
    if not (gate_count_ok and gate_flag_ok):
        fails += 1
    # Part 2: the chance table
    n2 = 0
    f2 = 0
    for k in range(cases // 3):
        count = rng.randint(1, 8)
        table = [f32(rng.uniform(-1.0, 2.0)) for _ in range(count)]
        level = rng.randint(-2, count + 2)
        got = run_chance(table, count, level)
        if level < 0 or level >= count:
            want = 0
        else:
            want = table[level]
        n2 += 1
        if got != want:
            f2 += 1
            if f2 <= 5:
                print("MISMATCH chance count=%d level=%d got=%08x want=%08x" % (count, level, got, want))
    got = run_chance([f32(0.7)] * 3, 3, 1, flag_def=0)
    no_flag = got == 0
    print("part2 chance table cases: %d, mismatches: %d; without the data flag gives 0: %s" % (n2, f2, no_flag))
    fails += f2 + (0 if no_flag else 1)
    print("TOTAL mismatches: %d" % fails)


if __name__ == "__main__":
    main()
