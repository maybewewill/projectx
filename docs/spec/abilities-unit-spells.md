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
* Buff effects (CONFIRMED sinks by scanning the buff classes' apply slots): `CBuffBloodlust` -> attack speed
  (`0x6F0C9090`) and movement (`0x6F202FF0`) sinks; `CBuffInnerFire` -> the armor sink (`0x6F2769C0`) and the
  attack-record modifiers (record `+4/+8` for the weapons, `+0x24`) plus two regeneration modifiers. Field meanings are
  the editor's (LIKELY): Inner Fire `DataA` damage fraction, `DataB` armor, `DataD` life regeneration, `DataE` mana
  regeneration, `DataC` the autocast range; Bloodlust `DataA` attack speed, `DataB` movement, `DataC` scale.
* Model: `UnitMods` (armor, move/attack deltas, a new percentage damage modifier `bonuses.damage_pct` multiplying the
  attack's raw damage by `1 + pct` in `damage.rs`, regeneration), `Game::apply_mods_buff` (replace on refresh), undone
  exactly on removal (`BuffEffect::Mods`). UNSURE: the damage modifier is applied to the raw damage after the dice
  (the original keeps it in the record modifiers, order against Critical Strike not read); the `Dur` extra term is 0.
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
* Effect slot `0x424` = `0x6F1CC450` (CONFIRMED structure): hero test, `HeroDur` / `Dur`, the buff apply
  `0x6F1CC320`. The buff (`CBuffCripple`, apply `0x6F1B7FD0`) negates its values (sign flips) and hands two to the
  speed buff base `CBuffSpeed` (movement `0x6F202FF0` and attack speed `0x6F0C9090` sinks, CONFIRMED by scanning the
  apply slots) and one to the attack-record damage modifiers (slot `0x2D4`, negated list at `+0xF8`).
* Model: `UnitMods { move_delta = -A, attack_delta = -B, damage_pct = -C }` as a negative, magic, dispellable buff
  `Bcri`; refresh replaces the values. UNSURE: the order of the percentage damage modifier against other modifiers.

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
