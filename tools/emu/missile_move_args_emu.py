"""Which values the missile launch hands to the mover (Game.dll 1.26a): the real move issue 0x6F47A650 is run with the
move-target routine 0x6F497080 stubbed to record its five stack arguments.

0x6F47A650 (ret 0x24, thiscall on the CMissile's position component at missile+0x78) takes nine stack arguments:
p1 = pointer to a float, p2..p4 = the target point (x, y, z), p5 = pointer to a float, p6 = message id, p7 = the
notified object, p8 = a float pointer forwarded to 0x6F497080, p9 = 0 selects 0x6F497080 (point move).
The call site 0x6F2BCC50 (missile) passes p1 = &missile+0x8C... (see the script output for the mapping).

Usage: GAME_DLL=... py tools/emu/missile_move_args_emu.py
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

mu, D = emu.mu, emu.DATA
EXTRA = 0x00400000
mu.mem_map(EXTRA, 0x10000)

ISSUE, GETMOVER, CLAMP, MOVE_TARGET, NOTIFY = 0x6F47A650, 0x6F47A540, 0x6F469490, 0x6F497080, 0x6F471AB0
WORLD_PTR = 0x6FAB7368
POS, VT, STUBRET, WORLD, MOVER = EXTRA, EXTRA + 0x100, EXTRA + 0x200, EXTRA + 0x400, EXTRA + 0x800
LOG = []


def r32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def w32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def fb(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def bf(b):
    return struct.unpack("<f", struct.pack("<I", b))[0]


def ret_to(pop, eax=None):
    esp = mu.reg_read(UC_X86_REG_ESP)
    if eax is not None:
        mu.reg_write(UC_X86_REG_EAX, eax)
    mu.reg_write(UC_X86_REG_EIP, r32(esp))
    mu.reg_write(UC_X86_REG_ESP, esp + 4 + pop)


def hook(uc, eip, size, user):
    if eip == GETMOVER:
        ret_to(0, MOVER)
    elif eip == CLAMP:
        ret_to(0, 0)
    elif eip == MOVE_TARGET:
        esp = mu.reg_read(UC_X86_REG_ESP)
        args = [r32(esp + 4 + 4 * i) for i in range(5)]
        LOG.append([(a, [bf(r32(a + 4 * k)) for k in range(3)]) for a in args[:4]] + [args[4]])
        ret_to(0x14, 7)
    elif eip == NOTIFY:
        LOG.append(("notify", r32(mu.reg_read(UC_X86_REG_ESP) + 4), r32(mu.reg_read(UC_X86_REG_ESP) + 8)))
        ret_to(8, 0)
    elif eip == STUBRET:
        ret_to(0, 0x1234)


mu.hook_add(UC_HOOK_CODE, hook)
mu.mem_write(STUBRET, b"\xc3")
w32(VT + 0x14, STUBRET)
w32(POS, VT)
w32(WORLD_PTR, WORLD)
w32(WORLD + 0x6C, fb(-100.0))
w32(WORLD + 0x70, fb(-200.0))

F1, F5 = EXTRA + 0x900, EXTRA + 0x910
w32(F1, fb(111.0))
w32(F5, fb(555.0))
args = [F1, fb(1000.0), fb(2000.0), fb(33.0), F5, 0xD019C, 0xAAAA0000, EXTRA + 0x920, 0]
emu.call(ISSUE, ecx=POS, stack_args=args)
for row in LOG:
    print(row)


# ---- the missile side: 0x6F2BCC50 (thiscall on the missile; args: target point pointer, a, b) ----
START = 0x6F2BCC50
LOG2 = []


def hook2(uc, eip, size, user):
    if eip == ISSUE:
        esp = mu.reg_read(UC_X86_REG_ESP)
        a = [r32(esp + 4 + 4 * i) for i in range(9)]
        LOG2.append(a)
        ret_to(0x24, 1)


mu.hook_add(UC_HOOK_CODE, hook2)
MIS = EXTRA + 0x2000
w32(MIS + 0x8C, fb(900.0))
w32(MIS + 0x54, fb(77.0))
VEC = EXTRA + 0x1000
VEC2 = EXTRA + 0x1100
for i, v in enumerate((1.5, 2.5, 3.5)):
    w32(VEC2 + 4 * i, fb(v))
for i, v in enumerate((1000.0, 2000.0, 33.0)):
    w32(VEC + 4 * i, fb(v))
emu.call(START, ecx=MIS, stack_args=[VEC2, VEC, 0])
a = LOG2[0]
print("move issue args:", [hex(x) for x in a])
for name, p in (("p1", a[0]), ("p5", a[4]), ("p8", a[7])):
    print(name, "->", hex(p), bf(r32(p)) if 0x00400000 <= p < 0x00410000 or 0x100000 <= p < 0x110000 else "?")
print("p6 (message)", hex(a[5]), "p7 (object)", hex(a[6]), "p9", hex(a[8]), "p2..p4", [bf(x) for x in a[1:4]])
