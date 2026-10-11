"""Differential test of the 0x22 sync value pieces against the original (docs/spec/lockstep.md section 15).

Runs the real code of Game.dll 1.26a in Unicorn on synthetic inputs and compares it with the Python model
(which crates/sim/src/sync.rs follows):

  * 0x6F39E5C0  payload hash (dword/byte rol-3 chain)
  * 0x6F280B90  word hash added into an accumulator
  * 0x6F280BE0  8-byte generator-state hash
  * 0x6F280C80  CRandData slot hash (45 streams)
  * 0x6F545840  the sync value itself (registry fold, tick clock, X, Y, R), with a fake CNetData, world object,
                generator object and registry whose slots are CCheatData objects (real vtable 0x6F940A90)

    GAME_DLL=/path/Game.dll python3 tools/emu/net_checksum_emu.py [--embed-rs]

`--embed-rs` prints the vectors of the sync value as a Rust array (inputs and the original's output; no game
bytes), used by crates/sim/tests/sync_vectors.rs.
"""
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402
from emu import mu, call, u32  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

M = 0xFFFFFFFF


def rol(x, n):
    return ((x << n) | (x >> (32 - n))) & M


def byte_hash(b):
    return ((b * 0x7A2D) ^ b) & M


def word_hash(x):
    return sum(byte_hash((x >> (8 * i)) & 255) for i in range(4)) & M


def payload_hash(b):
    acc = 0
    n = len(b) // 4
    for i in range(n):
        acc = rol(int.from_bytes(b[4 * i:4 * i + 4], 'little') ^ acc, 3)
    for x in b[4 * n:]:
        acc = rol(x ^ acc, 3)
    return acc


def registry_fold(slots):
    acc = 0
    for v in reversed(slots):
        if v is not None:
            acc = rol(v ^ acc, 3)
    return acc


def sync_value(t, a, x, y, r):
    v = t ^ rol(a ^ 0x17AB, 3)
    v = rol(v, 3) ^ x
    v = rol(v, 3) ^ y
    v = rol(v, 3) ^ r
    return rol(v, 3)


# Memory map for the synthetic objects (emu.py maps DATA = 0x200000..0x210000).
BUF = emu.DATA + 0x1000
ACC = emu.DATA + 0x2000
REG_OBJ = emu.DATA + 0x3000
SLOTS = emu.DATA + 0x3100
SLOT_OBJS = emu.DATA + 0x3200     # 20 objects of 16 bytes
NETDATA = emu.DATA + 0x4000       # 0x2300 bytes would not fit in 0x10000 with the others; use a sparse window
WORLD = emu.DATA + 0x8000
RNGOBJ = emu.DATA + 0x9000
CHEAT_VT = 0x6F940A90
TLS_FN = 0x6F4C34D0

# CNetData needs offsets up to 0x1C70 (+4): map more memory.
EXTRA = 0x00400000
mu.mem_map(EXTRA, 0x10000)
NETDATA = EXTRA
# The receive loop has an SEH prologue (fs:[0]); with no segment base that is linear address 0.
mu.mem_map(0, 0x1000)


def wr32(a, v):
    mu.mem_write(a, struct.pack('<I', u32(v)))


def rd32(a):
    return struct.unpack('<I', bytes(mu.mem_read(a, 4)))[0]


def _tls_stub(uc, address, size, user):
    ret = rd32(uc.reg_read(UC_X86_REG_ESP))
    uc.reg_write(UC_X86_REG_EAX, REG_OBJ)
    uc.reg_write(UC_X86_REG_ESP, uc.reg_read(UC_X86_REG_ESP) + 4)
    uc.reg_write(UC_X86_REG_EIP, ret)


mu.hook_add(UC_HOOK_CODE, _tls_stub, begin=TLS_FN, end=TLS_FN)


