import os, sys, struct
sys.path.insert(0, '/home/user/projectx/tools/emu')
os.environ.setdefault('GAME_DLL', 'Game.dll')
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), 'pylib'))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), 'pylib'))
import emu
from emu import mu, call, BASE
import pefile
pe = emu.pe
def f32(x): return struct.unpack('<f', struct.pack('<f', x))[0]
def rd(addr): return struct.unpack('<f', bytes(mu.mem_read(addr, 4)))[0]
def wr(addr, v, fmt='<f'): mu.mem_write(addr, struct.pack(fmt, v))
# replay the start-up constant stubs in 0x6F84D000..0x6F84E800
def replay():
    text = bytes(mu.mem_read(0x6F84D000, 0x1800))
    i = 0
    n_c7 = 0; n_i2r = 0
    while i < len(text) - 10:
        if text[i] == 0xC7 and text[i+1] == 0x05:
            addr, imm = struct.unpack('<II', text[i+2:i+10])
            if 0x6faa0000 <= addr < 0x6fb00000:
                mu.mem_write(addr, struct.pack('<I', imm)); n_c7 += 1
            i += 10; continue
        if text[i] == 0xB9 and text[i+5] == 0xBA and text[i+10] == 0xE9:
            pass
        i += 1
    # I2R stubs: B9 addr ; (BA n) ; E9 -> 6F6EF350 (ECX = result, EDX = n)
    j = 0
    while j < len(text) - 12:
        if text[j] == 0xBA and text[j+5] == 0xB9 and text[j+10] == 0xE9:
            n, addr = struct.unpack('<II', text[j+1:j+5] + text[j+6:j+10])
            call(0x6F6EF350, ecx=addr, edx=n); n_i2r += 1
            j += 11; continue
        if text[j] == 0xB9 and text[j+5] == 0xBA and text[j+10] == 0xE9:
            addr, n = struct.unpack('<II', text[j+1:j+5] + text[j+6:j+10])
            call(0x6F6EF350, ecx=addr, edx=n); n_i2r += 1
            j += 11; continue
        j += 1
    return n_c7, n_i2r
if __name__ == '__main__':
    print(replay())
    for a in (0x6faae4f4, 0x6faae4a0, 0x6faae478, 0x6faae504, 0x6faae4fc):
        print(hex(a), rd(a))
