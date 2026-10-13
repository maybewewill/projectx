"""Unicorn run of the Hex / Polymorph target-type selection of Game.dll 1.26a (`AOhx`, `CAbilityHex`, vtable 0x6F8B1704;
docs/spec/abilities-orc-heroes.md section 16, task S39).

The ORIGINAL selector `0x6F12DAA0` (ECX = ability, stack = level index, the target's movement class `unit+0x1FC`, `ret 8`)
runs with the real class vtable (the default lists `nshe`/`nshf`/`nsha`/`nshw` come from slots `0x42C`..`0x438`), the real
token-to-code routine `0x6F4C3F50`, the real vector helpers `0x6F02C530` / `0x6F02C490`, the real roll wrapper `0x6F285350`
and F2I. Replaced by Python hooks (each stub is `nop; ret n` and the hook sets EAX):
  * the profile lookup `0x6F113DE0` (returns the case's list text for the class key `DataB`..`DataE`, or 0),
  * Storm's `SStrTokenize` (the import behind the thunk `0x6F6EB606`: skip leading delimiters, copy a token, advance),
  * the unit type lookup `0x6F29D2F0` (a type exists when its code is in the case's set),
  * Storm's `SMemAlloc` / `SMemReAlloc` / `SMemFree` (a bump allocator).

  H <class> <list text or '-'> <existing codes> | <stream a b before> | <returned code> <stream a b after>
(codes are the big-endian words of the four characters; the list text has its blanks replaced by `~` and tabs by `^`.)

Usage: GAME_DLL=/path/Game.dll py tools/emu/hex_emu.py OUT.txt COUNT SEED
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
import chain_emu as C  # noqa: E402
import abilities4_emu as A  # noqa: E402
from test_emu import py_seed  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ECX, UC_X86_REG_EAX  # noqa: E402

mu = emu.mu
DATA = emu.DATA
w32, r32, stub, ret, patch = C.w32, C.r32, C.stub, C.ret, C.patch

SELECT = 0x6F12DAA0
VT_HEX = 0x6F8B1704
ABIL = DATA + 0xA000
STRS = DATA + 0xB000
OBJ = DATA + 0x500
P = DATA + 0x520
BASE = DATA + 0x1000
HEAP = DATA + 0xC000
heap_next = [HEAP]
case = {"text": {}, "exists": set()}
STREAM = 0x18
DELIMS = " ,-_\t"


def stream_ptr(k):
    return BASE + 4 + 8 * k


def cstr(addr):
    out = bytearray()
    while True:
        b = bytes(mu.mem_read(addr, 1))[0]
        if b == 0:
            return bytes(out)
        out.append(b)
        addr += 1


def alloc(n):
    a = (heap_next[0] + 15) & ~15
    heap_next[0] = a + max(int(n), 16)
    mu.mem_write(a, bytes(max(int(n), 16)))
    return a


def hook_stub(n_ret, fn):
    addr = stub(b"\x90" + ret(n_ret))
    mu.hook_add(UC_HOOK_CODE, fn, begin=addr, end=addr)
    return addr


def setup():
    mu.mem_write(OBJ + 0x10, struct.pack("<I", P))
    mu.mem_write(P + 0xC, struct.pack("<I", BASE))
    mu.mem_write(0x6F4C34D0, b"\xb8" + struct.pack("<I", OBJ) + b"\xc3")
    mu.mem_write(0x6F4CC1E0, b"\xc3")
    w32(ABIL, VT_HEX)

    def on_prof(uc, a, s, u):
        esp = uc.reg_read(UC_X86_REG_ESP)
        cls = r32(esp + 8)
        t = case["text"].get(cls)
        if t is None:
            uc.reg_write(UC_X86_REG_EAX, 0)
        else:
            addr = STRS + 0x100 * (cls % 8)
            mu.mem_write(addr, t.encode("latin1") + b"\0")
            uc.reg_write(UC_X86_REG_EAX, addr)

    patch(0x6F113DE0, hook_stub(8, on_prof))

    def on_tok(uc, a, s, u):
        esp = uc.reg_read(UC_X86_REG_ESP)
        pstr, buf, size = r32(esp + 4), r32(esp + 8), r32(esp + 12)
        p = r32(pstr)
        s_ = cstr(p)
        i = 0
        while i < len(s_) and chr(s_[i]) in DELIMS:
            i += 1
        j = i
        while j < len(s_) and chr(s_[j]) not in DELIMS:
            j += 1
        tokb = s_[i:j][: size - 1]
        mu.mem_write(buf, tokb + b"\0")
        w32(pstr, p + j)
        uc.reg_write(UC_X86_REG_EAX, 1 if j > i else 0)

    patch(0x6F6EB606, hook_stub(0x14, on_tok))

    def on_look(uc, a, s, u):
        uc.reg_write(UC_X86_REG_EAX, 1 if uc.reg_read(UC_X86_REG_ECX) in case["exists"] else 0)

    patch(0x6F29D2F0, hook_stub(0, on_look))

    # Storm allocator: SMemAlloc(size, name, line, flags) 0x6F6EB5B2, SMemReAlloc(ptr, size, name, line, flags)
    # 0x6F6EB5CA, SMemFree(ptr, name, line, flags) 0x6F6EB558 (all stdcall)
    def on_alloc(uc, a, s, u):
        esp = uc.reg_read(UC_X86_REG_ESP)
        uc.reg_write(UC_X86_REG_EAX, alloc(r32(esp + 4)))

    def on_realloc(uc, a, s, u):
        esp = uc.reg_read(UC_X86_REG_ESP)
        old, size = r32(esp + 4), r32(esp + 8)
        new = alloc(size)
        if old:
            mu.mem_write(new, bytes(mu.mem_read(old, min(size, 0x100))))
        uc.reg_write(UC_X86_REG_EAX, new)

    patch(0x6F6EB5B2, hook_stub(0x10, on_alloc))
    patch(0x6F6EB5CA, hook_stub(0x14, on_realloc))
    patch(0x6F6EB558, stub(ret(0x10)))


def code_of(name):
    v = 0
    for ch in name.encode("latin1")[:4]:
        v = ((v << 8) | ch) & 0xFFFFFFFF
    return v


LISTS = [
    "npig,nsea,ncrb,nhmc,nrat,nfro,nech,necr,nrac",
    "nalb,nvul,nsno",
    "nsha,npng",
    "nshw,npnw",
    "nfro",
    "nrat, nfro  nech",
    "a-b_c,\tnfro,",
    "nshe,nshf",
    "nsea,,ncrb,",
    "",
]


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    A.patch_seh()
    setup()
    lines = []
    for _ in range(count):
        cls = r.choice([2, 0x10, 0x20, 1, 4, 8, 0x40, 0x80, 0])
        case["text"] = {}
        for key in (2, 0x10, 0x20, 1):
            if r.random() < 0.8:
                case["text"][key] = r.choice(LISTS)
        # the class passed is the one the original maps; the profile lookup is asked with the same class value
        case["exists"] = set()
        pool = {t for lst in LISTS for t in lst.replace(",", " ").replace("-", " ").replace("_", " ").split()}
        pool |= {"nshe", "nshf", "nsha", "nshw"}
        for t in pool:
            if r.random() < 0.75:
                case["exists"].add(code_of(t))
        heap_next[0] = HEAP
        for k in range(45):
            emu.write_state(stream_ptr(k), *py_seed(r.getrandbits(32)))
        a, b = emu.read_state(stream_ptr(STREAM))
        got = emu.call(SELECT, ecx=ABIL, stack_args=(0, cls))
        a2, b2 = emu.read_state(stream_ptr(STREAM))
        texts = ";".join("%d=%s" % (k, case["text"][k].replace(" ", "~").replace("	", "^") or "<empty>") for k in sorted(case["text"]))
        exists = ",".join("%08x" % c for c in sorted(case["exists"]))
        lines.append("H %d %s %s | %08x %08x | %08x %08x %08x" % (cls, texts or "-", exists, a, b, got & 0xFFFFFFFF, a2, b2))
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote %d lines" % len(lines))


if __name__ == "__main__":
    main()
