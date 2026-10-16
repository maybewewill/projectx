# Neutral and creep abilities of Warcraft III 1.26a (S42)

Model: `crates/sim/src/neutral_spells.rs` (hooks in `cast.rs`, `abilities.rs`, `natives/unit.rs`, `natives/unit_attr.rs`,
`unit_spells.rs`, `auras.rs`, `combat.rs`; engine loader `crates/engine/src/objdata.rs`). Tests:
`crates/sim/tests/neutral_spells.rs`, `crates/engine/tests/neutral_aliases.rs` (real archives, optional).
Method: the ability atlas (`docs/research/ability-atlas/`), `tools/analysis/ability_orders.py` for order ids, IDA
decompiles of the class overrides, the user's `AbilityData.slk` / `UnitAbilities.slk` / `UnitData.slk` (not committed).
Labels: **CONFIRMED** = read in the machine code; **LIKELY** = strong inference; **UNSURE** = stand-in.
No Unicorn run was made in S42: every number that is used is either a decompiled structure on top of functions that
earlier sessions Unicorn-checked (the PRD pair `0x6F1D21F0` / `0x6F1A3470` of Critical Strike, the heal / thunder bolt /
carrion swarm effects) or a counter increment, so no new numeric kernel needed a differential test. Confidence below.

## 1. How the creep versions work (CONFIRMED against the real data)

The creep camps do not use special classes for most abilities. Their ability ids are **alias rows** of `AbilityData.slk`:
the `code` column names the base ability. 118 base codes have more than one row; e.g. `ACcl`/`AIcl` -> `AOcl`, `ACbl`/`ACbb`
-> `Ablo`, `ACsw`/`AIos` -> `Aslo`, `ACpv` -> `Awar`, `ACvs` -> `Aven`, `ACmi` -> `Amim`, `Anh1`/`Anh2` -> `Anhe`, `ACtn`/`Arsw`
-> `AOwd` (Serpent Ward, modelled under the hero id `AOsw`). The game builds the base class and reads the level records of the
alias row. The model now does the same:

* `engine::objdata::load_neutral_aliases` groups the rows by their `code` column, picks the modelled ability of the group
  (`neutral_spells::standard_for_group`: a static parent class first, then the code itself, then a modelled member such as
  `AOsw`), loads the level records of every other row under its own id and fills `ObjectText::spell_aliases`. Item rows
  (`AI..`) are left to `item_abilities.rs`. Checked on the real archives: 17 sampled aliases resolve as expected.
* `Game::set_spell_alias(alias, base)` installs the map. A unit that gets an alias row (type `abilList`, `UnitAddAbility`)
  receives `Ability { base: modelled code, source: alias row }` (`Ability::aliased`); `UnitRemoveAbility`,
  `GetUnitAbilityLevel` and friends map the id the same way. Everything keyed by the base code (order ids, the effect
  dispatch, the passive hooks, autocast) works unchanged; `Game::ability_level_record` / the cast core read the level
  records of `source`, so the creep numbers are used, not the hero's.
* Not done: hero ability rows with a second base unit (a unit holding both `Aspo` and `Aven` keeps one entry), the
  `UnitMakeAbilityPermanent` / `SetUnitAbilityLevel` natives keep the raw id (rare for creeps).

## 2. Classes with a class of their own (RTTI parent + override reading)

`STATIC_PARENTS` (base code, modelled parent, order id of the derived class; order ids Unicorn-checked by
`ability_orders.py`, cast getter slot `0x348`):

| class | parent | what the overrides do | confidence |
|---|---|---|---|
| `ACtb` CreepThunderBolt | `AHtb` | no override but codes; order 852252 | LIKELY (identical class body) |
| `ANfb` FireBolt (also `ACfb`, `Awfb`) | `AHtb` | slot `0x42c` (`0x6F18F760`) only; order 852231 | LIKELY |
| `ACtc` CreepThunderClap (also `ACt2`) | `AHtc` | slot `0x430` = `0x6F18AA90`: reads `Dur`/`HeroDur`, `DataA` for the area and spawns the visual effect through the effect factory (`+856`/`+852` calls), no damage or buff; order 852253 | CONFIRMED (decompile) / LIKELY (equal numbers) |
| `ANbf` BreathOfFire (`ANcf`, `ACbc`) | `AUcs` (Carrion Swarm) | AI hint slot `0x100` (`0x6F156160`, a mana/target-count test of the AI) and the effect spawn `0x42c`; order 852580 | LIKELY |
| `ACbf` BreathOfFrost | `ANbf` | effect spawn `0x42c` (`0x6F1C2B90`, builds the missile object); order 852560 | LIKELY |
| `ACrn` CreepReincarnation | `AOre` | slot `0x80` (`0x6F1D3100`) runs the base init and registers the order ids 852314 / 852315; slot `0x284` (`0x6F1E5F40`) is the death-time check that issues them | CONFIRMED structure / LIKELY same numbers |
| `Anhe` NeutralHeal (`Anh1`, `Anh2`) | `Ahea` | slots `0x348`, `0x308`, `0x318` return the neutral order ids 852248 / 852249 / 852250 | CONFIRMED (getters) |

Autocast of the aliased abilities uses the modelled ability's toggles (`unit_spells::TOGGLES`); the neutral heal toggles
are in `neutral_spells::toggle_order`.

## 3. New behaviour

* **Envenomed Spears** (`Aven`, `CAbilityVenomSpear < CAbilityPoisonAttack`, no override but codes). On a surviving hit the
  attacker poisons an organic air/ground target: `Bpoi` for `Dur` (`HeroDur` for heroes), `DataA` damage per second
  (damage type 23, first tick 0.01 s, then 1.0 s), no slow (the slow part is the `0x34C..0x354` overrides of `Aspo`).
  LIKELY; `DataD` stacking (`Bpsd`) is not modelled.
* **Magic Immunity** (`Amim`, `CAbilityMagicImmunity < CAbilityPassive`). Init slot `0x80` = `0x6F110730` calls
  `0x6F07A3A0` (CONFIRMED: increments the unit counters at `+0x15C` / `+0x160`, the first count dispels), the removal slot
  `0x6F110700` calls `0x6F077320` (decrements). Wired to `UnitObj::magic_immune` through `sync_neutral_spells` (called from
  `sync_auras`, so on creation and ability changes). The second counter (`+0x160`) is not modelled separately.
