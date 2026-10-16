"""V5 Unicorn runs of Sleep (`AUsl`, `CAbilitySleep` vtable 0x6F9215CC) of Game.dll 1.26a (docs/spec/abilities-undead-ne-heroes.md).

Mode E: the real unit-target effect `0x6F1FBA80` (real level getters, hero test `0x6F1D2660`, `0x6F02F880` = Dur + ability
`+0x9C`, the three buff-id getters `0x6F1D6000` BUsl / `0x6F1D5F00` BUst / `0x6F1D5F80` BUsp, soft add) on a fake ability and a
fake target. Stubbed: the buff lookup `0x6F0787D0` (returns a fake live buff for the ids the case marks as present), the buff
creators `0x6F1F9CD0` (CBuffSleep) and `0x6F1F9DA0` (CBuffSleepPause), registration `0x6F079CA0`, the info builder. Every call
into a buff (refresh slot 0x358 / 0x360, init 0x324, apply 0x354 / 0x358) is recorded as an event with the duration it carries.

Mode W: the real damage-event handler `0x6F1F6870` of `CBuffSleep` (slot 0xFC): a record amount that is not 0.0 ends the buff
(buff slot 0x34C) unless the sleeper has `BUst` or `BUsp`.

Row E: E <Dur> <HeroDur> <+0x9C> <DataA> <hero> <dead> <has BUsl> <has BUst> <has BUsp> | <events> (kind duration)...
Row W: W <amount> <has BUst> <has BUsp> | <ended>
Usage: GAME_DLL=/path/Game.dll py tools/emu/v5_sleep_emu.py [--dump DIR] [CASES] [SEED]
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import abilities4_emu as A  # noqa: E402
import emu  # noqa: E402
import spell_buff_emu as S  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
wr, rd = A.wr, A.rd

VT_SLEEP_ABIL = 0x6F9215CC
VT_SLEEP_BUFF = 0x6F92126C
ID_SL, ID_ST, ID_SP = 0x4255736C, 0x42557374, 0x42557370

OLD = {ID_SL: S.M + 0x9000, ID_ST: S.M + 0x9200, ID_SP: S.M + 0x9400}   # fake live buffs
OVT_SL, OVT_ST, OVT_SP = S.M + 0xB000, S.M + 0xB400, S.M + 0xB800
NEWB = {"sl": S.M + 0x9A00, "sp": S.M + 0x9C00}
NVT = S.M + 0xA000
VAR = S.M + 0xA800     # +0 has BUsl, +4 has BUst, +8 has BUsp, +0xC end counter
events = []


def hook_stack(addr, tag, n_args, deref):
    """Records (tag, deref'd argument words) at the entry of a stub function."""
    def cb(uc, address, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        events.append((tag, [rd(rd(esp + 4 * (i + 1))) if i in deref else rd(esp + 4 * (i + 1)) for i in range(n_args)]))
    mu.hook_add(UC_HOOK_CODE, cb, begin=addr, end=addr)


def install():
    S.install()
    # lookup: first stack argument = buff id; returns the fake live buff or 0
    def block(buff_id, var):
        # cmp eax,id ; jne +8 ; mov eax,[var] ; ret 0x14
        return b"\x3D" + S.a32(buff_id) + b"\x75\x08\xA1" + S.a32(var) + b"\xC2\x14\x00"

    lk = S.new_code(b"\x8B\x44\x24\x04" + block(ID_SL, VAR) + block(ID_ST, VAR + 4) + block(ID_SP, VAR + 8)
                    + b"\x31\xC0\xC2\x14\x00")
    # the case stores the fake live buff pointer (or 0) in the three cells
    A.patch_jump(0x6F0787D0, lk)
    A.patch_jump(0x6F079CA0, A.stub_ret(4, 0))
    mu.mem_write(NVT, b"\0" * 0x400)
    # new buffs: allocators return fake objects whose slots record
    for key, fn in (("sl", 0x6F1F9CD0), ("sp", 0x6F1F9DA0)):
        mu.mem_write(NEWB[key], b"\0" * 0x40)
        wr(NEWB[key], NVT)
        A.patch_jump(fn, A.stub_ret(0, NEWB[key]))
    wr(NVT + 0x324, A.stub_ret(4, 0))
    wr(NVT + 0x354, A.stub_ret(0xC, 0))
    wr(NVT + 0x358, A.stub_ret(0xC, 0))
    hook_stack(rd(NVT + 0x324), "init", 1, {0})
    hook_stack(rd(NVT + 0x354), "apply354", 3, {1})
    hook_stack(rd(NVT + 0x358), "apply358", 3, {1})
    # live buffs: BUsl refresh slot 0x358 (3 args); BUst refresh slot 0x360 (4 args); BUsp refresh slot 0x360 (3 args)
    for key, vt, slot, n, tag in ((ID_SL, OVT_SL, 0x358, 3, "refresh358"), (ID_ST, OVT_ST, 0x360, 4, "refresh360st"),
                                  (ID_SP, OVT_SP, 0x360, 3, "refresh360sp")):
        mu.mem_write(vt, b"\0" * 0x400)
        wr(vt + slot, A.stub_ret(4 * n, 0))
        wr(vt + 0x34C, S.new_code(b"\xFF\x05" + S.a32(VAR + 0xC) + b"\xC2\x08\x00"))      # end the buff: count
        hook_stack(rd(vt + slot), tag, n, {1})
        mu.mem_write(OLD[key], b"\0" * 0x40)
        wr(OLD[key], vt)
    OLD_END[0] = rd(OVT_SL + 0x34C)


def run_effect(cases, r):
    install()
    rows = []
    for _ in range(cases):
        dur, hdur, data_a = S.rbits(r), S.rbits(r), S.rbits(r)
        extra = S.rbits(r) if r.random() < 0.3 else 0
        hero = r.random() < 0.5
        dead = r.random() < 0.1
        has = [int(r.random() < 0.3) for _ in range(3)]
        A.make_ability(VT_SLEEP_ABIL, 0x41557375, 75)
        S.level(A.LVL, dur, hdur, [data_a])
        wr(A.OBJ + 0x9C, extra)
        S.make_target(hero)
        wr(S.TGT + 0x5C, 0x100 if dead else 0)
        wr(VAR, OLD[ID_SL] if has[0] else 0)
        wr(VAR + 4, OLD[ID_ST] if has[1] else 0)
        wr(VAR + 8, OLD[ID_SP] if has[2] else 0)
        events.clear()
        S.call(0x6F1FBA80, ecx=A.OBJ, stack_args=(S.TGT,))
        rows.append((dur, hdur, extra, data_a, int(hero), int(dead), has, list(events)))
    return rows


def run_wake(cases, r):
    rows = []
    for _ in range(cases):
        amount = S.rbits(r)
        has = [int(r.random() < 0.3) for _ in range(2)]
        mu.mem_write(S.BUFF, b"\0" * 0x400)
        mu.mem_write(S.FVT, bytes(mu.mem_read(VT_SLEEP_BUFF, 0x400)))
        wr(S.FVT + 0x34C, OLD_END[0])
        wr(S.BUFF, S.FVT)
        wr(S.BUFF + 0x30, S.TGT)
        mu.mem_write(S.TGT, b"\0" * 0x400)
        wr(VAR, 0)
        wr(VAR + 4, OLD[ID_ST] if has[0] else 0)
        wr(VAR + 8, OLD[ID_SP] if has[1] else 0)
        wr(VAR + 0xC, 0)
        rec = S.M + 0xA400
        mu.mem_write(rec, b"\0" * 0x40)
        wr(rec + 0x10, amount)
        S.call(0x6F1F6870, ecx=S.BUFF, stack_args=(rec,))
        rows.append((amount, has[0], has[1], rd(VAR + 0xC)))
    return rows


OLD_END = [0]


def main():
    args = sys.argv[1:]
    dump = None
    if args and args[0] == "--dump":
        dump, args = args[1], args[2:]
    cases = int(args[0]) if args else 400
    seed = int(args[1]) if len(args) > 1 else 5
    r = random.Random(seed)
    rows = run_effect(cases, r)
    for dur, hdur, extra, da, hero, dead, has, ev in rows[:6]:
        print("E dur=%08x hdur=%08x ex=%08x A=%08x hero=%d dead=%d has=%s -> %s" % (dur, hdur, extra, da, hero, dead, has,
              [(k, ["%08x" % x for x in a]) for k, a in ev]))
    if dump:
        os.makedirs(dump, exist_ok=True)

        def ev_text(ev):
            parts = []
            for k, a in ev:
                parts.append(k if k == "init" else "%s %08x" % (k, a[1]))
            return " ; ".join(parts) or "-"

        with open(os.path.join(dump, "sleep_effect.txt"), "w") as fh:
            for dur, hdur, extra, da, hero, dead, has, ev in rows:
                fh.write("E %08x %08x %08x %08x %d %d %d %d %d | %s\n" % (
                    dur, hdur, extra, da, hero, dead, has[0], has[1], has[2], ev_text(ev)))
        wake = run_wake(cases, r)
        with open(os.path.join(dump, "sleep_wake.txt"), "w") as fh:
            for amount, st, sp, ended in wake:
                fh.write("W %08x %d %d | %d\n" % (amount, st, sp, ended))
    print("cases", len(rows))


if __name__ == "__main__":
    main()
