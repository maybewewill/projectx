"""Unicorn vectors for the TriggerWaitForSound wait (docs/spec/natives-lane-f.md, section 5).

The real instructions 0x6F4475F8..0x6F4476A0 of the trigger-execution state machine 0x6F447340 (case 3, the branch that
waits on a sound object) run on a synthetic frame. Answered by the harness: the sound length getter 0x6F3DC110 (returns
the length in ms) and the position getter 0x6F3E8BF0 (writes the position in seconds). Everything else, the soft-float
I2R / DIV / SUB / GT / LT helpers included, is the binary. The harness stops at 0x6F4475DE, where the code starts the
resume timer, and reads the wait the timer receives.

One case per line, hex bit patterns for reals, decimal for integers:

  dur_ms playing pos_ms offset_bits wait_bits

  GAME_DLL=/path/to/Game.dll python3 tools/emu/sound_wait_emu.py OUT.txt [N_CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)

mu = emu.mu
START = 0x6F4475F8
STOP = 0x6F4475DE
GET_LEN = 0x6F3DC110
GET_POS = 0x6F3E8BF0
THOUSAND = 0x6FAAE5B4


def rd32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


STATE = {"ms": 0, "pos_bits": 0, "done": False, "result": None}


def hook(uc, addr, size, _):
    esp = uc.reg_read(UC_X86_REG_ESP)
    if addr == GET_LEN:
        uc.reg_write(UC_X86_REG_EAX, STATE["ms"] & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_EIP, rd32(esp))
        uc.reg_write(UC_X86_REG_ESP, esp + 4)
    elif addr == GET_POS:
        out = rd32(esp + 4)
        wr32(out, STATE["pos_bits"])
        uc.reg_write(UC_X86_REG_EAX, out)
        uc.reg_write(UC_X86_REG_EIP, rd32(esp))
        uc.reg_write(UC_X86_REG_ESP, esp + 8)
    elif addr == STOP:
        STATE["result"] = rd32(rd32(esp))
        STATE["done"] = True
        uc.emu_stop()


mu.hook_add(UC_HOOK_CODE, hook, begin=GET_LEN, end=GET_LEN)
mu.hook_add(UC_HOOK_CODE, hook, begin=GET_POS, end=GET_POS)
mu.hook_add(UC_HOOK_CODE, hook, begin=STOP, end=STOP)


def real_div_ms(ms):
    """DIV(I2R(ms), 1000.0) with the real helpers."""
    num = emu.call(0x6F6EF350, ecx=emu.DATA + 0x7000, edx=ms)  # I2R: out in ecx, value in edx; eax = out
    num_bits = rd32(num)
    wr32(emu.DATA + 0x7010, num_bits)
    out = emu.DATA + 0x7020
    emu.call(0x6F6EFE10, ecx=out, edx=emu.DATA + 0x7010, stack_args=[THOUSAND])
    return rd32(out)


def run_case(dur_ms, playing, pos_ms, offset_bits):
    STATE["ms"] = dur_ms
    STATE["pos_bits"] = real_div_ms(pos_ms if playing else dur_ms)
    STATE["done"] = False
    esp = emu.TOP - 0x200
    for i in range(0, 0x80, 4):
        wr32(esp + i, 0)
    wr32(esp + 0x18, offset_bits)  # the offset the thread record holds (v27)
    wr32(esp, 0)
    mu.reg_write(UC_X86_REG_ESP, esp)
    mu.reg_write(UC_X86_REG_EDI, 0x00250000)  # the sound object, opaque to the stubs
    mu.reg_write(UC_X86_REG_ESI, 0x00260000)
    mu.reg_write(UC_X86_REG_EBP, 0)
    mu.reg_write(UC_X86_REG_ECX, 0)
    mu.emu_start(START, 0, timeout=5_000_000, count=200_000)
    if not STATE["done"]:
        raise RuntimeError(f"did not reach the timer start: eip {mu.reg_read(UC_X86_REG_EIP):#x}")
    return STATE["result"]


def f2b(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def main():
    out_path = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 300
    rng = random.Random(int(sys.argv[3]) if len(sys.argv) > 3 else 1)
    lines = []
    for i in range(n):
        dur = rng.choice([0, 1, 999, 1000, 1001, 5000, 12345, 600000, rng.randrange(0, 900000)])
        playing = rng.random() < 0.5
        pos = rng.randrange(0, max(1, dur + 2000))
        kind = rng.randrange(6)
        if kind == 0:
            off = 0.0
        elif kind == 1:
            off = rng.uniform(-3.0, 0.0)
        elif kind == 2:
            off = rng.uniform(0.0, 3.0)
        elif kind == 3:
            off = rng.uniform(0.0, dur / 1000.0 + 1.0)
        elif kind == 4:
            off = float(rng.randrange(-5, 20))
        else:
            off = rng.uniform(-1000.0, 1000.0)
        ob = f2b(off)
        r = run_case(dur, playing, pos, ob)
        lines.append(f"{dur} {int(playing)} {pos} {ob:08x} {r:08x}")
    with open(out_path, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    nz = sum(1 for l in lines if int(l.split()[4], 16) != 0)
    print(f"{n} cases, {nz} with a non-zero wait -> {out_path}")


if __name__ == "__main__":
    main()
