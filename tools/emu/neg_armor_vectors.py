"""Reference values of the negative-armor table (VA 0x6FAB4300..0x6FAB4354) as the original's start-up code leaves them.

The table is in BSS and written by the C/C++ static initialisers, so this runs the same initialiser ranges as
tools/emu/runtime_consts.py (jasscomp/jcomp.py) in Unicorn and reads the 22 dwords back. Output is a TSV of
"<va hex>\t<bits hex>" lines, written to the path given. Keep the output OUTSIDE the repository: it is data from the
user's Game.dll.

    GAME_DLL=/path/to/Game.dll python3 tools/emu/neg_armor_vectors.py /path/outside/repo/neg_armor.tsv

The Rust test (crates/sim/tests/damage.rs) reads the file named by NEG_ARMOR_VECTORS.
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "jasscomp"))
import jcomp  # noqa: E402

FIRST_VA = 0x6FAB4300
END_VA = 0x6FAB4358  # exclusive; 21 armor entries plus the -99999.0 sentinel at 0x6FAB4354


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    H = jcomp.Harness()
    n, failed = jcomp.run_crt_init(H)
    print(f"initialisers run: {n}, failed: {len(failed)}")
    with open(sys.argv[1], "w") as fh:
        for va in range(FIRST_VA, END_VA, 4):
            bits = H.rd32(va)
            f = struct.unpack("<f", struct.pack("<I", bits))[0]
            print(f"{va:#010x}\t{bits:#010x}\t{f!r}")
            fh.write(f"{va:#010x}\t{bits:#010x}\n")


if __name__ == "__main__":
    main()
