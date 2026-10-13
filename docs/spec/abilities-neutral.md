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
    area spells). Needs `GAME_DLL` for the RNG in its tests. Confidence: LIKELY (no Unicorn run; the chance chain re-uses the
    verified Critical Strike functions).

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
| `ANca` | CAbilityCleavingAttack | CAbilityPassive | 9 | NOT DONE |
| `ANch` | CAbilityCharm | CAbilitySimpleSpell | 3 | NOT DONE |
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
| `ANht` | CAbilityHowlOfTerror | CAbilityRoar | 3 | NOT DONE |
| `ANia` | CAbilityIncinerateArrow | CAbilityRangerArrow | 10 | NOT DONE |
| `ANic` | CAbilityIncinerate | CAbilityCleavingAttack | 8 | NOT DONE |
| `ANin` | CAbilityInferno | CAbilitySimpleSpell | 10 | NOT DONE |
| `ANlm` | CAbilityLavaMonster | CAbilityWaterElemental | 1 | NOT DONE |
| `ANmo` | CAbilityMonsoon | CAbilitySimpleSpell | 13 | NOT DONE |
| `ANmr` | CAbilityMindRot | CAbilitySimpleSpell | 2 | NOT DONE |
| `ANms` | CAbilityManaShield | CAbilitySimpleSpell | 14 | NOT DONE |
| `ANpa` | CAbilityParasite | CAbilityAutoTargetSpell | 6 | NOT DONE |
| `ANpi` | CAbilityPermImmolation | CAbilityPassive | 10 | NOT DONE |
| `ANpr` | CAbilityPreservation | CAbilitySimpleSpell | 4 | NOT DONE |
| `ANrc` | CAbilityRainOfChaos | CAbilitySimpleSpell | 13 | NOT DONE |
| `ANre` | CAbilityNeutralRegen | CAbility | 5 | NOT DONE |
| `ANrf` | CAbilityRainOfFire | CAbilityBlizzard | 1 | NOT DONE |
| `ANrg` | CAbilityRoboGoblin | CAbilityMorph | 18 | NOT DONE |
| `ANsa` | CAbilitySanctuary | CAbilityPreservation | 2 | NOT DONE |
| `ANse` | CAbilitySpellShieldAoe | CAbilitySpellShield | 5 | NOT DONE |
| `ANsg` | CAbilitySummonGrizzly | CAbilityWaterElemental | 1 | NOT DONE |
| `ANsi` | CAbilitySilence | CAbilitySimpleSpell | 12 | NOT DONE |
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
| `ANtm` | CAbilityTransmute | CAbilitySimpleSpell | 4 | NOT DONE |
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
| `Acyc` | CAbilityCyclone | CAbilitySimpleSpell | 8 | NOT DONE |
| `Adev` | CAbilityDevour | CAbilityCargoLoad | 7 | NOT DONE |
| `Advc` | CAbilityDevourCargo | CAbilityCargoHold | 8 | NOT DONE |
| `Alit` | CAbilityLightningAttack | CAbility | 15 | NOT DONE |
| `Amim` | CAbilityMagicImmunity | CAbilityPassive | 5 | magic immunity count (new) |
| `Anhe` | CAbilityNeutralHeal | CAbilityHeal | 2 | alias of Ahea (852248/49/50) |
| `Apig` | CAbilityPandaImmolation | CAbilityPermImmolation | 1 | NOT DONE |
| `Arsk` | CAbilityResistantSkin | CAbilityPassive | 2 | NOT DONE |
| `Ashm` | CAbilityShadowMeld | CAbilityButton | 21 | NOT DONE |
| `Aven` | CAbilityVenomSpear | CAbilityPoisonAttack | 0 | on-hit poison (new) |
| `Awar` | CAbilityWarStomp | CAbilityPassive | 9 | Pulverize (new) |
