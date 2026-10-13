# Undead and Night Elf hero spells (S26): Death Coil, Frost Nova, Entangling Roots, Force of Nature

Code: `crates/sim/src/undead_ne.rs` (class effects inside the cast pipeline of `cast.rs`), tests
`crates/sim/tests/undead_ne_spells.rs`, harness `tools/emu/undead_ne_emu.py`, vectors
`crates/sim/tests/data/undead_ne/relation.txt`. Labels: CONFIRMED / LIKELY / UNSURE as in abilities-5.md.

## Classes (RTTI, vtables; effect slots)

| Ability | Class | vtable | Effect slot |
|---|---|---|---|
| AUdc | CAbilityDeathCoil | `0x6F8B1F84` | `0x424` = `0x6F0AD110` |
| AUfn | CAbilityFrostNova | `0x6F8F6384` | `0x424` = `0x6F1606C0` (primary), `0x434` = `0x6F176CD0` (area), `0x430` = `0x6F18A830` (slow buff) |
| AEer | CAbilityEntanglingRoots | `0x6F8A6824` | `0x424` = `0x6F0B8290` |
| AEfn | CAbilityForceOfNature | `0x6F8AF504` | `0x418` = `0x6F0B9CE0` (point effect), `0x380` = `0x6F0A1160` (target validation) |
| AUdp, AUcs, AUsl, AUav, AEsh, AEbl, AEim | DeathPact `0x6F8A3DE4`, CarrionSwarm `0x6F91150C`, Sleep `0x6F9215CC`, ShadowStrike `0x6F91F8D4`, Blink `0x6F91087C`, Immolation `0x6F8D7DD4` | | **not done** (vtables located only) |

The cast sequence, mana payment and cooldown are the shared ones of abilities-5.md. All four share slot `0x338`
`0x6F054880`. Level record getters (CONFIRMED by disassembly): `0x6F02E9E0` = DataA (`+0x20`), `0x6F02EA50` = DataB
(`+0x24`), `0x6F02E900` = Area (`+0x18`), `0x6F02E760` Dur, `0x6F02E7D0` HeroDur.

## Death Coil (CONFIRMED structure, Unicorn on the selector)
`v = DataA`. The selector `0x6F08EAC0` (EAX = ability, ESI = target unit, stack = table) returns heal (+1), nothing (0)
or damage (-1): index `2*(ally | 2*enemy) + undead` into the table at `0x6F88E1F4` = `[-1,1,0,1,-1,0,-1,0]`
(Holy Light uses `0x6F88E1D4` = `[1,-1,1,0,0,-1,0,-1]`). `enemy` = `0x6F285B00` (caster owner not passively allied to
the target owner, CONFIRMED by `0x6F3A3400`: bit of the alliance mask), `ally` = `0x6F285B80` (second mask test,
modelled as same owner: UNSURE), `undead` = class bit 2 (`+0x248`) or race 5. Damage `-1`: `v / 2.0` (constant
`0x6FAAE504` = 2.0), class `0x40000`. Heal `+1`: `v`, sign-flipped into the record; an ethereal target multiplies
by Misc `EtherealHealBonus` (not modelled). **Unicorn: 800 cases over both tables, 0 mismatches.**

## Frost Nova (structure CONFIRMED, slow strength UNSURE)
Primary target `0x6F1606C0`: DataB through the damage slot (`0x120`), class `0x200` (slot `0x42C` = 512). Area
`0x6F176CD0`: group builder `0x6F2A3270` over Area (`0x6F02E900`) around the target, callback `0x6F1607B0` per unit:
an invulnerable target (slot `0x11C`) takes 0.0, others DataA; the buff slot `0x430` applies HeroDur for a hero,
else Dur (+ the `+0x9C` term). Enemy-only filter (`0x8000` flag) LIKELY. **The slow amounts are in the buff class and
were not decoded**: the model reads DataC (move) and DataD (attack) as deltas. Buff id `Bfro` is a stand-in.

## Entangling Roots (UNSURE model)
Duration HeroDur / Dur, buff via `0x6F0B8220`. The model roots with a full move slow and ticks DataA damage each
second (tick period and class not decoded; no DoT code found in the cast slot, it lives in the buff class).

## Force of Nature (structure CONFIRMED, order UNSURE)
`0x6F0B9CE0`: tree query in Area (`0x6F2AE9E0`, filter `0x6F09DF40`), `N = F2I(DataA)` (`0x6F6EEC20`) caps the count,
UnitID (`+0x44`, host API `Game::set_summon_unit`) created per tree with timed life Dur (`BEfn`), trees die. Tree order
is destructable creation order and the tree test is "second id character is `T`" (UNSURE).

## Open
* Buff classes (Frost Nova slow, roots DoT) and BuffIDs; ethereal factor; `ally` relation meaning (`0x6F3A36D0`).
* Order ids come from public lists (852001 deathcoil, 852226 frostnova, 852171 entanglingroots, 852176 forceofnature): UNSURE.
* Remaining abilities of the task (AUdp, AUcs, AUsl, AUav, AEsh, AEbl, AEim) are not implemented.
