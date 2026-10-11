"""Unicorn run of the basic-attack execute with the real evasion check (Game.dll 1.26a).

Builds on tools/emu/execute_row_emu.py (same stubs and object layout). The miss gate `0x6F2BEAC0` and its helpers run
for real (`0x6F2BEA20`, `0x6F2BB960`, the best-of walks `0x6F077420` / `0x6F077360`), so do the evasion routines in
the fake ability objects (score `0x6F08BDA0`, EvadeAttack `0x6F08BD20`, chance `0x6F02E9E0`).

The ability iterator `0x6F4786B0` (through `0x6F03FA30`, whose lookup is NOT decoded: the walk overwrites EDX with the
unit's mask AND before the call, see the spec, section 12.3) is MODELLED here as returning every fake entry in turn
(the walk's index is reset at each walk). So this run shows the consultation point and its effects, not which unit's
entries the original would find. The owner field (+0x18) of each entry is written but not used by the model.

Cases (RNG stream 2 draw r = 0.5 unless noted); "owned by" names the owner field of the entry only:
  A  one entry owned by the attacker (ECX of the gate), chance 1.0
  B  one entry owned by the target, chance 1.0
  C  one entry owned by the target, chance 0.0 (must not evade: no draw)
  D  one entry owned by the target, chance 0.25, r 0.5 (must not evade: 0.25 > 0.5 is false)
  E  one entry per owner, chance 1.0 (the first one evades)
For each: whether the target damage routine (vtable slot 0x120) was entered, whether the miss text `0x6F2BB970` ran,
whether the evasion effect `0x6F052FB0` ran (and on which object), the stream index passed to the RNG helper, and the
order of the events (the dice draw `0x6F0C6940`, the gate, the walks, the draw, the effect).

Usage: GAME_DLL=/path/to/Game.dll PYTHONPATH=<unicorn> python3 tools/emu/execute_evade_emu.py
"""
import json
import os
import struct
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import execute_row_emu as X  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_EAX,
    UC_X86_REG_ECX,
    UC_X86_REG_EDX,
    UC_X86_REG_EIP,
    UC_X86_REG_ESP,
)

mu = X.mu
EXEC = X.EXEC
GET_AMOUNT = X.GET_AMOUNT
GATE = 0x6F2BEAC0  # miss gate (__fastcall ECX = unit, EDX = target, two stack arguments)
GATE_A = 0x6F2BEA20  # helper of the gate, runs the 0x158/0x164 walk
WALK_A = 0x6F077360  # best-of walk: scores 0x158, executes 0x164
WALK_B = 0x6F077420  # best-of walk: scores 0x15C, executes 0x168 (EvadeAttack)
ITER = 0x6F4786B0  # ability iterator (no arguments)
RNG = 0x6F2853D0  # RNG helper: ECX = output buffer, EDX = stream
EFFECT = 0x6F052FB0  # evasion effect (ECX = ability)
MISS_TEXT = 0x6F2BB970  # miss notice (ECX = unit)
EVADE = 0x6F08BD20
SCORE = 0x6F08BDA0
CHANCE = 0x6F02E9E0
S8450 = 0x6F038450

RVAL = X.emu.DATA + 0x9000
LIST_OBJ = X.emu.DATA + 0x9100  # ability objects, 0x100 bytes apart
VT_E = X.emu.DATA + 0x9400  # evasion vtable: real score 0x15C, real EvadeAttack 0x168
VT_N = X.emu.DATA + 0x9600  # neutral vtable (0x158/0x164 score and execute do nothing)
DEFN = X.emu.DATA + 0x9800
TBLN = X.emu.DATA + 0x9A00
STUB_FLAG = X.emu.DATA + 0x9C20  # slot 0x268 (enabled): returns 1
STUB_SCORE0 = X.emu.DATA + 0x9C40  # slots 0x158: writes 0.0 to *out
STUB_EXEC0 = X.emu.DATA + 0x9C60  # slots 0x164: nothing

STATE = {"entries": [], "cur_owner": None, "idx": 0, "rng_stream": None, "rng_calls": 0, "order": [],
         "effect_on": [], "miss": 0}
FS_SKIPS = []


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


