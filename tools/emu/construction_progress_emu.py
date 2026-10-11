"""Unicorn probe of the construction progress setter of CAbilityBuildInProgress (Game.dll 1.26a).

Runs the original percent handler (vtable slot 191, 0x6F054690, one integer argument p) with a fake
owner unit whose life methods are recorders, and records:
  * the value the handler passes to the owner's slot 0x130 (argument, as float bits),
  * the value it finally passes to the owner's slot 0x124 (life set, as float bits),
  * whether the tick 0x6F045600 is called (it is skipped by setting this+0x48 = 1 in this probe),
  * for p == 100, whether the completion slot 205 (0x6F060020 entry, vtable offset 0x334) is called.

The owner's slot 0x130 is a recorder that returns a pointer to a float cell holding OWN_MAX (a configurable
value), so the set value shows how the owner's answer is combined with the progress.

Usage:  GAME_DLL=/path/Game.dll python3 tools/emu/construction_progress_emu.py [--json out.json]
"""
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402

mu = emu.mu
call = emu.call
DATA = emu.DATA

HANDLER = 0x6F054690         # CAbilityBuildInProgress vf191: percent handler (ECX = this, stack = p)
SET_PCT = 0x6F025BD0         # called first with p (not read; recorded only)
TICK = 0x6F045600            # in-progress tick (skipped in this probe)
FALLBACK = 0x6F472890        # owner fallback (not used: owner is set)
SLOT_LIFE_SET = 0x124 // 4   # owner slot 73: set life (float by pointer)
SLOT_LIFE_CALC = 0x130 // 4  # owner slot 76: value computed from the argument; returns pointer
SLOT_COMPLETE = 0x334 // 4   # object slot 205: completion

OBJ = DATA + 0x5000
OWN = DATA + 0x5400
VT_OBJ = DATA + 0x5800
VT_OWN = DATA + 0x5C00
MAXCELL = DATA + 0x6000      # OWN_MAX cell
ARGCELL = DATA + 0x6010      # cell used by the recorders
LOG = DATA + 0x6100
STUB = DATA + 0x7000
RET = DATA + 0x6F00

_nstub = [0]


def _stub(code_fn):
    base = STUB + 0x40 * _nstub[0]
    _nstub[0] += 1
    mu.mem_write(base, code_fn(base))
    return base


def _log_slot(k, n_args):
    """Recorder: writes ecx, edx, and the first two stack args into LOG[k], returns eax = [MAXCELL] pointer
    for the calc slot, else 0."""
    raise NotImplementedError


def build_stubs():
    global _nstub
    _nstub[0] = 0
    out = {}
    # Generic recorder: store ECX, EDX, [esp+4], [esp+8] into LOG+16*k, then return with eax=eax_value.
    def rec(k, eax_value, ret_bytes):
        log = LOG + 16 * k

        def gen(base):
            c = b""
            c += b"\x89\x0D" + struct.pack("<I", log)                 # mov [log], ecx
            c += b"\x89\x15" + struct.pack("<I", log + 4)             # mov [log+4], edx
            c += b"\x8B\x44\x24\x04" + b"\xA3" + struct.pack("<I", log + 8)  # mov eax,[esp+4]; mov [log+8],eax
            c += b"\x8B\x44\x24\x08" + b"\xA3" + struct.pack("<I", log + 12)  # mov eax,[esp+8]; mov [log+12],eax
            c += b"\xB8" + struct.pack("<I", eax_value)               # mov eax, value
            c += (b"\xC2" + struct.pack("<H", ret_bytes)) if ret_bytes else b"\xC3"
            return c
        return _stub(gen)

    out["setpct"] = rec(0, 0, 4)            # 0x6F025BD0 (thiscall, one stack arg, ret 4)
    out["tick"] = rec(1, 0, 0)              # 0x6F045600 (fastcall, no stack args)
    out["life_set"] = rec(2, 0, 4)          # owner slot 73 (one stack arg: pointer to float)
    out["complete"] = rec(3, 0, 0)          # object slot 205 (no args)
    out["calc"] = rec(4, MAXCELL, 4)        # owner slot 76: one stack arg (out pointer), ret 4; [esp+8] at entry = pending B
    return out


