"""Prints vtable slots of Game.dll: py tools/analysis/_vslot.py VTABLE OFFSET... (offsets in hex bytes)"""
import os
import struct
import sys
import pefile
pe = pefile.PE(os.environ["GAME_DLL"], fast_load=True)
img = pe.get_memory_mapped_image()
base = pe.OPTIONAL_HEADER.ImageBase
vt = int(sys.argv[1], 16)
for o in sys.argv[2:]:
    o = int(o, 16)
    print(hex(o), hex(struct.unpack_from("<I", img, vt - base + o)[0]))
