#!/usr/bin/env python3
"""Runs the ORIGINAL JASS compiler of the user's Game.dll (1.26a) inside Unicorn.

Entry used: FUN_6f458900 (Compile.cpp:143 region). Its real callers (6f2db1a5, 6f3b1cad,
6f3b53a6, 6f73730b) pass ECX = pointer to a segment array {len, char* text} pairs ending with
a NULL text pointer, EDX = a name pointer (p3 -> scanner +0x84), [esp+4] = p4 (scanner +0x88),
[esp+8] = p5 (scanner +0x80 and +0xD8, nonzero in the callers).

Output: the driver FUN_6f458510 stores the emitted buffer at ctx+0xD0 (ctx = the scanner
object at the caller frame, E-0xE8 for the top call). FUN_6f458900 then calls FUN_6f454390
(Jass.cpp, script installation); we stop at its entry and read the buffer.
"""
import os, sys, struct, time
HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, EMU)
sys.path.insert(0, os.path.join(HERE, "..", "rngtools", "pylib"))
sys.path.insert(0, os.path.join(EMU, "pylib"))
import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_UNMAPPED, UC_PROT_ALL
from unicorn.x86_const import (UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_EAX, UC_X86_REG_ECX,
                               UC_X86_REG_EDX, UC_X86_REG_EBP)

DLL = os.environ.get("GAME_DLL", "Game.dll")
STUB = 0x00400000
STUB_SIZE = 0x10000
HEAP = 0x10000000
HEAP_SIZE = 0x4000000
TOPSTACK = 0x00100000 + 0xF000
SENT = 0x00300000
DATA = 0x00200000
CALLER_TAG = 0x6F2CFCA0          # the EDX the real callers pass (p3)
ENTRY = 0x6F458900
EMIT_COPY = 0x6F4560D0          # FUN_6f4560d0: copies the emitted words, *param = byte count
EMIT_BUF_HOOK = 0x6F454390       # FUN_6f454390, called once the output buffer exists
CTX_D0 = 0xD0
CTX_OFF_FROM_E = 0xE8            # ctx = E - 0xE8 for the top-level call (E = entry esp)
LOG = []

