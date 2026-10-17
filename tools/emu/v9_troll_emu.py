"""V9: the Feral Spirit cast effect (group E of feral_emu.py) run through the CAbilitySpiritTroll vtable (0x6F92828C, item ability `AIsh`).
The effect pointer is read from the Troll class's own vtable at the slot where CAbilitySpiritWolf holds 0x6F1DF330 (asserted to be the
same function); the class overrides only the code / name / order getters and the slot 0x3E4 (stubbed as in feral_emu.py).

Usage: GAME_DLL=/path/Game.dll py tools/emu/v9_troll_emu.py OUT.txt COUNT SEED E
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import feral_emu  # noqa: E402
import emu  # noqa: E402
import abilities4_emu as A  # noqa: E402

WOLF, TROLL = 0x6F927B2C, 0x6F92828C
slot = next(o for o in range(0, 0x448, 4) if A.rd(WOLF + o) == feral_emu.F_EFFECT)
assert A.rd(TROLL + slot) == feral_emu.F_EFFECT, "Troll vtable slot %#x differs" % slot
sys.stderr.write("effect slot %#x identical in both vtables\n" % slot)
feral_emu.VT_SF = TROLL
feral_emu.main()
