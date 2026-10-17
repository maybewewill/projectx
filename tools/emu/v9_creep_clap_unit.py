"""V9: the per-unit body 0x6F1607B0 (shared with War Stomp / Thunder Clap) run for real with the CAbilityCreepThunderClap vtable
(0x6F8F7044, Creep Thunder Clap `ACtc`). Reuses tools/emu/stomp_emu.py with the class swapped.

Usage: GAME_DLL=/path/Game.dll py tools/emu/v9_creep_clap_unit.py OUT.txt COUNT SEED
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import stomp_emu  # noqa: E402

stomp_emu.VT_STOMP = 0x6F8F7044
stomp_emu.main()
