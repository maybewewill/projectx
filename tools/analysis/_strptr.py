"""Reads the C string a Game.dll pointer cell points at: py tools/analysis/_strptr.py CELL...   (GAME_DLL = the user's Game.dll)"""
import os
import struct
import sys
import pefile
pe = pefile.PE(os.environ["GAME_DLL"], fast_load=True)
img = pe.get_memory_mapped_image()
base = pe.OPTIONAL_HEADER.ImageBase
for a in sys.argv[1:]:
    cell = int(a, 16)
    p = struct.unpack_from("<I", img, cell - base)[0]
    s = bytes(img[p - base:p - base + 40]).split(b"\0")[0]
    print(hex(cell), hex(p), s)