* **Pulverize** (`Awar`, `CAbilityWarStomp < CAbilityPassive`; the Sea Giant ability, not the orc War Stomp
  `AOws`). CONFIRMED structure:
  * swing start (slot `0x124`, `0x6F170F90`): effective chance `count * PRD(DataA)` (`0x6F163160` = `I2R(count)` times the table
    value of `0x6F1D21F0`, the same pair as Critical Strike); a **stream 40** draw `d`: `chance < d` -> counter + 1, else the
    ability is armed (`flag 0x200`) and the counter is 1. A target refused by the spell immunity test ends the handler first.
  * cancel (slot `0x128`, `0x6F171140`) clears the flag (here: the next swing start clears it).
  * execute (slot `0x170`, `0x6F1775F0`): an armed ability clears the flag and calls the area body `0x6F2C31D0` around the
    **caster** with the rings `(DataC, DataB)` and `(DataD, 0.5 * DataB)`, damage class `0x20` (type 5), the level's `targs`
    (`ground,enemy,neutral`); the callback `0x6F171180` refuses an illusion caster. The collector `0x6F2C2E80` gives a unit the
    amount of the **smallest ring that contains it** (CONFIRMED by reading the loop).
  * Rust: `neutral_swing_start` (called after `orc_swing_start`), `neutral_execute` (before the normal execute; order against the
    attack's own damage UNSURE). Ring membership is the plain circle (the collision-adjusted `0x6F476C40` is UNSURE as for all
    area spells). Needs `GAME_DLL` for the RNG in its tests. **V3: CONFIRMED (Unicorn, 300 rows each, `v3_cores_emu.py` `PR` and `PE`)**: the execute slot `0x6F1775F0` hands the area body exactly two rings `(DataC, DataB)` and `(DataD, 0.5 * DataB)`, class `0x20`, the ability's `targs` word, the caster as source and the callback `0x6F171180` (`neutral_spells::pulverize_rings`; the model now takes the largest ring as query radius and the smallest containing ring per unit); the effective chance `0x6F163160` (count at `+0x88` times the PRD table value of DataA) equals `orc::effective_chance`. Swing-start gates (cooldown counter `+0x3C`, slot `0x268`, target test `0x6F26D590`, immunity) and the stream-40 draw are decoded, not run: the arm test is `!(chance < draw)` as an x87 compare.

## 4. Measured use on the melee creeps (UnitData race `creeps`, resolved through `code`)

Of 133 distinct creep ability rows, 64 were already covered by the modelled bases; the aliases above add the creep rows of
those bases at once. The remaining creep rows by number of unit types: `Advc`/`ACdv` Devour (9; class `Devour < CargoLoad`,
needs the cargo / digestion model), `Ashm` Shadow Meld (8, night elf), `Alit` Lightning Attack (7), `Arsk`/`ACrk` Resistant Skin
(6, class `Passive`, no level data in the table), `ACad` Animate Dead (4, parent `AUan` is not modelled), `ACcy`/`Acyc` Cyclone (3),
`Afrb` Frost Breath (3), `Aspa` Spider Attack (3), `ACce`/`ANca` Cleaving Attack (3), `Apig` Panda Immolation, `Abu5`/`Abur`
Burrow, `ACsa`/`AHfa` Searing Arrows, `ACps`/`Apos` Possession, `ACcw`/`AHca` Cold Arrows, the
Drunken Haze / Brawler pair, Mana Shield, Silence, Charm, Doom, Rain of Fire / Chaos, Volcano, Tornado, Stampede and the neutral
hero spells (Pit Lord, Beastmaster summons, Firelord, Alchemist, Tinker, Dark Ranger, Pandaren) are not modelled: each needs its own
effect (see the table; the "extra overrides" column counts class overrides beyond the code / order / init getters).

## 5. Class table (factory rows with prefix AC / AN / Ac plus the ones above)

