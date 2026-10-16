# Unit (non-hero) spells of the melee races in Warcraft III 1.26a (Game.dll) (S31)

Scope: the spells of ordinary units that the four melee computer players and the autocast rules use. Model:
`crates/sim/src/unit_spells.rs` (hooks in `cast.rs`, `auras.rs`, `natives/order.rs`). Tests:
`crates/sim/tests/unit_spells.rs`. Unicorn harness: `tools/emu/unit_spells_emu.py`. Addresses are 1.26a; no code or
tables are copied. Method: capstone reading of the vtable slots (class identity and slot lists from the RTTI
vtable map and the ability atlas in `docs/research/ability-atlas/`), Unicorn runs of the numeric/selection cores.

Labels: **CONFIRMED** = read in the machine code (and Unicorn-run where a count is given); **LIKELY** = strong
inference; **UNSURE** = a stand-in; **NOT DONE** = not modelled.

## 0. Order ids (CONFIRMED)

All ids below were re-checked against the original getters (`tools/analysis/ability_orders.py --verify`, cast = slot
`0x348`, autocast on = `0x308`, off = `0x318`; Unicorn-checked): Heal 852063/64/65, Inner Fire 852066/67/68, Bloodlust
852101/02/03, Cripple 852189 (no autocast pair), Curse 852190/91/92, Faerie Fire 852149/50/51, Web 852211/12/13, Raise
Dead 852197/98/99, Abolish Magic 852132/33/34, Dispel 852057, Purge 852111. First found from the registration code of
the DLL (immediates pushed beside the order strings): the registration code of the DLL pushes the order string and its id as immediates; the ids are read from there
(`heal` = 0xD005F = 852063 and so on, ids are `0xD0000 + index`). Heal 852063 / healon 852064 / healoff 852065;
innerfire 852066/67/68; dispel 852057; purge 852111; polymorph 852074; bloodlust 852101/02/03; lightningshield
852110; ensnare 852106; healingward 852109; raisedead 852197/98/99; unholyfrenzy 852209; cripple 852189; curse
852190/91/92; web 852211/12/13; rejuvination 852160; roar 852164; faeriefire 852149/50/51; autodispel 852132/33/34;
sentinel 852182; scout 852181.

## 1. The autocast lifecycle of `CAbilityAutoTargetSpell` (17 classes share it)

Static reading (CONFIRMED structure; the values marked below are UNSURE):

* A one-shot agent timer (message `0xD01C2`) lives on the ability. Activation (slot `0x438` = `0x6F044340`) arms it
  with 0.02 s (constant `0x6FAAE484`, runtime value `0x3CA3D70A`); deactivation (slot `0x43C`) sets the disabled flag
  `0x100000` and cancels it.
* The handler (`0x6F0443A0`) re-reads the period from slot `0x468` (`0x6F0231E0`: the constant `0x6FAAE50C` =
  **3.0 s**). It scans only when the autocast flag `0x80000` is on and slot `0x3D8` (`0x6F0379E0`) accepts the
  caster: its order list is empty, or the current order is **attack `0xD000F` or attack-move `0xD0014`**. Further
  gates on the caster (`+0x5C & 0x1000000`, `+0x20 & 0x4000000`, `+0x198 > 0`) are not modelled.
* The target search is slot `0x448` (`0x6F030680`): a group around the caster (radius from slot `0x440`, which
  combines the level's `Rng` with the caster's collision size and a scale), the per-unit callback `0x6F0232E0`
  calls slot `0x44C` (`0x6F044570`) with the best-so-far.
