"""Leading-record readers of the replay game setup, run on the original Game.dll under Unicorn.

Docs: docs/spec/replay-load.md, section 3. The setup builder `0x6F443860` reads the decompressed
replay body from its own stream object G (ECX), positionally, from byte 0. This harness runs the
exact readers it calls, in its order, on a synthetic body placed in one chunk of G:

    cstr  -> `0x6F309A00` (ECX=G, EDX=dst; reads a NUL-terminated string of at most 2047 bytes;
             the string is stored through `0x6F4C5CF0`, which is replaced by a hook that records
             the bytes and stores a pointer into dst+8)
    cstr  -> same, only when build >= 0x178B
    block -> `0x6F6521B0` (ECX=G, EDX=block: cstr<=32 at +0x00, cstr<=16 at +0x20, cstr<=128 at
             +0x30, u32 at +0xB0, u32 at +0xB4)
    blob  -> `0x6F443720` (ECX=G, EDX=blob: u16 length <= 128, then the bytes)
    u32   -> `0x6F4C2D30` into G+0x430 and G+0x434 (build < 0x1770 and >= 0x1770 both reach it)
    u32   -> G+0x404, G+0x408; 20 raw bytes -> G+0x438 (`0x6F650770`, build >= 0x17AA)
    cstr  -> `0x6F309A00` into G+0x40C

The build number is the value the setup stores at G+0x238 (`0x6F534A40`); it is passed in here.

Usage:
    GAME_DLL=/path/Game.dll python3 -I tools/emu/replay_leading_emu.py [--cases N] [--seed S]
        [--json out.json] [--w3g-layout]

Without `--json`, prints the landings of three fixed bodies and the number of random cases where
the emulator and the Python model (`model_read`) disagree.
"""
import json
import os
import random
import struct
import sys

import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP,
                               UC_X86_REG_EIP)

PATH = os.environ.get("GAME_DLL")
if not PATH:
    sys.exit("set GAME_DLL to the Game.dll 1.26a path")

pe = pefile.PE(PATH, fast_load=True)
BASE = pe.OPTIONAL_HEADER.ImageBase
SIZE = pe.OPTIONAL_HEADER.SizeOfImage
img = bytes(pe.get_memory_mapped_image())
MAPSZ = (SIZE + 0xFFF) & ~0xFFF

mu = Uc(UC_ARCH_X86, UC_MODE_32)
mu.mem_map(BASE, MAPSZ)
mu.mem_write(BASE, img[:MAPSZ].ljust(MAPSZ, b"\0"))
STACK = 0x0010_0000
mu.mem_map(STACK, 0x10000)
TOP = STACK + 0xF000
DATA = 0x0020_0000
mu.mem_map(DATA, 0x20000)
SENT = 0x0030_0000
mu.mem_map(SENT, 0x1000)

G = DATA + 0x2000            # the setup object (stream + fields up to +0x460)
BODY = DATA + 0x4000         # synthetic decompressed body (one chunk)
DST_A = DATA + 0x6000        # cstr destinations / scratch
BLOCK = DATA + 0x6100        # 0xB8 bytes, the 184-byte block
BLOB = DATA + 0x6300         # u16 + 128 bytes
TAIL = DATA + 0x6500         # 20 raw bytes
STRSTORE = DATA + 0x8000     # hook-side string storage (0x100 per string)
HOLDER_MAX = 0x460

# The hook for `0x6F4C5CF0` (stores a string handle into a holder's +8). Recorded strings per run.
_strings = []
_strptr = [STRSTORE]


