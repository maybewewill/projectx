"""Unicorn checks for the buff timed-life chain of Game.dll 1.26a (docs/spec/abilities-2.md).

Runs the original code of:
  * CBuffTimedLife::vf200 (0x6F0D4CB0): the duration branch (start the expiry timer or not);
  * the timed-life message handler (0x6F0D3130): expiry message 0xD01C4 removes the buff,
    other messages go to 0x6F0D1D20.

Callees that are not under test are replaced in the emulated image by recorder stubs. A
recorder stub writes its slot id and its first two stack arguments into a log, then returns
with the callee's own stack cleanup. Virtual calls go through fake vtables that point at the
recorders, so the test sees which virtual slot the code calls and with which arguments.

Usage (the user's own Game.dll):
  GAME_DLL=/path/Game.dll python3 tools/emu/abilities2_emu.py
Prints one line per case and a summary; exits non-zero on a mismatch with the model.
"""
import math
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402  (maps Game.dll, defines call(), mu, DATA, STACK)
from unicorn.x86_const import UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
call = emu.call
DATA = emu.DATA

F_ZERO = 0x6FAAE470          # runtime-filled constant 0.0 (zero in the file)
VX200 = 0x6F0D4CB0           # CBuffTimedLife::vf200 (apply)
HANDLER = 0x6F0D3130         # timed-life message handler (removal on 0xD01C4)
FN_OWNER = 0x6F0D1C40        # owner lookup called by vf200
FN_ATTACH = 0x6F0214A0       # called by vf200 after the owner
FN_REMOVE = 0x6F079CC0       # buff removal (ability list unlink, notify)
FN_EXPIRE_OTHER = 0x6F0D1D20 # handler for messages other than 0xD01C4
FN_FALLBACK = 0x6F472890     # fallback owner used by the handler when +0x30 is zero
MSG_EXPIRE = 0xD01C4

# Slots of the timed-life class that vf200 and the handler call through (vtable index = offset / 4).
SLOT_VF208 = 0x340 // 4      # called by vf200 with no argument (init hook)
SLOT_VF211 = 0x34C // 4      # timer start: (duration pointer, flag)
SLOT_VF59 = 0xEC // 4        # pause/resume hook on the non-positive branch (no argument)
SLOT_FM0 = 0                 # FloatMini::vf0 (value setter), called with (pointer, 1)

LOG = DATA + 0x400           # log: [slot id, arg1, arg2] per recorder, 3 dwords each
STUBS = DATA + 0x1000        # recorder stubs, 0x40 bytes each
VT_BUFF = DATA + 0x2000      # fake vtable of the buff (slots 0..255 used)
VT_FM = DATA + 0x2800        # fake vtable of the embedded FloatMini sub-objects
OBJ = DATA + 0x3000          # the buff object
DUR = DATA + 0x3400          # the duration float cell
UNIT = DATA + 0x3500         # a dummy unit pointer
MSG = DATA + 0x3600          # a dummy message object: [0] unused, [8] = message id
RET_NOP = DATA + 0x3700      # a bare "ret" (used for unused slots)

ARG_STUB_CLEAN = {
    # slot name -> number of stack bytes the callee removes (ret N)
    "vf208": 0,
    "vf211": 8,
    "vf59": 0,
    "fm0": 8,
    "owner": 8,
    "attach": 4,
    "remove": 4,
    "expire_other": 4,
}
_stub_index = {}


def _stub(name, slot_id):
    """Emits a recorder for one slot: mov [LOG+12k], id; mov eax,[esp+4]; mov [LOG+12k+4], eax;
    mov eax,[esp+8]; mov [LOG+12k+8], eax; ret N."""
    k = len(_stub_index)
    _stub_index[name] = k
    base = STUBS + 0x40 * k
    log = LOG + 12 * k
    code = b""
    code += b"\xC7\x05" + struct.pack("<II", log, slot_id)          # mov dword [log], id
    code += b"\x8B\x44\x24\x04"                                      # mov eax, [esp+4]
    code += b"\xA3" + struct.pack("<I", log + 4)                     # mov [log+4], eax
    code += b"\x8B\x44\x24\x08"                                      # mov eax, [esp+8]
    code += b"\xA3" + struct.pack("<I", log + 8)                     # mov [log+8], eax
    n = ARG_STUB_CLEAN[name]
    code += b"\xC2" + struct.pack("<H", n) if n else b"\xC3"         # ret N
    mu.mem_write(base, code)
    return base, log


def _log(name):
    k = _stub_index[name]
    return struct.unpack("<III", bytes(mu.mem_read(LOG + 12 * k, 12)))


def _clear_logs():
    mu.mem_write(LOG, b"\0" * (12 * 16))


