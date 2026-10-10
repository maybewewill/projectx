"""Dumps the values Game.dll's C/C++ static initialisers write at start-up.

Many float constants used by the simulation (e.g. 0.1 at 0x6FAAE4A0, 1.0 at 0x6FAAE4F4) are zero in
the file image and filled by small start-up stubs. This script maps the user's Game.dll in Unicorn
(via jasscomp/jcomp.py), runs the CRT initialiser ranges, and prints every dword in the writable
.data section that differs from the file image, as hex and as float32.

    GAME_DLL=/path/to/Game.dll python3 tools/emu/runtime_consts.py [out.tsv] [0xADDR ...]
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "jasscomp"))
import jcomp  # noqa: E402
import pefile  # noqa: E402


def main():
    args = sys.argv[1:]
    out = args[0] if args and not args[0].startswith("0x") else None
    wanted = [int(a, 16) for a in args if a.startswith("0x")]
    H = jcomp.Harness()
    pe = pefile.PE(jcomp.DLL, fast_load=True)
    img = pe.get_memory_mapped_image()
    data = [s for s in pe.sections if s.Name.rstrip(b"\0") == b".data"][0]
    lo = H.base + data.VirtualAddress
    hi = lo + data.Misc_VirtualSize
    n, failed = jcomp.run_crt_init(H)
    rows = []
    for va in range(lo, hi - 3, 4):
        before = struct.unpack_from("<I", img, va - H.base)[0] if va - H.base + 4 <= len(img) else 0
        after = H.rd32(va)
        if after != before:
            f = struct.unpack("<f", struct.pack("<I", after))[0]
            rows.append((va, after, f))
    print(f"initialisers run: {n}, failed: {len(failed)}, changed dwords in .data: {len(rows)}")
    if out:
        with open(out, "w") as fh:
            for va, v, f in rows:
                fh.write(f"{va:#010x}\t{v:#010x}\t{f!r}\n")
    for w in wanted:
        v = H.rd32(w)
        print(f"{w:#010x}\t{v:#010x}\t{struct.unpack('<f', struct.pack('<I', v))[0]!r}")


if __name__ == "__main__":
    main()
