"""Disassembles Game.dll code: py tools/analysis/_dis.py ADDR [COUNT]  (GAME_DLL = the user's Game.dll)"""
import os
import sys
import capstone
import pefile
pe = pefile.PE(os.environ["GAME_DLL"], fast_load=True)
img = pe.get_memory_mapped_image()
base = pe.OPTIONAL_HEADER.ImageBase
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
a = int(sys.argv[1], 16)
n = int(sys.argv[2]) if len(sys.argv) > 2 else 20
for i, ins in enumerate(md.disasm(img[a - base:a - base + 0x400], a)):
    if i >= n:
        break
    print("%x  %s %s" % (ins.address, ins.mnemonic, ins.op_str))
