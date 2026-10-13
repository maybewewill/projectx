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
| AUdp, AUcs, AUsl, AUav, AEsh, AEbl, AEim | DeathPact `0x6F8A3DE4`, CarrionSwarm `0x6F91150C`, Sleep `0x6F9215CC`, ShadowStrike `0x6F91F8D4`, Blink `0x6F91087C`, Immolation `0x6F8D7DD4` | | S28, see the last section |

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
* The remaining seven abilities are in the S28 section below.

## S28: Death Pact, Blink, Vampiric Aura, Carrion Swarm, Sleep, Shadow Strike, Immolation

Code `crates/sim/src/undead_ne.rs` (+ `auras.rs` `AuraKind::Vampiric`, `cast.rs` `BuffEffect::Sleep`, lifesteal and
wake hooks in `combat.rs`), harness `tools/emu/undead_ne2_emu.py` (run once per `CHECK=deathpact|vampiric|blink`, `--dump
crates/sim/tests/data/undead_ne`), tests `undead_ne_vectors.rs` (Unicorn vectors) and `undead_ne_s28.rs` (behaviour).
Level getters used: DataA `0x6F02E9E0`, DataB `0x6F02EA50`, DataC `0x6F02EAC0`, DataD `0x6F02EB30`, DataE `0x6F02EBA0`,
Area `0x6F02E900`, Dur `0x6F02E760`, HeroDur `0x6F02E7D0`. Buff id getters (vtable slot 7 of the buff class): Sleep
`'BUsl'`, Shadow Strike `'BEsh'`, Immolation `'BEim'`, Vampiric `'BUav'`, Frost Nova `'Bfro'`, Roots `'BEer'` (all CONFIRMED;
this closes the "buff id is a stand-in" note for Frost Nova and Roots). Sleep keepers `'BUst'` = `CBuffSleepStun`, `'BUsp'` =
`CBuffSleepPause`.

