"""Unicorn checks of the spell cast path of Game.dll 1.26a (docs/spec/abilities-4.md, pass 5).

What this runs (ORIGINAL code, the image of the user's Game.dll):
  1. Class identity: for each base code, the concrete class whose vtable slot 7 (the code getter)
     returns it. AHtb -> CAbilityThunderBolt, AHhb -> CAbilityHolyBolt, Aslo -> CAbilitySlow.
  2. The cast start (vtable slot 0x338 of CAbilityThunderBolt, shared body 0x6F054880) on a fake
     ability with a real level record. The path is 0x6F054880 -> slot 0x340 (0x6F0290E0) -> slot 0x378
     (0x6F074080, the cast start, posts events). Before the events the cost pre-check runs:
     0x6F029080 reads Cost of the current level (slot 0x3D4 -> 0x6F069170 -> 0x6F02E840), converts it
     with the original I2R (0x6F6EF350), adds the value of slot 0x36C (0.0) with the soft add
     (0x6F6EF000) and calls the unit check 0x6F283C70 with that value.
  3. The value that reaches 0x6F283C70 (its first stack argument, read at the function entry) is
     compared with I2R(Cost) computed by the original I2R. Printed as a table for the Rust test.

What it does NOT settle (see the spec): the comparison inside 0x6F283C70 (x87 equality against the
agile value at unit+0xB8 read by 0x6F4773A0; the agile object is not built here, so the run stops in
0x6F03FA30), the payment and the cooldown. Calls that need the agent world are stubbed (event posters,
the fallback owner 0x6F472890, the SEH helper 0x6F155920 returns true). Import stubs and the fs:[0]
replacement are described in the code.

Usage (the user's own Game.dll):
  GAME_DLL=/path/Game.dll python3 tools/emu/abilities4_emu.py
Prints one line per check and a summary; exits non-zero on a mismatch.
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402  (maps Game.dll, applies the start-up constants, provides call() and mu)
import pefile  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
DATA = emu.DATA
BASE = emu.BASE

VT_THUNDERBOLT = 0x6F8F56DC  # CAbilityThunderBolt (code AHtb)
VT_HOLYBOLT = 0x6F8B1B4C     # CAbilityHolyBolt (code AHhb)
VT_SLOW = 0x6F92280C         # CAbilitySlow (code Aslo)
SLOT_CODE = 0x1C             # vtable slot 7 (the code getter)
SLOT_CAST = 0x338            # shared cast body 0x6F054880
F_CAST = 0x6F054880
F_I2R = 0x6F6EF350           # soft integer -> real (EDX = integer, ECX = out pointer)
F_UNIT_CHECK = 0x6F283C70    # unit-side cost check (ECX = unit, arg = cost value)
F_COST_PRE = 0x6F029080      # cost pre-check
F_OWNER = 0x6F472890         # fallback owner (returns the global unit when a pointer is null)
F_EVENT = (0x6F2963F0, 0x6F295AC0, 0x6F295BC0)   # event posters (need the agent world)
F_SEH_HELPER = 0x6F155920    # thiscall(ability), bool; stubbed TRUE here

OBJ = DATA + 0x1000
UNIT = DATA + 0x2000
DEF = DATA + 0x3000
LVL = DATA + 0x3200
TIB_BLK = DATA + 0xB000
TIB_CTX = DATA + 0xB100
TIB_INNER = DATA + 0xB200
STUB_AREA = DATA + 0x8000
IMPORT_AREA = DATA + 0x4000      # 668 import stubs x 16 bytes: 0x4000..0x69C0
CAPTURE = DATA + 0xC000
AHTB = 0x41487462            # 'AHtb' as the code value (big-endian rawcode)
AHHB = 0x41486862
ASLO = 0x41736C6F

_stub_next = [STUB_AREA]


def f2u(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def u2f(x):
    return struct.unpack("<f", struct.pack("<I", x & 0xFFFFFFFF))[0]


def rd(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def stub_ret(nbytes, eax=0):
    """mov eax, value; ret N. N is the stdcall/thiscall argument cleanup of the callee."""
    a = _stub_next[0]
    code = b"\xB8" + struct.pack("<I", eax & 0xFFFFFFFF)
    code += (b"\xC2" + struct.pack("<H", nbytes)) if nbytes else b"\xC3"
    mu.mem_write(a, code)
    _stub_next[0] += 0x20
    return a


def patch_jump(fn, target):
    mu.mem_write(fn, b"\xE9" + struct.pack("<i", target - (fn + 5)))


def patch_seh():
    """The image has no FS base. fs:[0] accesses become absolute accesses to one TIB cell.
    Same lengths: mov eax,fs:[0] (6) -> mov eax,[abs] (6); mov fs:[0],eax (6) -> mov [abs],eax (6);
    mov fs:[0],ecx (7) -> mov [abs],ecx + nop (7)."""
    img = emu.img
    tib = TIB_BLK - 0x100
    mu.mem_write(tib, b"\0" * 16)
    n = 0
    for pat, rep in (
        (b"\x64\xa1\x00\x00\x00\x00", b"\x8b\x05" + struct.pack("<I", tib)),
        (b"\x64\xa3\x00\x00\x00\x00", b"\x89\x05" + struct.pack("<I", tib)),
        (b"\x64\x89\x0d\x00\x00\x00\x00", b"\x89\x0d" + struct.pack("<I", tib) + b"\x90"),
    ):
        i = 0
        while True:
            i = img.find(pat, i)
            if i < 0:
                break
            mu.mem_write(BASE + i, rep)
            i += len(pat)
            n += 1
    return n


ARITY = {
    "TlsGetValue": 4, "TlsSetValue": 8, "TlsFree": 4, "GetLastError": 0, "GetCurrentThreadId": 0,
    "InterlockedIncrement": 4, "InterlockedDecrement": 4, "GlobalFree": 4, "GlobalLock": 4,
    "GlobalUnlock": 4, "GlobalSize": 4, "GlobalAlloc": 8, "ReleaseMutex": 4, "VirtualFree": 12,
    "VirtualAlloc": 16, "VirtualProtect": 16, "CreateSemaphoreA": 16, "OpenMutexA": 12,
    "TerminateProcess": 8, "SetUnhandledExceptionFilter": 4,
}


def fill_imports():
    """The IAT of the file is unresolved (RVAs). Each import gets "xor eax,eax; ret N" (N from ARITY,
    0 otherwise: the return value of an unmodelled import is 0). TlsGetValue returns the TIB block."""
    pe = pefile.PE(emu.PATH, fast_load=False)
    slot = IMPORT_AREA
    for ent in pe.DIRECTORY_ENTRY_IMPORT:
        for imp in ent.imports:
            name = imp.name.decode() if imp.name else "ord%d" % imp.ordinal
            n = ARITY.get(name, 0)
            if name == "TlsGetValue":
                code = b"\xb8" + struct.pack("<I", TIB_BLK) + b"\xc2\x04\x00"
            else:
                code = b"\x31\xc0" + ((b"\xc2" + struct.pack("<H", n)) if n else b"\xc3")
            mu.mem_write(slot, code)
            mu.mem_write(imp.address, struct.pack("<I", slot))
            slot += 0x10


def setup_context():
    wr(TIB_BLK + 13 * 4, TIB_CTX)       # TLS table entry 13 = the per-thread context
    wr(TIB_CTX + 0x10, TIB_INNER)
    wr(TIB_INNER, 0)                    # flags byte 0x20 clear


def setup_stubs():
    patch_seh()
    fill_imports()
    setup_context()
    for fn in F_EVENT:
        patch_jump(fn, stub_ret(0, 0))
    patch_jump(F_OWNER, stub_ret(0, UNIT))
    patch_jump(F_SEH_HELPER, stub_ret(0, 1))


def make_ability(vtable, code, cost_level0):
    """Fake ability: real vtable, raw code at +0x34, definition at +0x54 (loaded, one level),
    current level index 0 at +0x50, owner unit at +0x30."""
    mu.mem_write(OBJ, b"\0" * 0x400)
    mu.mem_write(UNIT, b"\0" * 0x400)
    mu.mem_write(DEF, b"\0" * 0x100)
    mu.mem_write(LVL, b"\0" * 0x68 * 4)
    wr(OBJ, vtable)
    wr(OBJ + 0x20, 0)
    wr(OBJ + 0x30, UNIT)
    wr(OBJ + 0x34, code)
    wr(OBJ + 0x50, 0)
    wr(OBJ + 0x54, DEF)
    wr(DEF + 0x2C, 1)
    wr(DEF + 0x50, 1)
    wr(DEF + 0x54, LVL)
    wr(LVL + 0x10, cost_level0)         # Cost (integer, level record +0x10)
    wr(LVL + 0x14, f2u(5.0))            # Cool (real, +0x14)


class Capture:
    """Reads the first stack argument of 0x6F283C70 at its entry (the cost value)."""

    def __init__(self):
        self.value = None
        self.hook = None

    def arm(self):
        self.value = None

        def cb(uc, addr, size, user):
            if addr == F_UNIT_CHECK and self.value is None:
                esp = uc.reg_read(UC_X86_REG_ESP)
                # The caller pushes the soft-add result POINTER (0x6F6EF000 returns its out pointer),
                # so the value is the float stored there.
                ptr = struct.unpack("<I", bytes(uc.mem_read(esp + 4, 4)))[0]
                self.ptr = ptr
                self.value = struct.unpack("<I", bytes(uc.mem_read(ptr, 4)))[0]
                uc.emu_stop()

        self.hook = mu.hook_add(UC_HOOK_CODE, cb, begin=F_UNIT_CHECK, end=F_UNIT_CHECK)

    def disarm(self):
        if self.hook is not None:
            mu.hook_del(self.hook)
            self.hook = None


def reference_i2r(n):
    """The original's I2R, run on the emulator (EDX = integer, ECX = output pointer)."""
    out = CAPTURE + 0x100
    wr(out, 0)
    emu.call(F_I2R, ecx=out, edx=n & 0xFFFFFFFF)
    return rd(out)


