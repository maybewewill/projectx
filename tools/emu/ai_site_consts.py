"""Dumps the start-up values of the constants used by the AI build-spot search (docs/spec/ai-natives.md section 8)."""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402


def r32(a):
    return struct.unpack('<I', bytes(emu.mu.mem_read(a, 4)))[0]


for a in [0x6FAB4EB0, 0x6FAB4EB4, 0x6FAB4EB8, 0x6FAB68A4, 0x6FAB68A8, 0x6FADB3F8, 0x6FAAE464, 0x6FAAE4F4,
          0x6FAAE470, 0x6FAAE570, 0x6FAAE580, 0x6FAAE590, 0x6FAAE614, 0x6FAB7368]:
    v = r32(a)
    print(hex(a), hex(v), struct.unpack('<f', struct.pack('<I', v))[0])
