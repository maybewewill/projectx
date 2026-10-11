"""Unicorn run of the basic-attack execute path of CAbilityAttack (Game.dll 1.26a, VA 0x6F0CF660).

Question answered (attack-damage.md section 11.2): which writes set the damage record's `+0x20` field (the damage-type row
and the ethereal slot index) on the path of an ordinary attack, and which value it has when the target's damage routine
(`0x6F2A40D0`, through the target's vtable slot `0x120`) is called.

How it works: the real execute code runs. The routines it calls are replaced by small stubs (the stub pops the
arguments with the callee's own `ret N`, so the stack stays exact). The record is not located by hand: the stub of the
attack-handler callback `0x6F2763A0` receives the record pointer as an argument and remembers it. A memory-write hook
on record+0x20 then logs every write, with the EIP that made it. Stubs and objects:
  - ability object (ECX): owner at +0x30, the weapon arrays at +0xDC (missile type), +0x88/+0x94/+0xA0 (dice, sides,
    bonus), +0xE8/+0xF4 (weapon fields), ability lists masked to -1 (no list walk);
  - target and owner objects with a fake vtable: slot 0xB8 (returns a zero vector), slot 0x120 (target damage: the
    stub records the record pointer and the value of record+0x20 when called, then stops).
  - GetDamageAmount `0x6F0C6940` stub: writes 3.0 to its argument and returns the argument (its real code is a
    roll; the amount does not change the row).
  - range predicate `0x6F4766E0` returns 1 (the attack goes through to the damage path).

Usage (outside the repository for the output):
    GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn, pefile> python3 tools/emu/execute_row_emu.py
It prints one line per weapon-type value (0..9): the writes to record+0x20 with the EIP of each writer, and the
value found in record+0x20 when the target damage routine was entered (or the reason it did not get there).
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402  (maps Game.dll and provides mu, DATA, TOP, SENT)
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE, UC_HOOK_MEM_READ  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_EAX,
    UC_X86_REG_EBX,
    UC_X86_REG_ECX,
    UC_X86_REG_EDI,
    UC_X86_REG_EDX,
    UC_X86_REG_EBP,
    UC_X86_REG_EIP,
    UC_X86_REG_ESI,
    UC_X86_REG_ESP,
)

mu = emu.mu
EXEC = 0x6F0CF660
GET_AMOUNT = 0x6F0C6940
CTOR = 0x6F2556E0
CB_A = 0x6F2763A0  # attack-handler callback, args (target, record); ret 8
CB_B = 0x6F2764C0  # attack-handler callback, args (target, record); ret 8
FN_32D150 = 0x6F32D150  # fastcall (ECX owner, EDX weapon), no stack args
RANGE = 0x6F4766E0  # range predicate, returns eax
FN_71C0 = 0x6F0C71C0
FN_6B30 = 0x6F0C6B30  # thiscall, 2 stack args (out pointers), ret 8
FN_8450 = 0x6F038450  # thiscall (ECX buffer), 1 stack arg, ret 4
FN_7200 = 0x6F0C7200
FN_26F2B0 = 0x6F26F2B0
FN_BB970 = 0x6F2BB970
FN_8DA0 = 0x6F0C8DA0
FN_BEAC0 = 0x6F2BEAC0
FN_BB960 = 0x6F2BB960
FN_2BEA20 = 0x6F2BEA20
FN_6F6EF000 = 0x6F6EF000  # softfloat helpers are not needed here: stubbed as returning a pointer to zero
FN_6F6EEEF0 = 0x6F6EEEF0

FS_READ = 0x6F0CF667  # mov eax, fs:[0] (SEH frame) is replaced by a constant
FS_WRITE = 0x6F0CF687  # mov fs:[0], eax (SEH frame install) is skipped
FS_WRITE_EPI = 0x6F0D0213  # mov fs:[0], ecx (SEH frame restore) is skipped
STOP_SEEN = {"stop": None}

AB = emu.DATA + 0x1000  # ability object (ECX)
OWN = emu.DATA + 0x2000  # owner unit
TGT = emu.DATA + 0x3000  # target unit
VT_OWN = emu.DATA + 0x4000
VT_TGT = emu.DATA + 0x4100
STUB_AREA = emu.DATA + 0x4200  # generic stub code (ret 0)
VEC = emu.DATA + 0x5000  # zero vector returned by slot 0xB8 and friends
SCRATCH = emu.DATA + 0x6000

WRITES = []  # (eip, value) for writes to record+0x20
STATE = {"record": None, "row_at_target": None, "target_called": False, "trace": []}


def rd32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def arg(n):
    """n-th stack argument of a function at its entry (n=1 is the first after the return address)."""
    esp = mu.reg_read(UC_X86_REG_ESP)
    return rd32(esp + 4 * n)


def ret_to(n_pop):
    """Return from a stub: pops the return address and n_pop bytes of arguments."""
    esp = mu.reg_read(UC_X86_REG_ESP)
    ret = rd32(esp)
    mu.reg_write(UC_X86_REG_ESP, esp + 4 + n_pop)
    mu.reg_write(UC_X86_REG_EIP, ret)


def stub_zero_ret(n_pop, eax=0):
    def f(eip):
        mu.reg_write(UC_X86_REG_EAX, eax)
        ret_to(n_pop)
    return f


def stub_get_amount(eip):
    # thiscall, one stack argument: the amount pointer. Returns the pointer in eax (as the caller expects).
    p = arg(1)
    mu.mem_write(p, struct.pack("<f", 3.0))
    mu.reg_write(UC_X86_REG_EAX, p)
    ret_to(4)


def stub_cb(eip):
    # args (target, record): the record is the second argument.
    tgt = arg(1)
    rec = arg(2)
    STATE["record"] = rec
    mu.reg_write(UC_X86_REG_EAX, 0)
    ret_to(8)


def stub_cb_zero(eip):
    mu.reg_write(UC_X86_REG_EAX, 0)
    ret_to(8)


def stub_b30(eip):
    # thiscall, two out pointers: zero the vector the caller reads
    out = arg(1)
    for k in range(3):
        mu.mem_write(out + 4 * k, struct.pack("<f", 0.0))
    mu.reg_write(UC_X86_REG_EAX, 0)
    ret_to(8)


def stub_8450(eip):
    # thiscall, ECX = out buffer, one argument: write zero dwords around the buffer so the caller takes its
    # "no object" branch.
    ecx = mu.reg_read(UC_X86_REG_ECX)
    for k in range(-2, 4):
        wr32(ecx + 4 * k, 0)
    mu.reg_write(UC_X86_REG_EAX, 0)
    ret_to(4)


def stub_fastcall_pair(eip):
    mu.reg_write(UC_X86_REG_EAX, 0)
    ret_to(0)


def stub_range(eip):
    # the range predicate is called as thiscall with 3 stack arguments in the caller (push 1; push eax; push ecx)
    mu.reg_write(UC_X86_REG_EAX, 1)
    ret_to(12)


def stub_return_vec(eip):
    # slot 0xB8 of target/owner: thiscall, no stack argument, returns a pointer to a zero vector
    mu.reg_write(UC_X86_REG_EAX, VEC)
    ret_to(0)


def stub_ret_zero(n):
    def f(eip):
        mu.reg_write(UC_X86_REG_EAX, 0)
        ret_to(n)
    return f


def stub_target_damage(eip):
    # slot 0x120 of the target: (out, record, 1, 0) with ret 0x10
    rec = arg(2)
    STATE["target_called"] = True
    STATE["record_at_target"] = rec
    STATE["row_at_target"] = rd32(rec + 0x20)
    mu.reg_write(UC_X86_REG_EAX, 0)
    mu.emu_stop()


def stub_71c0(eip):
    mu.reg_write(UC_X86_REG_EAX, VEC)
    ret_to(0)


def stub_26f2b0(eip):
    mu.reg_write(UC_X86_REG_EAX, 0)
    ret_to(4)


def stub_7200(eip):
    mu.reg_write(UC_X86_REG_EAX, 0)
    ret_to(4)


def stub_bb970(eip):
    mu.reg_write(UC_X86_REG_EAX, 0)
    ret_to(0)


def stub_beac0(eip):
    # the miss check (__fastcall, two stack arguments): returns 0 (no miss)
    mu.reg_write(UC_X86_REG_EAX, 0)
    ret_to(8)


def stub_bb960(eip):
    mu.reg_write(UC_X86_REG_EAX, 0)
    ret_to(4)


def stub_generic_unknown(eip):
    STATE["trace"].append(("unhandled call", eip))
    mu.emu_stop()


HOOKS = {
    GET_AMOUNT: stub_get_amount,
    CB_A: stub_cb,
    CB_B: stub_cb_zero,
    FN_32D150: stub_fastcall_pair,
    RANGE: stub_range,
    FN_71C0: stub_71c0,
    FN_6B30: stub_b30,
    FN_8450: stub_8450,
    FN_7200: stub_7200,
    FN_26F2B0: stub_26f2b0,
    FN_BB970: stub_bb970,
    FN_BEAC0: stub_beac0,
    FN_BB960: stub_bb960,
}


def on_code(uc, eip, size, user):
    if eip == FS_READ:
        mu.reg_write(UC_X86_REG_EAX, 0)
        mu.reg_write(UC_X86_REG_EIP, FS_READ + 6)
        return
    if eip == FS_WRITE:
        # mov fs:[0], eax (SEH frame install, 6 bytes): no TEB in the emulator
        mu.reg_write(UC_X86_REG_EIP, FS_WRITE + 6)
        return
    if eip == FS_WRITE_EPI:
        # mov fs:[0], ecx (SEH frame restore in the epilogue, 7 bytes)
        mu.reg_write(UC_X86_REG_EIP, FS_WRITE_EPI + 7)
        return
    if eip == CTOR:
        # the record is the constructor's `this` (ECX); the real constructor runs and writes +0x20 = 0
        STATE["record"] = mu.reg_read(UC_X86_REG_ECX)
        return
    if eip in HOOKS:
        STATE["trace"].append(("stub", eip))
        HOOKS[eip](eip)
        return
    if eip == VT_TGT_SLOT_120:
        stub_target_damage(eip)
        return


VT_TGT_SLOT_120 = None  # set in setup from the stub address table


def on_write(uc, access, addr, size, value, user):
    rec = STATE["record"]
    if rec is not None and addr == rec + 0x20:
        WRITES.append((hex(mu.reg_read(UC_X86_REG_EIP)), value & 0xFFFFFFFF))


def setup(weapon_type_value, atk_type_value, weapon=0):
    global VT_TGT_SLOT_120
    for a in (AB, OWN, TGT, VT_OWN, VT_TGT, VEC):
        mu.mem_write(a, b"\0" * 0x400)
    # vtables: slot 0xB8 -> stub_return_vec, slot 0x120 -> target damage stub (real object stubs below)
    stub_vec_addr = STUB_AREA
    stub_t_addr = STUB_AREA + 0x40
    stub_out_addr = STUB_AREA + 0x80
    for a in (stub_vec_addr, stub_t_addr, stub_out_addr):
        mu.mem_write(a, b"\xc3")
    HOOKS[stub_vec_addr] = stub_return_vec
    HOOKS[stub_t_addr] = stub_target_damage
    HOOKS[stub_out_addr] = stub_ret_zero(0)
    for slot in range(0, 0x200, 4):
        wr32(VT_TGT + slot, stub_out_addr)
        wr32(VT_OWN + slot, stub_out_addr)
    wr32(VT_TGT + 0xB8, stub_vec_addr)
    wr32(VT_OWN + 0xB8, stub_vec_addr)
    wr32(VT_TGT + 0x120, stub_t_addr)
    wr32(VT_TGT + 0x11C, stub_out_addr)
    wr32(VT_TGT + 0x128, stub_out_addr)
    VT_TGT_SLOT_120 = stub_t_addr
    wr32(TGT, VT_TGT)
    wr32(OWN, VT_OWN)
    # ability object
    wr32(AB + 0x30, OWN)
    wr32(AB + 0x1DC, 0xFFFFFFFF)
    wr32(AB + 0x1E0, 0xFFFFFFFF)
    wr32(AB + 0x250, 0xFFFFFFFF)
    wr32(AB + 0x254, 0xFFFFFFFF)
    wr32(AB + 0x2B8, 0)
    wr32(AB + 0x20, 0)
    for w in range(2):
        wr32(AB + 0x88 + 4 * w, 2)  # dice
        wr32(AB + 0x94 + 4 * w, 6)  # sides
        wr32(AB + 0xA0 + 4 * w, 0)  # bonus
        wr32(AB + 0xAC + 4 * w, 0)
        wr32(AB + 0xDC + 4 * w, weapon_type_value if w == weapon else 0)  # weapon missile type (weapTp)
        wr32(AB + 0xF4 + 4 * w, atk_type_value if w == weapon else 0)  # attack type (atkType)
        wr32(AB + 0xE8 + 4 * w, 0x1234)
        wr32(AB + 0x238 + 4 * w, 0)
        wr32(AB + 0x26C + 8 * w, 0)
        wr32(AB + 0x238 + 4 * w, 0)
    STATE["record"] = None
    STATE["target_called"] = False
    STATE["row_at_target"] = None
    del WRITES[:]
    STATE["trace"] = []


def run(weapon_type_value, atk_type_value, weapon=0):
    setup(weapon_type_value, atk_type_value, weapon)
    mu.hook_add(UC_HOOK_CODE, on_code)
    mu.hook_add(UC_HOOK_MEM_WRITE, on_write)
    # stack: args of the execute call (this=ECX=ability), stack args target, weapon, flag 0x180, p5, p6, p7
    esp = emu.TOP - 0x400
    args = [TGT, weapon, 1, 0, 0, 0]
    for a in reversed(args):
        esp -= 4
        mu.mem_write(esp, struct.pack("<I", a & 0xFFFFFFFF))
    esp -= 4
    mu.mem_write(esp, struct.pack("<I", emu.SENT))
    mu.reg_write(UC_X86_REG_ESP, esp)
    mu.reg_write(UC_X86_REG_ECX, AB)
    mu.reg_write(UC_X86_REG_EDX, 0)
    try:
        mu.emu_start(EXEC, emu.SENT, timeout=5_000_000, count=2_000_000)
    except Exception as exc:  # unicorn raises on bad memory access; report it
        STATE["trace"].append(("exception", str(exc), hex(mu.reg_read(UC_X86_REG_EIP))))
    return {
        "weapon_type": weapon_type_value,
        "atk_type": atk_type_value,
        "writes": list(WRITES),
        "target_called": STATE["target_called"],
        "row_at_target": STATE["row_at_target"],
        "record": STATE["record"],
        "trace": STATE["trace"][-4:],
    }


if __name__ == "__main__":
    import json
    import subprocess

    if len(sys.argv) > 3 and sys.argv[1] == "--one":
        # one (weapTp, atkType) pair per process: the emulator state of one run must not leak into the next
        print(json.dumps(run(int(sys.argv[2]), int(sys.argv[3]))))
    else:
        for wt in range(1, 9):
            for at in range(0, 7):
                out = subprocess.run([sys.executable, "-I", os.path.abspath(__file__), "--one", str(wt), str(at)],
                                     capture_output=True, text=True, env=os.environ)
                line = out.stdout.strip()
                if not line:
                    print("weapTp %d atkType %d: no output %s" % (wt, at, out.stderr.strip()[-300:]))
                    continue
                r = json.loads(line)
                print("weapTp %d atkType %d: target_called=%s row_at_target=%s writes=%s trace=%s" % (
                    wt, at, r["target_called"], r["row_at_target"], r["writes"], r["trace"][-1:]))
