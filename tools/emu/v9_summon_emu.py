"""V9: the summon loop 0x6F163390 (slot 0x3C0) on the other classes that share it: CAbilitySummonGrizzly ANsg, CAbilitySummonQuillbeast
ANsq, CAbilitySummonWarEagle ANsw, CAbilityLavaMonster ANlm, CAbilityWateryMinion ANwm. Reuses tools/emu/v5_summon_emu.py with the
vtable list swapped (the class slot 0x3E4, the timed-life buff attach, is stubbed as in V5). Rows in the V5 format.

Usage: GAME_DLL=/path/Game.dll py tools/emu/v9_summon_emu.py OUT.txt COUNT SEED     (COUNT = 150 per class, i.e. 750)
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import v5_summon_emu as V5  # noqa: E402

V5.VTABLES = (0x6F8F1574, 0x6F8F1964, 0x6F8F1D54, 0x6F89F7F4, 0x6F8F0D94)
V5.main()
