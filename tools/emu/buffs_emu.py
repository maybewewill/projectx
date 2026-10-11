"""Unicorn checks of the buff / aura decision code of Game.dll 1.26a (docs/spec/buffs.md).

Routines run for real:
  - the three strength comparisons used by the aura refresh:
        0x6F07C450  "the new value exceeds the stored one by the tolerance"
        0x6F0D2290  "the stored value exceeds the new one by the tolerance"
        0x6F0D2380  "the two values are within the tolerance"
    (fastcall: ECX = pointer to the new value, EDX = pointer to a record whose dword at +4 is the stored value);
  - the aura buff refresh decision CBuffAura::vf212 0x6F0D5070 (callees recorded);
  - the set-duration rule CBuff::vf212 0x6F0D4EF0 (callees recorded);
  - the scan period: soft DIV(I2R(2), 4.0) at 0x6F6EFE10 (the value the aura proximity listener gets).

Part 1 (compare): random and edge (new, stored) pairs; the output file holds one line per case
    `fn  new_bits  stored_bits  result`
for `crates/sim/tests/buff_vectors.rs` (gated by BUFF_VECTORS; the values are not committed).
Part 2 (refresh): for random (new, stored, slot-215 answer) the recorded callee sequence is written as
    `refresh  new_bits  stored_bits  slot215  setdur,store`
Part 3 (set duration): for random (duration, remaining) the recorded calls are written as
    `setdur  dur_bits  remaining_bits  calls  flag80`

Usage (the user's own Game.dll; output stays outside the repository; one mode per process because the
refresh mode patches the set-duration routine):
    GAME_DLL=/path/Game.dll python tools/emu/buffs_emu.py compare|refresh|setdur out.tsv [cases]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402  (maps Game.dll, applies the start-up values)

mu = emu.mu
DATA = emu.DATA

CMP_NEW_GT = 0x6F07C450  # stored < new, |new - stored| >= 0.001
CMP_STORED_GT = 0x6F0D2290  # stored > new, |new - stored| >= 0.001
CMP_EQUAL = 0x6F0D2380  # |new - stored| < 0.001
REFRESH = 0x6F0D5070
SETDUR = 0x6F0D4EF0
DIV = 0x6F6EFE10
I2R = 0x6F6EF350
CANCEL = 0x6F477D30

PAIR = DATA + 0x1000  # new value cell
REC = DATA + 0x1010  # record with the stored value at +4
LOG = DATA + 0x2000
RETCELL = DATA + 0x2800
STUBS = DATA + 0x3000
VT = DATA + 0x4000
OBJ = DATA + 0x5000
TIMER_VT = DATA + 0x6000
DUR = DATA + 0x6100
REMAIN = DATA + 0x6110
RET_NOP = DATA + 0x6200

F32 = lambda x: struct.unpack("<I", struct.pack("<f", x))[0]  # noqa: E731


def _fl(bits):
    return struct.unpack("<f", struct.pack("<I", bits))[0]


def run_cmp(fn, new_bits, stored_bits):
    mu.mem_write(PAIR, struct.pack("<I", new_bits))
    mu.mem_write(REC, struct.pack("<II", 0, stored_bits))
    return emu.call(fn, ecx=PAIR, edx=REC) & 0xFF


# --- recorder stubs -------------------------------------------------------------------------------
_stubs = {}


def stub(name, ident, clean=0, ret=0):
    """A recorder: logs `ident` (+ first two stack args) at LOG+16*k, returns `ret` in EAX, `ret N`."""
    k = len(_stubs)
    base = STUBS + 0x40 * k
    log = LOG + 16 * k
    code = b""
    code += b"\xC7\x05" + struct.pack("<II", log, ident)
    code += b"\x8B\x44\x24\x04" + b"\xA3" + struct.pack("<I", log + 4)
    code += b"\x8B\x44\x24\x08" + b"\xA3" + struct.pack("<I", log + 8)
    code += bytes([0xA1]) + struct.pack("<I", RETCELL + 4 * k)  # mov eax, [cell]
    mu.mem_write(RETCELL + 4 * k, struct.pack("<I", ret))
    code += (b"\xC2" + struct.pack("<H", clean)) if clean else b"\xC3"
    mu.mem_write(base, code)
    _stubs[name] = (k, base, log)
    return base


def clear_logs():
    mu.mem_write(LOG, b"\0" * 0x400)


def fired():
    out = []
    for name, (k, _, log) in sorted(_stubs.items(), key=lambda kv: kv[1][0]):
        ident = struct.unpack("<I", bytes(mu.mem_read(log, 4)))[0]
        if ident:
            out.append(name)
    return out


def set_ret(name, value):
    k, base, _ = _stubs[name]
    # the stub loads its answer from a data cell (patching code would be missed by Unicorn's translation cache)
    mu.mem_write(RETCELL + 4 * k, struct.pack("<I", value))


def setup_refresh_world():
    mu.mem_write(RET_NOP, b"\xC3")
    mu.mem_write(VT, struct.pack("<I", RET_NOP) * 256)
    slot215 = stub("slot215", 1)  # vf215: answer set per case
    slot218 = stub("slot218", 2, clean=4)  # vf218 (store the new value), arg pointer
    setdur = stub("setdur", 3, clean=8)  # 0x6F0D4EF0: (unit, duration pointer)
    mu.mem_write(VT + 0x35C, struct.pack("<I", slot215))
    mu.mem_write(VT + 0x368, struct.pack("<I", slot218))
    mu.mem_write(REFRESH_SETDUR_ADDR, b"\xE9" + struct.pack("<i", setdur - (REFRESH_SETDUR_ADDR + 5)))


REFRESH_SETDUR_ADDR = SETDUR


def run_refresh(new_bits, stored_bits, s215):
    mu.mem_write(OBJ, b"\0" * 0x200)
    mu.mem_write(OBJ, struct.pack("<I", VT))
    mu.mem_write(OBJ + 0xCC, struct.pack("<II", 0, stored_bits))  # the stored value cell (+0xD0)
    mu.mem_write(PAIR, struct.pack("<I", new_bits))
    set_ret("slot215", s215)
    clear_logs()
    emu.call(REFRESH, ecx=OBJ, stack_args=[0x11, PAIR])
    return fired()


def run_setdur(dur_bits, remaining_bits):
    """CBuff::vf212: calls slots 109, 114, 201, then the timer holder's slot 6 / 210 / 211."""
    mu.mem_write(OBJ, b"\0" * 0x200)
    mu.mem_write(OBJ, struct.pack("<I", VT))
    mu.mem_write(OBJ + 0x84, struct.pack("<I", TIMER_VT))
    mu.mem_write(DUR, struct.pack("<I", dur_bits))
    mu.mem_write(REMAIN, struct.pack("<I", remaining_bits))
    clear_logs()
    emu.call(SETDUR_REAL, ecx=OBJ, stack_args=[0x11, DUR])
    flags = struct.unpack("<I", bytes(mu.mem_read(OBJ + 0x20, 4)))[0]
    return fired(), flags