def class_identity_checks(results):
    """Calls vtable slot 7 of each class and compares with the code. The thiscall takes no args."""
    for name, vt, code in (
        ("CAbilityThunderBolt", VT_THUNDERBOLT, AHTB),
        ("CAbilityHolyBolt", VT_HOLYBOLT, AHHB),
        ("CAbilitySlow", VT_SLOW, ASLO),
    ):
        fn = rd(vt + SLOT_CODE)
        got = emu.call(fn, ecx=OBJ)
        results.append(("class %s (vtable %#x) slot 7 = %#010x" % (name, vt, code), got == code))


def cast_value_checks(results):
    """Runs the cast start for several Cost values and compares the value that reaches 0x6F283C70
    with the original I2R of Cost. The cast runs to the unit check and then stops (capture)."""
    cap = Capture()
    costs = [0, 1, 75, 1234567, -5, 16777219, 2147483647, -2147483648]
    for cost in costs:
        make_ability(VT_THUNDERBOLT, AHTB, cost & 0xFFFFFFFF)
        cap.arm()
        try:
            emu.call(F_CAST, ecx=OBJ)
        except RuntimeError:
            pass   # the capture stops the run inside the unit check (expected)
        except Exception:
            pass
        cap.disarm()
        want = reference_i2r(cost)
        got = cap.value
        ok = got is not None and (got == want or (got == 0x80000000 and want == 0x80000000))
        results.append(("cast cost=%d unit-check value=%s want I2R=%#010x got=%s" % (
            cost, "none" if got is None else "%#010x" % got, want,
            "none" if got is None else "%#010x" % got), ok))
        if got is not None:
            print("  cost %11d: value %#010x (%g)  original I2R %#010x" % (
                cost, got, u2f(got), want))
    return costs


def main():
    setup_stubs()
    results = []
    class_identity_checks(results)
    cast_value_checks(results)
    failures = 0
    for label, ok in results:
        print("%-72s %s" % (label, "ok" if ok else "MISMATCH"))
        if not ok:
            failures += 1
    print("checks=%d failures=%d" % (len(results), failures))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
