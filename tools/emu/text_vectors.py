#!/usr/bin/env python3
"""Differential vectors for the text conversions of Game.dll 1.26a.

Calls the ORIGINAL routines inside Unicorn (the user's own Game.dll, mapped at its image
base; nothing from it is written into the repository) and records the observed pairs:

  * r2s     R2S native body, helper 0x6F6F0360 with width 0 and precision 3 (the native's
            own push order)
  * r2sw    the same helper with an arbitrary width and precision (R2SW passes width=arg2,
            precision=arg3)
  * s2r     S2R helper 0x6F6F0520 (text -> real)
  * lex_real the JASS lexer's real-literal action 0x6F466690 (token text -> real)

    GAME_DLL=/path/to/Game.dll python3 tools/emu/text_vectors.py \
        --n 4000 --seed 1 --out /path/to/text_vectors.json \
        [--embed-rs crates/softfloat/tests/text_embedded.rs]

Output schema (hex strings, no escapes):

    {"format": "softfloat-text-vectors-1", "dll_sha256": "...", "cases": [
        {"fn": "r2sw", "x": "3fc00000", "width": "00000000", "precision": "00000002",
         "text": "<hex of the output bytes>"},
        {"fn": "s2r", "text": "<hex of the input bytes>", "ret": "3fc00000"},
        {"fn": "lex_real", "text": "<hex>", "ret": "3dccccce"}, ...]}

Stubs for imports the helpers call (the image's import address table is not bound by
this loader, so each IAT slot is pointed at a Python implementation):

  * isdigit (MSVCR80, C locale): true for '0'..'9' only.
  * sprintf (MSVCR80): implements the conversions the helpers use (`%d`, `%*d`, `%0*d`
    with C flag/width semantics, including a negative `*` width meaning left-justified).

Inputs for which the original does not return within the instruction budget are skipped.
"""

import argparse
import json
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import softfloat_vectors as sv  # noqa: E402  (the Game loader and the call convention)
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EIP  # noqa: E402

FORMAT = "softfloat-text-vectors-1"

R2S = 0x6F6F0360
S2R = 0x6F6F0520
LEX_REAL = 0x6F466690
IAT_ISDIGIT = 0x6F86D324
IAT_SPRINTF = 0x6F86D32C
IAT_MEMSET = 0x6F86D3E4  # reached through the thunk at 0x6F7E11C2
CONST_FLT_MAX_STORE = 0x6F84E340  # writes 0x7F7FFFFF to 0x6FAAE624 (outside sv.init_constants range)

STUB_BASE = 0x00400000
ISDIGIT_STUB = STUB_BASE + 0x00
SPRINTF_STUB = STUB_BASE + 0x10
MEMSET_STUB = STUB_BASE + 0x20
MEM_BASE = 0x00500000
MEM_SIZE = 0x20000
OUT_BUF = MEM_BASE + 0x0000   # R2S output, 4 KiB
STR_BUF = MEM_BASE + 0x2000   # S2R / lexer input text, 4 KiB
RES_SLOT = MEM_BASE + 0x4000  # real result slot
LEX_OBJ = MEM_BASE + 0x4100   # lexer object: +0x98 text pointer, +0x24 result
OUT_MAX = 4096


def _read_c_string(mu, addr, limit=4096):
    out = bytearray()
    for i in range(limit):
        b = bytes(mu.mem_read(addr + i, 1))[0]
        if b == 0:
            break
        out.append(b)
    return bytes(out)


