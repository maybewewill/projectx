import sys, struct
import os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), 'pylib'))
import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EIP
PATH = __import__("os").environ.get("GAME_DLL", "Game.dll")
pe = pefile.PE(PATH, fast_load=True)
BASE = pe.OPTIONAL_HEADER.ImageBase
SIZE = pe.OPTIONAL_HEADER.SizeOfImage
img = bytes(pe.get_memory_mapped_image())
MAPSZ = (SIZE + 0xfff) & ~0xfff
mu = Uc(UC_ARCH_X86, UC_MODE_32)
mu.mem_map(BASE, MAPSZ)
mu.mem_write(BASE, img[:MAPSZ].ljust(MAPSZ, b'\0'))
STACK = 0x00100000; mu.mem_map(STACK, 0x10000); TOP = STACK + 0xf000
DATA = 0x00200000; mu.mem_map(DATA, 0x10000)
SENT = 0x00300000; mu.mem_map(SENT, 0x1000)
G_PTR_SLOT = 0x6fab73d8
GADDR = DATA + 0x100          # JASS global RNG state (A,B)
mu.mem_write(G_PTR_SLOT, struct.pack('<I', GADDR))
def _apply_start_up_values():
    """Copy every .data dword that the CRT/static initialisers change into this emulator.

    Many constants (0.1 at 0x6FAAE4A0, 1.0 at 0x6FAAE4F4, the -1.0 cache sentinel at 0x6FAAE464, the
    SSE2 flag at 0x6FAE759C, vtable pointers, ...) are zero in the file image and are written at
    start-up. Earlier harnesses that only mapped this file saw zeros for all of them (audit 2026-10-11,
    docs/spec/runtime-constants-audit.md). The initialisers run in a separate jcomp harness (about a
    second); their dwords are copied here. Values that point into that harness's private stack, data or
    heap are not reproducible in this emulator and are skipped (reported in the return value).
    """
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), 'jasscomp'))
    import jcomp  # noqa: E402  (runs the CRT initialiser ranges; same as tools/emu/runtime_consts.py)
    H = jcomp.Harness(path=PATH)
    jcomp.run_crt_init(H)
    sec = [s for s in pe.sections if s.Name.rstrip(b'\0') == b'.data'][0]
    lo = BASE + sec.VirtualAddress
    hi = lo + sec.Misc_VirtualSize
    applied = skipped = 0
    for va in range(lo, hi - 3, 4):
        after = H.rd32(va)
        before = struct.unpack_from('<I', img, va - BASE)[0] if va - BASE + 4 <= len(img) else 0
        if after == before:
            continue
        if 0x00100000 <= after < 0x00400000 or 0x10000000 <= after < 0x14000000:
            skipped += 1
            continue
        mu.mem_write(va, struct.pack('<I', after))
        applied += 1
    return applied, skipped

START_UP_APPLIED, START_UP_SKIPPED = _apply_start_up_values()
# The same values, written explicitly (kept as documentation of the three constants that earlier
# harnesses set by hand; they equal the start-up values). G_PTR_SLOT above is set after the copy.
mu.mem_write(0x6faae464, struct.pack('<I', 0xBF800000))  # -1.0
mu.mem_write(0x6faae614, struct.pack('<I', 0x3456BF95))  # 2.0e-7 (was wrongly 0.0 before 2026-10-11)
mu.mem_write(0x6faae478, struct.pack('<I', 0x38D1B717))  # 1e-4, the zero snap of 0x6F483E70 (stub 0x6F84DC90; 2026-10-11)
mu.mem_write(G_PTR_SLOT, struct.pack('<I', GADDR))  # the start-up copy above may hold jcomp's own pointer
MAX_INSNS = 50_000_000
def u32(x): return x & 0xffffffff
def call(fn, ecx=None, edx=None, stack_args=(), cleanup=True):
    """Push args (already in order given: last element at lowest address after pushing ret)."""
    esp = TOP
    for a in reversed(stack_args):   # stack_args[0] ends at [esp+4] after return address push
        esp -= 4; mu.mem_write(esp, struct.pack('<I', u32(a)))
    esp -= 4; mu.mem_write(esp, struct.pack('<I', SENT))   # return address at lowest slot
    mu.reg_write(UC_X86_REG_ESP, esp)
    if ecx is not None: mu.reg_write(UC_X86_REG_ECX, u32(ecx))
    if edx is not None: mu.reg_write(UC_X86_REG_EDX, u32(edx))
    mu.emu_start(fn, SENT, timeout=5_000_000, count=MAX_INSNS)
    eip = mu.reg_read(UC_X86_REG_EIP)
    if eip != SENT:
        # Unicorn stops silently at the instruction/time limit; a truncated call must not look like
        # a result.
        raise RuntimeError(f"call {fn:#x} did not return (stopped at {eip:#x}); raise emu.MAX_INSNS")
    return mu.reg_read(UC_X86_REG_EAX)
def read_state(addr):
    return struct.unpack('<II', bytes(mu.mem_read(addr, 8)))
def write_state(addr, a, b):
    mu.mem_write(addr, struct.pack('<II', a, b))
