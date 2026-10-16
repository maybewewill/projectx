"""V5 Unicorn run of the Frost Armor retaliation handler `CBuffFrostArmor` event handler `0x6F0BA3F0` of Game.dll 1.26a
(docs/spec/abilities-undead-ne-heroes.md, V5; companion of `v5_frostarmor_emu.py`).

ORIGINAL handler on a fake buff, a fake damage record and a fake attacker. Gates in the original order: attacker non-null,
record flag bit 0 (ranged), record class 0x10, attacker slot 0x11C (invulnerable), spell immunity `0x6F28D750`; then the effect
`0x6F053040`; then the chill buff (`CBuffFrost`, allocator `0x6F0B5CA0`, id getter `0x6F1998D0`): a live buff on the attacker is
refreshed through its slot 0x360, otherwise a new one is created (slot 0x324) and applied through slot 0x35C. The chill duration
is the value the Frost Armor buff stored at cast (`+0xD0`); the illusion flag is bit 30 of the ARMORED unit's `+0x5C`.
Record: +0 attacker, +0xC flags, +0x14 class.

Row: H <flags> <class> <invulnerable> <immune> <found> <illusion> <duration> | <effect> <new> <refresh> <dur arg> <flag arg>
Usage: GAME_DLL=/path/Game.dll py tools/emu/v5_frostarmor_h_emu.py [--dump FILE] [CASES] [SEED]
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
import spell_buff_emu as S  # noqa: E402

mu = emu.mu
wr, rd = A.wr, A.rd

VT_BUFF = 0x6F8A10A4
REC = S.M + 0xA000
ATT = S.M + 0xA400
AVT = S.M + 0xA800
OLD = S.M + 0xAC00
OVT = S.M + 0xB000
NEW = S.M + 0xB400
NVT = S.M + 0xB800
VAR = S.M + 0xBC00      # +0 invulnerable, +4 immune, +8 found
LOG = S.M + 0xBD00      # +0 effect, +4 new, +8 refresh, +0x10 new duration, +0x14 new flag, +0x18/+0x1C/+0x20 refresh words


def run(cases, r):
    S.install()
    A.patch_jump(0x6F28D750, S.new_code(b"\xA1" + S.a32(VAR + 4) + b"\xC2\x08\x00"))
    A.patch_jump(0x6F053040, S.new_code(b"\xFF\x05" + S.a32(LOG) + b"\xC2\x04\x00"))   # ret 4: one argument (the attacker); ebp and edi are saved registers
    A.patch_jump(0x6F1998D0, A.stub_ret(0, 0x42667261))
    A.patch_jump(0x6F0787D0, S.new_code(b"\xA1" + S.a32(VAR + 8) + b"\xC2\x14\x00"))
    A.patch_jump(0x6F0B5CA0, A.stub_ret(0, NEW))
    mu.mem_write(NVT, b"\0" * 0x400)
    wr(NEW, NVT)
    wr(NVT + 0x324, A.stub_ret(4, 0))
    rec_new = (b"\xFF\x05" + S.a32(LOG + 4)
               + b"\x8B\x44\x24\x08\x8B\x00\xA3" + S.a32(LOG + 0x10)           # *arg2 (duration)
               + b"\x8B\x44\x24\x10\xA3" + S.a32(LOG + 0x14)                   # arg4 (illusion flag)
               + b"\xC2\x10\x00")
    wr(NVT + 0x35C, S.new_code(rec_new))
    mu.mem_write(OVT, b"\0" * 0x400)
    mu.mem_write(OLD, b"\0" * 0x40)
    wr(OLD, OVT)
    rec_ref = (b"\xFF\x05" + S.a32(LOG + 8)
               + b"\x8B\x44\x24\x08\x8B\x00\xA3" + S.a32(LOG + 0x18)           # *arg2 (duration; arg1 is the buff id)
               + b"\x8B\x44\x24\x10\xA3" + S.a32(LOG + 0x20)                   # arg4 (illusion flag)
               + b"\xC2\x10\x00")
    wr(OVT + 0x360, S.new_code(rec_ref))
    rows = []
    for _ in range(cases):
        flags = r.choice((0, 0, 0, 1, 1))
        cls = r.choice((0x10, 0x10, 0x10, 0x20, 0x10))
        inv = int(r.random() < 0.15)
        imm = int(r.random() < 0.15)
        found = int(r.random() < 0.4)
        illusion = int(r.random() < 0.3)
        dur = S.rbits(r)
        mu.mem_write(REC, b"\0" * 0x40)
        mu.mem_write(ATT, b"\0" * 0x200)
        mu.mem_write(AVT, b"\0" * 0x400)
        wr(ATT, AVT)
        wr(AVT + 0x11C, S.new_code(b"\xA1" + S.a32(VAR) + b"\xC3"))
        wr(REC, ATT)
        wr(REC + 0xC, flags)
        wr(REC + 0x14, cls)
        mu.mem_write(S.TGT, b"\0" * 0x400)
        wr(S.TGT + 0x5C, 0x40000000 if illusion else 0)
        mu.mem_write(S.BUFF, b"\0" * 0x400)
        mu.mem_write(S.FVT, bytes(mu.mem_read(VT_BUFF, 0x400)))
        wr(S.BUFF, S.FVT)
        wr(S.BUFF + 0x30, S.TGT)
        wr(S.BUFF + 0xD0, dur)
        wr(VAR, inv)
        wr(VAR + 4, imm)
        wr(VAR + 8, OLD if found else 0)
        mu.mem_write(LOG, b"\0" * 0x80)
        S.call(0x6F0BA3F0, ecx=S.BUFF, stack_args=(REC,))
        rows.append((flags, cls, inv, imm, found, illusion, dur, [rd(LOG + 4 * i) for i in range(9)]))
    return rows


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 5
    rows = run(cases, random.Random(seed))
    for row in rows[:8]:
        print("H", row[:7], " ".join("%x" % x for x in row[7]))
    if dump:
        with open(dump, "w") as fh:
            for f, c, inv, imm, found, ill, dur, log in rows:
                fh.write("H %d %x %d %d %d %d %08x | %d %d %d %08x %08x %08x %08x\n" % (
                    f, c, inv, imm, found, ill, dur, log[0], log[1], log[2], log[4], log[5], log[6], log[8]))
    print("cases", len(rows))


if __name__ == "__main__":
    main()