def _ret4_stub(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    uc.reg_write(UC_X86_REG_EIP, rd32(esp))
    uc.reg_write(UC_X86_REG_ESP, esp + 8)


# 0x6F549180, 0x6F551D80 and 0x6F54B810 are called with one stack argument (retn 4); the net checksum is
# accumulated before they run, so they are replaced by no-ops.
for _a in (0x6F549180, 0x6F551D80, 0x6F54B810):
    mu.hook_add(UC_HOOK_CODE, _ret4_stub, begin=_a, end=_a)

NODE = emu.DATA + 0xA000
PAY = emu.DATA + 0xB000


def net_step(net_value, block_id, payload):
    """Runs 0x6F5530D0 (the receive loop) on a one-node queue; returns CNetData+0x227C afterwards."""
    mu.mem_write(PAY, payload + b'\0' * 8)
    wr32(NODE + 4, 0)                 # next node
    wr32(NODE + 8, PAY)               # payload pointer
    wr32(NODE + 12, len(payload))     # payload length
    mu.mem_write(NODE + 0x14, bytes([block_id, 0]))   # block id, replay/net flag = 0
    wr32(NETDATA + 0xF40, NODE)
    wr32(NETDATA + 0x173C, 0)
    wr32(NETDATA + 0x610, 1)          # (flag != 0) == (+0x610 == 0): 0 == 0
    wr32(NETDATA + 0x278, 4)
    wr32(NETDATA + 0x227C, net_value)
    call(0x6F5530D0, ecx=NETDATA, stack_args=(1,))
    return rd32(NETDATA + 0x227C)


def check_net(rng, n=300):
    bad = 0
    net = 0
    for _ in range(n):
        ln = rng.randrange(2, 60)
        pay = bytes(rng.randrange(256) for _ in range(ln))
        bid = rng.choice([0x1E, 0x1F])
        want = (net + word_hash((bid + word_hash(payload_hash(pay))) & M)) & M
        got = net_step(net, bid, pay)
        if got != want:
            bad += 1
            if bad < 5:
                print('NET MISMATCH', hex(bid), pay.hex(), hex(got), hex(want))
        net = want
    # Leave (0x17: reason u32, player u8, result u32, extra u32) and forced end (0x2F: mode u32, countdown u32).
    for _ in range(60):
        reason, result, extra = rng.getrandbits(32), rng.getrandbits(32), rng.getrandbits(32)
        player = rng.randrange(256)
        pay = struct.pack('<IBII', reason, player, result, extra)
        want = (net + word_hash((0x17 + word_hash(reason) + word_hash(player) + word_hash(result)) & M)) & M
        got = net_step(net, 0x17, pay)
        if got != want:
            bad += 1
            if bad < 8:
                print('LEAVE MISMATCH', hex(got), hex(want))
        net = want
        mode, countdown = rng.getrandbits(32), rng.getrandbits(32)
        pay = struct.pack('<II', mode, countdown)
        want = (net + word_hash((0x2F + word_hash(mode) + word_hash(countdown)) & M)) & M
        got = net_step(net, 0x2F, pay)
        if got != want:
            bad += 1
            if bad < 8:
                print('END MISMATCH', hex(got), hex(want))
        net = want
    return bad


def check_hashes(rng, n=300):
    bad = 0
    for _ in range(n):
        ln = rng.randrange(0, 40)
        data = bytes(rng.randrange(256) for _ in range(ln))
        mu.mem_write(BUF, data + b'\0' * 8)
        got = call(0x6F39E5C0, ecx=BUF, edx=ln)
        if got != payload_hash(data):
            bad += 1
        x = rng.getrandbits(32)
        acc0 = rng.getrandbits(32)
        wr32(ACC, acc0)
        call(0x6F280B90, ecx=x, edx=ACC)
        if rd32(ACC) != (acc0 + word_hash(x)) & M:
            bad += 1
        a, b = rng.getrandbits(32), rng.getrandbits(32)
        mu.mem_write(BUF, struct.pack('<II', a, b))
        wr32(ACC, 0)
        call(0x6F280BE0, ecx=BUF, edx=ACC)
        if rd32(ACC) != (word_hash(a) + word_hash(b)) & M:
            bad += 1
    # CRandData::hash: object with the 45 streams at +4.
    for _ in range(20):
        words = [rng.getrandbits(32) for _ in range(90)]
        mu.mem_write(BUF, struct.pack('<I', 0) + struct.pack('<90I', *words))
        got = call(0x6F280C80, ecx=BUF)
        want = sum(word_hash(w) for w in words) & M
        if got != want:
            bad += 1
    # CCheatData::hash
    for _ in range(20):
        f = rng.getrandbits(32)
        wr32(BUF, CHEAT_VT)
        wr32(BUF + 4, f)
        if call(0x6F3834D0, ecx=BUF) != f & 0xFFFFEFFF:
            bad += 1
    return bad


def run_sync(t, slots, f1c, f40, f50, rng_a, rng_b):
    """Runs 0x6F545840 on a fake CNetData; returns CNetData+0x1C64."""
    # registry object: +0xC = 20, +0x10 = slot array
    wr32(REG_OBJ + 0xC, 20)
    wr32(REG_OBJ + 0x10, SLOTS)
    for i in range(20):
        v = slots[i]
        if v is None:
            wr32(SLOTS + 4 * i, 0)
        else:
            obj = SLOT_OBJS + 16 * i
            wr32(obj, CHEAT_VT)          # vtable pointer = CCheatData's (slot 0 returns [obj+4] & ~0x1000)
            wr32(obj + 4, v)
            wr32(SLOTS + 4 * i, obj)
    wr32(NETDATA + 0x2F8, 0)
    wr32(NETDATA + 0x1C70, t)
    wr32(NETDATA + 0x1C64, 0xDEADBEEF)
    wr32(WORLD + 0x1C, f1c)
    wr32(WORLD + 0x40, f40)
    wr32(WORLD + 0x50, f50)
    wr32(0x6FAB778C, WORLD)
    wr32(0x6FAB73D8, RNGOBJ)
    wr32(RNGOBJ, rng_a)
    wr32(RNGOBJ + 4, rng_b)
    call(0x6F545840, ecx=NETDATA)
    return rd32(NETDATA + 0x1C64)


def check_sync(rng, n=400, embed=None):
    bad = 0
    for k in range(n):
        slots = [None] * 20
        for i in range(20):
            if rng.random() < 0.35:
                slots[i] = rng.getrandbits(32) & 0xFFFFEFFF   # the stub masks bit 12
        if k % 7 == 0:
            slots = [None] * 20
        t = rng.choice([0, rng.randrange(0, 1 << 24) // 25 * 25, rng.getrandbits(32)])
        f1c = rng.getrandbits(32)
        f40 = rng.choice([M, rng.getrandbits(32), 0])
        f50 = rng.getrandbits(32) if k % 3 else rng.randrange(0, 100000)
        ra, rb = rng.getrandbits(32), rng.getrandbits(32)
        got = run_sync(t, slots, f1c, f40, f50, ra, rb)
        x = f1c if f40 == M else f40
        r = (word_hash(ra) + word_hash(rb)) & M
        want = sync_value(t, registry_fold(slots), x, f50, r)
        if got != want:
            bad += 1
            if bad < 5:
                print('MISMATCH', k, hex(got), hex(want), slots[:3], hex(t), hex(f1c), hex(f40), hex(f50))
        if embed is not None and k < 40:
            fold = registry_fold(slots)
            embed.append((t, fold, x, f50, r, got))
    return bad


def main():
    rng = random.Random(0x22)
    bad_h = check_hashes(rng)
    embed = [] if '--embed-rs' in sys.argv else None
    bad_s = check_sync(rng, embed=embed)
    bad_n = check_net(rng)
    print('hash mismatches:', bad_h, ' sync mismatches:', bad_s, ' net mismatches:', bad_n)
    bad_s += bad_n
    if embed is not None:
        print('// (t, registry fold, x, y, r, value) from 0x6F545840 on synthetic inputs')
        print('pub const VECTORS: &[(u32, u32, u32, u32, u32, u32)] = &[')
        for row in embed:
            print('    (' + ', '.join('0x%08X' % v for v in row) + '),')
        print('];')
    sys.exit(1 if bad_h or bad_s else 0)


if __name__ == '__main__':
    main()