class Harness:
    def __init__(self, path=DLL, log=False):
        self.log = log
        raw = open(path, "rb").read()
        import hashlib
        self.sha256 = hashlib.sha256(raw).hexdigest()
        pe = pefile.PE(data=raw, fast_load=True)
        self.base = pe.OPTIONAL_HEADER.ImageBase
        size = pe.OPTIONAL_HEADER.SizeOfImage
        img = bytes(pe.get_memory_mapped_image())
        mapsz = (size + 0xFFF) & ~0xFFF
        self.mu = Uc(UC_ARCH_X86, UC_MODE_32)
        mu = self.mu
        mu.mem_map(self.base, mapsz)
        mu.mem_write(self.base, img[:mapsz].ljust(mapsz, b"\0"))
        mu.mem_map(0x0, 0x1000)                 # fs:[0] -> TIB ExceptionList
        mu.mem_write(0x0, struct.pack("<I", 0xFFFFFFFF))
        mu.mem_map(0x00100000, 0x100000)         # stack, 1 MiB
        mu.mem_map(DATA, 0x10000)
        mu.mem_map(SENT, 0x1000)
        mu.mem_map(STUB, STUB_SIZE)
        mu.mem_map(HEAP, HEAP_SIZE)
        mu.mem_write(0x6FAB73D8, struct.pack("<I", DATA + 0x1000))
        self.heap_ptr = HEAP + 0x10
        self.iat_names = {}
        self.stub_table = {}
        self._install_imports(pe)
        self.image_pe = pe
        self.unimpl = set()
        self.events = []

    # ---------------------------------------------------------------- imports
    def _install_imports(self, pe):
        pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"]])
        slot = STUB
        for e in pe.DIRECTORY_ENTRY_IMPORT:
            dll = e.dll.decode()
            for imp in e.imports:
                nm = imp.name.decode() if imp.name else "Ord%d" % imp.ordinal
                key = "%s!%s" % (dll, nm)
                self.iat_names[imp.address] = key
                self.stub_table[slot] = key
                self.mu.mem_write(imp.address, struct.pack("<I", slot))
                slot += 16
        self.mu.hook_add(UC_HOOK_CODE, self._stub_hook, begin=STUB, end=STUB + STUB_SIZE - 1)

    def _stub_hook(self, uc, address, size, user):
        if address not in self.stub_table:
            return
        key = self.stub_table[address]
        h = IMPORTS.get(key)
        if h is None:
            self.unimpl.add(key)
            raise RuntimeError("unimplemented import %s called from %s; stack=%s" % (key, self.caller_str(), self.backtrace()))
        h(self, key)

    def backtrace(self, n=40):
        esp = self.mu.reg_read(UC_X86_REG_ESP)
        out = []
        for k in range(0, 0x800, 4):
            try:
                v = self.rd32(esp + k)
            except Exception:  # noqa
                break
            if self.base <= v < self.base + 0x1000000 and len(out) < n:
                out.append("%08x" % v)
        return " ".join(out)

    def caller_str(self):
        esp = self.mu.reg_read(UC_X86_REG_ESP)
        ret = struct.unpack("<I", bytes(self.mu.mem_read(esp, 4)))[0]
        return "%08x" % ret

    # ---------------------------------------------------------------- helpers
    def rd32(self, a):
        return struct.unpack("<I", bytes(self.mu.mem_read(a, 4)))[0]

    def wr32(self, a, v):
        self.mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))

    def rdbytes(self, a, n):
        return bytes(self.mu.mem_read(a, n))

    def cstr(self, a, limit=1 << 20):
        out = bytearray()
        for i in range(limit):
            b = self.mu.mem_read(a + i, 1)[0]
            if b == 0:
                break
            out.append(b)
        return bytes(out)

    def alloc(self, n, align=16):
        p = (self.heap_ptr + align - 1) & ~(align - 1)
        self.heap_ptr = p + n
        if self.heap_ptr > HEAP + HEAP_SIZE:
            raise MemoryError("harness heap exhausted")
        return p

    def args(self, n):
        esp = self.mu.reg_read(UC_X86_REG_ESP)
        return [self.rd32(esp + 4 + 4 * k) for k in range(n)]

    def ret(self, value, nargs, stdcall):
        esp = self.mu.reg_read(UC_X86_REG_ESP)
        ret = self.rd32(esp)
        self.mu.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        self.mu.reg_write(UC_X86_REG_ESP, esp + 4 + (4 * nargs if stdcall else 0))
        self.mu.reg_write(UC_X86_REG_EIP, ret)

    # ---------------------------------------------------------------- run
    def call(self, fn, ecx=None, edx=None, stack=(), count=50_000_000, stop_at=None):
        mu = self.mu
        esp = TOPSTACK
        for a in reversed(stack):
            esp -= 4
            mu.mem_write(esp, struct.pack("<I", a & 0xFFFFFFFF))
        esp -= 4
        mu.mem_write(esp, struct.pack("<I", SENT))
        mu.reg_write(UC_X86_REG_ESP, esp)
        self.entry_esp = esp
        if ecx is not None:
            mu.reg_write(UC_X86_REG_ECX, ecx & 0xFFFFFFFF)
        if edx is not None:
            mu.reg_write(UC_X86_REG_EDX, edx & 0xFFFFFFFF)
        self.stopped_at = None
        hk = None
        if stop_at is not None:
            hk = mu.hook_add(UC_HOOK_CODE, self._stop_hook, begin=stop_at, end=stop_at)
        try:
            mu.emu_start(fn, SENT, timeout=120_000_000, count=count)
        finally:
            if hk is not None:
                mu.hook_del(hk)
        return mu.reg_read(UC_X86_REG_EAX)

    def _stop_hook(self, uc, address, size, user):
        self.stopped_at = address
        self.esp_at_stop = uc.reg_read(UC_X86_REG_ESP)
        uc.emu_stop()