SETDUR_REAL = SETDUR


def setup_setdur_world(real_entry):
    """Stubs for the virtual calls of 0x6F0D4EF0 and the timer cancel; the function itself is untouched."""
    mu.mem_write(RET_NOP, b"\xC3")
    mu.mem_write(VT, struct.pack("<I", RET_NOP) * 256)
    s109 = stub("s109", 1)
    s114 = stub("s114", 2)
    s201 = stub("s201", 3, clean=4)
    s210 = stub("s210", 4)
    s211 = stub("s211", 5, clean=8)
    cancel = stub("cancel", 6)
    mu.mem_write(VT + 0x1B4, struct.pack("<I", s109))
    mu.mem_write(VT + 0x1C8, struct.pack("<I", s114))
    mu.mem_write(VT + 0x324, struct.pack("<I", s201))
    mu.mem_write(VT + 0x348, struct.pack("<I", s210))
    mu.mem_write(VT + 0x34C, struct.pack("<I", s211))
    # timer holder: slot 6 (+24) returns a pointer to the remaining time; arg = pointer (stack), ret 4
    getter_code = b"\xB8" + struct.pack("<I", REMAIN) + b"\xC2\x04\x00"
    mu.mem_write(DATA + 0x6300, getter_code)
    mu.mem_write(TIMER_VT, struct.pack("<I", RET_NOP) * 16)
    mu.mem_write(TIMER_VT + 24, struct.pack("<I", DATA + 0x6300))
    mu.mem_write(CANCEL, b"\xE9" + struct.pack("<i", cancel - (CANCEL + 5)))


