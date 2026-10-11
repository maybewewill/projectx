"""Checks that the emulator holds the start-up values of the constants the repo relies on.

    GAME_DLL=/path/to/Game.dll python3 tools/emu/test_runtime_consts.py

The expected values are the float bit patterns the start-up initialisers leave in Game.dll 1.26a
(tools/emu/runtime_consts.py; audit in docs/spec/runtime-constants-audit.md). They are not copied from
the binary's code, only the constant values. The check fails if the emulator still sees the file-image
zeros, which is what the harnesses did before emu.py ran the initialisers.
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402  (maps Game.dll, applies the start-up values)

EXPECT = {
    0x6FAAE464: 0xBF800000,  # -1.0 (GetRandomReal offset; also the cache-unloaded sentinel of the upkeep readers)
    0x6FAAE478: 0x38D1B717,  # 1e-4 (timer minimum, sim fix 2026-10-11)
    0x6FAAE47C: 0x3A83126F,  # 0.001 (movement turn-rate floor, handicap minimum)
    0x6FAAE480: 0x3C23D70A,  # 0.01
    0x6FAAE4A0: 0x3DCCCCCD,  # 0.1 (one sim step)
    0x6FAAE4F4: 0x3F800000,  # 1.0
    0x6FAAE564: 0x42000000,  # 32.0 (playable-area clamp margin)
    0x6FAAE574: 0x42C80000,  # 100.0
    0x6FAAE594: 0x43960000,  # 300.0 (acquire threshold, attack-cycle-2)
    0x6FAAE5E8: 0x3C8EFA35,  # pi/180
    0x6FAAE614: 0x3456BF95,  # 2e-7 (GetRandomReal draw-skip threshold)
    0x6FAAE3C8: 0xBF800000,  # -1.0 upkeep gold-tax cache: unloaded sentinel
    0x6FAB7370: 0x3CF5C290,  # 0.03 (movement walk timer)
    0x6FAB7540: 0x3A83126F,  # 0.001 (movement-2/3 magnitude compare; the old 3.4e-8 was wrong)
    0x6FAB4354: 0xC7C34F80,  # -99999.0 (negative-armor sentinel)
    0x6FACC8CC: 0x3BA3D70A,  # 0.005 (lockstep tick increment)
    0x6FAE759C: 0x00000001,  # CRT SSE2 flag = 1: the float-to-int helper 0x6F7E11E0 takes the truncating path
}


def main():
    bad = []
    for addr, want in EXPECT.items():
        got = struct.unpack('<I', bytes(emu.mu.mem_read(addr, 4)))[0]
        if got != want:
            bad.append(f"{addr:#010x}: emulator {got:#010x}, expected {want:#010x}")
    if bad:
        print("\n".join(bad))
        sys.exit(1)
    print(f"ok: {len(EXPECT)} start-up constants (applied {emu.START_UP_APPLIED} dwords, "
          f"skipped {emu.START_UP_SKIPPED} jcomp-private values)")


if __name__ == "__main__":
    main()
