"""V7: maps the level-record getters of Game.dll 1.26a to the record offsets (marker values in a fake level record).

Usage: GAME_DLL=/path/Game.dll py tools/emu/v7_getters.py
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402

wr, rd = A.wr, A.rd
GETTERS = {
    "E760": 0x6F02E760, "E7D0": 0x6F02E7D0, "E890": 0x6F02E890, "E900": 0x6F02E900, "E9E0": 0x6F02E9E0,
    "EA50": 0x6F02EA50, "EAC0": 0x6F02EAC0, "EB30": 0x6F02EB30, "EBA0": 0x6F02EBA0, "F880": 0x6F02F880,
    "E6F0": 0x6F02E6F0, "E640": 0x6F02E640, "EA50b": 0x6F02EA50,
}


def main():
    A.setup_stubs()
    A.make_ability(A.VT_THUNDERBOLT, A.AHTB, 7)
    for off in range(0, 0x68, 4):
        wr(A.LVL + off, 0x1000 + off)
    out = emu.DATA + 0x5000
    for name, fn in GETTERS.items():
        wr(out, 0xDEAD)
        try:
            emu.call(fn, ecx=A.OBJ, stack_args=(out, 0))
            v = rd(out)
            print(name, hex(v), "-> level offset", hex(v - 0x1000) if 0x1000 <= v < 0x1068 else "?")
        except Exception as e:  # noqa: BLE001
            print(name, "error", e)


if __name__ == "__main__":
    main()