| code | class | RTTI parent | extra overrides | status |
|---|---|---|---|---|
| `ACad` | CAbilityCreepAnimateDead | CAbilityAnimateDead | 0 | NOT DONE |
| `ACbf` | CAbilityBreathOfFrost | CAbilityBreathOfFire | 1 | alias of AUcs (852560) |
| `ACdv` | CAbilityCreepDevour | CAbilityDevour | 0 | NOT DONE |
| `ACrn` | CAbilityCreepReincarnation | CAbilityReincarnation | 2 | alias of AOre |
| `ACtb` | CAbilityCreepThunderBolt | CAbilityThunderBolt | 0 | alias of AHtb (order 852252) |
| `ACtc` | CAbilityCreepThunderClap | CAbilityThunderClap | 1 | alias of AHtc (852253) |
| `ANab` | CAbilityAcidBomb | CAbilitySimpleSpell | 4 | NOT DONE |
| `ANba` | CAbilityBlackArrow | CAbilityFlamingArrows | 5 | NOT DONE |
| `ANbf` | CAbilityBreathOfFire | CAbilityCarrionSwarm | 2 | alias of AUcs (852580) |
| `ANbr` | CAbilityBattleRoar | CAbilitySimpleSpell | 5 | NOT DONE |
| `ANbs` | CAbilityBlackArrowSpell | CAbilitySimpleSpell | 2 | NOT DONE |
| `ANca` | CAbilityCleavingAttack | CAbilityPassive | 9 | Cleaving Attack (S43) |
| `ANch` | CAbilityCharm | CAbilitySimpleSpell | 3 | Charm (S43; target rule and effect CONFIRMED in V6) |
| `ANcl` | CAbilityChannel | CAbilitySimpleSpell | 21 | NOT DONE |
| `ANcr` | CAbilityChemicalRage | CAbilityMorph | 8 | NOT DONE |
| `ANcs` | CAbilityClusterRockets | CAbilitySimpleSpell | 16 | NOT DONE |
| `ANdb` | CAbilityDrunkenBrawler | CAbilityCriticalStrike | 0 | NOT DONE |
| `ANdc` | CAbilityDarkConversion | CAbilitySleep | 15 | NOT DONE |
| `ANde` | CAbilityDemolish | CAbilityWarStomp | 4 | NOT DONE |
| `ANdh` | CAbilityDrunkenHaze | CAbilitySilence | 5 | NOT DONE |
| `ANdo` | CAbilityDoom | CAbilitySimpleSpell | 4 | NOT DONE |
| `ANdp` | CAbilityDarkPortal | CAbilitySimpleSpell | 13 | NOT DONE |
| `ANef` | CAbilityElementalFury | CAbilitySimpleSpell | 14 | NOT DONE |
| `ANeg` | CAbilityEngineeringUpgrade | CAbilityPassive | 13 | NOT DONE |
| `ANfb` | CAbilityFireBolt | CAbilityThunderBolt | 1 | alias of AHtb (852231) |
| `ANfd` | CAbilityFingerOfDeath | CAbilitySimpleSpell | 11 | NOT DONE |
| `ANfl` | CAbilityForkedLightning | CAbilitySimpleSpell | 2 | NOT DONE |
| `ANfy` | CAbilityFactory | CAbilityPassive | 8 | NOT DONE |
| `ANgl` | CAbilityNeutralG2L | CAbilityNeutralSpell | 4 | NOT DONE |
| `ANhs` | CAbilityHealingSpray | CAbilityClusterRockets | 7 | NOT DONE |
| `ANht` | CAbilityHowlOfTerror | CAbilityRoar | 3 | Howl of Terror (S43) |
| `ANia` | CAbilityIncinerateArrow | CAbilityRangerArrow | 10 | NOT DONE |
| `ANic` | CAbilityIncinerate | CAbilityCleavingAttack | 8 | NOT DONE |
| `ANin` | CAbilityInferno | CAbilitySimpleSpell | 10 | NOT DONE |
| `ANlm` | CAbilityLavaMonster | CAbilityWaterElemental | 1 | NOT DONE |
| `ANmo` | CAbilityMonsoon | CAbilitySimpleSpell | 13 | NOT DONE |
| `ANmr` | CAbilityMindRot | CAbilitySimpleSpell | 2 | NOT DONE |
| `ANms` | CAbilityManaShield | CAbilitySimpleSpell | 14 | Mana Shield (S43) |
| `ANpa` | CAbilityParasite | CAbilityAutoTargetSpell | 6 | NOT DONE |
| `ANpi` | CAbilityPermImmolation | CAbilityPassive | 10 | NOT DONE |
| `ANpr` | CAbilityPreservation | CAbilitySimpleSpell | 4 | NOT DONE |
| `ANrc` | CAbilityRainOfChaos | CAbilitySimpleSpell | 13 | NOT DONE |
| `ANre` | CAbilityNeutralRegen | CAbility | 5 | NOT DONE |
| `ANrf` | CAbilityRainOfFire | CAbilityBlizzard | 1 | alias of AHbz (852238, S43) |
| `ANrg` | CAbilityRoboGoblin | CAbilityMorph | 18 | NOT DONE |
| `ANsa` | CAbilitySanctuary | CAbilityPreservation | 2 | NOT DONE |
| `ANse` | CAbilitySpellShieldAoe | CAbilitySpellShield | 5 | NOT DONE |
| `ANsg` | CAbilitySummonGrizzly | CAbilityWaterElemental | 1 | NOT DONE |
| `ANsi` | CAbilitySilence | CAbilitySimpleSpell | 12 | Silence (S43) |
| `ANsl` | CAbilitySoulPreservation | CAbilitySimpleSpell | 13 | NOT DONE |
| `ANso` | CAbilitySoulBurn | CAbilitySimpleSpell | 2 | NOT DONE |
| `ANsp` | CAbilityNeutralSpies | CAbilityNeutralSpell | 4 | NOT DONE |
| `ANsq` | CAbilitySummonQuillbeast | CAbilityWaterElemental | 1 | NOT DONE |
| `ANss` | CAbilitySpellShield | CAbilitySimpleSpell | 4 | NOT DONE |
| `ANst` | CAbilityStampede | CAbilitySimpleSpell | 17 | NOT DONE |
| `ANsu` | CAbilitySubmerge | CAbilityBurrow | 3 | NOT DONE |
| `ANsw` | CAbilitySummonWarEagle | CAbilityWaterElemental | 1 | NOT DONE |
| `ANsy` | CAbilitySummonFactory | CAbilitySimpleSpell | 4 | NOT DONE |
| `ANth` | CAbilityThorns | CAbilityPassive | 4 | NOT DONE |
| `ANtm` | CAbilityTransmute | CAbilitySimpleSpell | 4 | Transmute (S50; amounts CONFIRMED in V6) |
| `ANto` | CAbilityTornado | CAbilitySimpleSpell | 14 | NOT DONE |
| `ANvc` | CAbilityVolcano | CAbilitySimpleSpell | 13 | NOT DONE |
| `ANwm` | CAbilityWateryMinion | CAbilityWaterElemental | 2 | NOT DONE |
| `Acan` | CAbilityCannibalize | CAbilityClosestTargetSpell | 31 | NOT DONE |
| `Acar` | CAbilityCargoHold | CPower | 22 | NOT DONE |
| `Acha` | CAbilityChaos | CAbilityChaosBase | 1 | NOT DONE |
| `Achd` | CAbilityCargoHoldDeath | CAbilityPurge | 4 | NOT DONE |
| `Achl` | CAbilityChaosCargoLoad | CAbilityChaosBase | 1 | NOT DONE |
| `Aclf` | CAbilityCloudOfFog | CAbilitySilence | 8 | NOT DONE |
| `Acmg` | CAbilityControlMagic | CAbilitySimpleSpell | 5 | NOT DONE |
| `Acoa` | CAbilityCoupleArcher | CAbilityCouple | 2 | NOT DONE |
| `Acoh` | CAbilityCoupleHippogryph | CAbilityCouple | 1 | NOT DONE |
| `Acoi` | CAbilityCoupleInstant | CAbilityClosestTargetSpell | 21 | NOT DONE |
| `Acor` | CAbilityCorrosiveBreath | CAbilityPassive | 9 | NOT DONE |
| `Acpf` | CAbilityCorporealForm | CAbilityMorph | 13 | NOT DONE |
| `Acri` | CAbilityCripple | CAbilitySimpleSpell | 2 | NOT DONE |
| `Acrs` | CAbilityCurse | CAbilityAutoTargetSpell | 5 | NOT DONE |
| `Acyc` | CAbilityCyclone | CAbilitySimpleSpell | 8 | Cyclone (S43) |
| `Adev` | CAbilityDevour | CAbilityCargoLoad | 7 | NOT DONE |
| `Advc` | CAbilityDevourCargo | CAbilityCargoHold | 8 | NOT DONE |
| `Alit` | CAbilityLightningAttack | CAbility | 15 | NOT DONE |
| `Amim` | CAbilityMagicImmunity | CAbilityPassive | 5 | magic immunity count (new) |
| `Anhe` | CAbilityNeutralHeal | CAbilityHeal | 2 | alias of Ahea (852248/49/50) |
| `Apig` | CAbilityPandaImmolation | CAbilityPermImmolation | 1 | NOT DONE |
| `Arsk` | CAbilityResistantSkin | CAbilityPassive | 2 | marker, no behaviour (S43) |
| `Ashm` | CAbilityShadowMeld | CAbilityButton | 21 | NOT DONE |
| `Aven` | CAbilityVenomSpear | CAbilityPoisonAttack | 0 | on-hit poison (new) |
| `Awar` | CAbilityWarStomp | CAbilityPassive | 9 | Pulverize (new) |