def _hook_strcall(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
    arg = struct.unpack("<I", bytes(uc.mem_read(esp + 4, 4)))[0]
    holder = uc.reg_read(UC_X86_REG_ECX)
    raw = bytes(uc.mem_read(arg, 2048))
    s = raw.split(b"\0", 1)[0]
    p = _strptr[0]
    _strptr[0] += 0x100
    uc.mem_write(p, s + b"\0")
    uc.mem_write(holder + 8, struct.pack("<I", p))
    _strings.append(s)
    uc.reg_write(UC_X86_REG_ESP, esp + 8)     # __thiscall, one stack argument: `ret 4`
    uc.reg_write(UC_X86_REG_EIP, ret)


mu.hook_add(UC_HOOK_CODE, _hook_strcall, begin=0x6F4C5CF0, end=0x6F4C5CF0)


def _hook_memcpy(uc, addr, size, user):
    """MSVCR80 memcpy (cdecl, reached through the IAT jump at 0x6F7E15EC)."""
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret, dst, src, n = struct.unpack("<IIII", bytes(uc.mem_read(esp, 16)))
    if n:
        uc.mem_write(dst, bytes(uc.mem_read(src, n)))
    uc.reg_write(UC_X86_REG_ESP, esp + 4)
    uc.reg_write(UC_X86_REG_EAX, dst)
    uc.reg_write(UC_X86_REG_EIP, ret)


mu.hook_add(UC_HOOK_CODE, _hook_memcpy, begin=0x6F7E15EC, end=0x6F7E15EC)


def _call(fn, ecx=None, edx=None, stack_args=()):
    esp = TOP
    for a in reversed(stack_args):
        esp -= 4
        mu.mem_write(esp, struct.pack("<I", a & 0xFFFFFFFF))
    esp -= 4
    mu.mem_write(esp, struct.pack("<I", SENT))
    mu.reg_write(UC_X86_REG_ESP, esp)
    if ecx is not None:
        mu.reg_write(UC_X86_REG_ECX, ecx & 0xFFFFFFFF)
    if edx is not None:
        mu.reg_write(UC_X86_REG_EDX, edx & 0xFFFFFFFF)
    mu.emu_start(fn, SENT, timeout=5_000_000, count=5_000_000)
    if mu.reg_read(UC_X86_REG_EIP) != SENT:
        raise RuntimeError(f"call {fn:#x} did not return")
    return mu.reg_read(UC_X86_REG_EAX)


def _init_stream(data):
    """G: one chunk holding the whole body, position 0."""
    mu.mem_write(G, b"\0" * HOLDER_MAX)
    mu.mem_write(BODY, data.ljust(len(data) + 16, b"\0"))
    mu.mem_write(G + 0x04, struct.pack("<I", BODY))   # chunk pointer (minus base 0)
    mu.mem_write(G + 0x08, struct.pack("<I", 0))      # chunk base offset
    mu.mem_write(G + 0x0C, struct.pack("<I", len(data)))
    mu.mem_write(G + 0x10, struct.pack("<I", len(data)))   # limit
    mu.mem_write(G + 0x14, struct.pack("<I", 0))           # position
    _strings.clear()
    _strptr[0] = STRSTORE


def _pos():
    return struct.unpack("<I", bytes(mu.mem_read(G + 0x14, 4)))[0]


def _limit_hit():
    return struct.unpack("<I", bytes(mu.mem_read(G + 0x10, 4)))[0] < _pos()


def _rd(addr, n):
    return bytes(mu.mem_read(addr, n))


def _cstr_call(dst):
    """0x6F309A00 into dst. Returns the string it stored, b"" when the string was empty."""
    before = len(_strings)
    _call(0x6F309A00, ecx=G, edx=dst)
    return _strings[-1] if len(_strings) > before else b""


def run_original(data, build):
    """Runs the original readers in the order of 0x6F443860. Returns the landings."""
    _init_stream(data)
    out = {}
    # 0x6F309A00 writes the string handle into holder dst+8 (dst = G+0x418 / G+0x424).
    out["str_a"] = _cstr_call(G + 0x418)
    out["str_a_holder"] = struct.unpack("<I", _rd(G + 0x418 + 8, 4))[0]
    if build >= 0x178B:
        out["str_b"] = _cstr_call(G + 0x424)
    _call(0x6F6521B0, ecx=G, edx=BLOCK)
    out["block"] = _rd(BLOCK, 0xB8)
    _call(0x6F443720, ecx=G, edx=BLOB)
    out["blob"] = _rd(BLOB, 2 + 128)
    _call(0x6F4C2D30, ecx=G, stack_args=(G + 0x430,))
    _call(0x6F4C2D30, ecx=G, stack_args=(G + 0x434,))
    if build >= 0x17AA:
        _call(0x6F650770, ecx=G, edx=TAIL)
        out["tail20"] = _rd(TAIL, 20)
    _call(0x6F4C2D30, ecx=G, stack_args=(G + 0x404,))
    _call(0x6F4C2D30, ecx=G, stack_args=(G + 0x408,))
    out["str_tail"] = _cstr_call(G + 0x40C)
    out["pos"] = _pos()
    out["limit"] = struct.unpack("<I", _rd(G + 0x10, 4))[0]
    out["error"] = _limit_hit()
    return out


# ----------------------------------------------------------------------------------------------
# Python model of the same reads (the semantics of FUN_6f4c2e10, FUN_6f4c2fd0, FUN_6f4c2d30).

class Model:
    def __init__(self, data):
        self.d = data
        self.pos = 0
        self.limit = len(data)
        self.err = False

    def fail(self):
        """FUN_6f4c2b30 sets the position to limit + 1 on a failed read."""
        self.err = True
        self.pos = self.limit + 1

    def u8(self):
        if self.pos + 1 > self.limit:
            self.fail()
            return None
        v = self.d[self.pos]
        self.pos += 1
        return v

    def u16(self):
        if self.pos + 2 > self.limit:
            self.fail()
            return None
        v = struct.unpack_from("<H", self.d, self.pos)[0]
        self.pos += 2
        return v

    def u32(self):
        if self.pos + 4 > self.limit:
            self.fail()
            return None
        v = struct.unpack_from("<I", self.d, self.pos)[0]
        self.pos += 4
        return v

    def bytes_(self, n):
        if self.pos + n > self.limit:
            self.fail()
            return None
        v = self.d[self.pos:self.pos + n]
        self.pos += n
        return v

    def cstr(self, n):
        """Read up to a NUL that fits in n bytes (the NUL counts). Error when it does not fit."""
        out = b""
        i = 0
        while True:
            if self.pos >= self.limit:
                self.err = True
                self.pos = self.limit + 1
                return None
            c = self.d[self.pos]
            self.pos += 1
            if c == 0:
                return out
            out += bytes([c])
            i += 1
            if i >= n:
                # n bytes copied without a NUL: the original sets pos = limit + 1
                self.pos = self.limit + 1
                self.err = True
                return None


def model_read(data, build):
    m = Model(data)
    out = {}
    a = m.cstr(2048)
    out["str_a"] = a
    if build >= 0x178B:
        out["str_b"] = m.cstr(2048)
    out["block"] = {
        "c32": m.cstr(32), "c16": m.cstr(16), "c128": m.cstr(128),
    }
    if not m.err:
        out["block"]["u32_b0"] = m.u32()
        out["block"]["u32_b4"] = m.u32()
    if not m.err:
        n = m.u16()
        out["blob"] = m.bytes_(n) if (n is not None and n <= 128) else None
        if n is not None and n > 128:
            m.err = True
            m.pos = m.limit + 1
    if not m.err:
        out["u32_430"] = m.u32()
        out["u32_434"] = m.u32()
    if build >= 0x17AA and not m.err:
        out["tail20"] = m.bytes_(20)
    if not m.err:
        out["u32_404"] = m.u32()
        out["u32_408"] = m.u32()
    if not m.err:
        out["str_tail"] = m.cstr(2048)
    out["pos"] = m.pos
    out["limit"] = m.limit
    out["error"] = m.err or m.pos > m.limit
    return out


def compare(orig, model, build):
    """Returns a list of mismatch descriptions (empty when the two agree)."""
    bad = []
    if orig["str_a"] != (model["str_a"] or b""):
        bad.append(f"str_a {orig['str_a']!r} != {model['str_a']!r}")
    if build >= 0x178B and orig.get("str_b") != (model.get("str_b") or b""):
        bad.append(f"str_b {orig.get('str_b')!r} != {model.get('str_b')!r}")
    blk = orig["block"]
    mb = model["block"]
    if mb.get("c32") is not None and blk[0:32].split(b"\0", 1)[0] != mb["c32"]:
        bad.append("block c32")
    if mb.get("c16") is not None and blk[0x20:0x30].split(b"\0", 1)[0] != mb["c16"]:
        bad.append("block c16")
    if mb.get("c128") is not None and blk[0x30:0xB0].split(b"\0", 1)[0] != mb["c128"]:
        bad.append("block c128")
    if "u32_b0" in mb and struct.unpack_from("<I", blk, 0xB0)[0] != mb["u32_b0"]:
        bad.append("block u32 +0xB0")
    if "u32_b4" in mb and struct.unpack_from("<I", blk, 0xB4)[0] != mb["u32_b4"]:
        bad.append("block u32 +0xB4")
    if model.get("blob") is not None:
        n = struct.unpack_from("<H", orig["blob"], 0)[0]
        if orig["blob"][2:2 + n] != model["blob"] or n != len(model["blob"]):
            bad.append("blob")
    if orig["pos"] != model["pos"] or orig["error"] != model["error"]:
        bad.append(f"pos/error {orig['pos']},{orig['error']} != {model['pos']},{model['error']}")
    return bad


def random_body(rng, build):
    def cs(maxlen):
        n = rng.randint(0, maxlen - 1)
        return bytes(rng.randint(1, 255) for _ in range(n)) + b"\0"
    b = bytearray()
    b += cs(40) if rng.random() < 0.9 else b""
    if build >= 0x178B:
        b += cs(40)
    b += cs(30) + cs(20) + cs(130)
    b += struct.pack("<II", rng.getrandbits(32), rng.getrandbits(32))
    n = rng.randint(0, 130)
    b += struct.pack("<H", n) + bytes(rng.getrandbits(8) for _ in range(n))
    b += struct.pack("<II", rng.getrandbits(32), rng.getrandbits(32))
    if build >= 0x17AA:
        b += bytes(rng.getrandbits(8) for _ in range(20))
    b += struct.pack("<I", rng.getrandbits(32)) + struct.pack("<I", rng.getrandbits(32))
    b += cs(60)
    return bytes(b)


def encode_string(data):
    """The stat-string encoder (0x6F651860): mask byte per group of 7, then b|1 / b+1 bytes."""
    out = bytearray()
    for i in range(0, len(data), 7):
        chunk = data[i:i + 7]
        mask = 1
        for j, b in enumerate(chunk):
            if b % 2 == 1:
                mask |= 1 << (j + 1)
        out.append(mask)
        for b in chunk:
            out.append(b if b % 2 == 1 else b + 1)
    return bytes(out)


def w3g_layout_body():
    """The body crates/w3g/tests/synthetic.rs writes (its GameInfo layout, then the 0x19 record)."""
    def cstr(s):
        return s + b"\0"
    b = bytearray(struct.pack("<I", 0x110))
    b += bytes([0x00, 1]) + cstr(b"Host") + bytes([1, 0])
    b += cstr(b"Test Game") + cstr(b"")
    settings = bytearray(struct.pack("<I", 0x00018000)) + bytes([0])
    settings += struct.pack("<HH", 116, 84) + struct.pack("<I", 0xDEADBEEF)
    settings += cstr(b"Maps\\FrozenThrone\\(2)EchoIsles.w3x") + cstr(b"Host") + bytes([0])
    b += cstr(encode_string(bytes(settings)))
    b += struct.pack("<III", 12, 1, 0)
    b += bytes([0x16, 2]) + cstr(b"Guest") + bytes([1, 0]) + struct.pack("<I", 0)
    rec = bytes([2]) + bytes([1, 100, 2, 0, 0, 0, 0x02, 1, 100]) + bytes([2, 100, 2, 0, 1, 1, 0x04, 1, 100])
    b += bytes([0x19]) + struct.pack("<H", len(rec) + 4 + 2) + rec
    b += struct.pack("<I", 0x12345678) + bytes([0, 2])
    return bytes(b)


def demo_w3g(build=6059):
    body = w3g_layout_body()
    o = run_original(body, build)
    m = model_read(body, build)
    print("w3g-layout body, first 24 bytes:", body[:24].hex())
    print("  original str_a      :", o["str_a"])
    print("  model    str_a      :", m["str_a"])
    print("  original block      :", o["block"][:0x40].split(b"\0")[0], "...")
    print("  original blob       :", o["blob"][:12].hex())
    print("  original pos/limit  :", o["pos"], o["limit"], "error:", o["error"])
    print("  model pos/error     :", m["pos"], m["error"])
    print("  mismatches vs model :", compare(o, m, build))


def main(argv):
    if "--w3g-layout" in argv:
        demo_w3g()
        return
    cases = 300
    seed = 1
    json_out = None
    if "--cases" in argv:
        cases = int(argv[argv.index("--cases") + 1])
    if "--seed" in argv:
        seed = int(argv[argv.index("--seed") + 1])
    if "--json" in argv:
        json_out = argv[argv.index("--json") + 1]
    rng = random.Random(seed)
    vectors = []
    mism = 0
    for i in range(cases):
        build = rng.choice([6059, 6059, 6040, 6000, 5999])
        body = random_body(rng, build)
        o = run_original(body, build)
        m = model_read(body, build)
        bad = compare(o, m, build)
        if bad:
            mism += 1
            if mism <= 5:
                print("MISMATCH case", i, build, bad)
        vectors.append({"body": body.hex(), "build": build, "orig": {
            "str_a": (o["str_a"] or b"").hex(), "pos": o["pos"], "error": o["error"]}})
    print(f"random cases: {cases}, mismatches: {mism}")
    if json_out:
        with open(json_out, "w") as f:
            json.dump(vectors, f)


if __name__ == "__main__":
    main(sys.argv[1:])