# ----------------------------------------------------------------- import models
def _stdcall(n):
    def deco(fn):
        fn.nargs = n
        fn.stdcall = True
        return fn
    return deco

def _cdecl(n=0):
    def deco(fn):
        fn.nargs = n
        fn.stdcall = False
        return fn
    return deco

def _mk(fn, nargs, stdcall):
    def h(H, key):
        return fn(H, key, H.args(nargs), nargs, stdcall)
    return h

def _def(name, fn, nargs, stdcall):
    IMPORTS[name] = (lambda f, n, s: (lambda H, key: _dispatch(H, f, n, s)))(fn, nargs, stdcall)

def _dispatch(H, fn, nargs, stdcall):
    args = H.args(nargs)
    val = fn(H, args)
    H.ret(0 if val is None else val, nargs, stdcall)

IMPORTS = {}

# Storm.dll ordinals (names not in the image): the call shapes are confirmed from the callers
# (Ord401(size, file, line, flags) returns a pointer; Ord403(ptr, file, line, flags) frees).
_CRYPT = None
def _crypt_table():
    global _CRYPT
    if _CRYPT is None:
        t = [0] * 0x500
        seed = 0x00100001
        for i1 in range(256):
            for i2 in range(5):
                idx = i1 + i2 * 256
                seed = (seed * 125 + 3) % 0x2AAAAB
                t1 = (seed & 0xFFFF) << 16
                seed = (seed * 125 + 3) % 0x2AAAAB
                t2 = seed & 0xFFFF
                t[idx] = t1 | t2
        _CRYPT = t
    return _CRYPT

def mpq_hash(s, htype=0):
    """MPQ-style string hash (type 0 = the table index used by Storm SStrHash, per the project's
    tools/emu/verify_strings.py model of Game.dll's own copy at 0x6f68cc10)."""
    t = _crypt_table()
    s1, s2 = 0x7FED7FED, 0xEEEEEEEE
    for c in s:
        c = ord(chr(c).upper()) if c < 128 else c
        s1 = (t[(htype << 8) + c] ^ ((s1 + s2) & 0xFFFFFFFF)) & 0xFFFFFFFF
        s2 = (c + s1 + s2 + (s2 << 5) + 3) & 0xFFFFFFFF
    return s1

ALLOC_SIZE = {}

def storm_alloc(H, a):
    size, file, line, flags = a
    n = max(size, 1)
    p = H.alloc(n)
    H.mu.mem_write(p, b"\0" * n)
    ALLOC_SIZE[p] = n
    return p

def storm_free(H, a):
    return 0

def storm_realloc(H, a):
    ptr, size, file, line, flags = a
    n = max(size, 1)
    p = H.alloc(n)
    H.mu.mem_write(p, b"\0" * n)
    if ptr:
        old = ALLOC_SIZE.get(ptr, 0)
        cp = min(old, n)
        if cp:
            H.mu.mem_write(p, bytes(H.mu.mem_read(ptr, cp)))
    ALLOC_SIZE[p] = n
    return p

def storm_strhash(H, a):
    return mpq_hash(H.cstr(a[0]))

_def("Storm.dll!Ord401", storm_alloc, 4, True)
_def("Storm.dll!Ord403", storm_free, 4, True)
_def("Storm.dll!Ord405", storm_realloc, 5, True)
_def("Storm.dll!Ord590", storm_strhash, 1, True)

def storm_strdup(H, a):
    src, file, line = a
    data = H.cstr(src)
    p = storm_alloc(H, (len(data) + 1, 0, 0, 0))
    H.mu.mem_write(p, data + b"\0")
    return p
_def("Storm.dll!Ord507", storm_strdup, 3, True)