## 6. S43: more creep abilities (no Unicorn run; every number below is a decompiled structure or the editor data)

Tests: `crates/sim/tests/neutral_spells.rs` (the `*_s43` fixtures). Order ids from `tools/analysis/ability_orders.py`
(slot `0x348`): Mana Shield on 852589 / off 852590 (slot `0x318`), Charm 852581, Cyclone 852144, Silence 852592, Howl of
Terror 852588, Rain of Fire 852238. Level data from the user's `AbilityData.slk` and the editor meta table
(`AbilityMetaData.slk`). Rule for all of them: the model adds only the override, the rest is an existing module.

| code | class (parent) | what is modelled | confidence |
|---|---|---|---|
| `ANca` / `ACce` | `CAbilityCleavingAttack` (`Passive`) | swing start `0x6F1B1440` arms the flag `0x200` unless the target test `0x6F26D590` refuses; execute slot `0x154` `0x6F1B7310` calls the area body `0x6F2C31D0` (class `0x20`, circle `Area`) with `DataA` times the attack's damage. Model: after the main hit, every enemy ground unit within `Area` of the target except the target takes `DataA * damage` (damage type 5, like Pulverize). `ACce` is an alias row of `ANca` (`code` column), so it resolves like the other aliases. Open: whether the callback `0x6F1B14F0` really skips the main target, whether `damage` is the pre- or post-armor value of the record (the hit amount is used), ranged attackers | LIKELY |
| `ANms` | `CAbilityManaShield` (`SimpleSpell`), buff `CBuffManaShield` | cast puts `BNms` on the caster; the apply slot `0x354` (`0x6F1116E0`) stores its 3rd argument in the cell at `+0xCC` (value at `+0xD0`) and its 4th in the cell at `+0xD4` (value `+0xD8`); the cast effect `0x6F13D310` passes `(unit, Dur (hero rule), DataA, DataB)`, so **`DataA` = damage absorbed per mana point (cell `+0xCC`), `DataB` = share of each hit (`+0xD8`)** (CONFIRMED by a Unicorn trace of the effect with distinct data). Stock `ANms` `DataA` 1 / `DataB` 1 (`ACmf`: 2 / 1), `Dur` 0. **Damage hook `0x6F126DD0`: CONFIRMED (`MS`, 300 Unicorn rows, `mana_shield_hook_matches_the_original`, `neutral_spells::mana_shield_hit`)**: a record whose source does not resolve or whose flag `0x10` is set is untouched; `covered = amount * DataB` (soft multiply); nothing happens when `|covered - 0.0| < 0.001` (`0x6FAAE47C`; NaN proceeds); `need = covered / DataA` through the safe divide `0x6F022530` (a zero `DataA` gives 1.0 for `covered == 0`, else plus or minus `FLT_MAX`); when the mana is below `need` the covered part becomes `mana * DataA`; the mana spent is `covered / DataA`; events in this order: the buff is destroyed first if `covered == 0.0`, then the record's amount is reduced by `covered` (`0x6F255790`, **CONFIRMED by `RR`, 300 rows**: the amount becomes exactly 0.0 when it is not above the reduction, else a soft subtraction), then the mana is spent, then the buff is destroyed if the mana afterwards is not above 0 (NaN stays). The old model forced the mana to 0 and ignored all the degenerate cases. Not modelled: the mana sink `0x6F283DB0` also walks the unit's mana-modifier abilities (the model subtracts plainly); the record's second amount (`+0x18`). Off order 852590 destroys the buff (LIKELY). The hook sits in the two life sinks `unit_damage_target` and `apply_damage` | CONFIRMED (hook, reduce, field roles); mana sink modifiers not modelled |
| `Acyc` / `ACcy` | `CAbilityCyclone`, buffs `CBuffCyclone` / `CBuffCycloneTwo` (`Bcyc`, `Bcy2`) | unit target (ground, enemy, neutral, organic); hero-rule duration (`Dur` / `HeroDur`); target stunned (`Bcyc`) and invulnerable (`Bcy2`) for that time. The effect slot `0x6F1CCC70` reads the Misc key `CycloneStasis` only to pick the effect object, so it has no simulation effect here. The split of stun / invulnerability between the two buffs is a guess | LIKELY |
| `ANsi` | `CAbilitySilence`, buff `CBuffSilence < CBuffSpeed` (`BNsi`) | point (or unit) target; every enemy air / ground organic or neutral unit within `Area` of the point, not spell immune, gets `BNsi` for the hero-rule duration; a silenced unit cannot start a spell (`begin_cast` and the stage rechecks refuse it with the stunned error, `Game::is_silenced`). A running channel is not interrupted (as in `cast.rs`). `DataA` (`silenceFlags`, 8) is not decoded: all spells are blocked; the `CBuffSpeed` speed fields are zero in 1.26a | LIKELY / UNSURE (flags) |
| `ANch` | `CAbilityCharm` (`SimpleSpell`) | unit target (air, ground, nonhero, enemy, neutral, organic) whose type level is at most `DataA` (editor `NCH1` "max creep level"); the target changes owner for good (food and counters move, owner events fire, orders are stopped). The only extra overrides are the target rule `0x6F14F350` (AI hint) and the effect spawn | LIKELY |
| `ANht` | `CAbilityHowlOfTerror < CAbilityRoar` (`BNht`) | the mirror of Roar (`unit_spells::roar`): enemy air / ground units within `Area` of the caster lose `DataA` of their damage for the hero-rule duration. The buff base is the Roar one (damage fraction sink) | LIKELY |
| `ANrf` | `CAbilityRainOfFire < CAbilityBlizzard` | static parent `AHbz` with order 852238; the editor fields are the Blizzard `HBZ1..6` set, so the wave engine of `human_heroes.rs` runs unchanged. `DataE` (5/10/15, only in the creep row) is not read | LIKELY |
| `Arsk` / `ACrk` | `CAbilityResistantSkin < CAbilityPassive` | no behaviour of its own: its slots return constants (`0xCC` -> 0, `0x1A0` = the base "can act" test), so the ability is a marker that other code reads (Misc armor / spell resistance keys); attached through the alias layer only | CONFIRMED (overrides trivial) |