def _f(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def setup():
    # Stubs for the callees that are not under test (patched in the emulated image only).
    fm0, _ = _stub("fm0", 0x10)
    vf208, _ = _stub("vf208", SLOT_VF208)
    vf211, _ = _stub("vf211", SLOT_VF211)
    vf59, _ = _stub("vf59", SLOT_VF59)
    owner, _ = _stub("owner", 0x20)
    attach, _ = _stub("attach", 0x21)
    remove, _ = _stub("remove", 0x22)
    other, _ = _stub("expire_other", 0x23)
    # Patch the original's callees (the image is only changed inside this emulator).
    mu.mem_write(FN_OWNER, b"\xB8\x00\x00\x00\x00" + b"\xC2\x08\x00")  # mov eax,0; ret 8 (no owner; args unit, 0)
    mu.mem_write(FN_ATTACH, b"\xE9" + struct.pack("<i", attach - (FN_ATTACH + 5)))  # jmp recorder
    mu.mem_write(FN_REMOVE, b"\xE9" + struct.pack("<i", remove - (FN_REMOVE + 5)))
    mu.mem_write(FN_EXPIRE_OTHER, b"\xE9" + struct.pack("<i", other - (FN_EXPIRE_OTHER + 5)))
    # 0x6F472890 is the "current player" fallback used when the buff has no unit (+0x30 == 0).
    mu.mem_write(FN_FALLBACK, b"\xB8" + struct.pack("<I", UNIT) + b"\xC3")  # mov eax, UNIT; ret
    # Fake vtables: every slot defaults to a bare return, then the slots under test are set.
    mu.mem_write(RET_NOP, b"\xC3")
    vt = struct.pack("<I", RET_NOP) * 256
    mu.mem_write(VT_BUFF, vt)
    mu.mem_write(VT_BUFF + 4 * SLOT_VF208, struct.pack("<I", vf208))
    mu.mem_write(VT_BUFF + 4 * SLOT_VF211, struct.pack("<I", vf211))
    mu.mem_write(VT_BUFF + 4 * SLOT_VF59, struct.pack("<I", vf59))
    mu.mem_write(VT_FM, struct.pack("<I", fm0) + struct.pack("<I", RET_NOP) * 255)


def make_object(flags=0):
    mu.mem_write(OBJ, b"\0" * 0x300)
    mu.mem_write(OBJ, struct.pack("<I", VT_BUFF))
    mu.mem_write(OBJ + 0x20, struct.pack("<I", flags))
    # Embedded FloatMini sub-objects at +0xA0, +0xA8 and +0xCC: each starts with its vtable pointer.
    for off in (0xA0, 0xA8, 0xCC):
        mu.mem_write(OBJ + off, struct.pack("<I", VT_FM))


def run_apply(duration_bits, flag):
    """vf200(this=OBJ, unit, duration pointer, flag). Returns (flags, calls) where calls is the
    list of (slot name, arg1, arg2) recorded, in order of the log slots that fired."""
    setup_done = getattr(run_apply, "_setup", False)
    if not setup_done:
        setup()
        run_apply._setup = True
    make_object()
    mu.mem_write(DUR, struct.pack("<I", duration_bits))
    _clear_logs()
    emu_call(VX200, OBJ, [UNIT, DUR, flag])
    flags = struct.unpack("<I", bytes(mu.mem_read(OBJ + 0x20, 4)))[0]
    calls = fired_calls()
    return flags, calls


def emu_call(fn, ecx, stack_args):
    call(fn, ecx=ecx, stack_args=stack_args)


def fired_calls():
    out = []
    names = sorted(_stub_index, key=lambda n: _stub_index[n])
    for name in names:
        ident, a1, a2 = _log(name)
        # A recorder that did not run keeps the zero it was cleared to; ids are non-zero.
        if ident != 0:
            out.append((name, a1, a2))
    return out


def run_handler(msg_id):
    setup_done = getattr(run_apply, "_setup", False)
    if not setup_done:
        setup()
        run_apply._setup = True
    make_object()
    mu.mem_write(MSG + 8, struct.pack("<I", msg_id))
    _clear_logs()
    emu_call(HANDLER, OBJ, [MSG])
    return fired_calls()


def model_apply(duration_bits, flag):
    """Expected calls and flag word of vf200 (CONFIRMED from the disassembly, checked here).

    Always: the owner helper (ret 8, args unit and 0), slot 208 (init hook), the attach helper.
    duration > 0.0 (ordered compare; NaN, -0.0 and negatives are not greater): slot 211 starts the
    expiry timer and flag 0x80 is set. Otherwise: both FloatMini sub-objects are set to 0.0 and
    slot 59 is called, and no timer starts. The argument flag (3rd) == 0 sets 0x01000000.
    """
    d = struct.unpack("<f", struct.pack("<I", duration_bits))[0]
    calls = ["vf208", "attach"]
    flags = 0
    if d > 0.0:
        calls.append("vf211")
        flags |= 0x80
    else:
        calls += ["fm0", "vf59"]
    if flag == 0:
        flags |= 0x01000000
    return sorted(calls), flags


def main():
    durations = {
        "1.0": _f(1.0),
        "0.5": _f(0.5),
        "1e-38": _f(1e-38),
        "denormal": 0x00000001,
        "0.0": _f(0.0),
        "-0.0": 0x80000000,
        "-1.0": _f(-1.0),
        "-inf": _f(float("-inf")),
        "+inf": _f(float("inf")),
        "nan": 0x7FC00000,
        "3.4e38": _f(3.4e38),
    }
    failures = 0
    cases = 0
    for label, bits in durations.items():
        for flag in (0, 1):
            flags, calls = run_apply(bits, flag)
            names = sorted(c[0] for c in calls)
            want_calls, want_flags = model_apply(bits, flag)
            ok = names == want_calls and (flags & 0x01000080) == want_flags
            cases += 1
            if not ok:
                failures += 1
            print("apply duration=%-9s flag=%d calls=%s flags=%08x %s" % (
                label, flag, names, flags, "ok" if ok else "MISMATCH want %s %08x" % (want_calls, want_flags)))
    for msg, label in ((MSG_EXPIRE, "0xD01C4"), (0xD01A1, "0xD01A1"), (0xD01A7, "0xD01A7")):
        calls = run_handler(msg)
        names = [c[0] for c in calls]
        want = ["remove"] if msg == MSG_EXPIRE else ["expire_other"]
        ok = names == want
        cases += 1
        if not ok:
            failures += 1
        print("handler msg=%s calls=%s %s" % (label, names, "ok" if ok else "MISMATCH want %s" % want))
    print("cases=%d failures=%d" % (cases, failures))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