def storm_strcmp_i(H, a):
    s1, s2, maxlen = a
    x = H.cstr(s1)[:maxlen & 0x7FFFFFFF].upper()
    y = H.cstr(s2)[:maxlen & 0x7FFFFFFF].upper()
    return 0 if x == y else (1 if x > y else 0xFFFFFFFF)
_def("Storm.dll!Ord508", storm_strcmp_i, 3, True)

def c_memcpy(H, a):
    dst, src, n = a
    if n:
        H.mu.mem_write(dst, bytes(H.mu.mem_read(src, n)))
    return dst
def c_memset(H, a):
    dst, c, n = a
    if n:
        H.mu.mem_write(dst, bytes([c & 0xFF]) * n)
    return dst
def c_strlen(H, a):
    return len(H.cstr(a[0]))
def c_strncmp(H, a):
    s1 = H.cstr(a[0])[:a[2]]
    s2 = H.cstr(a[1])[:a[2]]
    return (s1 > s2) - (s1 < s2) & 0xFFFFFFFF
def c_strcmp(H, a):
    s1, s2 = H.cstr(a[0]), H.cstr(a[1])
    return ((s1 > s2) - (s1 < s2)) & 0xFFFFFFFF
def c_strstr(H, a):
    s = H.cstr(a[0]); t = H.cstr(a[1])
    i = s.find(t)
    return 0 if i < 0 else a[0] + i
def c_malloc(H, a):
    p = H.alloc(max(a[0], 1))
    return p
def c_calloc(H, a):
    n = a[0] * a[1]
    p = H.alloc(max(n, 1))
    H.mu.mem_write(p, b"\0" * max(n, 1))
    return p
def c_free(H, a):
    return 0
def c_realloc(H, a):
    old, n = a
    p = H.alloc(max(n, 1))
    return p
def c_isdigit(H, a):
    return 1 if 0x30 <= (a[0] & 0xFF) <= 0x39 else 0
def c_memmove(H, a):
    return c_memcpy(H, a)

for nm, fn, n, sc in [
        ("memcpy", c_memcpy, 3, False), ("memmove", c_memmove, 3, False), ("memset", c_memset, 3, False),
        ("strlen", c_strlen, 1, False), ("strcmp", c_strcmp, 2, False), ("strncmp", c_strncmp, 3, False),
        ("strstr", c_strstr, 2, False), ("malloc", c_malloc, 1, False), ("calloc", c_calloc, 2, False),
        ("free", c_free, 1, False), ("realloc", c_realloc, 2, False), ("isdigit", c_isdigit, 1, False)]:
    _def("MSVCR80.dll!" + nm, fn, n, sc)

def k_noop(H, a):
    return 0
for nm in ["OutputDebugStringA", "Sleep", "_lock", "_unlock", "_clearfp", "_control87"]:
    _def("MSVCR80.dll!" + nm, k_noop, 0, False) if nm.startswith("_") else _def("KERNEL32.dll!" + nm, k_noop, 0, True)

def c_ident(H, a):
    return a[0]
def c_malloc_crt(H, a):
    return storm_alloc(H, (a[0], 0, 0, 0))
def k_init_cs(H, a):
    return 0
def k_create_event(H, a):
    return 0x7000 + len(ALLOC_SIZE)  # any non-zero handle
_def("MSVCR80.dll!_decode_pointer", c_ident, 1, False)
_def("MSVCR80.dll!_encode_pointer", c_ident, 1, False)
_def("MSVCR80.dll!_malloc_crt", c_malloc_crt, 1, False)
_def("KERNEL32.dll!InitializeCriticalSection", k_init_cs, 1, True)
_def("KERNEL32.dll!CreateEventA", k_create_event, 4, True)
_def("MSVCR80.dll!_lock", k_noop, 1, False)
_def("MSVCR80.dll!_unlock", k_noop, 1, False)

def c_dllonexit(H, a):
    return a[0]
def k_interlocked_exchange(H, a):
    tgt, val = a
    old = H.rd32(tgt)
    H.wr32(tgt, val)
    return old
