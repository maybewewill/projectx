#!/usr/bin/env python3
"""The REAL compile path of the game, run in Unicorn (docs/spec/jass-frontend.md, "Real compile path").

`jcomp.py` runs the compiler entry 0x6F458900 with a one-segment text and an empty native table, which is
not how the game calls it. The game (callers 0x6F2DB0F0 for AI, 0x6F3B1C30 for maps) does this:

  * the natives are registered first (0x6F3D4020: 1,167 common natives; 0x6F2E3D90: the AI natives) into
    a table that lives in slot 5 of the per-thread block (TlsGetValue(dwTlsIndex @ 0x6FAB7BF4));
  * the source is a list of {name, text} segments (common.j, Blizzard.j, war3map.j / common.j,
    common.ai, race script) ending with a NULL text, ECX = the list, EDX = the error reporter
    (0x6F2CFCA0 for AI, 0x6F3B18A0 for maps), stack args p4 = 0 and p5 = 1 (strict return analysis;
    p5 = 0 is the lenient mode used for 23 whitelisted legacy maps);
  * a native declaration is checked against the registered table (error 34 unknown, 37 signature).

This module builds that environment once, snapshots the memory, and restores it for every compile, so a few
thousand probes take minutes. Usage:

    GAME_DLL=/path/Game.dll python3 realpath.py          # self test
    from realpath import Lab; lab = Lab(); errs, ok = lab.compile([("a.j", "...")])

Errors are collected (the reporter is stubbed to return), `ok` is True when the compiler reached the
output stage (it emits bytecode only when no error was reported).
"""
import os
import struct
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
os.environ.setdefault("GAME_DLL", "Game.dll")
import jcomp as J  # noqa: E402
from jcomp import ENTRY, EMIT_COPY, EMIT_BUF_HOOK, ERR_REPORTER, CTX_D0, CTX_OFF_FROM_E, CALLER_TAG  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX  # noqa: E402

STACK_BASE = 0x20000000
STACK_SIZE = 0x4000000          # the real recursion depth of the grammar needs far more than 60 KB
TLS_INDEX = 1
DW_TLS_INDEX = 0x6FAB7BF4       # the game's TLS index variable
REG_COMMON = 0x6F3D4020         # registers the 1,167 common natives
REG_AI = 0x6F2E3D90             # registers the AI natives