### Death Pact `CAbilityDeathPact`, effect slot `0x424` = `0x6F09FD50` (CONFIRMED, Unicorn 500 cases, 0 mismatches)
`life` = the target's current life (unit slot `0x12C`). `mana = DataC == 0.0 ? life * DataA : DataA`, `heal = DataD == 0.0
? life * DataB : DataB` (soft multiply, x87 compare, NaN is not equal). DataE == 0: the owner gets the kill credit
(`0x6F2829D0`), the target's flags lose `0x200` and gain `0x1000` (meaning UNSURE, probably "killed by an ability"), the
target's life is set to 0 (slot `0x124`); then, only if the target is not an illusion (`+0x5C & 0x40000000`), the caster
receives `mana` through AddMana (`0x6F279140`, listeners `84..98`) and `heal` through AddLife (`0x6F277C20`, slot `0x128`,
listeners `68..82`), each only when `0.0 != value` (a NaN counts as non-zero). Order: credit, set life, mana, life.
DataE != 0 is a drain variant (the target loses both amounts via slot `0x128` and the caster gets the reported
changes): UNSURE, not modelled (the model does nothing). Default data (A = 0, B = 1, C = D = E = 0) are the public
data and are UNSURE. Unicorn setup: unit slots `0x124/0x128/0x12C`, the three helpers and the credit call are Python hooks
that record their arguments; the ability, the level record and the target are synthetic.

### Blink `CAbilityBlink`: validation `0x6F1AEB10`, cast start `0x6F1AEFF0`, effect `0x6F1AF180`
DataA is the maximum range. `0x6F1AEB10` (CONFIRMED, Unicorn 500 cases, 0 mismatches with the placement query stubbed to
the identity): with `d2` the squared caster-to-point distance (soft) and `R2 = DataA * DataA`, if `d2 > R2` the point becomes
`caster + (dx, dy) * (0.8 * DataA * recip(sqrt(d2)))` (0.8 = `0x6FAAE4EC`, normalise helper `0x6F07B870`), else it is kept.
The original then asks the caster's placement query (unit slot `0xDC`, pathing flags `0x6F2ABF20`) for a legal spot and
writes it back; not modelled. `0x6F1AEFF0` clamps the stored point into the playable area (`0x6F469490`), runs the
validation and, if the point is still beyond DataA (`R2 < d2`), replaces it by the caster's position (no move). The effect
(`0x6F1AF180`) places the caster through unit slot `0x180` with the placement flags. Model: `blink_project`, the second
range check, the playable clamp, `store_unit_position`. Without the soft-float tables the projection cannot run, so an
out-of-range point falls through to "no move".

### Vampiric Aura `CAbilityAuraVampiric` / `CBuffAuraVampiric`, handler slot `0x170` = `0x6F1AD980` (CONFIRMED, Unicorn 500 cases, 0 mismatches)
The buff stores DataA at `+0xD0` (a fraction, e.g. 0.15..0.6). Stage order after a hit of `damage` on `victim` by the
buff's unit: the record's ignore flag (`+0xC & 1`), a mechanical victim (class `+0x248 & 8`), the pair must be enemies
(`0x6F285B20` = not passively allied), the victim must not be invulnerable (slot `0x11C`), the victim must not answer message
`0xD0142` (`0x6F28D750`; not modelled), `damage > 0` (soft `le` `0x6F01FD60`); then `heal = damage * fraction` (soft
multiply `0x6F01FD00`); the heal is given with AddLife of the attacker when neither side is an illusion or Misc
`IllusionsCanRestoreLife` is non-zero (`0x6F009F90`); the effect routine `0x6F053040` runs whenever the gates were passed.
The aura plumbing is the existing one (`AuraKind::Vampiric`, default buff `'BUav'`, no stat sink). The model heals after an
attack's damage in `combat.rs` (`attack_damage`), with the final damage as `damage`; spell damage does not steal (the
handler is attack-side; whether spell damage reaches it is not decoded). The handler's registration point is UNSURE.

### Carrion Swarm `CAbilityCarrionSwarm`, effect `0x6F1E7D20` -> `0x6F1DD4F0` (argument roles CONFIRMED, geometry UNSURE)
`0x6F1DD4F0` reads Area (`0x6F02E900`), DataA, DataB, DataC, DataD and the caster's position; if the squared distance to
the target point is below `20.0^2` (`0x6FAB35F4`) the direction is the caster's facing, otherwise `normalize(point -
caster)`; two vectors of length Area and DataC are built (the swath's start and end widths) and the bounding rectangle is
clamped to the map; the line missile `CMissileCarrionSwarm` (`0x6F8FBAAC`) is launched (unit slot `0x42C`) with DataB the
distance, DataA the damage, DataD the cap. The missile's hit walk is not decoded; the model hits each live enemy
non-structure unit once inside the trapezoid in order of distance along the line and cuts the last hit at DataD.

### Sleep `CAbilitySleep` effect `0x6F1FBA80` (rule CONFIRMED, rest UNSURE)
Duration = HeroDur for a hero target, else Dur; buff `'BUsl'` through the buff factory (or `CBuffSleep` when the data have no
such buff). `CBuffSleep`'s event handler `0x6F1F6870` (slot `0xFC`): a damage event with amount `!= 0.0` ends the buff unless
the unit has `'BUst'` or `'BUsp'`. A second buff (`'BUst'` or `'BUsp'`, data-driven or `CBuffSleepStun`) is applied with
DataA as its argument; its meaning is UNSURE and not modelled. Model: `BuffEffect::Sleep` counts as a stun while it lasts;
`wake_sleeping` is called from `apply_damage` and `unit_damage_target`.

### Shadow Strike `CAbilityShadowStrike` effect `0x6F1F2670` (structure CONFIRMED, field roles UNSURE)
The effect builds a damage record with amount = DataE (`+0x44`, flags `0x800000`), then calls the missile launcher (slot
`0xD4`, `0x6F1D5490`) of `CMissileShadowStrike` (`0x6F916CFC`; the object type id is `'MEsh'`) with DataA, DataB, DataC,
DataD and a value from `0x6F02E6F0`; the launcher (`0x6F11BFF0`) stores them in the missile's properties. On arrival
(`0x6F1FB760`, slot `0xCC`) the buff `'BEsh'` is created with those stored values. The buff has one slow factor applied to
movement (`0x6F202FF0`) and attack speed (`0x6F0C9090`) together (`0x6F128E70`). Which Data field is the factor, the damage
per tick, the decay power and steps is not decoded; the model uses DataA = damage per second, DataB = slow fraction, no
decay, ticks every 1.0 s for Dur / HeroDur, flight = distance over the registered missile speed. UNSURE all of it.

### Immolation `CAbilityImmolation` / `CBuffImmolation` (structure CONFIRMED, numbers UNSURE)
An immediate toggle. The buff constructor (`0x6F12C4D0` / `0x6F12C690`) stores a drain and puts a regeneration modifier of
minus the drain on the mana property (and life for the other variant), then arms the pulse message `0xD01C1` through
`0x6F124800` / `0x6F1248D0` (`0x6F4778F0`). The ability's slot `0x36C` returns DataC. The pulse handler is not decoded; the
model takes DataA = damage per pulse to enemies within Area, DataB = mana drain per second, DataC = mana needed to turn it
on, a pulse every 1.0 s, and turns itself off when the mana cannot pay the drain. Order ids (public lists, UNSURE): 852177
immolation, 852178 unimmolation (both map to the toggle in the model). The immediate order class now reaches the cast
pipeline (`order.rs`, `begin_cast` accepts `CastTarget::None` for `undead_ne::is_immediate` abilities).

### Still open
Death Pact DataE != 0; Blink placement query; Carrion Swarm missile walk; Sleep DataA; Shadow Strike roles and decay;
Immolation pulse; whether Vampiric also reacts to spell damage; Frost Nova slow strength (the buff `'Bfro'` is
data-driven: the slow numbers are not in the ability's own code path) and Entangling Roots DoT (the buff `'BEer'` uses a
counted disable like a stun, `0x6F08BAF0`; the damage source was not found, DataA is passed to the buff constructor in EBX).
Order ids: public lists (852218 deathpact, 852525 blink, 852129 carrionswarm, 852227 sleep, 852527 shadowstrike).