* Slot `0x44C` per candidate, in this order: the owner's visibility test `0x6F284EE0`; the target validity slot
  `0x3EC` (`0x6F04F7A0`: the level's targs flags, then the class slot `0x3F0`; any nonzero code rejects); a hostile
  candidate is rejected unless its flag `+0x5C & 0x100`; the class filter slot `0x450`; then the class comparator
  slot `0x458` with (candidate, its distance value `V`, best, best's `V`, threshold `T`).
* After a successful cast order the next delay is `max(total cast time (slot 0x3E0), 1.0)`; without a target it is
  the 3.0 s period.
* UNSURE: the radius (the model uses `Rng`), `V`/`T` units (squared world units here; the original compares cell
  units), the default autocast flag (the model has Heal on; the source of the default was not found), walking into
  range before the cast (the model casts only when in range).

## 2. Heal (`Ahea`, `CAbilityHeal`, vtable `0x6F8C9674`)

Data of the first level (read from the game's `AbilityData.slk`, not committed): cost 5, cooldown 1, range 250,
`DataA` 25 (the amount), `BuffID` `Bhea`, targets: air, ground, friend, vulnerable, invulnerable, self, organic,
non-ancient, neutral.

* Effect slot `0x424` = `0x6F108280` (CONFIRMED): amount = `DataA`; an ethereal target (`+0x158 > 0`) multiplies it
  by Misc `EtherealHealBonus`; the life adder `0x6F277C20` (model: add and clamp to the maximum life); then the
  marker buff `Bhea` (code getter `0x6F0DA880`) with the duration `1.0 + (Cool + 0.01)` (`0x6FAAE480` = 0.01, the
  ability's float holder at `+0x90/+0x94`; LIKELY), set by the buff's set-duration slot (never shorten).
* Extra target rule, slot `0x3F0` = `0x6F0F0290` (CONFIRMED): an illusion target is refused unless Misc
  `IllusionsCanRestoreLife`; a target with `maxlife - life < 1.0` returns error `0x1C` (refused). So full-life units
  cannot be healed by hand either.
* Autocast filter, slot `0x450` = `0x6F0F0370` (CONFIRMED): the base filter `0x6F030730` (caster flag
  `+0x20 & 0x40000000`), illusion candidates refused unless Misc `IllusionsGetAutocast`, the candidate must be an ally
  (`0x6F285B80`), and a `Bhea` buff on it made by another healer refuses it.
* Comparator, slot `0x458` = `0x6F0DADD0` (CONFIRMED, **Unicorn 600 cases, 0 mismatches**, life-fraction helper
  stubbed): `a` = candidate in range (`V <= T`, ordered x87 compare), `b` = best in range. `a && !b` takes the
  candidate, `!a && b` keeps the best, both in range take the strictly lower life fraction (`life/maxlife`, helper
  `0x6F020640`, LIKELY meaning), neither in range take the strictly smaller `V`. NaN keeps the best.
* Model: `unit_spells.rs` (`heal`, `heal_prefers`, `pick_target`, the autocast timer), the `healon`/`healoff`
  immediate orders, behaviour tests (amount, cap, mana, cooldown, marker lifetime 2.01 s, refusals, autocast target
  choice, foreign marker, toggling).

## 3. Inner Fire (`Ainf`, `CAbilityInnerFire` vtable `0x6F8DC54C`) and Bloodlust (`Ablo`, `CAbilityBloodlust` `0x6F90EE4C`)

Both are `CAbilityAutoTargetSpell` classes with one unique effect slot (`0x424`) and a unique autocast filter (slot
`0x450`); both use the shared comparator (below).

* Data of the first level: Inner Fire cost 35, range 500, Dur 60 (hero 60), `DataA` 0.1, `DataB` 5, `DataC` 500, buff
  `Binf`, targets air, ground, friend, neutral, self. Bloodlust cost 40, range 600, Dur 60 (hero 60), `DataA` 0.4,
  `DataB` 0.25, `DataC` 0.3, buff `Bblo`, targets air, ground, friend, organic, self, neutral.
* Effect slots (CONFIRMED structure): Inner Fire `0x6F13BFA0` reads `DataA, DataB, DataD, DataE`; Bloodlust
  `0x6F1CB080` reads `DataB, DataA`; both pick `HeroDur` for a hero target (`0x6F1D2660`), `Dur + extra` otherwise,
  look for the buff by its code (`0x6F10F480` / the Bloodlust getter): found = the buff's apply slot `0x358` (runs the
  set-duration slot `0x6F0D4EF0` first, never shorten, then rebuilds the effects), not found = a new buff.
* Buff effects, **CONFIRMED by Unicorn, 400 cases each, 0 mismatches** (V1, `tools/emu/spell_buff_emu.py`, vectors
  `crates/sim/tests/data/unit_spells/{bloodlust,innerfire}.txt`, test `crates/sim/tests/spell_buff_vectors.rs`; the real
  effect slot, buff creator, buff apply slot and holder-to-sink routines on a fake buff; only the buff allocator, the
  existing-buff lookup, the holder set-value objects and the final sinks are stubs):
  * Bloodlust: duration = `HeroDur` for a hero target, else `Dur` plus the ability's `+0x9C` by soft add; the movement sink
    `0x6F202FF0` gets `DataB` and the attack speed sink `0x6F0C9090` gets `DataA`, both unchanged (`bloodlust_values`).
    `DataC` goes to the model-scale holder (presentation).
  * Inner Fire: same duration rule; `DataB` is the armor sink `0x6F2769C0` value (always called); `DataD` / `DataE` are the
    life / mana regeneration modifiers (`0x6F02DC60` / `0x6F02DD00`), created only when the `0.001` property test
    (`prop_differs(value, 0)`) says the value differs from zero (`inner_fire_regen`); `DataA` is a damage **fraction**
    that becomes an INTEGER bonus per weapon slot (`0x6F1236F0`, `inner_fire_weapon_bonus`): skipped when the slot's
    `targs` is `1`, `0x40`, `0x80` or `0x100`; else `F2I(add(mul(DataA, X), 0.5))` with `X` the weapon's AVERAGE damage
    (`orc::command_base_damage`, the same getter the Command Aura uses); when that is 0 the result is the sign of `DataA`
    (+1, -1, or 0 for exactly zero; NaN gives -1), so a positive fraction always adds at least one point; a negative amount
    is clamped from below by `-(bonus + count)`; the amount is added to the attack ability's damage counter (`0x6F0C6C10`)
    and kept in the buff for the exact undo.
* Model: `UnitMods` (armor, move/attack deltas, regeneration, `damage_int`), `Game::apply_mods_buff` (replace on refresh),
  undone exactly on removal (`BuffEffect::Mods`). **Corrected in V1**: Inner Fire used a percentage multiplier
  (`damage_pct`) on the rolled damage; it is now the integer `damage_int` computed from the target's attack record at
  application time (weapon slot 0 only: the sim keeps one damage counter, so the second weapon's amount is not
  represented). Still UNSURE: the refresh path (buff slot `0x358`, same argument mapping by reading only), a duration
  of zero or less (the model applies nothing; the original passes the value to the set-duration slot), and the
  `Dur` extra term (0 in the model).
* Autocast filters (CONFIRMED, static): Bloodlust `0x6F1AFF10` and Inner Fire `0x6F123DC0` accept a candidate only if
  it has the attack ability and a current attack target that is hostile to its player (`0x6F285AC0` is the passive
  alliance mask test), does not have the buff, and has life fraction above 0.1 (`0x6FAAE4A0`); with the fraction below
  0.5 (Bloodlust, `0x6FAAE4D0`) or 0.3 (Inner Fire, `0x6FAAE4BC`) the caster must be at full mana (the compare of
  `0x6F02DCA0` current mana against `0x6F020730` max mana). Inner Fire further needs the candidate in reach of its
  target (`0x6F0CBAE0` + `0x6F0207A0`; LIKELY reading as an attack range test) and a final call `0x6F1452A0` when the
  ability flag 2 is set (not decoded, taken as true). Neither spell defaults to autocast on in the model (UNSURE).
* Shared comparator `0x6F023400` (**Unicorn 600 cases, 0 mismatches**): a candidate in range (`V <= T`) beats an
  out-of-range best; two in-range units: the best stays if `Vb > Vc`, so the original keeps the FARTHER of two
  in-range units and replaces it only by one that is not strictly nearer; two out-of-range units: the strictly nearer
  wins. (Slot `0x464` is 0 for these spells.)

## 4. Cripple (`Acri`, `CAbilityCripple` vtable `0x6F914394`, a `CAbilitySimpleSpell`, no autocast)

* Data (first level): cost 175, range 600, Dur 60 / HeroDur 10, `DataA` 0.75, `DataB` 0.5, `DataC` 0.5, buff `Bcri`,
  targets air, ground, enemy, organic, neutral. Editor labels: `DataA` movement speed reduction, `DataB` attack speed
  reduction, `DataC` damage reduction.
* Effect slot `0x424` = `0x6F1CC450`, **CONFIRMED by Unicorn, 400 cases, 0 mismatches** (V1,
  `tools/emu/spell_buff_emu.py`, `crates/sim/tests/data/unit_spells/cripple.txt`, `spell_buff_vectors.rs`; the real effect,
  creator `0x6F1CC320`, `CBuffCripple` apply `0x6F1B7FD0`, `0x6F0D6460`, sink routines and the damage routine `0x6F1B2BC0`):
  duration as for Bloodlust (`HeroDur` for a hero, else `Dur` + `+0x9C`); the movement sink gets `-DataA`, the attack
  speed sink `-DataB` (sign bit flipped), `DataC` reaches the damage holder (`+0xF0`) unchanged (`cripple_values`).
  The damage cut is an INTEGER per weapon slot, not a percentage (`cripple_weapon_change`): `amount = F2I(add(mul(DataC, X),
  0.5))`, `X` the weapon's average damage (`orc::command_base_damage`), a negative amount clamped from below by
  `-(bonus + count)`, no `targs` filter and no "at least one" rule (a zero amount changes nothing); the weapon's damage
  counter (`0x6F0C6C10`) changes by `-amount` and the amount is kept for the undo.
* Model: `UnitMods { move_delta = -A, attack_delta = -B, damage_int = -amount of weapon slot 0 }` as a negative, magic,
  dispellable buff `Bcri`; refresh replaces the values. **Corrected in V1**: the model had `damage_pct = -DataC`
  (a percentage of the rolled damage). Still UNSURE: the refresh slot `0x360` path (same arguments by reading only), the
  weapon-2 amount (the sim keeps one damage counter).

## 5. Faerie Fire (`Afae`, `CAbilityFaerieFire` vtable `0x6F8AA36C`, an AutoTargetSpell)

* Data: cost 45, range 700, Dur 90 / HeroDur 60, `DataA` 4 (defense reduction), `DataB` always autocast (0), buff `Bfae`,
  targets air, ground, enemy, neutral.
* Effect slot `0x424` = `0x6F0B8750` (CONFIRMED structure): hero test, duration, `DataA` into the buff apply
  `0x6F0B8640`; `CBuffFaerieFire` has the armor sink only (scan of its apply slots). Model: armor `-DataA` for the
  duration (negative, magic, dispellable); refresh replaces the value. Revealing invisible units (detection) is not
  modelled. Autocast (filter slot `0x450` = `0x6F09C240`) is NOT DONE; the order ids 852150/852151 toggle nothing.

## 6. Unholy Frenzy (`Auhf`, `CAbilityUnholyFrenzy` vtable `0x6F8F8984`, a SimpleSpell) and the damage-over-time base

* Data: cost 50, range 500, Dur 45 / HeroDur 45, `DataA` 0.75 (attack speed bonus), `DataB` 4 (damage per second),
  buff `BUhf`, targets air, ground, organic (no relation listed).
* Effect slot `0x424` = `0x6F18C070` (CONFIRMED structure): hero rule duration; `DataA`, `DataB` into `0x6F18BF60`,
  which finds the buff by its code (`0x6F158DB0`) and re-applies it through slot `0x37C`, else creates it.
* `CBuffUnholyFrenzy` is a `CBuffDoT` (RTTI base; message handler `0x6F177450` forwards to `CBuffDoT::vf3`
  `0x6F0D52D0`). CONFIRMED in `CBuffDoT`: apply `0x6F0D5250` arms the one-shot timer `0xD01E2` with the delay of slot
  `0x358` (`0x6F0ADBD0`: constant `0x6FAAE480` = **0.01 s**); its handler starts the repeating timer `0xD01C1` with the
  period of slot `0x35C` (`0x6F0ADBE0`: constant `0x6FAAE4F4` = **1.0 s**) and runs the tick `0x6F0D54B0` at once. So
  ticks come at 0.01, 1.01, 2.01 ... s. The tick: skipped while the target carries `BUsl` (`0x6F1D6000`, not
  modelled) or by the `MagicImmunesResistDamage` / immunity counters; amount `>= 0` is damage from the buff's
  creator with the class word of slot `0x374` (for Unholy Frenzy `0x40000` = bit 18 = `DAMAGE_TYPE_DEATH`); a negative
  amount is a heal through the life adder `0x6F277C20` (ethereal bonus). The per-tick amount is slot `0x360`
  (`0x6F0ADC30`: the stored value at `+0xEC`), taken as `DataB` (period 1.0 s, so DPS = amount, LIKELY).
* The attack speed part: `CBuffUnholyFrenzy` slot `0x314` (`0x6F177400`) adds `DataA` through the attack speed sink and
  re-applies it every 0.1 s (`0xD01C3`, `0x6FAAE4A0`) while the target's attack ability exists (only a refresh of the
  same value; not modelled separately).
* Model: `UnitMods { attack_delta = DataA }` plus `TickRec` (damage `DataB` of type 18 per tick, kills run the death
  dispatch with the caster as killer). Re-casting keeps one tick stream and replaces the values. 5 ticks in a 5 s test
  buff (ticks at 0.01 .. 4.01 s). UNSURE: the stored tick amount is `DataB` unchanged.

## 7. Rejuvenation (`Arej`, `CAbilityRejuvination` vtable `0x6F8CA7D4`, a SimpleSpell)

* Data: cost 125, range 400, Dur 12 / HeroDur 12, `DataA` 400 (hit points gained), `DataB` mana gained (0), `DataC`
  allow when full, buff `Brej`, targets air, ground, friend, vulnerable, invulnerable, self, organic, neutral.
* Effect slot `0x424` = `0x6F108530` (CONFIRMED structure): hero rule duration `T`; `DataA`, `DataB`, `DataC` into
  `0x6F1083D0`, which finds the buff by its code (`0x6F0DAB00`) and re-applies through slot `0x358`, else creates it.
* Buff apply `0x6F0F0650` (CONFIRMED): when `T > 0` the buff duration is `T + 0.01` (`0x6FAAE480`); the life per tick
  is the soft division `DataA / T` (and `DataB / T` for mana), stored at `+0xE0` / `+0xE8`. The timer `0xD01C1` is
  armed in `0x6F0F05A0` as a repeating timer with the period `0x6FAAE4F4` = 1.0 s (the first tick after 1.0 s). The tick
  `0x6F0E3A20` heals through the life adder `0x6F277C20` (ethereal target: times Misc `EtherealHealBonus`), mana
  through `0x6F279140`; an immune target (`vf 0x11C`) is skipped. 12 ticks of `DataA / 12` fit the 12.01 s buff.
* Model: shared `TickRec` heal ticks (`hot`), mana part and the "allow when full" rule NOT DONE (the cast is allowed on
  full life), UNSURE: whether a refresh restarts the tick phase.

## 8. Dispel Magic (`Adis`, `CAbilityDispelMagic` vtable `0x6F8C3A24`, family of 3 with Abolish Magic and the item forms)

* Data: cost 75, range 500, Area 200, `DataA` mana loss (0), `DataB` 200 (summoned unit damage), no buff; targets air,
  ground, ward, invulnerable, vulnerable, tree.
* The class overrides only slot `0x418` (`0x6F0EF320`, the area effect: group around the target point via the builder
  `0x6F2A3270`, per-unit worker `0x6F0EEF00` as callback `0x6F0EF210`) and the order getter. The worker (static
  reading, partly decoded): a summoned (`+0x248 & 4`) or illusion (`+0x5C & 0x40000000`) unit, the presence of a timed
  life buff, a relation test `0x6F285B20` and the ability's target legality `0x6F28D750` pick between two damage
  records, one with class `0x4000` (magic) and the amount handed in (`DataB`), one of amount 0 with the class flags
  `0x10`; the buff removal itself runs inside the target's damage handling (not decoded).
* Model (`dispel_magic`): units in `Area` of the target point in the circle query's order; an enemy loses positive
  dispellable buffs, anyone else negative ones; an enemy summoned unit or illusion takes `DataB` magic damage (type 14)
  from the caster. All of that is LIKELY/UNSURE (the worker's flow is not fully decoded); `DataA` mana loss and the
  timed-life kill path (`buffs.md`) are NOT DONE for the spell.

## 9. Raise Dead (`Arai`, `CAbilityRaiseDead` vtable `0x6F8CB89C`, an AutoTargetSpell with 292 slots)

* Data (2 levels): cost 75, cooldown 8, Dur 40 / HeroDur 40, Area 900, range 600, targets `dead`; level 1: `DataA` 2
  units of `uske`; level 2: `DataA` 1 and `DataB` 1 (type two `uskm` from `DataD`); buff `Brai`; editor labels
  RAI1/RAI2 units summoned (type one / two), RAI3/RAI4 the unit types.
* Effect `0x6F0E6AA0` (CONFIRMED structure): reads `DataA` and `DataB` (integers by `f2i`), the unit types
  (`0x6F0E69E0`, `0x6F0E6A40`), the hero-rule duration, then makes the units from corpses: two loops (type one, type
  two) each using the corpse list of the group builder `0x6F0FBB30` / callback `0x6F0FBA50` (slot `0x488`) around the
  caster within `Area`; event `0xD015A` and the removal of the corpse (`0x6F29ED60`).
* The autocast target (slots `0x47C` = `0x6F0E6CC0`, `0x480` = `0x6F0E6D20`: corpse filters, not decoded) is a corpse
  in reach: the cast has no target and refuses when there is none (the readiness error "no corpse").
* Model: `raise_dead` takes the first `DataA` corpses (dead, not removed, not structure/mechanical/illusion, within
  `Area`, circle query order) for type one (`UnitID`) and the next `DataB` for type two (`State::raise_dead_second`,
  default the skeletal mage), removes the corpse, builds the unit at its position for the caster's owner with the
  summoned bit and a `Brai` timed life of the hero-rule duration. Autocast: a scan (shared lifecycle, off by default)
  that casts when corpses are in reach and the caster is idle or attacking. UNSURE: the corpse list order and filters,
  the unit placement (the original searches a free spot), the second level's `DataD` type from the data (model:
  default `uskm`), the default autocast flag.

## 10. Roar (`Aroa`, `CAbilityRoar` vtable `0x6F8A21BC`, 252 slots, not an AutoTargetSpell)

* Data: cost 100, Area 500, Dur 45 / HeroDur 45, `DataA` 0.25 (damage increase), buff `Broa`; targets air, ground, friend,
  self. Labels: ROA1 damage increase, ROA2 defense increase, ROA3 life regeneration, ROA4 mana regeneration.
* An instant spell: overrides the effect slot `0x3C0` (`0x6F1EED60`), the readiness slot `0x3DC` (`0x6F1EEBF0`) and the
  group slots `0x3E8` / `0x3EC`. `CBuffRoar` shares the Inner Fire buff base (armor sink, attack-record modifiers).
* Model: friends (own/allied units and the caster) within `Area` of the caster, not structures, get `Broa` with
  `damage_pct = DataA`, `armor = DataB`, `life_regen = DataC` for the hero-rule duration. The readiness rule of
  `0x6F1EEBF0` (likely "some friendly unit in reach") is NOT DONE; the data's later fields are not modelled.

## 11. Not done, with what was found

| Ability | Class / vtable | Found | Missing |
|---|---|---|---|
| Purge `Aprg` | `CAbilityPurge` `0x6F8C46CC`, effect `0x6F107E10`, worker `0x6F0EEF00` | labels: A/B movement/attack update frequency, C summoned damage 400, D/E pause durations, F mana loss; Dur 15 / HeroDur 5 | the decaying slow schedule (`0x6F107CC0`), dispel part shared with Dispel |
| Polymorph `Aply` | `CAbilityPolymorph` `0x6F8A922C`, effect `0x6F12F560`, buff `CBuffPolymorph` | A max creep level 5, B..E morph unit lists; target filter refuses illusions (`+0x5C & 0x40000000`) and heroes | the morph (type swap) machinery |
| Lightning Shield `Alsh` | `CAbilityLightningShield` `0x6F8DB48C`, buff `0x6F8CF344` | DPS `DataA` 20 halved per 0.5 s tick (`DataA / 2.0`, period `recip(2.0)` CONFIRMED), first tick 0.01 s, per-victim marker buff `Blsa`, Area 160 | the tick's victim filter and damage class (`0x6F124800` / `0x6F13C410`) |
| Ensnare `Aens`, Web `Aweb` | `CAbilityEnsnare` `0x6F8A5B4C`, `CAbilityWeb` (AutoTargetSpell) `0x6F8A5F7C`; effect creates a net missile, `CBuffEnsnare*` | labels: A air unit lower duration, B height, C melee attack range; Dur 12 / HeroDur 3 (Ensnare) | the net missile, the root buff effect |
| Curse `Acrs` | `CAbilityCurse` `0x6F8A9694` (AutoTargetSpell), effect `0x6F0B8450`, filter `0x6F09BF80` | A chance to miss 0.33, Dur 120 / 60 | the attacker-side miss roll (combat has only the target-side evasion walk) |
| Healing Ward `Ahwd`, Stasis Trap `Asta`, Serpent Ward `Aeye` | `CAbilityHealingWard` `0x6F8B0A44` (summon `ohwd`, buff `Bhwd`), `CAbilityWard` | summon spells with a unit type per level | the ward's own healing aura ability, placement |
| Abolish Magic `Aadm` | `CAbilityAutoDispelMagic` `0x6F8C35AC` | autocast pair 852133/852134, cast 852132, A mana loss, B summoned damage 300, C 1 | the autocast filter `0x6F0E2B90` and the dispel worker |
| Huntress Sentinel `Aesn`, Disease Cloud `Aap1`, Dryad immunity | `CAbilitySentinel` `0x6F91EE04` (summon-like with a tree target) | data rows | not read |
