# Hero and hero-like abilities, S48 (`crates/sim/src/hero_extra.rs`)

Sources: ability atlas (`docs/research/ability-atlas/factory.tsv`: class, vtable, parent), cast order getters
(`docs/research/ability-orders/after-fix/getters.tsv`, slot `0x348`), the 1.26a `Units\AbilityData.slk` rows (War3Patch layer)
for the meaning and values of the level fields. **Nothing here ran in Unicorn**: the order ids are Unicorn-checked
(earlier lane), all effect numbers are LIKELY or UNSURE as marked. Tests: `crates/sim/tests/hero_extra.rs` (12).

Wiring (a few lines each): `cast.rs` (`run_effect` calls `hero_extra::run_effect` first, `takes_no_target`, the order table,
`channel_time` override, `end_cast` hook, `drop_spell_buff` hook, dead-target legality for Scarabs), `undead_ne.rs`
(timer kinds 20..22, state field), `combat.rs` (`hero_orb_on_hit` after `spell_on_hit`), `orc.rs` (`AOwd` alias).

| Code | Class (vtable) | Order | Model | Confidence |
|---|---|---|---|---|
| `AEfk` Fan of Knives | `CAbilityFanOfKnives` `0x6F8AAC14` < SimpleSpell | 852526 immediate | enemies (not structure / mechanical) in `Area` of the caster take `DataA`; if `n*DataA > DataB` each takes `DataB/n` | LIKELY (cap rule from the tooltip; launcher `0x6F2C5C40` not decoded); `DataD` (100) unused |
| `AEmb` Mana Burn | `CAbilityManaBurn` `0x6F8DEFBC` | 852179 unit | burn `min(mana, DataA)`, damage `burned * DataC` | LIKELY; `DataB` (0.25) role UNSURE; the real cast is a bolt (instant here) |
| `AEme` Metamorphosis | `CAbilityMetamorphosis` `0x6F8D9A1C` < Morph | 852180 immediate | after `Dur` (1.5 s) buff `BEme` for `HeroDur`, `+DataE` max and current life, removed at the end | UNSURE: the morph into `UnitID` (`Edmm`), its damage/armor/abilities are not applied |
| `AEpa` Poison Arrows | `CAbilityPoisonArrows` `0x6F8E118C` < RangerArrow | orb | on hit: bonus `DataA` (spell damage, poison type) and a `Bpoi` poison of `DataB`/s for `HeroDur`/`Dur`; `Cost` per hit | UNSURE (family pattern 10/20/30 + 4/6/8) |
| `AEsf` Starfall | `CAbilityStarfall` `0x6F8EFFB4` < Tranquility | 852183 immediate, **channel** | channel `HeroDur`; every `DataB` s all enemies (structures too) in `Area` take `DataA` | UNSURE (the original drops stars on targets, `DataC` = star area); channel hooks via cast.rs |
| `AEst` Scout | `CAbilityScout` `0x6F91E16C` < WaterElemental | 852181 immediate | `f2i(DataA)` units of `UnitID` with timed life `Dur` (`BEst`) | LIKELY (shared summon loop `0x6F163390`) |
| `AEsv` Spirit of Vengeance | `CAbilitySpiritOfVengeance` `0x6F92685C` < WaterElemental | 852528 immediate | as Scout (`espv`, `BEsv`) | LIKELY |
| `AEtq` Tranquility | `CAbilityTranquility` `0x6F8EFBC4` < Spell | 852184 immediate, **channel** | channel `HeroDur`; every `DataB` s each non-enemy unit in `Area` (caster included) heals `DataA` | UNSURE (pulse timing, `DataC`) |
| `AUan` Animate Dead | `CAbilityAnimateDead` `0x6F90CC84` < SimpleSpell | 852217 immediate | up to `f2i(DataA)` nearest non-hero corpses in `Area` become new units of the same type for the caster's player, timed life `Dur` (`BUan`), corpse removed | LIKELY; `DataB` = 1 (invulnerable?) not modelled |
| `ACad` creep Animate Dead | `CAbilityCreepAnimateDead` `0x6F90D0CC` < AnimateDead | 852246 immediate | same body | LIKELY |
| `AUcb` Carrion Scarabs | `CAbilityCarrionScarabs` `0x6F8CBD34` < RaiseDead | 852551 corpse target | `f2i(DataA)` units of the scarab type (level `UnitID`, else `Game::set_extra_unit`) at the corpse, permanent, corpse removed | UNSURE: `DataC` is a rawcode string in the slk (`ucs1..3`), `DataE` = 5 not modelled |
| `AUds` Dark Summoning | `CAbilityDarkSummoning` `0x6F88C32C` | 852220 unit | up to `f2i(DataA)` of the caster's own non-building units within `Area` of the target move to the caster | UNSURE (unit vs point target, placement) |
| `AUls` Locust Swarm | `CAbilityLocustSwarm` `0x6F8DDA84` | 852556 immediate | `f2i(DataC)` units of `UnitID` (`uloc`) for `Dur` | LIKELY for the summon; the life steal (`DataD`) and the return are not modelled |
| `AHca` Cold Arrows | `CAbilityColdArrows` `0x6F913ADC` < RangerArrow | orb | on hit: `DataA` bonus (cold), `BHca` slow move `-DataB`, attack `-DataC`, `HeroDur`/`Dur`; `Cost` 8 per hit | UNSURE (bonus is added to the attack in the original) |
| `AHfa` Searing Arrows | `CAbilityFlamingArrows` `0x6F8AE384` < RangerArrow | orb | on hit: `DataA` bonus (fire), structures allowed; `Cost` per hit | UNSURE |
| `ACdv` creep Devour | `CAbilityCreepDevour` `0x6F8A571C` < Devour | 852247 unit | target (non-hero) disabled (`Bdvv`) for `DataA` (5) s, then killed by the devourer; released if the devourer dies | UNSURE |
| `AOwd` | `CAbilityWard` `0x6F8F91E4` | 852504 | the base code of the Serpent Ward: `orc.rs` runs `AOsw` and now `AOwd` identically | CONFIRMED (class identity) |
| `AAns` | `CAbilityNeutralSpell` `0x6F886C64` < Button | none | constant only: a button class, no effect in the sim | LIKELY |
| `ARal` Rally | `CAbilityRally` `0x6F87C6F4` < Interfaced | smart/rally | only a stored rally point (`Game::set_rally_point` / `rally_point`); the unit rally natives and the spawn use are not wired | UNSURE |

Orbs: always on once learned (the autocast toggle orders `852243..852245` etc. are not wired); a hit with less mana than `Cost`
skips the orb. The orb damage is delivered as a separate spell-type hit after the attack (UNSURE).
Open: Unicorn runs of the effect bodies (`0x6F08C100` launcher call, `0x6F135220`, Starfall/Tranquility effect objects `XEsf`
`XEtq`), Metamorphosis morph, orb autocast toggles, stock-data wiring of `UnitID`/`DataC` rawcodes into `SpellLevel`.