_def("MSVCR80.dll!__dllonexit", c_dllonexit, 3, False)
_def("KERNEL32.dll!InterlockedExchange", k_interlocked_exchange, 2, True)

for _nm in ["EnterCriticalSection", "LeaveCriticalSection", "DeleteCriticalSection"]:
    _def("KERNEL32.dll!" + _nm, k_init_cs, 1, True)

def storm_strcpy_n(H, a):
    dst, src, size = a
    data = H.cstr(src)[: max(size - 1, 0) & 0xFFFFFFFF]
    if size:
        H.mu.mem_write(dst, data + b"\0")
    return dst
_def("Storm.dll!Ord501", storm_strcpy_n, 3, True)

def storm_578(H, a):
    H.events.append(("storm578", tuple(hex(x) for x in a)))
    return 0
_def("Storm.dll!Ord578", storm_578, 4, True)

ERR_REPORTER = 0x6F2CFCA0   # the EDX the callers pass: the scanner's error reporter (p3)

import re as _re
def c_vformat(H, fmt, va):
    """Minimal printf: %s %d %i %u %x %X %c %% %ld/%lu and width/precision digits (no floats)."""
    out = []
    ptr = [va]
    def nxt():
        v = struct.unpack("<I", bytes(H.mu.mem_read(ptr[0], 4)))[0]
        ptr[0] += 4
        return v
    i = 0
    rx = _re.compile(r"%([-+ #0]*)(\*|\d+)?(?:\.(\d+))?(l|h)?([diuxXcs%])")
    pos = 0
    for m in rx.finditer(fmt):
        out.append(fmt[pos:m.start()]); pos = m.end()
        flags, width, prec, _len, conv = m.groups()
        if conv == "%":
            out.append("%"); continue
        if width == "*":
            width = str(nxt() & 0x7FFFFFFF if True else 0)
        if conv == "s":
            v = H.cstr(nxt()).decode("latin-1")
            if prec: v = v[: int(prec)]
        elif conv == "c":
            v = chr(nxt() & 0xFF)
        elif conv in "di":
            v = str(struct.unpack("<i", struct.pack("<I", nxt()))[0])
        elif conv == "u":
            v = str(nxt())
        else:
            v = ("%" + ("X" if conv == "X" else "x")) % nxt()
            v = v[1:]
        w = int(width) if width else 0
        out.append(v.ljust(w) if "-" in (flags or "") else v.rjust(w))
    out.append(fmt[pos:])
    return "".join(out)

def c_vsnprintf(H, a):
    buf, size, fmt, va = a
    text = c_vformat(H, H.cstr(fmt).decode("latin-1"), va).encode("latin-1")
    if size:
        H.mu.mem_write(buf, text[: size - 1] + b"\0")
    H.events.append(("vsnprintf", text[:300].decode("latin-1")))
    return len(text)
_def("MSVCR80.dll!_vsnprintf", c_vsnprintf, 4, False)
_def("KERNEL32.dll!IsDebuggerPresent", lambda H, a: 0, 0, True)
_def("KERNEL32.dll!GetSystemInfo", lambda H, a: 0, 1, True)
_TLS = {}
def k_tls_alloc(H, a):
    idx = len(_TLS) + 1
    _TLS[idx] = 0
    return idx
def k_tls_get(H, a):
    return _TLS.get(a[0], 0)
def k_tls_set(H, a):
    _TLS[a[0]] = a[1]
    return 1
_def("KERNEL32.dll!TlsAlloc", k_tls_alloc, 0, True)
_def("KERNEL32.dll!GetLastError", lambda H, a: 0, 0, True)
_def("KERNEL32.dll!SetLastError", lambda H, a: 0, 1, True)
_def("KERNEL32.dll!TlsGetValue", k_tls_get, 1, True)
_def("KERNEL32.dll!TlsSetValue", k_tls_set, 2, True)

def trap(name):
    def f(H, a):
        raise RuntimeError("trapped import call: " + name)
    return f