Not done (reasons):

* `Advc` / `ACdv` Devour: `CAbilityDevour < CargoLoad`, needs the cargo and digestion model (units inside a unit).
* `Ashm` Shadow Meld: `CAbilityShadowMeld < CAbilityButton`, 21 overrides (button, order and night-time invisibility); the model has no invisibility state.
* `Alit` Lightning Attack: `CAbility` (not Passive), the data are two fractions (0.25) and the effect slot only builds a lightning effect object (`0x6F134E40`); the damage rule is not decoded.
* `ACad` creep Animate Dead: parent `AUan` is not modelled.
* `ANto` Tornado (14 overrides, a summoned moving unit), `ANvc` Volcano (13 overrides, a building-like effect): each needs its own effect.
* Hurl Boulder: no factory class carries that name (it is an attack / unit weapon, not a spell class).
* Neutral heroes: Naga Sea Witch has Mana Shield (here), Tornado, Forked Lightning (`ANfl`, 2 overrides, not decoded) and Frost Arrows; Dark Ranger has Silence and Charm (here), Black Arrow (`ANba < FlamingArrows`) and Life Drain; Pit Lord has Rain of Fire, Howl of Terror, Cleaving Attack (here) and Doom (`ANdo`); Brewmaster, Beastmaster (summons `ANsg`/`ANsq`/`ANsw`, Stampede), Firelord (Incinerate `ANic < CleavingAttack`, Soul Burn, Lava Spawn, Volcano), Alchemist (Acid Bomb, Chemical Rage, Healing Spray, Transmute) and Tinker (Cluster Rockets, Robo-Goblin, Engineering Upgrade, Pocket Factory) remain NOT DONE.

## 7. S44: neutral heroes and the invisibility model (no Unicorn run; structure and numbers from IDA + the editor data)

Invisibility is in `docs/spec/abilities-invisibility.md` (`invisibility.rs`: Sorceress Invisibility `Aivs`, Permanent
Invisibility / Ghost, Shadow Meld, the detectors, `IsUnitInvisible` / `IsUnitDetected` / `IsUnitVisible`).
Order ids from `docs/research/ability-orders/getters.tsv` (slot `0x348`). Tests: `crates/sim/tests/neutral_heroes.rs`.

| code | class (parent) | what is modelled | confidence |
|---|---|---|---|
| `ANsg` / `ANsq` / `ANsw` (852594 / 5 / 6) | `CAbilitySummon{Grizzly,Quillbeast,WarEagle} < CAbilityWaterElemental` (one override each: the code) | the Water Elemental summon loop: `f2i(DataA)` units of the level's `UnitID`, timed life `Dur` with buffs `BNsg` / `BNsq` / `BNsw` | LIKELY (same flags word `0x300001` and slot layout as `AHwe`) |
| `ANlm` (852667) | `CAbilityLavaMonster < CAbilityWaterElemental` | same summon with `BNlm`; the splitting of the Lava Spawn (`DataB..F`) is not modelled | LIKELY / the split NOT DONE |
| `ANdh` (852585) | `CAbilityDrunkenHaze < CAbilitySilence` | enemy air / ground organic units within `Area` of the target get `BNdh`: miss chance `DataB` (the attacker-side miss walk shared with Curse, `attacker_miss_scores`) and movement `-DataC`; the walk sharing is CONFIRMED by the doc of `0x6F2BEAC0` (Haze, Silence, Cloud of Fog are listed), the roles of `DataB` / `DataC` follow the tooltip order | LIKELY / roles UNSURE |
| `ANdb` | `CAbilityDrunkenBrawler < CAbilityCriticalStrike` (0 overrides) | the Critical Strike arming and damage with `DataA` chance and `DataB` multiplier (`Game::crit_base`), evasion `DataD` scored with the `AEev` entries in the miss gate | crit LIKELY (0 overrides), evasion UNSURE |
| `ANso` (852668) | `CAbilitySoulBurn < CAbilitySimpleSpell` | `BNso` for the hero-rule duration: `DataA` damage per second (first tick 0.01 s) and silence | structure LIKELY, `DataB` / `DataC` and the silence UNSURE |
| `ANab` (852662) | `CAbilityAcidBomb < CAbilitySimpleSpell` | `BNab`: armor `-DataC`, `DataD` damage per second for 15 s | UNSURE (field roles from the value shapes) |
| `ANfl` (852587) | `CAbilityForkedLightning < CAbilitySimpleSpell` | the target and the nearest other enemies within `Area` of it, `DataB` units in all, take `DataA` | UNSURE (the original forks along a line `DataC` x `DataD`) |
| `ANbf` Breath of Fire | alias of Carrion Swarm (S42) | unchanged | LIKELY |

Not done (reason): Stampede `ANst` (17 overrides, a herd of moving units), Storm Earth and Fire `ANef` (14 overrides, three
summoned elementals with their own abilities), Chemical Rage `ANcr` and Robo-Goblin `ANrg` (`CAbilityMorph`, 8 / 18 overrides),
Healing Spray `ANhs` and Cluster Rockets `ANcs` (a channelled `CAbilityClusterRockets` with 7 / 16 overrides),
Transmute `ANtm`, Incinerate `ANic` (an orb-like cleave, 8 overrides), Volcano `ANvc`, Tornado `ANto`, Engineering Upgrade
`ANeg`, Pocket Factory `ANsy` (factory units), Black Arrow `ANba`, Doom `ANdo`, Dark Ranger Life Drain.

## 8. S50: the remaining AN* abilities (no Unicorn run; IDA structure + the editor's own labels)

