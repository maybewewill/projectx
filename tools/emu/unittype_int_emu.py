"""Unicorn check of the integer cell reader used by the unit-type loader (Game.dll 1.26a, VA 0x6F4CE050).

What it checks (docs/spec/unit-types.md, section 5.2):
- a text cell read as an integer goes through FUN_6F4CE050 (fastcall: EDX = NUL-terminated text);
- text that starts with an apostrophe is a 4-character raw code: up to four bytes until the closing
  apostrophe, packed big-endian (first byte in the high position), NUL stops early;
- any other text is passed to the C library as strtol(text, NULL, 0) (base 0: optional sign, 0x hex,
  leading-zero octal, decimal) and the 32-bit result is returned as an unsigned value.

The import slot of strtol (MSVCR80, IAT 0x6F86D3DC) is pointed at a one-instruction stub (RET) whose
code hook computes the C library result in Python with glibc/MSVC semantics (whitespace skip, sign,
overflow clamped to LONG_MAX/LONG_MIN, parsing stops at the first invalid character). Nothing from the
user's game data is read: the inputs are synthetic strings.

Run:  GAME_DLL=/path/to/Game.dll python3 tools/emu/unittype_int_emu.py
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402  (maps Game.dll and provides call())
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX  # noqa: E402

FUNC = 0x6F4CE050
IAT_STRTOL = 0x6F86D3DC
STUB = 0x00300100          # inside the emu SENT page, one RET byte
TEXT = emu.DATA + 0x2000   # scratch area for the input string

LONG_MAX = 0x7FFFFFFF
LONG_MIN = -0x80000000


def model_strtol(s):
    """C strtol(s, NULL, 0) with 32-bit long, returns the value as an unsigned 32-bit integer."""
    i = 0
    n = len(s)
    while i < n and s[i] in " \t\n\v\f\r":
        i += 1
    neg = False
    if i < n and s[i] in "+-":
        neg = s[i] == "-"
        i += 1
    base = 10
    if i + 1 < n and s[i] == "0" and s[i + 1] in "xX" and i + 2 < n and s[i + 2] in "0123456789abcdefABCDEF":
        base = 16
        i += 2
    elif i < n and s[i] == "0":
        base = 8
    digits = "0123456789abcdef"[:base] if base != 16 else "0123456789abcdefABCDEF"
    val = 0
    overflow = False
    start = i
    while i < n and s[i] in digits:
        d = int(s[i], 16)
        val = val * base + d
        if val > (1 << 40):
            overflow = True
        i += 1
    if i == start:
        return 0
    if neg:
        val = -val
    if overflow or val > LONG_MAX:
        if neg:
            val = LONG_MIN
        else:
            val = LONG_MAX
    if val < LONG_MIN:
        val = LONG_MIN
    return val & 0xFFFFFFFF


def model_cell_int(s):
    """Model of FUN_6F4CE050 on a NUL-free string."""
    if not s.startswith("'"):
        return model_strtol(s)
    v = 0
    count = 4
    i = 1
    while True:
        c = s[i] if i < len(s) else "\0"
        count -= 1
        if c == "\0":
            return v & 0xFFFFFFFF
        if c == "'":
            break
        v = ((v << 8) | ord(c)) & 0xFFFFFFFF
        i += 1
        if count == 0:
            return v
    return v & 0xFFFFFFFF


strtol_args = []


def strtol_hook(uc, address, size, user):
    """Runs when the stub is reached: [esp]=return, [esp+4]=str, [esp+8]=endptr, [esp+12]=base."""
    esp = uc.reg_read(UC_X86_REG_ESP)
    p = struct.unpack("<I", bytes(uc.mem_read(esp + 4, 4)))[0]
    base = struct.unpack("<I", bytes(uc.mem_read(esp + 12, 4)))[0]
    endp = struct.unpack("<I", bytes(uc.mem_read(esp + 8, 4)))[0]
    raw = bytes(uc.mem_read(p, 64))
    text = raw.split(b"\x00")[0].decode("latin-1")
    strtol_args.append((text, base, endp))
    uc.reg_write(UC_X86_REG_EAX, model_strtol(text))


def run():
    emu.mu.mem_write(IAT_STRTOL, struct.pack("<I", STUB))
    emu.mu.mem_write(STUB, b"\xc3")
    emu.mu.hook_add(UC_HOOK_CODE, strtol_hook, begin=STUB, end=STUB)
    cases = [
        "0", "7", "-7", "+7", "  12", "12abc", "abc", "", "0x1f", "0X1F", "-0x10",
        "017", "08", "2147483647", "2147483648", "-2147483648", "-2147483649",
        "4294967296", "0x7fffffffff", "'hfoo'", "'ab'", "'abcdef'", "'a", "'", "0x",
    ]
    bad = 0
    for s in cases:
        emu.mu.mem_write(TEXT, s.encode("latin-1") + b"\x00")
        strtol_args.clear()
        got = emu.call(FUNC, ecx=0, edx=TEXT) & 0xFFFFFFFF
        want = model_cell_int(s)
        ok = got == want
        bad += 0 if ok else 1
        print("%-16r got=%08x want=%08x %s" % (s, got, want, "ok" if ok else "MISMATCH"))
        if strtol_args and strtol_args[0][1] != 0:
            # the call must use base 0 and a NULL end pointer
            print("  unexpected strtol arguments", strtol_args[0])
            bad += 1
    print("cases", len(cases), "mismatches", bad)
    return bad


if __name__ == "__main__":
    sys.exit(1 if run() else 0)
