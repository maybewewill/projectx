"""Differential tests of the day/night clock setup and the day rule of Game.dll 1.26a (2026-10-11).

Checked here, each against the original code run in Unicorn (docs/spec/vision.md section 13):
  * the rate of the time-of-day modifier: `0x6f429770` builds the FloatModifier of game state 2 with the rate
    `scale * DayHours / DayLength` (softfloat MUL with the scale first, then DIV by DayLength). The Misc reader
    `0x6f009ee0` is stubbed to return the chosen keys, the scale getter `0x6f4773a0` to return the chosen scale, the
    handle lookup `0x6f03fa30` and the world query `0x6f48ab80` are stubbed (the agile world is not built). The rate is
    read from the modifier constructor's argument (the DIV result, `0x6f4774e0`);
  * the day rule of `0x6f42c210`: day iff Dawn <= t < Dusk (f32 compares), with t from the time-of-day getter stub and
    the day flag taken from the setter `0x6f473140` (the run stops at the call site).

Prints one line per case: `rate <length> <hours> <scale> <rate bits hex>` and `day <t> <dawn> <dusk> <flag>`. The
Rust test `crates/sim/tests/time_of_day.rs` embeds the printed values (numbers only, no game bytes).

Usage (from the repo root):
    GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn, pefile> python3 tools/emu/time_of_day_emu.py
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "jasscomp"))
import jcomp  # noqa: E402  (Game.dll in Unicorn, Storm/CRT models, start-up initialisers)
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_EAX,
    UC_X86_REG_ECX,
    UC_X86_REG_EDX,
    UC_X86_REG_EIP,
    UC_X86_REG_ESP,
)

READER = 0x6F009EE0          # Misc reader: ECX = out (f32), stack: key, default; ret 8
SCALE_GET = 0x6F4773A0       # time-of-day scale getter: ECX = handle record, stack: out pointer; ret 4
LOOKUP = 0x6F03FA30          # handle lookup (ECX = id); replaced
WORLD_Q = 0x6F48AB80        # world query used by the modifier constructor; replaced (returns a dummy)
MOD_CREATE = 0x6F4774E0      # FloatModifier creation (the rate is its second argument)
TOD_SETUP = 0x6F429770       # the day/night timer setup (ECX = game object)
DAY_FN = 0x6F42C210          # the day/night switch (ECX = game object)
PROP_GET_NOOP = 0x6F4774A0   # property write of 0.0 in the switch: replaced by a no-op (ret 8)
DAY_SET = 0x6F473140         # the day flag setter (ECX = FloatListener/day object, stack: flag; ret 4)
STOP = 0x6F42C2DD            # call site of DAY_SET in 0x6F42C210
STUB_AGENT = 0x00300100      # the property's virtual slot 5 (agent getter), replaced


def bits(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def ret_with(uc, esp, value, pop_args):
    ret = int.from_bytes(bytes(uc.mem_read(esp, 4)), "little")
    uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop_args)
    uc.reg_write(UC_X86_REG_EIP, ret)


def make_harness(values, scale, t=None):
    """A fresh harness with the stubs installed. `values` maps Misc keys to floats, `scale` is the time scale,
    `t` (optional) the time of day returned by the getter in the switch."""
    H = jcomp.Harness(path=os.environ["GAME_DLL"])
    jcomp.run_crt_init(H)
    mu = H.mu
    objs = {}
    state = {"day": None, "log": []}

    def reader(uc, addr, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        out = uc.reg_read(UC_X86_REG_ECX)
        key = H.cstr(H.rd32(esp + 4)).decode()
        default = H.rd32(esp + 8)
        H.wr32(out, bits(values[key]) if key in values else default)
        ret_with(uc, esp, out, 8)

    def lookup(uc, addr, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ident = uc.reg_read(UC_X86_REG_ECX)
        ret_with(uc, esp, objs.get(ident, 0), 0)

    def scale_get(uc, addr, size, user):
        # the same getter serves the scale (rate setup) and the time of day (day switch)
        esp = uc.reg_read(UC_X86_REG_ESP)
        out = H.rd32(esp + 4)
        H.wr32(out, bits(scale if t is None else t))
        ret_with(uc, esp, out, 4)

    def world_query(uc, addr, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret_with(uc, esp, DUMMY, 4)

    def noop8(uc, addr, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret_with(uc, esp, 0, 8)

    def day_set(uc, addr, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        state["day"] = H.rd32(esp + 4)
        ret_with(uc, esp, 0, 4)

    def rate_arg(uc, addr, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        # [esp+4] = out pointer of the modifier, [esp+8] = pointer to the rate (the DIV result)
        state["rate"] = H.rd32(H.rd32(esp + 8))

    # the agile world is not built: the modifier constructor's world query returns a zeroed dummy object
    DUMMY = H.alloc(0x100, align=32)
    mu.mem_write(DUMMY, b"\0" * 0x100)
    mu.hook_add(UC_HOOK_CODE, reader, begin=READER, end=READER)
    mu.hook_add(UC_HOOK_CODE, lookup, begin=LOOKUP, end=LOOKUP)
    mu.hook_add(UC_HOOK_CODE, scale_get, begin=SCALE_GET, end=SCALE_GET)
    mu.hook_add(UC_HOOK_CODE, world_query, begin=WORLD_Q, end=WORLD_Q)
    mu.hook_add(UC_HOOK_CODE, noop8, begin=PROP_GET_NOOP, end=PROP_GET_NOOP)
    mu.hook_add(UC_HOOK_CODE, day_set, begin=DAY_SET, end=DAY_SET)
    mu.hook_add(UC_HOOK_CODE, rate_arg, begin=MOD_CREATE, end=MOD_CREATE)
    return H, objs, state


def rate_case(length, hours, scale):
    """The rate bits the setup stores for DayLength, DayHours and the scale."""
    values = {"DayLength": length, "DayHours": hours, "Dawn": 6.0, "Dusk": 18.0}
    H, objs, state = make_harness(values, scale)
    # the scale record and the time-of-day slot (a property with a virtual agent getter, see tod2.py)
    G = H.alloc(0x100, align=32)
    H.mu.mem_write(G, b"\0" * 0x100)
    scale_id, prop_id, slot_obj = 0x100201, 0x100300, H.alloc(0x200, align=32)
    H.mu.mem_write(slot_obj, b"\0" * 0x200)
    objs[scale_id] = slot_obj
    objs[prop_id] = slot_obj
    H.wr32(G + 0x5C + 8, scale_id)
    H.wr32(G + 0x5C + 0xC, 0x77)
    FAKE_V = H.alloc(0x100, align=32)
    H.mu.mem_write(FAKE_V, b"\0" * 0x100)
    H.wr32(FAKE_V + 0x14, STUB_AGENT)
    H.wr32(G + 0x28, FAKE_V)
    H.wr32(G + 0x28 + 4, 1)
    H.wr32(G + 0x30, prop_id)
    H.wr32(G + 0x34, 0x88)
    AGENT_OBJ = H.alloc(0x100, align=32)
    H.mu.mem_write(AGENT_OBJ, b"\0" * 0x100)
    H.mu.hook_add(UC_HOOK_CODE, lambda uc, a, s, u: ret_with(uc, uc.reg_read(UC_X86_REG_ESP), AGENT_OBJ, 0),
                  begin=STUB_AGENT, end=STUB_AGENT)
    H.call(TOD_SETUP, ecx=G, stack=())
    return state.get("rate")


def day_case(t, dawn, dusk):
    values = {"DayHours": 24.0, "Dawn": dawn, "Dusk": dusk}
    H, objs, state = make_harness(values, 1.0, t=t)
    G = H.alloc(0x100, align=32)
    H.mu.mem_write(G, b"\0" * 0x100)
    H.call(DAY_FN, ecx=G, stop_at=STOP, count=5_000_000)
    return state.get("day")


def main():
    rate_inputs = [(240.0, 12.0, 1.0), (240.0, 24.0, 0.5), (37.0, 9.0, 2.5), (3.0, 7.0, 0.1)]
    for length, hours, scale in rate_inputs:
        r = rate_case(length, hours, scale)
        print("rate %r %r %r %08x" % (length, hours, scale, r if r is not None else 0xFFFFFFFF))
    day_inputs = [(0.0, 6.0, 18.0), (5.99, 6.0, 18.0), (6.0, 6.0, 18.0), (12.0, 6.0, 18.0),
                  (17.99, 6.0, 18.0), (18.0, 6.0, 18.0), (23.5, 6.0, 18.0), (4.49, 4.5, 20.0),
                  (4.5, 4.5, 20.0), (19.99, 4.5, 20.0), (20.0, 4.5, 20.0), (5.0, 20.0, 4.0),
                  (21.0, 20.0, 4.0)]
    for t, dawn, dusk in day_inputs:
        print("day %r %r %r %r" % (t, dawn, dusk, day_case(t, dawn, dusk)))


if __name__ == "__main__":
    main()