Modules: `neutral_heroes2.rs` (dispatch, Transmute, Doom), `neutral_waves.rs` (Cluster Rockets, Healing Spray, Monsoon),
`neutral_rain.rs` (Stampede, Volcano), `neutral_forms.rs` (Storm Earth and Fire, Chemical Rage, Robo-Goblin, Engineering
Upgrade, Pocket Factory, Tornado and the tornado unit's abilities), `neutral_orbs.rs` (Black Arrow, Incinerate,
Incinerate Arrow), `neutral_misc.rs` (the rest). Tests: `crates/sim/tests/neutral_heroes2.rs`, `neutral_forms.rs`,
`neutral_orbs.rs`, `neutral_misc.rs`, `crates/engine/tests/neutral_aliases.rs` (real archives). Hooks in other files are
one to four lines each: `cast.rs` (timer kinds `0x60..=0x6F`, cast-end, channel time, the Parasite `Cast` field, the spell
target visibility gate), `natives/unit_event.rs` (the death notification), `combat.rs` (`neutral_orb_on_hit`, acquisition
gate), `unit_spells.rs` (`UnitMods::damage_flat`, Demolish in `spell_attack_bonus`), `orders.rs` (dropped orders).
Method: the field roles are the labels of `WorldEditStrings.txt` (`WESTRING_AEVAL_*` of `AbilityMetaData.slk`, which name
every DataA..F), the order ids come from `docs/research/ability-orders/getters.tsv`, the structure from IDA decompiles of
the class overrides and of the buff objects (`CBuff*` removal / apply slots). Level data: the user's `AbilityData.slk`.
The unit-code cells (`DataA` of Storm Earth and Fire / Dark Portal, `DataB` of Pocket Factory, `DataC..F` of Engineering
Upgrade) are read by `engine::objdata::load_neutral_lists` (`Game::set_neutral_unit_list`).