def tab(parts):
    return chr(9).join(str(p) for p in parts)


def edge_floats():
    base = [0.0, -0.0, 1.0, -1.0, 0.001, -0.001, 0.0009999, 0.0010001, 0.002, 4.0, 1.5, 3.0, 4.5, 0.1, 0.2, 0.3,
            1000.0, 1e-30, 1e30, float("inf"), float("-inf"), float("nan"), 0.75, 2.25, 0.0005, 0.0015]
    return [F32(x) for x in base]


def rand_val(rng):
    k = rng.random()
    if k < 0.3:
        return F32(rng.choice([0.5, 1, 1.5, 2, 2.25, 3, 4, 4.5, 0.75, 0.1, 0.2, 0.3]) * rng.choice([1, 1, -1]))
    if k < 0.6:
        base = rng.choice([0.0, 1.0, 3.0])
        return F32(base + rng.choice([-1, 1]) * rng.random() * rng.choice([0.0009, 0.001, 0.0011, 0.01, 0.1]))
    if k < 0.9:
        return F32(rng.uniform(-10, 10))
    return rng.getrandbits(32)


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "all"
    out_path = sys.argv[2] if len(sys.argv) > 2 else None
    cases = int(sys.argv[3]) if len(sys.argv) > 3 else 1200
    rng = random.Random(0xB0FF)
    lines = []
    pairs = []
    edges = edge_floats()
    pairs = [(a, b) for a in edges for b in edges]
    while len(pairs) < cases:
        pairs.append((rand_val(rng), rand_val(rng)))
    if mode == "compare":
        # the scan period (soft DIV of 2.0 by 4.0)
        mu.mem_write(PAIR, struct.pack("<I", F32(2.0)))
        mu.mem_write(PAIR + 4, struct.pack("<I", F32(4.0)))
        mu.mem_write(PAIR + 16, struct.pack("<I", 0))
        emu.call(DIV, ecx=PAIR + 16, edx=PAIR, stack_args=[PAIR + 4])
        period = struct.unpack("<I", bytes(mu.mem_read(PAIR + 16, 4)))[0]
        print("scan period DIV(2.0, 4.0) = %#010x (%r)" % (period, _fl(period)))
        lines.append(tab(["period", period]))
        n = 0
        for fn, name in ((CMP_NEW_GT, "new_gt"), (CMP_STORED_GT, "stored_gt"), (CMP_EQUAL, "equal")):
            for new, stored in pairs:
                r = run_cmp(fn, new, stored)
                lines.append(tab([name, new, stored, r]))
                n += 1
        print("compare cases: %d" % n)
    elif mode == "refresh":
        setup_refresh_world()
        n = 0
        for new, stored in pairs:
            for s215 in (0, 1):
                seq = run_refresh(new, stored, s215)
                lines.append(tab(["refresh", new, stored, s215, ",".join(seq)]))
                n += 1
        print("refresh cases: %d" % n)
    elif mode == "setdur":
        setup_setdur_world(SETDUR)
        n = 0
        durs = [F32(x) for x in (0.0, -0.0, -1.0, 0.5, 1.0, 4.0, 3.9999, 4.0001, float("inf"), float("nan"), 1e-30)]
        combos = [(d, r) for d in durs for r in durs]
        while len(combos) < cases:
            combos.append((rand_val(rng), rand_val(rng)))
        for d, r in combos:
            calls, flags = run_setdur(d, r)
            lines.append(tab(["setdur", d, r, ",".join(calls), 1 if flags & 0x80 else 0]))
            n += 1
        print("setdur cases: %d" % n)
    print("lines=%d" % len(lines))
    if out_path:
        with open(out_path, "w") as fh:
            fh.write(chr(10).join(lines) + chr(10))


if __name__ == "__main__":
    main()