def f32(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def fs_scan(lo, size):
    """Instructions that touch fs:[0] in [lo, lo+size): mov eax,fs:[0] (6), mov fs:[0],eax (6), mov fs:[0],ecx (7)."""
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


def on_code(uc, eip, size, user):
    for addr, length, reads in FS_SKIPS:
        if eip == addr:
            if reads:
                mu.reg_write(UC_X86_REG_EAX, 0)
            mu.reg_write(UC_X86_REG_EIP, addr + length)
            return
    if eip == WALK_A or eip == WALK_B:
        STATE["idx"] = 0
        STATE["order"].append("walk@%x(ecx=%x,edx=%x)" % (eip, mu.reg_read(UC_X86_REG_ECX),
                                                          mu.reg_read(UC_X86_REG_EDX)))
        return  # the real walk runs
    if eip == ITER:
        # MODEL, not the binary's lookup: the real 0x6F03FA30 keys on (+0x18 == EDX) and the walk's guard overwrites
        # EDX with the unit's mask AND before the call, so the owner that finds an entry is not known (see the spec,
        # section 12). This stub returns the entries of both groups, so the consultation point itself is exercised.
        owner = mu.reg_read(UC_X86_REG_EDX)
        items = [obj for obj, _own in STATE["entries"]]
        i = STATE["idx"]
        STATE["idx"] = i + 1
        STATE["order"].append("iter(owner=%x,%d)" % (owner, i))
        mu.reg_write(UC_X86_REG_EAX, items[i] if i < len(items) else 0)
        ret_to(0)
        return
    if eip == RNG:
        buf = mu.reg_read(UC_X86_REG_ECX)
        STATE["rng_stream"] = mu.reg_read(UC_X86_REG_EDX)
        STATE["rng_calls"] += 1
        STATE["order"].append("draw(stream=%d)" % STATE["rng_stream"])
        wr32(buf, rd32(RVAL))
        mu.reg_write(UC_X86_REG_EAX, buf)
        ret_to(0)
        return
    if eip == EFFECT:
        STATE["effect_on"].append(hex(mu.reg_read(UC_X86_REG_ECX)))
        STATE["order"].append("evade-effect")
        mu.reg_write(UC_X86_REG_EAX, 0)
        ret_to(0)
        return
    if eip == MISS_TEXT:
        STATE["miss"] += 1
        STATE["order"].append("miss-text")
        mu.reg_write(UC_X86_REG_EAX, 0)
        ret_to(0)
        return
    if eip == GET_AMOUNT:
        STATE["order"].append("dice")
    if eip == GATE:
        STATE["order"].append("gate(ecx=%x,edx=%x)" % (mu.reg_read(UC_X86_REG_ECX), mu.reg_read(UC_X86_REG_EDX)))
    if eip == S8450:
        ret_addr = rd32(mu.reg_read(UC_X86_REG_ESP))
        if X.EXEC <= ret_addr < X.EXEC + 0x800:
            X.on_code(uc, eip, size, user)  # the execute's zeroing stub
            return
        mu.reg_write(UC_X86_REG_EAX, 0)
        ret_to(4)
        return
    if eip == STUB_SCORE0:
        # neutral score slot: __thiscall (out, param, float), writes 0.0 to *out, ret 0xC
        wr32(arg(1), 0)
        mu.reg_write(UC_X86_REG_EAX, 0)
        ret_to(12)
        return
    if eip == STUB_EXEC0:
        # neutral execute slot: (param, float), nothing, ret 8
        mu.reg_write(UC_X86_REG_EAX, 0)
        ret_to(8)
        return
    if eip == STUB_FLAG:
        # slot 0x268 (enabled): __thiscall with one argument, returns 1
        mu.reg_write(UC_X86_REG_EAX, 1)
        ret_to(4)
        return
    X.on_code(uc, eip, size, user)


def setup_vtables():
    for a in (STUB_FLAG, STUB_SCORE0, STUB_EXEC0):
        mu.mem_write(a, b"\xc3")
    for a in (VT_E, VT_N):
        mu.mem_write(a, b"\0" * 0x400)
    wr32(VT_N + 0x158, STUB_SCORE0)
    wr32(VT_N + 0x15C, STUB_SCORE0)
    wr32(VT_N + 0x164, STUB_EXEC0)
    wr32(VT_N + 0x168, STUB_EXEC0)
    wr32(VT_N + 0x268, STUB_FLAG)
    wr32(VT_E + 0x158, STUB_SCORE0)
    wr32(VT_E + 0x164, STUB_EXEC0)
    wr32(VT_E + 0x15C, SCORE)
    wr32(VT_E + 0x168, EVADE)
    wr32(VT_E + 0x268, STUB_FLAG)


def setup_evasion(addr, chance, owner):
    mu.mem_write(addr, b"\0" * 0x80)
    wr32(addr, VT_E)
    wr32(addr + 0x18, owner)  # owner key read by the iterator
    wr32(addr + 0x3C, 0)  # count gate
    wr32(addr + 0x50, 0)  # level
    wr32(addr + 0x54, DEFN)  # definition
    mu.mem_write(DEFN, b"\0" * 0x100)
    mu.mem_write(TBLN, b"\0" * 0x200)
    wr32(DEFN + 0x2C, 1)
    wr32(DEFN + 0x50, 1)
    wr32(DEFN + 0x54, TBLN)
    wr32(TBLN + 0x20, f32(chance))


def run_case(name, attacker_entries, target_entries, chance, r_value):
    mu.mem_write(LIST_OBJ, b"\0" * 0x300)
    # the base setup (execute objects, weapon fields, the target and owner vtables) zeroes the units: it runs first
    X.setup(1, 1, 0)  # weapon: normal (weapTp 1), atkType 1 (normal)
    setup_vtables()
    wr32(RVAL, f32(r_value))
    for unit in (X.OWN, X.TGT):
        wr32(unit + 4, 5)  # reference count (2beac0 releases its own reference)
        wr32(unit + 0x1DC, 0)  # the walk guard (this+0x1E0 & this+0x1DC) != -1 holds for both units
        wr32(unit + 0x1E0, 0)
    wr32(X.AB + 0x1DC, 0xFFFFFFFF)  # the execute's own list is empty
    wr32(X.AB + 0x1E0, 0xFFFFFFFF)
    entries = []
    k = 0
    for owner, n in ((X.OWN, attacker_entries), (X.TGT, target_entries)):
        for _ in range(n):
            entries.append((LIST_OBJ + 0x100 * k, owner))
            k += 1
    for obj, owner in entries:
        setup_evasion(obj, chance, owner)
    # the last entry of each owner's group ends the walk (masks -1 at +0x24/+0x28)
    for owner in (X.OWN, X.TGT):
        group = [obj for obj, own in entries if own == owner]
        if group:
            wr32(group[-1] + 0x24, 0xFFFFFFFF)
            wr32(group[-1] + 0x28, 0xFFFFFFFF)
    STATE.update({"entries": entries, "idx": 0, "rng_stream": None, "rng_calls": 0, "order": [],
                  "effect_on": [], "miss": 0})
    X.STATE["target_called"] = False
    X.STATE["record"] = None
    X.STATE["row_at_target"] = None
    del X.WRITES[:]
    X.STATE["trace"] = []
    # the real gate, its helpers and the walks run: drop the execute-level stubs for them
    for real in (X.FN_BEAC0, X.FN_BB960):
        X.HOOKS.pop(real, None)
    global FS_SKIPS
    FS_SKIPS = fs_scan(GATE_A, 0x300) + fs_scan(GATE, 0x100) + fs_scan(X.EXEC, 0x800)
    mu.hook_add(UC_HOOK_CODE, on_code)
    esp = X.emu.TOP - 0x400
    args = [X.TGT, 0, 0, 0, 0, 0]
    for a in reversed(args):
        esp -= 4
        mu.mem_write(esp, struct.pack("<I", a & 0xFFFFFFFF))
    esp -= 4
    mu.mem_write(esp, struct.pack("<I", X.emu.SENT))
    mu.reg_write(UC_X86_REG_ESP, esp)
    mu.reg_write(UC_X86_REG_ECX, X.AB)
    mu.reg_write(UC_X86_REG_EDX, 0)
    err = None
    try:
        mu.emu_start(EXEC, X.emu.SENT, timeout=5_000_000, count=3_000_000)
    except Exception as exc:  # report unicorn errors with the eip
        err = "%s at %x" % (exc, mu.reg_read(UC_X86_REG_EIP))
    return {
        "case": name,
        "target_damage_called": bool(X.STATE["target_called"]),
        "miss_text": STATE["miss"],
        "evade_effect_on": STATE["effect_on"],
        "rng_stream": STATE["rng_stream"],
        "rng_calls": STATE["rng_calls"],
        "order": STATE["order"][:16],
        "error": err,
    }


def cases():
    return [
        ("A entry owned by attacker, chance 1.0", 1, 0, 1.0, 0.5),
        ("B entry owned by target, chance 1.0", 0, 1, 1.0, 0.5),
        ("C entry owned by target, chance 0.0", 0, 1, 0.0, 0.5),
        ("D entry owned by target, chance 0.25, r 0.5", 0, 1, 0.25, 0.5),
        ("E entries owned by both, chance 1.0", 1, 1, 1.0, 0.5),
    ]


def main():
    if len(sys.argv) > 2 and sys.argv[1] == "--one":
        name, a_n, t_n, chance, r = cases()[int(sys.argv[2])]
        print(json.dumps(run_case(name, a_n, t_n, chance, r)))
        return
    for i in range(len(cases())):
        out = subprocess.run([sys.executable, "-I", os.path.abspath(__file__), "--one", str(i)],
                             capture_output=True, text=True, env=os.environ)
        line = out.stdout.strip()
        if not line:
            print("case %d: no output %s" % (i, out.stderr.strip()[-400:]))
            continue
        print(line)


if __name__ == "__main__":
    main()