def _c_format(fmt, args):
    """The printf subset used by the helpers: `%[0][*|width]d`, and literal text."""
    out = []
    ai = 0
    i = 0
    while i < len(fmt):
        ch = fmt[i]
        if ch != "%":
            out.append(ch)
            i += 1
            continue
        i += 1
        zero = False
        if i < len(fmt) and fmt[i] == "0":
            zero = True
            i += 1
        width = 0
        left = False
        if i < len(fmt) and fmt[i] == "*":
            width = args[ai]
            ai += 1
            i += 1
            if width < 0:
                left = True
                width = -width
        else:
            while i < len(fmt) and fmt[i].isdigit():
                width = width * 10 + int(fmt[i])
                i += 1
        conv = fmt[i]
        i += 1
        if conv != "d":
            raise ValueError("unsupported conversion %" + conv)
        value = args[ai]
        ai += 1
        text = str(value)
        if zero and not left and len(text) < width:
            sign = "-" if text.startswith("-") else ""
            digits = text[len(sign):]
            text = sign + "0" * (width - len(text)) + digits
        if len(text) < width:
            text = text.ljust(width) if left else text.rjust(width)
        out.append(text)
    return "".join(out)


def install_import_stubs(game):
    """Maps the stub page, points the IAT slots at the stubs and installs their hooks."""
    mu = game.mu
    mu.mem_map(STUB_BASE, 0x1000)
    mu.mem_write(IAT_ISDIGIT, struct.pack("<I", ISDIGIT_STUB))
    mu.mem_write(IAT_SPRINTF, struct.pack("<I", SPRINTF_STUB))
    mu.mem_write(IAT_MEMSET, struct.pack("<I", MEMSET_STUB))

    def stdcall_args(uc, n):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
        args = [struct.unpack("<i", bytes(uc.mem_read(esp + 4 + 4 * k, 4)))[0] for k in range(n)]
        return esp, ret, args

    def isdigit_hook(uc, address, size, user):
        _, ret, (c,) = stdcall_args(uc, 1)
        uc.reg_write(UC_X86_REG_EAX, 1 if 0x30 <= c <= 0x39 else 0)
        uc.reg_write(UC_X86_REG_ESP, uc.reg_read(UC_X86_REG_ESP) + 4)
        uc.reg_write(UC_X86_REG_EIP, ret)

    def sprintf_hook(uc, address, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
        buf, fmt_ptr = struct.unpack("<II", bytes(uc.mem_read(esp + 4, 8)))
        fmt = _read_c_string(uc, fmt_ptr).decode("ascii")
        # variadic ints follow the fixed arguments; read as many as the format asks for
        nargs = fmt.count("%") + fmt.count("*")
        vals = [struct.unpack("<i", bytes(uc.mem_read(esp + 12 + 4 * k, 4)))[0] for k in range(nargs)]
        text = _c_format(fmt, vals).encode("ascii") + b"\0"
        uc.mem_write(buf, text)
        uc.reg_write(UC_X86_REG_EAX, len(text) - 1)
        uc.reg_write(UC_X86_REG_EIP, ret)
        uc.reg_write(UC_X86_REG_ESP, esp + 4)

    def memset_hook(uc, address, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret, dst, c, n = struct.unpack("<IIII", bytes(uc.mem_read(esp, 16)))
        if n:
            uc.mem_write(dst, bytes([c & 0xFF]) * n)
        uc.reg_write(UC_X86_REG_EAX, dst)
        uc.reg_write(UC_X86_REG_ESP, esp + 4)
        uc.reg_write(UC_X86_REG_EIP, ret)

    mu.hook_add(UC_HOOK_CODE, isdigit_hook, begin=ISDIGIT_STUB, end=ISDIGIT_STUB)
    mu.hook_add(UC_HOOK_CODE, sprintf_hook, begin=SPRINTF_STUB, end=SPRINTF_STUB)
    mu.hook_add(UC_HOOK_CODE, memset_hook, begin=MEMSET_STUB, end=MEMSET_STUB)


class TextGame:
    def __init__(self, path):
        self.game = sv.Game(path)
        self.game.init_constants()
        # the constant 0x6FAAE624 (= FLT_MAX) is written by a stub outside the range above
        self.game.call(CONST_FLT_MAX_STORE, count=100)
        self.mu = self.game.mu
        self.mu.mem_map(MEM_BASE, MEM_SIZE)
        install_import_stubs(self.game)
        self.sha256 = self.game.sha256

    def r2sw(self, x, width, precision):
        mu = self.mu
        mu.mem_write(OUT_BUF, b"\0" * OUT_MAX)
        mu.mem_write(RES_SLOT, struct.pack("<I", x & 0xFFFFFFFF))
        # helper: ECX = pointer to the real, EDX = output buffer, stack = width, precision
        self.game.call(R2S, ecx=RES_SLOT, edx=OUT_BUF,
                       stack=[width & 0xFFFFFFFF, precision & 0xFFFFFFFF])
        return _read_c_string(mu, OUT_BUF, OUT_MAX)

    def s2r(self, text):
        mu = self.mu
        mu.mem_write(STR_BUF, text + b"\0")
        mu.mem_write(RES_SLOT, struct.pack("<I", 0xDEADBEEF))
        self.game.call(S2R, ecx=RES_SLOT, edx=STR_BUF)
        return self.game.rd(RES_SLOT)

    def lex_real(self, text):
        mu = self.mu
        mu.mem_write(STR_BUF, text + b"\0")
        mu.mem_write(LEX_OBJ + 0x98, struct.pack("<I", STR_BUF))
        mu.mem_write(LEX_OBJ + 0x24, struct.pack("<I", 0xDEADBEEF))
        self.game.call(LEX_REAL, ecx=LEX_OBJ)
        return self.game.rd(LEX_OBJ + 0x24)


# ---- input generation ----------------------------------------------------
def f32_bits(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


TYPICAL_REALS = [
    0.0, -0.0, 1.0, -1.0, 0.1, -0.1, 0.5, 0.25, 1.5, -3.25, 2.0, 10.0, 100.0, 1000.0,
    3.14159, 9.999999, 99.995, 0.0049999, 0.001, 1e-6, 1e-30, 1e6, 123456.789,
    16777216.0, 16777217.0, 2147483520.0, 2147483648.0, 3e9, 1e10, 1e20, 1e30,
    3.4028234663852886e38,  # FLT_MAX
    float("inf"), float("-inf"), float("nan"),
]
SPECIAL_BITS = [
    0x00000000, 0x80000000, 0x00000001, 0x80000001, 0x007FFFFF, 0x7F800000, 0xFF800000,
    0x7F7FFFFF, 0xFF7FFFFF, 0x7FC00000, 0xFFC00000, 0x7F800001, 0x4F000000, 0x4EFFFFFF,
    0x4F800000, 0x3DCCCCCD, 0x3F800000, 0x41200000, 0x41A00000,
]


def real_inputs(rng, n):
    out = [f32_bits(v) for v in TYPICAL_REALS] + list(SPECIAL_BITS)
    while len(out) < n:
        r = rng.random()
        if r < 0.25:
            out.append(rng.getrandbits(32))
        elif r < 0.6:  # ordinary magnitudes
            e = rng.randint(127 - 30, 127 + 30)
            out.append((rng.getrandbits(1) << 31) | (e << 23) | rng.getrandbits(23))
        elif r < 0.8:  # rounded decimal values, as a game would store them
            digits = rng.randint(0, 6)
            v = round(rng.uniform(-1e4, 1e4), digits)
            out.append(f32_bits(v))
        else:  # the large path: 2^31 .. 2^40
            v = rng.uniform(2.0 ** 31, 2.0 ** 40) * rng.choice([1.0, -1.0])
            out.append(f32_bits(v))
    return out[:n]


def width_precision_inputs(rng, n):
    fixed = [(0, 0), (0, 3), (0, 2), (10, 2), (8, 3), (-8, 3), (-6, 1), (0, -1), (0, 6),
             (0, 9), (0, 12), (5, 0), (-1, 0), (3, -3), (20, 20), (0, 40), (200, 2)]
    out = list(fixed)
    while len(out) < n:
        r = rng.random()
        if r < 0.5:
            w = rng.randint(-12, 14)
            p = rng.randint(-2, 10)
        elif r < 0.85:
            w = rng.randint(-40, 80)
            p = rng.randint(-10, 30)
        else:
            w = rng.randint(-200, 200)
            p = rng.randint(-40, 150)
        out.append((w, p))
    return out[:n]


DIGITS = "0123456789"


def decimal_text(rng):
    """A decimal string with optional sign, digits, one dot, and sometimes garbage."""
    r = rng.random()
    s = ""
    if r < 0.25:
        s += rng.choice(["-", "+"])
    k = rng.random()
    if k < 0.3:
        ints = str(rng.randint(0, 999))
    elif k < 0.7:
        ints = "".join(rng.choice(DIGITS) for _ in range(rng.randint(0, 12)))
    else:
        ints = "".join(rng.choice(DIGITS) for _ in range(rng.randint(0, 25)))
    s += ints
    if rng.random() < 0.8:
        s += "."
        s += "".join(rng.choice(DIGITS) for _ in range(rng.randint(0, 14)))
    g = rng.random()
    if g < 0.08:
        s += rng.choice([" ", "\t", ";", "x", "e", "-", "+", "..", "a1", "  "])
        s += "".join(rng.choice(DIGITS + ".") for _ in range(rng.randint(0, 5)))
    elif g < 0.1:
        s = rng.choice([" ", "  ", "", ".", "-", "+", "--5", "+-5", "abc"]) + s
    return s


TYPICAL_TEXTS = ["0.1", "1.5", "-3.25", "1000000", "1000000.0", "0.001", "12abc", " 3", "",
                 "+7", ".5", "5.", "1.2.3", "-", "--5", "123.456", "2.5 x", "99999999999",
                 "0.000001", "3.14159265358979", "-0.0", "4294967296", "1e6", "100;"]

LEX_FORMS = ["0.1", "1.5", "3.25", "1000000.0", "0.001", "123.456", "5.", ".25", "0.0", "2.0",
             "99999999999.99999", "0.000001", "2147483647.5", "4294967296.5", "8388609.5"]


def lex_inputs(rng, n):
    out = list(LEX_FORMS)
    while len(out) < n:
        ints = "".join(rng.choice(DIGITS) for _ in range(rng.randint(0, 11)))
        fr = "".join(rng.choice(DIGITS) for _ in range(rng.randint(0, 14)))
        out.append(f"{ints}.{fr}")
    return out[:n]


# ---- driver ----------------------------------------------------------------
def build_cases(tg, rng, n, which):
    cases = []

    def run(fn, call, record):
        try:
            ret = call()
        except sv.Hang:
            return
        record.update(ret)
        cases.append(record)

    if "r2s" in which:
        for x in real_inputs(rng, n):
            def call(x=x):
                return {"text": tg.r2sw(x, 0, 3).hex()}
            run("r2s", call, {"fn": "r2s", "x": "%08x" % x})
    if "r2sw" in which:
        reals = real_inputs(rng, n)
        wp = width_precision_inputs(rng, n)
        for x, (w, p) in zip(reals, wp):
            def call(x=x, w=w, p=p):
                return {"text": tg.r2sw(x, w, p).hex()}
            run("r2sw", call, {"fn": "r2sw", "x": "%08x" % x, "width": "%08x" % (w & 0xFFFFFFFF),
                               "precision": "%08x" % (p & 0xFFFFFFFF)})
    if "s2r" in which:
        texts = list(TYPICAL_TEXTS) + [decimal_text(rng) for _ in range(max(0, n - len(TYPICAL_TEXTS)))]
        for s in texts:
            data = s.encode("ascii")
            def call(data=data):
                return {"ret": "%08x" % tg.s2r(data)}
            run("s2r", call, {"fn": "s2r", "text": data.hex()})
    if "lex_real" in which:
        for s in lex_inputs(rng, n):
            data = s.encode("ascii")
            def call(data=data):
                return {"ret": "%08x" % tg.lex_real(data)}
            run("lex_real", call, {"fn": "lex_real", "text": data.hex()})
    return cases


ALL = ["r2s", "r2sw", "s2r", "lex_real"]


def _is_small(x_hex):
    """True for |x| < 2^31 (no table needed to format it)."""
    a = int(x_hex, 16) & 0x7FFFFFFF
    return (a & 0x7F800000) < 0x4F000000


def write_embedded(cases, path, per_fn):
    """Writes the Rust embedded set: a spread of the recorded cases, numbers and hex only.

    Small-magnitude R2S / R2SW cases come first (they need no table and always run); a
    few large-magnitude ones follow. The S2R and lexer sets divide, so they need the table.
    """
    by_fn = {}
    for c in cases:
        by_fn.setdefault(c["fn"], []).append(c)
    r2s_small = [c for c in by_fn.get("r2s", []) if _is_small(c["x"])][:per_fn]
    r2s_big = [c for c in by_fn.get("r2s", []) if not _is_small(c["x"])][:10]
    r2sw_small = [c for c in by_fn.get("r2sw", []) if _is_small(c["x"])][:per_fn]
    r2sw_big = [c for c in by_fn.get("r2sw", []) if not _is_small(c["x"])][:10]
    lines = [
        "//! Embedded observed input/output pairs of the text conversions, recorded from the",
        "//! original `Game.dll` 1.26a. Generated by `tools/emu/text_vectors.py --embed-rs`.",
        "//! Strings are hex-encoded bytes. The first groups (|x| < 2^31) need no lookup table and",
        "//! always run; the others need the reciprocal table and run only with GAME_DLL.",
        "",
        "/// `(x, text)` for R2S (width 0, precision 3).",
        "pub const R2S: &[(u32, &str)] = &[",
    ]
    for c in r2s_small + r2s_big:
        lines.append(f'    (0x{c["x"]}, "{c["text"]}"),')
    lines.append("];")
    lines.append("")
    lines.append("/// `(x, width, precision, text)` for R2SW.")
    lines.append("pub const R2SW: &[(u32, u32, u32, &str)] = &[")
    for c in r2sw_small + r2sw_big:
        lines.append(f'    (0x{c["x"]}, 0x{c["width"]}, 0x{c["precision"]}, "{c["text"]}"),')
    lines.append("];")
    lines.append("")
    lines.append("/// `(text, result bits)` for S2R.")
    lines.append("pub const S2R: &[(&str, u32)] = &[")
    for c in by_fn.get("s2r", [])[:per_fn]:
        lines.append(f'    ("{c["text"]}", 0x{c["ret"]}),')
    lines.append("];")
    lines.append("")
    lines.append("/// `(text, result bits)` for the lexer's real literal.")
    lines.append("pub const LEX_REAL: &[(&str, u32)] = &[")
    for c in by_fn.get("lex_real", [])[:per_fn]:
        lines.append(f'    ("{c["text"]}", 0x{c["ret"]}),')
    lines.append("];")
    lines.append("")
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--dll", default=os.environ.get("GAME_DLL"), help="path to Game.dll 1.26a")
    ap.add_argument("--out", required=True, help="output JSON path")
    ap.add_argument("--n", type=int, default=4000, help="cases per function")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--only", default=",".join(ALL), help="comma separated function names")
    ap.add_argument("--embed-rs", help="also write the embedded Rust test set to this path")
    ap.add_argument("--embed-per-fn", type=int, default=40)
    args = ap.parse_args(argv)
    if not args.dll:
        ap.error("set GAME_DLL or pass --dll")
    tg = TextGame(args.dll)
    rng = random.Random(args.seed)
    which = set(args.only.split(","))
    cases = build_cases(tg, rng, args.n, which)
    if args.embed_rs:
        write_embedded(cases, args.embed_rs, args.embed_per_fn)
    doc = {"format": FORMAT, "dll_sha256": tg.sha256, "seed": args.seed, "cases": cases}
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(doc, f, separators=(",", ":"))
        f.write("\n")
    counts = {}
    for c in cases:
        counts[c["fn"]] = counts.get(c["fn"], 0) + 1
    print(json.dumps(counts, sort_keys=True))


if __name__ == "__main__":
    main()
