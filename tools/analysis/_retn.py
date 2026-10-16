"""Prints the ret instruction (with its stack cleanup) of Game.dll functions: py tools/analysis/_retn.py ADDR..."""
import os
import sys
import capstone
import pefile
pe = pefile.PE(os.environ["GAME_DLL"], fast_load=True)
img = pe.get_memory_mapped_image()
base = pe.OPTIONAL_HEADER.ImageBase
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
for a in sys.argv[1:]:
    a = int(a, 16)
    off = a - base
    out = []
    for ins in md.disasm(img[off:off + 0x600], a):
        if ins.mnemonic.startswith("ret"):
            out.append("%x %s %s" % (ins.address, ins.mnemonic, ins.op_str))
            break
    print(hex(a), out)