def setup():
    # Runtime constants written by the start-up initialisers (tools/emu/runtime_consts.py).
    mu.mem_write(0x6FAAE4A0, struct.pack("<I", 0x3DCCCCCD))  # 0.1
    mu.mem_write(0x6FAAE4F4, struct.pack("<I", 0x3F800000))  # 1.0
    mu.mem_write(0x6FAAE574, struct.pack("<I", 0x42C80000))  # 100.0
    st = build_stubs()
    # Patch the callees that are not under test, inside the emulated image only.
    mu.mem_write(SET_PCT, b"\xE9" + struct.pack("<i", st["setpct"] - (SET_PCT + 5)))
    mu.mem_write(TICK, b"\xE9" + struct.pack("<i", st["tick"] - (TICK + 5)))
    mu.mem_write(RET, b"\xC3")
    vt_obj = struct.pack("<I", RET) * 256
    mu.mem_write(VT_OBJ, vt_obj)
    mu.mem_write(VT_OBJ + 4 * SLOT_COMPLETE, struct.pack("<I", st["complete"]))
    vt_own = struct.pack("<I", RET) * 256
    mu.mem_write(VT_OWN, vt_own)
    mu.mem_write(VT_OWN + 4 * SLOT_LIFE_SET, struct.pack("<I", st["life_set"]))
    mu.mem_write(VT_OWN + 4 * SLOT_LIFE_CALC, struct.pack("<I", st["calc"]))
    return st


def f2b(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def b2f(b):
    return struct.unpack("<f", struct.pack("<I", b))[0]


def run(p, own_max, gate=0):
    mu.mem_write(LOG, b"\0" * (16 * 8))
    mu.mem_write(OBJ, b"\0" * 0x200)
    mu.mem_write(OBJ, struct.pack("<I", VT_OBJ))
    mu.mem_write(OBJ + 0x30, struct.pack("<I", OWN))
    mu.mem_write(OBJ + 0x48, struct.pack("<I", gate))   # +0x48 > 0 skips the tick and the completion
    mu.mem_write(OBJ + 0x20, struct.pack("<I", 0))
    mu.mem_write(OWN, struct.pack("<I", VT_OWN))
    mu.mem_write(MAXCELL, struct.pack("<I", f2b(own_max)))
    emu.call(HANDLER, ecx=OBJ, stack_args=[p])
    log = lambda k: struct.unpack("<IIII", bytes(mu.mem_read(LOG + 16 * k, 16)))
    setpct = log(0)
    life = log(2)
    calc = log(4)
    complete = log(3)
    tick = log(1)
    return {
        "p": p,
        "own_max": f2b(own_max),
        "setpct_arg": setpct[2],
        "calc_out_ptr": calc[2],       # [esp+4] of the calc call: out pointer (pushed last)
        "calc_in_value": b2f(struct.unpack("<I", bytes(mu.mem_read(calc[3], 4)))[0]) if calc[3] else None,
        "life_set_arg": life[2],       # [esp+4] of the life-set call: pointer to the value
        "life_set_value": b2f(struct.unpack("<I", bytes(mu.mem_read(life[2], 4)))[0]) if life[2] else None,
        "complete_called": complete[0] != 0 or complete[1] != 0 or complete[2] != 0 or complete[3] != 0,
        "tick_called": tick[0] != 0 or tick[1] != 0 or tick[2] != 0 or tick[3] != 0,
    }


def vectors(path):
    """Writes the setter vectors: one line per (factor bits, p): factor, p, life bits (after the setter),
    completion flag (p == 100 with the gate clear), tick flag (p != 100 with the gate clear)."""
    import random
    rnd = random.Random(1)
    factors = [1000.0, 0.0, 1.0, 37.5, 12345.678, 1e7, 3.0e38, 1.0e-40, -250.0]
    factors += [struct.unpack("<f", struct.pack("<I", rnd.getrandbits(32) & 0x7F7FFFFF))[0] for _ in range(6)]
    lines = ["# factor_bits\tp\tlife_bits\tcomplete\ttick"]
    for fac in factors:
        for p in range(0, 101):
            r = run(p, fac, gate=0)
            life = f2b(b2f(0) if r["life_set_value"] is None else r["life_set_value"])
            lines.append("%08x\t%d\t%08x\t%d\t%d" % (f2b(fac), p, life, int(r["complete_called"]), int(r["tick_called"])))
    with open(path, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("vectors:", len(lines) - 1)


def main():
    setup()
    if "--vectors" in sys.argv:
        vectors(sys.argv[sys.argv.index("--vectors") + 1])
        return
    rows = []
    for p in (0, 1, 50, 99, 100):
        rows.append(run(p, 1000.0))
    rows.append(run(100, 1000.0, gate=0))
    rows.append(run(37, 1000.0, gate=0))
    for r in rows:
        print(r)
    if "--json" in sys.argv:
        path = sys.argv[sys.argv.index("--json") + 1]
        with open(path, "w") as fh:
            json.dump(rows, fh, indent=1)


if __name__ == "__main__":
    main()