# ----------------------------------------------------------------- compile
def make_segments(H, text_bytes):
    """Input format of the callers: {len, char*} pairs, NULL text pointer ends the list."""
    body = text_bytes + b"\0"
    buf = H.alloc(len(body))
    H.mu.mem_write(buf, body)
    seg = H.alloc(16)
    H.mu.mem_write(seg, struct.pack("<IIII", len(text_bytes), buf, 0, 0))
    return seg

def compile_text(H, text, name=b"test.j", p4=0, p5=1, out_size_hint=None):
    """Runs FUN_6f458900 with the callers' convention. Returns (bytes_or_None, info dict)."""
    if isinstance(text, str):
        text = text.encode("latin-1")
    name_ptr = H.alloc(len(name) + 1)
    H.mu.mem_write(name_ptr, name + b"\0")
    seg = make_segments(H, text)
    H.stopped_at = None
    H.size_ptr = None
    def _cap(uc, address, size, user):
        H.size_ptr = struct.unpack("<I", bytes(uc.mem_read(uc.reg_read(UC_X86_REG_ESP) + 4, 4)))[0]
    def _err(uc, address, size, user):
        # The scanner/parser error reporter. Its message text comes from the game's WorldEditStrings
        # table (not in the upload), so only the arguments are recorded: arg1 (error kind),
        # arg2 (source line), arg3 (text pointer, empty here). Emulation stops; the compile failed.
        esp = uc.reg_read(UC_X86_REG_ESP)
        a = [struct.unpack("<I", bytes(uc.mem_read(esp + 4 + 4 * k, 4)))[0] for k in range(3)]
        H.error_args = a
        H.events.append(("error_report", [hex(x) for x in a]))
        uc.emu_stop()
    hk = H.mu.hook_add(UC_HOOK_CODE, _cap, begin=EMIT_COPY, end=EMIT_COPY)
    hk_err = H.mu.hook_add(UC_HOOK_CODE, _err, begin=ERR_REPORTER, end=ERR_REPORTER)
    try:
        ret = H.call(ENTRY, ecx=seg, edx=CALLER_TAG, stack=(p4, p5), stop_at=EMIT_BUF_HOOK)
    finally:
        H.mu.hook_del(hk)
        H.mu.hook_del(hk_err)
    info = {"stopped_at": H.stopped_at, "eax": ret}
    if getattr(H, "error_args", None) is not None:
        info["error"] = {"kind": H.error_args[0], "line": H.error_args[1]}
        H.error_args = None
        return None, info
    if H.stopped_at is None:
        info["note"] = "ran to return without reaching emit hook (parse failure?)"
        return None, info
    # ctx = E0 - 0xE8 where E0 = esp at the ENTRY call (the return address slot)
    ctx = H.entry_esp - CTX_OFF_FROM_E
    buf = H.rd32(ctx + CTX_D0)
    info["ctx"] = ctx
    info["buf"] = buf
    info["size"] = H.rd32(H.size_ptr) if H.size_ptr else None
    return buf, info

# ----------------------------------------------------------------- CRT start-up
# 0x6F7E137E: _initterm_e(0x6F874E4C, 0x6F874E58)  (C initialisers)
# 0x6F7E139A: _initterm(0x6F86DAB4, 0x6F874E48)    (C++ static constructors, pools etc.)
CRT_RANGES = [(0x6F874E4C, 0x6F874E58), (0x6F86DAB4, 0x6F874E48)]

def run_crt_init(H, verbose=False):
    failed = []
    n = 0
    for lo, hi in CRT_RANGES:
        for a in range(lo, hi, 4):
            fn = H.rd32(a)
            if fn == 0 or fn == 0xFFFFFFFF:
                continue
            n += 1
            try:
                H.call(fn, count=2_000_000)
            except Exception as e:  # noqa: BLE001
                failed.append((hex(a), hex(fn), str(e)[:120]))
                if verbose:
                    print("init fail", hex(fn), e)
    return n, failed