| code | class (parent) | what is modelled | confidence |
|---|---|---|---|
| `ANtm` (852665) | `CAbilityTransmute` (`SimpleSpell`), buff `CBuffTransmute < CBuffPause` | the buff action `0x6F1B59B0` -> `0x6F1B58B0`: the caster's owner gets `f2i(round(DataA * goldcost))` gold and `f2i(round(DataB * woodcost))` lumber (soft multiply, round `0x6F6EF330`), the target's life goes to zero (no killer). Target rule `0x6F19E7A0`: `0x6F1D2630` refuses a unit of race creeps whose level is above `f2i(DataC)` (error 51), then a summoned unit. The race is the one gap: the sim has no race field, a neutral player's unit (slot 12 and up) stands for a creep. The bounty add-on (`DataD`, `0x6F29DD30`) needs bounty values the sim lacks | gold / lumber / kill CONFIRMED, creep test UNSURE, bounty NOT DONE |
| `ANdo` (852583) | `CAbilityDoom`, buff `CBuffDoom < CBuffLifeDoT`, `CBuffDoomMinion` | `DataA` damage per second (first tick 0.01 s, class 15) until death, no duration, the buff cannot be dispelled (apply slot `0x354` sets the buff flag `0x200`, CONFIRMED), the unit cannot cast; the removal notification `0x6F0B6750` (CONFIRMED) creates `f2i(DataB)` units of `UnitID` at the corpse for the owner of the caster, timed life `DataC` (`BNdi`), summoned class bit cleared. Target rule `0x6F089BC0`: creep level `f2i(DataD)`, already doomed (error 0xD0) | structure CONFIRMED, damage type / silence LIKELY |
| `ANcs` (852652), `ANhs` (852664) | `CAbilityClusterRockets`, `CAbilityHealingSpray < ClusterRockets`, effects `XNcs`, `XNhs` | one effect creator `0x6F18B150` for both: area `Area`, `DataA` damage / heal, `DataD` cap, `DataE` building factor, wave interval `DataB`. Wave count: Cluster `0x6F161280` = `I2R(f2i(DataF / DataB))` (4 waves of 0.25 s over 1.01 s), Spray `0x6F19E680` = `DataF` (3..5 waves); effect lifetime (the channel time) Cluster `DataF`, Spray `DataB * DataF + 0.01` (`0x6F194AD0`). Amount per unit: the Blizzard formula (`human_heroes::blizzard_unit_damage`, Unicorn-checked there: cap `DataD` over the unit count, building factor, sign flipped for the heal flag), `CBuffAoe` marker `interval - 0.01`. Cluster hits enemy / neutral ground units and structures and stuns for `Dur` (the plain `BSTN`); Spray heals friendly organic units. An interrupted channel destroys the effect | counts and lifetimes CONFIRMED, first-wave time and damage type UNSURE |
| `ANst` (852593) | `CAbilityStampede`, buff `CBuffStampede` | channel for `HeroDur`; the channel end `0x6F1DF720` destroys the spawn timers (CONFIRMED). A lizard every `1 / DataA` s lands at a random point of the `Area` disc (stream 4: angle `2 pi U`, radius `Area * U`) and explodes `DataE` later for `DataC` damage in `DataD` | end hook CONFIRMED, spawn rule UNSURE |
| `ANvc` (852669) | `CAbilityVolcano`, effect `CEffectVolcano`, marker `CBuffVolcanoAoe` | `DataB` waves `DataC` s apart (first at once = "Lasts 35 seconds"); `DataE` damage inside half the area, `DataE * DataF` outside, structures `DataD` times, stun `Dur` / `HeroDur`; the destructible `Volc` is not placed | labels CONFIRMED, ring split UNSURE |
| `ANmo` (852591) | `CAbilityMonsoon` | the Starfall field set (`ESF1..3`) on a target point: a strike every `DataB` s for `HeroDur` on every enemy in `Area`, `DataA` damage, structures `DataC` times | LIKELY / UNSURE |
| `ANef` (852586) | `CAbilityElementalFury`, buff `CBuffElementalFury < CBuffTimedLife` | `DataA` lists the three elementals; the Brewmaster is hidden and paused, the elementals appear around it for `HeroDur`; at the end the survivors vanish and the Brewmaster returns at the first survivor; if all three die earlier it dies (the tooltip's reading) | LIKELY / UNSURE (death rule) |
| `ANcr` (852663), `ANrg` (852656 / 852657) | `CAbilityChemicalRage`, `CAbilityRoboGoblin` (`CAbilityMorph`) | `Dur` = transformation time, the unit becomes the level's `UnitID` (`morph_to`) with its learned skills kept; Chemical Rage returns `HeroDur` after the cast, Robo-Goblin on the way-back order after `Dur` again; Robo-Goblin adds `DataF` armor. The rage's speed / attack bonuses ("Info Panel Only" in the editor) come from the form's own unit record; the `DataE` strength bonus is not applied | form change LIKELY, bonuses UNSURE |
| `ANeg` | `CAbilityEngineeringUpgrade` (`Passive`) | `DataA` movement bonus (buff `BNeg`), `DataB` flat attack damage (the hero attack bonus), `DataC..F` ability upgrade pairs `from,to`: the ability read from row `from` is read from row `to` (the alias layer, `Ability::source`) | LIKELY |
| `ANsy` (852658) | `CAbilitySummonFactory` | the factory unit `UnitID` for `Dur` (`BNfy`); every `DataA` s it makes a goblin of type `DataB` for `DataC` s (`BNcg`); leash `DataE` and the goblins' explosion are not modelled | LIKELY |
| `ANto` (852597), `Atdg`, `Atsp` | `CAbilityTornado`; the tornado unit `ntor` carries `Atdg`, `Atsp`, `Atwa`, `Aasl` | the unit for `Dur` (`BNto`); `Atdg` every second: enemy structures within `Area` take `DataA`, within `DataB` they take `DataC`; `Atsp` tosses (stun + invulnerable, Cyclone style) one enemy ground non-structure unit in `Area` per 0.5 s scan for `Dur` / `HeroDur` and marks it `DataB` s; `Atwa` (random wandering) NOT DONE | summon LIKELY, `Atdg` LIKELY, `Atsp` UNSURE (air time `DataA` = 22 unused) |
| `ANba` / `ANbs` (852578 / 9 autocast, 852577 cast) | `CAbilityBlackArrow < FlamingArrows`, `CAbilityBlackArrowSpell` | autocast orb (default off): `Cost` mana per arrow, `DataA` damage, mark `BNba` for `Dur`; a marked unit that dies becomes `DataB` units of `UnitID` for the archer's owner, `DataC` s (`BNdm`). `ANbs` marks a unit by a cast | LIKELY |
| `ANic` / `ANia` | `CAbilityIncinerate < CleavingAttack`, `CAbilityIncinerateArrow` | each attack stacks the burn (`BNic`, `Dur` refreshed) and adds `DataA * stacks` damage; a unit that dies burning blasts `DataB` within `DataC` and `DataD` within `DataE` after `DataF` s. `ANia` is the autocast orb form (`Cost` per arrow, orders 852671 / 852672) | LIKELY |
| `ANbr` (852599) | `CAbilityBattleRoar` | friendly ground / air units (not buildings) within `Area` of the caster get `DataA` flat damage (`UnitMods::damage_flat`) for `Dur` | LIKELY, flat reading UNSURE (Roar's own `DataA` is a fraction) |
| `ANmr` (852565) | `CAbilityMindRot` | `DataA` mana per second for `Dur` / `HeroDur` | LIKELY |
| `ANde` | `CAbilityDemolish < WarStomp` | attack damage times `DataB` (building) / `DataC` (unit) / `DataD` (hero) when above zero, at `DataA` percent (only 100 modelled) | LIKELY |
| `ANpi` (`Apmf`, `Apig`) | `CAbilityPermImmolation` | every `Dur` the enemy ground organic units within `Area` take `DataA` | LIKELY |
| `ANrc` (852237) | `CAbilityRainOfChaos` | `DataB` infernals, one per `Dur`, each the Dreadlord Inferno effect with the `ANin` row at a random point of the `Area` disc | LIKELY / UNSURE (random point) |
| `ANin` (852232) | `CAbilityInferno` | a `STATIC_PARENTS` row of `AUin` (same data layout) | LIKELY |
| `ANfd` (852230) | `CAbilityFingerOfDeath` | `DataC` damage after the graphic delay `DataA` | LIKELY |
| `ANwm` (852598) | `CAbilityWateryMinion < WaterElemental` | the Water Elemental summon loop with `BNwm` | LIKELY |
| `ANpa` (852601) | `CAbilityParasite < AutoTargetSpell` | poison `DataA` per second for `Dur`; when the host dies `DataE` units of `UnitID` for `Cast` s (the class keeps its summoned unit duration in the `Cast` field, so the cast core must not wait on it); autocast NOT DONE | LIKELY |
| `ANcl` (852600) | `CAbilityChannel` | no effect of its own: the events of the cast and the channel time `DataA`; fallback order 852600 | LIKELY |
| `ANdp` (852229) | `CAbilityDarkPortal` | `DataB..DataC` units of the list `DataA`, in turn, at the point | UNSURE |
| `ANth`, `ANre` | aliases of `AUts`, `Aarm` | `neutral_spells::is_modelled` now also knows the unit-ability tables of `unit_abilities_a` / `_m`, so the creep alias rows of every modelled code resolve | CONFIRMED mechanism |

Not done, with reasons: `ANsu` Submerge (no data row at all), `ANss` / `ANse` Spell Shield and `ANsa` Sanctuary (item
rows: `Cool` and a buff only), `ANpr` Preservation, `ANsl` Soul Preservation, `ANdc` Dark Conversion (needs the unit race
test and a campaign sleep), `ANsp` Spies, `ANgl` Gold to Lumber (shop-like campaign abilities), `ANfy` Factory (a passive
marker the factory unit carries, no data), `Atwa` Tornado Wander (unit movement AI) and the Transmute bounty.

## V3 verification wave (Unicorn; shared harness `tools/emu/v3_cores_emu.py`, vectors `crates/sim/tests/v3_vectors.txt`)

CONFIRMED with 300 rows and 0 mismatches each: the Mana Shield hook (`MS`) and the damage-record reduce (`RR`), see the `ANms`
row. The invisibility and item results are in `abilities-invisibility.md` and `abilities-items.md` (section 18).

Checked by structure only (no run, labels unchanged): `ANca` Cleave (`0x6F1B7310`): the execute slot passes the class word
`0x20`, the callback `0x6F1B14F0` and an area shape built from `Area` and `DataA * the record's amount` to the generic area body
`0x6F2C31D0`; the callback returns 0 for the main target (`[ctx+4] == candidate`) and, for any other candidate, calls
`0x6F053040` and answers "the unit's flag `0x40000000` is clear" (so illusions are not hit). The generic body (also used by
`AIdm`, `AIfb`, `Awar`, `Aflk`, `Atau`, `Atdg`, `AOvd`, `AHav`, `Asds`) takes up to four `{radius, amount}` rings,
builds the query with the mini-program `0x6F2A3270` (opcodes `0xB` circle, `0x1D` class filter from the ability's `targs`
via `0x6F25D830`, `0x1F` end) and delivers through `0x6F2BFED0`. Stock `Silence` has `DataA` 8 and `DataB` = `DataC` = 0, so the
unread roles of `DataB` / `DataC` do not matter for stock data. Stock `Cyclone` `DataA` 1, `Charm` `DataA` 5 (the level rule
`0x6F1A2600`), `Howl of Terror` `DataA` 0.3.

## 9. V6 verification wave (Unicorn, `tools/emu/v6_emu.py`, vectors `crates/sim/tests/v6_vectors.txt`, tests `v6_vectors.rs`)

Every row is the ORIGINAL code on fake objects (callees that need the object model stubbed and recorded), 350 rows per kind,
0 mismatches after the fixes listed.

| Kind | Original | Model | Result |
|---|---|---|---|
| `TM` | Transmute buff body `0x6F1B58B0` (via `0x6F1B59B0`) | `neutral_heroes2::transmute_amounts` | **CONFIRMED**: gold = `f2i(round(A * goldcost))`, lumber = `f2i(round(B * woodcost))` with the factor of the gold the float behind `eax` (the buff's `+0xE0`) and the lumber one behind the third argument (`+0xE8`); the target's own bounty (`0x6F29DD30`, `0x6F29DDD0`) is added when the buff flag bit 10 (`DataD`) is set and `0x6F473170` answers yes; an illusion target gives nothing; the life setter (`0x6F2829D0`) and the flag set run in every case. Saturating `f2i` covered. The map of `DataA/DataB` into `+0xE0/+0xE8` (effect `0x6F1C18B0`, buff apply) and the creep-level test are not part of the rows |
| `CL` | creep level rule `0x6F1D2630` over the unit level `0x6F29DCA0` | `neutral_heroes2::{creep_level_refuses, unit_level_value}` | **CONFIRMED**; **bug fixed**: the Transmute, Doom and Charm target slots refuse a unit only when its type's **race is creeps (8)** and its unit level is above the limit; the unit level is the type `level` (0 when not positive), replaced by the level field of a level-changing buff when one answers yes to its slot `0x36C`, at most Misc `MaxHeroLevel` for an uppercase-first type code that is not an illusion, else at most `MaxUnitLevel`. The old model refused any unit of any race whose raw `level` was above the limit (Charm) or used the owner slot as the race (Transmute, Doom); it now reads the race from the type's pick record and falls back to the owner stand-in only without one |
| `CE` | Charm effect `0x6F1B13E0` | `neutral_spells::charm_kills` | **CONFIRMED**; **bug fixed**: an **illusion** target is killed (its life setter, vtable `0x124`, gets the constant 0.0; no owner change), every other target changes owner (`0x6F1B0C80`). The old model changed the owner of an illusion. The Charm limit is `f2i(DataA)` (the old model compared with the real) |

### Decoded in V6 but NOT verified (leads for later waves)

* **Cleaving Attack `ANca`** (execute `0x6F1B7310`): the area body `0x6F2C31D0` gets `{Area, DataA * record amount (+0x10)}`, count 1, flags
  `0x20`, class from the record, `targs`, callback `0x6F1B14F0`; but the **centre is not the target**: the code takes the attacker's position
  object (`vtable 0xB8`, then its `vtable 0x1C` with the `Area` and `0x6F10E370` / two soft adds), i.e. the area is built from the attacker's
  position and the swing direction (a cone or a shifted circle), not a circle around the target as `neutral_cleave` models. UNSURE until the
  geometry is decoded.
* **Cyclone `Acyc` / `ACcy`** (effect `0x6F1CCC70` -> `0x6F1CCB50`): duration = `HeroDur` for a hero target, else `Dur + [ability+0x9C]`
  (soft add); the buff class is **chosen by `DataA == 0 and Misc CycloneStasis == 0`** (`CBuffCycloneTwo` vs `CBuffCyclone`, creators `0x6F1C9160`
  / `0x6F1C9230`), not a stun plus invulnerability pair; before the buff is applied the target's buffs are **removed** (two calls of the removal
  walker `0x6F079FE0`: positive with the filters `(0,0,0,0,1,1,0,1)` and negative with `(0,0,0,1,...)`, timed-life buffs kept), and a target with
  unit flag `0x100` or whose `0x6F29C240` answers 2 is skipped. The model does none of that (UNSURE, flagged).
* **Soul Burn `ANso`**: the buff `CBuffSoulBurn` derives from `CBuffCripple < CBuffSpeed`, not from a damage-over-time class; the effect
  hands `(duration, target, DataA, DataB, DataD)` to the buff apply (slot `0x368`). The model's "`DataA` damage per second" reading is therefore
  UNSURE.
* **Ankh `AIrc`** (death handler `0x6F1F1D50`): `DataB` is raised to at least 1.0 (soft compare with the constant `1.0`), `DataC` below 0.0 is replaced by
  a property of the carrier read through `0x6F02DCA0` (the agile value at unit `+0xB8`); then the effect object is created (`0x6F1F0110`) and its
  slot `0x78` gets `(carrier, DataA, ..., &DataB', &DataC')`, the item's charge routine `0x6F2B8390` runs, the carrier gets flag `0x1000` and
  `0x6F2964E0` runs. Skipped when the carrier is an illusion, when a pathing test at the death point (`0x6F468F90`, mode 2) fails, or when the
  item has no charge record. The revival numbers live in the effect object (slot `0x78`), not decoded.
* **Cloak of Flames `AIcf`** (`0x6F13B960`): hands `(Dur, Area, DataA, the order id of slot 0x308)` to the buff apply (slot `0x368` of
  `CBuffImmolation < CBuffLightningShield`, `0x6F12C4D0`), which also creates two agile modifiers and a 0.1 s timer family (`0xD01BA`);
  the per-tick damage was not decoded.
* **Howl of Terror `ANht`**: only the target rules (`0x3EC`, `0x3F0`) differ from `Aroa`; the shared cast effect `0x6F1EED60` builds a group
  limited by `f2i(DataG)` (0 = no limit), ordered by `DataH` / `DataI` (comparators `0x6F1DC050` / `0x6F1DC030` / `0x6F1D3BF0`) and applies the buff
  through the slot `0x3EC` of the ability; the model ignores the limit and the order.
* **Web scroll `AIwb`**: the unit effect `0x6F0AB010` is byte-identical to `Aweb`'s `0x6F0AADD0` (S53); the effect goes through the buff factory
  (`0x6F07ECE0` / `0x6F084E70`), so its numbers were not isolated.
* **Dagger of Escape (`AEbl`) and Staff of Teleportation (`AHmt`)**: the item rows carry the base code itself (spec abilities-items.md s14), so
  the classes are the ones of the confirmed `AEbl` (Blink validation) and `AHmt` (sort comparator, channel delay, targs normaliser) rows.
* **Drunken Brawler `ANdb`**: the class overrides only the code getters (`0x4`, `0x1C`, `0x58`, `0x308`) of `CAbilityCriticalStrike`, so every
  slot that runs the confirmed `AOcr` PRD code is inherited unchanged; its evasion half (`DataD`) goes through the unresolved miss-gate walk.