class Lab:
    def __init__(self, which="common"):
        self.H = J.Harness()
        J.run_crt_init(self.H)
        H = self.H
        H.mu.mem_map(STACK_BASE, STACK_SIZE)
        J.TOPSTACK = STACK_BASE + STACK_SIZE - 0x10000
        blk = H.alloc(1024)
        H.mu.mem_write(blk, b"\0" * 1024)
        J._TLS[TLS_INDEX] = blk
        H.wr32(DW_TLS_INDEX, TLS_INDEX)
        H.call(REG_COMMON, count=400_000_000)
        if which == "ai":
            H.call(REG_AI, count=400_000_000)
        self._snapshot()

    # ---- snapshot / restore of everything a compile can touch
    def _snapshot(self):
        H = self.H
        size = (H.image_pe.OPTIONAL_HEADER.SizeOfImage + 0xFFF) & ~0xFFF
        spans = [(H.base, size), (0x0, 0x1000), (0x200000, 0x10000),
                 (J.HEAP, H.heap_ptr - J.HEAP + 0x1000), (STACK_BASE + STACK_SIZE - 0x100000, 0x100000)]
        self.snap = [(lo, bytes(H.mu.mem_read(lo, n))) for lo, n in spans]
        self.heap_ptr = H.heap_ptr
        self.alloc_size = dict(J.ALLOC_SIZE)
        self.tls = dict(J._TLS)

    def _restore(self):
        H = self.H
        for lo, data in self.snap:
            H.mu.mem_write(lo, data)
        if H.heap_ptr > self.heap_ptr:
            H.mu.mem_write(self.heap_ptr, b"\0" * (H.heap_ptr - self.heap_ptr))
        H.heap_ptr = self.heap_ptr
        J.ALLOC_SIZE.clear()
        J.ALLOC_SIZE.update(self.alloc_size)
        J._TLS.clear()
        J._TLS.update(self.tls)
        H.events = []
        H.error_args = None

    @staticmethod
    def _segments(H, segs):
        ptrs = []
        for name, text in segs:
            if isinstance(text, str):
                text = text.encode("latin-1")
            n = H.alloc(len(name) + 1)
            H.mu.mem_write(n, name.encode() + b"\0")
            t = H.alloc(len(text) + 1)
            H.mu.mem_write(t, text + b"\0")
            ptrs.append((n, t))
        arr = H.alloc(8 * (len(ptrs) + 1))
        for i, (n, t) in enumerate(ptrs):
            H.mu.mem_write(arr + 8 * i, struct.pack("<II", n, t))
        H.mu.mem_write(arr + 8 * len(ptrs), struct.pack("<II", 0, 0))
        return arr

    def compile(self, segs, p4=0, p5=1, stop_install=True, count=4_000_000_000):
        """Returns (errors, ok, info). errors = [(line, code)]; ok = the output stage was reached."""
        self._restore()
        H = self.H
        mu = H.mu
        seg = self._segments(H, segs)
        H.stopped_at = None
        H.size_ptr = None
        errs = []

        def cap(uc, address, size, user):
            H.size_ptr = struct.unpack("<I", bytes(uc.mem_read(uc.reg_read(UC_X86_REG_ESP) + 4, 4)))[0]

        def err(uc, address, size, user):
            esp = uc.reg_read(UC_X86_REG_ESP)
            a = [struct.unpack("<I", bytes(uc.mem_read(esp + 4 + 4 * k, 4)))[0] for k in range(3)]
            errs.append((a[1], uc.reg_read(UC_X86_REG_EDX)))
            ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
            uc.reg_write(UC_X86_REG_ESP, esp + 4 + 12)       # the reporter is `retn 0Ch`
            uc.reg_write(UC_X86_REG_EIP, ret)
            uc.reg_write(UC_X86_REG_EAX, 0)

        h1 = mu.hook_add(UC_HOOK_CODE, cap, begin=EMIT_COPY, end=EMIT_COPY)
        h2 = mu.hook_add(UC_HOOK_CODE, err, begin=ERR_REPORTER, end=ERR_REPORTER)
        t0 = time.time()
        try:
            H.call(ENTRY, ecx=seg, edx=CALLER_TAG, stack=(p4, p5),
                   stop_at=EMIT_BUF_HOOK if stop_install else None, count=count)
        finally:
            mu.hook_del(h1)
            mu.hook_del(h2)
        info = {"secs": time.time() - t0}
        if H.stopped_at is not None:
            ctx = H.entry_esp - CTX_OFF_FROM_E
            info["size"] = H.rd32(H.size_ptr) if H.size_ptr else None
            info["buf"] = H.rd32(ctx + CTX_D0)
        return errs, H.stopped_at is not None, info

    def accepts(self, text, **kw):
        errs, ok, _ = self.compile([("t.j", text)], **kw)
        return ok


if __name__ == "__main__":
    lab = Lab()
    f = "function f takes nothing returns nothing\nendfunction\n"
    print("plain function:", lab.accepts(f))
    print("native Sin (registered):", lab.accepts("native Sin takes real r returns real\n"))
    print("native Zork (not registered):", lab.accepts("native Zork takes nothing returns nothing\n"))
    print("native Rect + type rect (case-sensitive names):",
          lab.accepts("type rect extends handle\nnative Rect takes real a, real b, real c, real d returns rect\n"))
    print("return null in an integer function:",
          lab.accepts("function g takes nothing returns integer\nreturn null\nendfunction\n"))
    print("... with a nested null and a final value return:",
          lab.accepts("function g takes nothing returns integer\nif true then\nreturn null\nendif\nreturn 1\nendfunction\n"))
