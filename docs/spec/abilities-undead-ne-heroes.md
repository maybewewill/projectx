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

## Frost Nova (structure CONFIRMED; slow strength CONFIRMED in S33, see below)
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

## S33: Frost Nova slow, Roots pulse, Shadow Strike roles, Sleep keeper, Dark Ritual, Immolation notes

Harness `tools/emu/undead_ne3_emu.py` (`GAME_DLL=... CHECK=frost|pulse|apply|roots|launch|buffinit|immo py ... [--dump DIR] [N]`),
vectors `crates/sim/tests/data/undead_ne/{frost,pulse}.txt`, tests `undead_ne_vectors.rs`, `undead_ne_spells.rs`,
`undead_ne_s28.rs`. Class names from RTTI: `CBuffFrost` `0x6F9076A4`, `CBuffEntanglingRoots` `0x6F897764`,
`CBuffShadowStrike` `0x6F91FD04`, `CBuffSleepPause` `0x6F920EFC`, `CBuffImmolation` `0x6F8CFA14`, `CBuffImmolationAoe` (`BEia`) `0x6F8CF6B4`.
Buff property objects are 8 bytes: the vtable pointer at `+X` and the value at `+X+4`.

### Frost Nova (CONFIRMED)
* Strength: **not ability data but the Misc keys `FrostMoveSpeedDecrease` / `FrostAttackSpeedDecrease`** (stock MiscGame.txt: 0.5 and
  0.25; the reader's default for a missing key is 0.0). `CBuffFrost` slot `0x360` (`0x6F199F40`) reads them through the Misc real
  reader `0x6F009EE0` when its 4th argument is 0 (zeros otherwise) and hands both, sign flipped, to the shared setter `0x6F0D6500`
  (properties `+0xCC`/`+0xD4`, values `+0xD0` move / `+0xD8` attack). Apply = `0x6F202FF0` on the move ability and `0x6F0C9090` on
  the attack ability (the same Slow machinery as Aslo). **Unicorn: 400 cases, 0 mismatches** (`CHECK=frost`, `frost.txt`).
  Sim: `Game::set_frost_misc`; the engine fills it from `MiscData::frost_*`. The AbilityData row of AUfn has only DataA (area/nova
  damage 50/100/150) and DataB (target damage 100); DataC/DataD do not exist, the old model read zeros.
* Order (CONFIRMED by the call order in `0x6F1606C0`): the area routine (slot `0x434`) runs first, then the primary target's own
  DataB damage (class `0x200`); the primary routine applies **no buff**. The buff comes only from the per-unit area callback
  `0x6F1607B0`, which takes DataA and skips the buff for invulnerable or blocked (`0x6F28D750`) units. The primary
  target is the centre of the area and nothing in the callback excludes it, so it takes DataA + DataB and carries the buff
  (LIKELY; the group builder's mask `0x6F2A3270` was not decoded: bits `0xF000` cleared, `0x10000` set, `0x8000` when the target is allied).
  The sim now does the area first (all enemies in Area, target included), then DataB.

### Entangling Roots (pulse CONFIRMED)
`CBuffEntanglingRoots` uses the base periodic family. Roles (Unicorn `CHECK=roots`): the effect hands the buff a pointer to **DataA**
and the duration (HeroDur, or Dur + `+0x9C`); the base apply `0x6F0D5390` stores its 4th argument as the property value at `+0xEC` (`CHECK=apply`)
and slot `0x360` returns it as the pulse amount. First pulse: message `0xD01E2` armed by slot `0x358` (`0x6F0ADBD0` returns `0x6FAAE480`, the
start-up value **0.01 s**); its handler `0x6F0D52D0` re-arms `0xD01C1` with slot `0x35C` (`0x6F0ADBE0` returns `0x6FAAE4F4` = **1.0 s**) and
calls the pulse `0x6F0D54B0`; the buff flag `0x8000000` is set until the first pulse is done. Pulse body
(Unicorn `CHECK=pulse`, 600 cases, 0 mismatches, `sim::undead_ne::pulse`): nothing while the target has `'BUsl'`; nothing when
Misc `MagicImmunesResistDamage` is set and (target `+0x15C` > 0 and class test A, or `+0x158` > 0 and class test B); an amount below 0 (NaN is
not below) is multiplied by Misc `EtherealHealBonus` and given through AddLife; otherwise a damage record (amount DataA, **class `0x100000`**,
attacker = the buff's source, record flags 2, or 0 on the first pulse) goes to the target's slot `0x120`. The root/disarm part is applied by
`0x6F09BA50` (not modelled beyond the full move slow). Model: ticks at 0.01 s, 1.01 s, ...; damage type 20.

### Shadow Strike (data roles CONFIRMED, slow decay UNSURE)
Property flow checked in Unicorn (`CHECK=launch`, `CHECK=buffinit`): the missile launcher `0x6F1D5490` stores DataA, DataB, DataC, Cast,
DataD in the missile's properties `+0xE8/+0xF0/+0xF8/+0x108/+0x110`; the arrival `0x6F1FB760` passes them to the buff init slot `0x38C`
(`0x6F1D5290`), which puts **Cast into the property `+0x128` (value `+0x12C`)**, DataD into `+0x130`, the missile's duration word into `+0x138`,
DataB/DataC into `+0x104/+0x10C`, and DataA is the base apply's 4th argument (the pulse amount). The slots `0x358`/`0x35C`
(`0x6F1F61B0`/`0x6F1F61C0`) both return `+0x12C`: **the damage-over-time period and the first delay are the Cast field** (stock 3.0 s; with Dur 15.1 s
the pulses come at 3, 6, 9, 12, 15 s). DataE is the initial damage (record flags `0x800000`), DataA the amount of each pulse, pulse class `0x2000000`
(type 25). The slow: a 1.0 s timer (`0xD01C3`) recomputes it with slot `0x390` (`0x6F1DD190`: a ratio of two soft `pow` terms with exponent DataD
and the buff's remaining time) and applies it through slot `0x37C` (`0x6F11BBF0`, move and attack together, scaled by DataB/DataC). The decay is
not decoded; the model keeps a constant slow of DataB. Consequence for `cast.rs`: Shadow Strike's level Cast is not a cast-time wait
(`undead_ne::cast_field_is_period`).

### Sleep (second buff CONFIRMED)
`0x6F1FBA80`: the BuffID list of AUsl is `BUsl,BUsp,Bust`. After the main buff the effect looks up `'BUst'` then `'BUsp'`: with `'BUst'` present that buff is refreshed with
DataA, else `'BUsp'` (`CBuffSleepPause`, constructor `0x6F1F9DA0`) is refreshed or created, **DataA = its duration** (stock 2 s; the generic apply `0x6F0D66E0` passes
the pointer to the base duration setter, and a zero value starts no timer). While `'BUsp'` or `'BUst'` exists damage does not wake the sleeper (handler `0x6F1F6870`),
so a sleeper cannot be woken during the first DataA seconds. The non-hero duration is Dur + the ability's `+0x9C` term (`0x6F02F880`). Sim: `sleep()` adds the keeper marker buff.

### Death Pact and Dark Ritual
Death Pact's class derives from `CAbilityDarkRitual` and both share the effect slot `0x424` = `0x6F09FD50`. Dark Ritual (`AUdr`, order 852219,
getter `0x6F0BD230`) therefore uses the S28 body with DataA = mana factor (0.33/0.66/1.0) and DataB = 0. The DataE != 0 branch (the target loses life twice through slot
`0x128`, the caster receives what the target reports) is **dead with the stock data** (DataE is 0 in every AUdp/AUdr level); structure documented, not modelled.

### Vampiric Aura check `0x6F28D750`
The "message `0xD0142` victim test" of S28 was a misreading: `0x6F28D750(unit, source)` returns non-zero when the unit is flagged (`0x6F28D610`/`0x6F28D6D0`) or when any ability in its
list (`+0x1DC`) answers slot `0x26C(source)`, i.e. a generic "blocked by the victim's abilities" test also used by the Frost Nova callback and the Immolation victim code. Not modelled.

### Immolation (structure only, numbers still UNSURE)
`AEim` data: Dur 1, Area 160, DataA 10 (tooltip: damage per second), DataB 7 (mana per second), DataC 10; BuffID `BEim,BEia`. The ability's slot `0x3C0` (`0x6F13B550`) builds `CBuffImmolation` through
its slot `0x368` (`0x6F12C4D0`, 10 arguments; Unicorn `CHECK=immo`: unit, 0.0, &DataB, &Dur, &Area, &DataA, 0, owner, order id, ability). The buff keeps a mana
regeneration modifier of **minus DataB** (continuous, `+0x118`) and is removed when the unit's mana is below 1.0 (message `0xD01BA`, handler `0x6F1176E0`). Its pulse (`0xD01C1`,
`0x6F110190` -> slot `0x360` = `0x6F1249A0`) is skipped when the caster is dead or removed, has flags `0x1000000` / `0x4000000`, or has `'Bcyc'`; otherwise it enumerates the units in Area
(`0x6F088050`, callback `0x6F1101F0` -> slot `0x364` = `0x6F13B2A0`), skipping dead or blocked ones (`0x6F28D750`), and gives each a child buff `'BEia'` (`CBuffImmolationAoe`) or refreshes it.
The damage is therefore most likely dealt by the victim buffs on their own period, not by one global pulse; the model keeps the global 1 s pulse (Dur = 1 is consistent with stock data). Not decoded: the Aoe buff's damage.

### Not progressed in S33
Carrion Swarm missile walk, Blink placement, Inferno, Starfall, Tranquility, Metamorphosis, Death and Decay. Order ids from the original getters (`tools/analysis/ability_orders.py`):
AUdr 852219, AUdd 852221, AUin 852224, AEme 852180, AEsf 852183, AEtq 852184. Atlas pointers: Shockwave (`AOsh`) derives from `CAbilityCarrionSwarm`; Death and Decay effect slot `0x418` = `0x6F0A98F0`.

### S33 CONFIRMED list
Frost Nova slow source and sign (Unicorn 400), Frost Nova call order and the buff-less primary routine, Roots pulse period, first delay, amount, class and skip rules (Unicorn 600 plus the effect-argument run),
Shadow Strike data roles and the Cast pulse period (Unicorn property flow), Sleep keeper buff and DataA, Dark Ritual = Death Pact effect, Death Pact DataE branch is dead data, Immolation
mana drain = regen modifier of minus DataB and the off-below-1-mana rule, the identity of Vampiric's `0x6F28D750` check.

## S37: swath engine (Carrion Swarm / Shockwave), Immolation marker, Shadow Strike decay, Inferno, Death and Decay

Harness `tools/emu/undead_ne4_emu.py` (`GAME_DLL=... CHECK=launch|tick|aoe|decay|apply|inferno|dnd py ... [--dump DIR] [N] [SEED]`), vectors
`crates/sim/tests/data/undead_ne/{swarm_launch,swarm_tick,immo_marker,strike_decay,strike_apply,inferno_ctx,dnd_pulse}.txt`, tests
`undead_ne_s37_vectors.rs`, `undead_ne_s37_strike.rs`, `undead_ne_s37_ctx.rs` (Unicorn vectors, `GAME_DLL` needed for the soft-float tables) and
`undead_ne_s37.rs` (behaviour). Code `crates/sim/src/undead_ne.rs` (pure functions `safe_div`, `gt_eps`, `rescale`, `swath_*`, `strike_*`,
`inferno_ctx`, `dnd_pulse_amount`, `aoe_marker_duration`) plus `Game::set_spell_buff_slow` in `cast.rs`. Order ids (original getters,
`ability_orders.py --verify`): AUin 852224, AUdd 852221, AEsf 852183, AEtq 852184, AEme 852180, AUfa 852225 (AUfu autocast on/off 852458/852459),
AUim 852555, AUcb cast 852551 (autocast 852552/852553), AUls 852556, AEfk 852526, AEmb 852179, AEst 852181, AEsv 852528, AHfa cast 852173
(autocast 852174/852175).

### Level data roles corrected (CONFIRMED by the missile setup `0x6F1F28C0` run in Unicorn and by the stock rows)
`AUcs`/`AOsh`: `Area` = start width, `DataA` = damage per hit, `DataB` = **total cap** ("max damage"), `DataC` = **distance**, `DataD` = final
width (stock AUcs 100 / 75,125,200 / 300,600,1000 / 800 / 300; AOsh 125 / 75,130,200 / 900,1560,2400 / 800 / 125). The S28 text above that calls DataB the
distance and DataC the end width is wrong; the old model used those roles.

### The swath engine (shared by `AUcs` and `AOsh`: `CMissileShockwave` derives from `CMissileCarrionSwarm` and overrides no tick/hit/setup code)
* **Launch geometry** `0x6F1DD4F0` (CONFIRMED, Unicorn 400 cases, 0 mismatches, `sim::undead_ne::swath_launch`): `dx, dy` = point - caster (soft
  sub), `d2 = dx*dx + dy*dy` (soft mul/add). If `d2 >= 400.0` (`20.0` squared by the soft multiply; NaN and smaller take the other branch) the direction
  vector is `(dx, dy)` rescaled to length `Area` by `0x6F07B870` (`k = Area * recip(sqrt(x*x + y*y))`, `(x*k, y*k)`, the same code Blink uses);
  otherwise the polar vector `(Area*cos(f), Area*sin(f))` with `f` the caster's facing in radians (`0x6F10E370`: **cos for x, sin for y**).
  `start = caster + v1` (the swarm starts `Area` units in front of the caster, not at the caster), `end = start + rescale(v1, DataC)`.
* **Setup** `0x6F1F28C0` (CONFIRMED mapping by property logging): start x/y -> properties `+0x90/+0x98`, DataC -> `+0xA0`, Area -> `+0xC8`, DataD -> `+0xD0`,
  DataA -> `+0xD8`, DataB -> `+0xE0`, constant `3.0` (`0x6FAB35FC`) -> `+0xEC`, caster handle, owner index, targs word `(ability+0x4C & 0xFFFF0FFF) | 0x10000`
  (the relation bits replaced by "enemy"; with a unit target that is an enemy and not allied `0x8000` "neutral" is added). **The setup runs one tick at
  once** (at the start point).
* **Tick** `0x6F1EF6D0` (message `0xD01C1`; CONFIRMED numerics, Unicorn 600 cases, 0 mismatches, `swath_radius`, `swath_damage_each`, `swath_cap_after`,
  `swath_rearm`): `dist = sqrt(dx^2 + dy^2)` from the start point to the missile position, `frac = safe_div(dist, DataC)` (`0x6F022530`: a plain soft divide
  unless the divisor is 0.0; then 0/0 = 1.0, otherwise +-FLT_MAX by the numerator's sign), `radius = Area + (DataD - Area) * frac`. The group builder
  (`0x6F2A3270`, shape `0xB` = circle, mask `0x1F`, args radius/x/y) is asked for the units around the **missile position**; the callback `0x6F1E7B00` keeps at
  most **64** units that pass the target acceptance (`0x6F25D6F0` with the setup's targs word; enemy only for a point cast) and are not marked (below).
  With `n` units: `share = safe_div(cap, I2R(n))` (`0x6F0223C0`); each unit gets `D = DataA` when `gt_eps(share, DataA)` else `share`
  (`gt_eps` = `0x6F07C450`: `a > b` by the x87 compare **and** `|a - b| >= 0.001`). Per unit in list order: life before (unit slot `0x12C`), the hit
  `0x6F1DD3A0` (a damage record of class `0x8000`, record flags `0x41`, `0x51` for a blocked unit, attacker = the caster handle), life after; **the cap
  falls by the life really lost**: `cap = sub(cap, sub(before, after))`. Then `ec = ec - (0.1 + 0.02)` (property `+0xEC`, a countdown nothing reads),
  and the timer `0xD01C1` is re-armed with 0.1 s (`0x6FAB35F8`) only while `gt_eps(cap, 0.01)`.
* **Hit once**: each hit unit gets the marker buff `'BUcs'` (`0x6F1FB8E0`, duration `0.1 + 0.02`); the callback's per-ability predicate `0x6F1E79E0` refreshes
  the marker of a unit it meets again and rejects it. So a unit that stays inside the swath at consecutive ticks is hit once; one that leaves for one tick is hit
  again (LIKELY: read, not run).
* **Arrival** slot `0xBC` (`0x6F1F2A20`): when `gt_eps(cap, 0.0)` one last tick runs at the end point, then the missile is destroyed.
* **Not decoded (UNSURE)**: the flight itself (the model moves the missile linearly at the ability's `Missilespeed`: AUcs 1100, AOsh 1050 from start
  to end), the first tick's relation to the creation instant, and the class mapping of the sim's damage path for class `0x8000` (bit 15 is passed as the damage
  type). Orc lane: call `undead_ne::swath_cast(game, rt, caster, level, point, SHOCKWAVE_TARGS, SWATH_DAMAGE_TYPE, AOSH)`.

### Immolation (`AEim`): the victim buff `'BEia'`
`CBuffImmolationAoe` derives from `CBuffAoe` and overrides only the code getter, destructor and one id; `CBuffAoe` makes it a **duration-only marker** (slot `0x354`
returns 1 so the base bonus apply/remove `0x6F0AE0B0/0xD0` do nothing). Its duration is `value - 0.01` when `0.0 < value` (apply `0x6F0D6040` and
refresh `0x6F0D60A0`, both CONFIRMED, Unicorn 320 cases, `aoe_marker_duration`); the pulse callback `0x6F13B2A0` creates it with `Dur` or refreshes a live
one. **Where the damage comes from was not found**: no call to the damage-record constructor `0x6F2556E0`, the damage slot `0x120`, AddLife or the
target-damage routine appears within two call levels of any vtable function of `CBuffImmolation`, `CBuffLightningShield`, `CBuffImmolationAoe`, `CBuffAoe`
(`scanfn` over the RTTI vtables), and the area collector `0x6F2C31D0` only collects. The model keeps the global pulse (DataA per `Dur` seconds to
enemies in Area) and now also applies the marker. UNSURE: whether the original damages through a life-regeneration modifier on the victim.

### Shadow Strike (`AEsh`): the slow decay (CONFIRMED numerics)
Stock DataA 10 (pulse damage), DataB 0.5 (move slow), DataC absent (attack slow 0), DataD 3 (decay exponent), DataE 75 (initial damage), Cast 3 (pulse period),
Dur 15.1. Slot `0x390` (`0x6F1DD190`, Unicorn 400 cases, `strike_decay_factor`): `f = pow(|T - Dur|, DataD) / pow(Dur, DataD)` (soft pow, soft divide), snapped to 0.0
below 0.001 (`0x6F1D2A10`), 0.0 when the buff flag `0x80` is clear or `Dur` is within 0.001 of 0.0; `T` is the buff timer's slot `0x1C`. Slot `0x37C`
(`0x6F11BBF0`, Unicorn 400 cases, `strike_slow_amounts`): the move amount `m*prev - m*f` and the attack amounts `a*prev`, `(-a)*f` (flag bit 26 of the buff
flags) are ADDED to speed factors that start at 1.0 (`0x6F202FF0`, `0x6F0C9090`). The creation call uses `prev = 0, f = 1.0` (slow = `m`), afterwards `prev = 1.0`
is stored and the 1.0 s timer `0xD01C3` (`0x6F1D53B0`, handler clamps `f` at 0.0) applies the differences, so the net slow is `m * f`. UNSURE: `T` as the elapsed
time (the machine code of slot `0x1C` computes a difference of two timer cells that was not run; with the elapsed reading the slow starts at DataB and
decays to 0 at the end, the physically sensible direction). Sim: `strike_slow_tick`, `Game::set_spell_buff_slow`.

### Inferno (`AUin`): CONFIRMED context roles (Unicorn 320 cases, `inferno_ctx`)
Effect `0x6F13BC60` -> `0x6F117B40` copies the level record into the landing context: targs word, `Area` (landing circle), `DataA` damage 50, `DataB` the Infernal's
lifetime 180 (a timed life), `DataC` the **impact delay** 1.0 (the buff `CBuffInfernal` arms `0xD01C1` with it), `UnitID` `ninf`, first BuffID `BNin`, `Dur` 4 / `HeroDur`
2 (stun), plus constants 110.0 / 100.0 / 1000.0. The effect creates the unit through `0x6F13BB20` (`0x6F29F990` CreateUnit) and the buff; after the delay the landing
`0x6F142E60` queries the circle (shape `0xB`, mask `0x1F`; damage class `0x20000`) and per unit `0x6F13BCE0` builds the record (amount `DataA`, or 0.0 for a blocked
unit; flags `0x40`, `0x50` when blocked), delivers it through slot `0x120` and gives the unit the stun buff with `HeroDur` for a hero (`0x6F1D2660`) else `Dur`
(refreshing a live one). A second query (`0x6F2AE9E0`, callback `0x6F10F1F0`) handles debris (not modelled). Order 852224.

### Death and Decay (`AUdd`): CONFIRMED victim pulse (Unicorn 320 cases, `dnd_pulse_amount`)
`CBuffDeathAndDecayAoe` (`'BUdd'`, a `CBuffAoe`) handler `0x6F098180` (message `0xD01C1`): re-arms 1.0 s first, then `amount = DataA * max_life` (soft multiply, unit slot `0x130`
= max life), a record of class `0x4000000` with attacker = the buff's source unit and flags 0 is delivered through slot `0x120` (also when the amount is 0.0), and the buff is
removed when the victim's dead flag (`+0x5C & 0x100`) is set. The buff's init slot `0x35C` (`0x6F09FBF0`) starts the first pulse after a random delay: one draw of **RNG stream
`0x16`** (`0x6F2853D0`, `U` in [0, 1)). The ability's effect `0x6F0A98F0` first removes the caster's earlier channel buffs (three buff kinds), queries destructables in
Area (`0x6F2AE9E0`) and creates the effect object `XUdd` (`CEffectDeathAndDecay` < `CEffectTranquility` < `CEffectRangeListener`: the buff follows range-listener events).
Model: local channel (Dur 35 s from the cast, `dnd_stop` for the lifecycle owner), a 0.1 s poll stands in for the range listener, enemies only (UNSURE), first pulse after the
stream-22 draw, then every 1.0 s. DataB (stock 1) is not used by the pulse; role unknown.

## V2: verification wave (Unicorn against the original, 0 mismatches required)

Harnesses `tools/emu/v2_*_emu.py` (`GAME_DLL=... py tools/emu/v2_NAME_emu.py --dump crates/sim/tests/data/v2 400 SEED`), vectors
`crates/sim/tests/data/v2/*.txt`, tests `crates/sim/tests/v2_vectors.rs`.

* Thorns Aura / Spiked Carapace reflect (`AEah`, `AUts`; `CBuffAuraThorns` handler `0x6F1AD4E0`): CONFIRMED, 400 cases (`v2_thorns_emu.py`):
  percent mode (buff flag `0x100`) `strength * (amount - reduced)`, flat mode the bare `strength`, both floored at 1.0; the gates (record
  ignore flag, class `0x10`, allied, invulnerable, blocked) and "an illusion owner delivers nothing but plays the effect" match. The model
  had the right percent arithmetic; the flat mode was added. Open: what sets the percent flag (stock `DataB = 1`).
* Fan of Knives (`AEfk`), Mana Burn (`AEmb`) and Spirit Link (`Aspl`): see `abilities-hero-extra.md` and `abilities-unit-m.md` (three bugs fixed).

## V5 verification (Unicorn, 0 mismatches required)

### Frost Armor (`AUfa`) CONFIRMED (cast values and retaliation gates)

`tools/emu/v5_frostarmor_emu.py` runs the real effect `0x6F0BA520` (unit target) and the buff creator `0x6F0BA2E0` on a fake ability
and target, 400 cases (`tests/data/v5/frost_armor_cast.txt`); `tools/emu/v5_frostarmor_h_emu.py` runs the real retaliation handler
`0x6F0BA3F0` of `CBuffFrostArmor` on a fake buff, record and attacker, 400 cases (`frost_armor_hit.txt`); test
`tests/v5_frost_armor_vectors.rs` (`undead_ne::frost_armor_cast`, `frost_armor_hit_runs`).

* The buff apply `0x6F0A1280` gets `(target, &DataA, &chill, &DataB, buff id)`: `DataA` is the **buff's own duration** (base apply slot
  `0x320`), the **chill duration** goes to the first holder (`+0xCC`, value `+0xD0`) and `DataB` to the second (`+0xD4`, value
  `+0xD8`), which `0x6F09E080` hands to the armor sink `0x6F2769C0` (undone by `0x6F09E0D0`). So armor = `DataB`, as the model had.
* **Bug fixed:** the chill duration is chosen at cast by the hero test of the unit that **gets the armor** (`HeroDur` for a hero,
  else `Dur + ability+0x9C`); the model tested the attacker at hit time.
* Handler gates in the original order: record ignore flag bit 0 (ranged), class `0x10`, attacker slot `0x11C` (invulnerable),
  spell immunity `0x6F28D750`; then the effect `0x6F053040` and the chill: a live `Bfro` on the attacker is refreshed through slot
  `0x360`, otherwise created (slot `0x324`) and applied through slot `0x35C`; both take the stored duration and a flag argument =
  bit 30 of the **armored** unit's `+0x5C` (illusion). With the flag set `0x6F199EA0` / `0x6F199F40` use strengths 0.0 instead of the
  Misc `FrostMoveSpeedDecrease` / `FrostAttackSpeedDecrease` (**bug fixed**: the chill ignored the flag; Frost Nova's own
  slot has the same rule and is unchanged here).
* Not run: the autocast form `AUfu`, the handler's registration, the Misc strengths (Frost Nova's run covers them).

### Sleep (`AUsl`) CONFIRMED (V5)

`tools/emu/v5_sleep_emu.py`: the real effect `0x6F1FBA80` (400 cases, `tests/data/v5/sleep_effect.txt`) and the real wake handler `0x6F1F6870` (400 cases, `sleep_wake.txt`), test `tests/v5_sleep_vectors.rs` (`undead_ne::sleep_plan`, `sleep_wakes`). Confirmed: a dead target (`+0x5C & 0x100`) is untouched; main buff duration HeroDur (hero) else Dur+`+0x9C`, refreshed if live (slot 0x358) else created (0x354); keeper: BUst present -> refreshed with DataA (slot 0x360, 4 args), else BUsp present -> refreshed with DataA, else a new BUsp created with DataA; wake: amount != 0.0 (NaN counts) and no BUst/BUsp. No model bug found; `sleep()` now follows `sleep_plan`.
