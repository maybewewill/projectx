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
# runtime float constants that the file image leaves as zero (assumed values)
mu.mem_write(0x6faae464, struct.pack('<f', -1.0))  # u + (-1.0) -> u-1
mu.mem_write(0x6faae614, struct.pack('<f', 0.0))
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
    mu.emu_start(fn, SENT, timeout=5_000_000, count=200000)
    return mu.reg_read(UC_X86_REG_EAX)
def read_state(addr):
    return struct.unpack('<II', bytes(mu.mem_read(addr, 8)))
def write_state(addr, a, b):
    mu.mem_write(addr, struct.pack('<II', a, b))
