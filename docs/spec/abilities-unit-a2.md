# Unit abilities, batch A part 2 (S51)

Model: `crates/sim/src/unit_abilities_a2.rs` (registry `ABILITIES`, hooks through `unit_abilities_a.rs` into `cast.rs`),
economy hooks in `crates/sim/src/production.rs`. Tests: `crates/sim/tests/unit_abilities_a2.rs`.
This finishes the "not done" list of `abilities-unit-a.md`. Level fields are the 1.26a `AbilityData.slk` first-level rows
labelled by `AbilityMetaData.slk` + `WorldEditStrings.txt` (read from the user's archives, not committed); classes and
parents are from the ability atlas (`docs/research/ability-atlas/factory.tsv`); order ids are the original cast getters
(`docs/research/ability-orders/getters.tsv`) or the original order-name table (`orders/names.rs`).
Labels: CONFIRMED = machine code, LIKELY = class parent or editor field meaning, UNSURE = a stand-in. **No Unicorn
number run was made in this batch** (no new bit-exact numeric core was needed: every number is a level-record field used
directly), so nothing here is CONFIRMED by emulation; the model reuses already-checked cores (`unit_damage_target`,
buff framework, income changer).

## Economy family (Agld, Ahar, Ahrl, Aaha, Abgm, Aegm, Aent)

The harvest, mine and entangle systems of `production.rs` were driven only by the unit TYPE table (`TypeTech`, filled by
the engine from the ability lists). They now also read the unit's own abilities, so `UnitAddAbility` makes them work and the
level record supplies the numbers. A type row that already carries the ability codes wins (the engine's behaviour is
unchanged).

| Code | Class (parent) | Model | Label |
|---|---|---|---|
| Ahar | CAbilityHarvest (HarvestLumber) | the unit can harvest gold and lumber; `DataA` damage to a tree = lumber per hit, `DataB` lumber capacity, `DataC` gold capacity (labels "Damage to Tree", "Lumber Capacity", "Gold Capacity"). Order `harvest` 852018: target = a mine (starts a gold cycle from the nearest hall), a point under a tree (starts a lumber cycle on that tree); `returnresources` 852020 (no target or a hall) sends a carried load to the hall now | LIKELY |
| Ahrl | CAbilityHarvestLumber (HarvestReturn) | ghoul: lumber only, `DataA` 2 per hit, `DataB` 20 capacity; the shared `harvest` order resolves to Ahrl when the unit has no Ahar (`order_alias`) | LIKELY |
| Aaha | CAbilityAcolyteHarvest (CPower) | gold in the haunted mode (attach to a haunted mine, the mine pays); order `acolyteharvest` 852185 on the mine | LIKELY |
| Agld | CAbilityGoldMine (GoldMineBase) | a unit that gains `Agld` becomes a resource node of `DataA` gold (12500 in the row; fallback: the host's `ability_resource` amount); losing it removes the node. `DataB`/`DataC` (mining duration, mining capacity) stay in the harvest cycle constants | LIKELY (amount), UNSURE (DataB/C unused) |
| Abgm | CAbilityBlightedGoldMine (PossessedGoldMine) | a unit with `Abgm` is a haunted mine for acolytes: `DataA` gold per payment, `DataB` seconds, `DataC` miners (at most 8); the payment period is `DataB * slots / workers` (CONFIRMED rule of economy-5.md); `DataD` (mining ring radius 200) unused | LIKELY |
| Aegm | CAbilityEntangledGoldMine (PossessedGoldMine) | the same for wisps: `DataA`, `DataB`, 5 miners by default | LIKELY |
| Aent | CAbilityEntangle (CPower) | the hall with `Aent` turns a neutral gold mine within `Rng` into `UnitID` after `Cast` seconds: autocast scan once per second (the engine's stand-in) and the manual order `entangle` 852147 on a mine. The old mine is hidden and its gold moves to the new one (`take_over_mine`) | LIKELY |

`Aall` (CAbilityAllied, neutral "Allied": activation radius 350, interaction type, show buttons) is a campaign
interaction marker with no simulation effect and is not modelled.

## On-hit and on-death effects (Acor, Afak, Aflk, Afsh, Afrb, Afrz, Aliq, Adda)

Hook: `Game::a2_on_hit(attacker, target)` runs after the existing on-hit effects of `combat.rs::hit_with_bash` (one line
there) for a target that survived; `Game::a2_on_unit_died` is called from the unit death dispatch
(`natives/unit_event.rs`, one line). Extra damage goes through `unit_damage_target` (the Unicorn-checked target side).
Tests: `crates/sim/tests/unit_abilities_a2_strike.rs` (real attacks through the attack cycle).

| Code | Class (parent) | Model | Label |
|---|---|---|---|
| Acor | CAbilityCorrosiveBreath (Passive) | hit on a structure: buff `Bcor` (CBuffCorrosiveBreath < CBuffLifeDoT, code from the atlas) deals `DataA` ("Damage Per Second") per second for `Dur`, damage type acid | LIKELY |
| Afak | CAbilityFlamingAttack (FlamingArrows, an orb like Searing Arrows) | each hit pays `Cost` mana (skipped when short); the target takes `DataA` ("Damage Bonus") fire damage; others inside `DataE` ("Half Damage Radius", 75) take `DataB` ("Medium Damage Factor") of it, inside `Area` take `DataC` ("Small Damage Factor"); `DataD` = full damage radius (1) | LIKELY (labels), UNSURE (ring geometry) |
| Aflk | CAbilityFlakCannon (Passive) | hit on an air target: other enemies/neutrals of air kind take `DataC` inside `Area` ("full" radius), `DataD` inside `DataA`, `DataE` inside `DataB` (labels: medium/small damage radius and amounts) | LIKELY; UNSURE whether the target also takes the full tier (modelled: no) |
| Afsh | CAbilityFragShards (Passive) | the same on ground targets; the stock row lists medium radius 275 and small radius 250, so the radii are sorted ascending and paired with the amounts in descending order | LIKELY / UNSURE (sorting) |
| Afrb | CAbilityFrostBreath (CAbility) | hit: the Frost Attack slow `Bfro` (Misc FrostMoveSpeedDecrease/FrostAttackSpeedDecrease, `frost_apply`) for `Dur`/`HeroDur` | LIKELY |
| Afrz | CAbilityFreezingBreath (Passive) | hit on a structure: `Bfrz` (CBuffFreezingBreath < CBuffStun, atlas) stuns it and every enemy structure within `Area` for `Dur` | LIKELY |
| Aliq | CAbilityLiquidFire (Passive) | hit on a structure: `Bliq` (CBuffLiquidFire < CBuffPoisonAttack) for `Dur`: `DataA` damage per second (fire), attack speed `-DataC`, movement `-DataB`; `DataD` (repairs allowed) has no sink | LIKELY / UNSURE (DataD) |
| Adda | CAbilityDeathDamageAoe (CAbility) | when the unit dies, after `Dur` (0.3 s): `DataB` damage inside `DataA`, `DataD` inside `DataC` (labels: full/partial radius and amount) to enemies of the dead unit's player (targs ground, structure, debris, enemy, ward); damage type demolition | UNSURE (trigger and damage type) |

Not modelled in this family: `Aimp` (Impaling Bolt) and `Agyb` (Gyro Bombs) have no data fields in the 1.26a rows and
`Alit` (Lightning Attack: "Graphic Delay", "Graphic Duration") is presentation only.

## Trees (Aeat, Agra)

The cast infrastructure takes unit and point targets, so a tree order is a point order at the tree (the nearest standing tree
within 128 units of the point, `Game::tree_near`) and the tree is a `production` tree (its wood goes to 0 when felled,
`Game::fell_tree`). The caster must stand within 160 units of the tree when the order takes effect (the unit walks up to it in
the original; the model has no approach, so a farther tree makes the order a no-op: a stand-in). Range gates are skipped for
these classes (`ignores_range`, two lines in `cast.rs`). Timers: kind `0x51`. Tests: `unit_abilities_a2.rs`.

| Code | Class (parent) | Model | Label |
|---|---|---|---|
| Aeat | CAbilityEatTree (SimpleSpell), order `eattree` 852146 (cast getter, CONFIRMED) | after `DataA` ("Rip Delay") the tree is felled; `DataB` ("Eat Delay") seconds later the caster gets `Beat` (CBuffEatTree < CBuffRejuvination, atlas) for `Dur` + 0.01 s healing `DataC` ("Hit Points Gained", 500) / `Dur` per second as the Rejuvenation tick (`register_tick`, hot) | LIKELY (numbers from the labels), UNSURE (which delay the heal waits for) |
| Agra | CAbilityGrabTree (EatTree), order `grabtree` 852511 (getter, CONFIRMED) | after `DataA` ("Attach Delay") the tree is felled and `Bgra` (CBuffGrabTree) marks the unit; `DataE` ("Maximum Attacks", 15) attack hits use it up; `DataB` ("Remove Delay") seconds after the last one the buff goes. The weapon switch (`DataC` disabled / `DataD` enabled attack index) is NOT modelled | UNSURE |

`Aesn` (Sentinel, order 852182, `XEsn` effect class with no `UnitID`) is not modelled: the owl's vision and lifetime are not
in the level row and no unit type is named.

## Unit conversion (Acoa, Acoh, Acoi, Adec, Abtl, Alam)

Tests: `crates/sim/tests/unit_abilities_a2_units.rs`. A rawcode data field (`DataA` of `Acoa`/`Acoh`/`Adec`, partner types
as text in the SLK) is stored by the host's level loader as the BITS of the `F32` (`UnitID` has its own field). Engine hookup
(not done, `crates/engine` is outside this lane): `objdata` must load `Acoa`, `Acoh`, `Adec`, `Abtl` and the economy codes
into `spell_levels` with that convention.

| Code | Class (parent) | Model | Label |
|---|---|---|---|
| Acoa / Acoh | CAbilityCoupleArcher / CoupleHippogryph (CAbilityCouple) | order `coupletarget` 852507 (getter of `Acoi`, CONFIRMED value) on an own, free unit of the partner type `DataA`: both are removed and a unit of `UnitID` (Hippogryph Rider) is built at the caster's place; the unit's own class answers the shared order (`order_alias`). `Rng` 100 (mount distance) is not enforced (no approach) | LIKELY |
| Acoi | CAbilityCoupleInstant (ClosestTargetSpell) | order `coupleinstant` 852508 (primary getter): the closest eligible partner is taken; the search radius is not decoded (modelled unlimited). The ability carries no data row; the data come from `Acoa` / `Acoh` | UNSURE |
| Adec | CAbilityDecouple (SimpleSpell), order 852509 (getter) | the rider is removed and units `DataA` and `DataB` are built 32 units either side of it with the rider's life fraction (the fraction is a choice, UNSURE); cooldown from `Cool` by the cast core | LIKELY |
| Abtl | CAbilityBattlestations (Spell), order 852099 (getter) | own units of type `UnitID` within `Area` go into the holder's cargo while it has room (capacity from the hold abilities, `unit_abilities_a.rs`); units on a harvest cycle only when `DataB` ("Summon Busy Units") is set, and leave the cycle (`end_harvest`). The walk is skipped | UNSURE |
| Alam | CAbilitySacrificialLamb (SimpleSpell), order `requestsacrifice` 852201 (getter) | on an own structure (targs structure,player): the acolyte is removed and a Shade (`ushd`) is built at the structure. In the original the pit's `Asac` (CAbilityTrain) does the conversion; the type name is a stand-in | UNSURE |

Not modelled: `Acha` (Chaos, no data row) and `Achl` (Chaos cargo load: only `UnitID` ncpn; the chaos transformation table
is not in the level rows).

## Corpses and dispel (Acan, Aast, Advm, Agyd)

Corpses are dead, not removed, non-structure, non-mechanical units (`Game::corpse_ok`). A corpse target is a unit target
(`hero_extra::target_ok` accepts a dead target for `wants_corpse` classes, one line). Tests:
`crates/sim/tests/unit_abilities_a2_corpse.rs`.

| Code | Class (parent) | Model | Label |
|---|---|---|---|
| Acan | CAbilityCannibalize (ClosestTargetSpell), orders `cannibalize` 852188 (primary getter) and 852535 (cast getter) | the unit stands still and once a second, up to `Dur` (33 s), heals `DataA` ("Hit Points per Second", 10) while its life is below the smaller of its max and `DataB` ("Max Hit Points", 800); the corpse is eaten (removed) when it ends. Moving, dying or being stunned ends it and leaves the corpse. The instant order picks the closest corpse within 500 (search radius not decoded) | UNSURE (stop rules; the original is a channel) |
| Aast | CAbilityAncestralSpirit (ClosestTargetSpell), orders `ancestralspirit` 852490 / 852491 (getters) | an own non-hero corpse (targs ground, player, dead; closest within `Rng` 350 for the instant order) comes back (`resurrect_unit`) with `DataA` ("Life Restored Factor") of its max life and `DataB` ("Mana Restored Factor") of its max mana | UNSURE (that it resurrects is read from the labels) |
| Advm | CAbilityDevourMagic (SimpleSpell), order `devourmagic` 852536 (getter) | the Dispel Magic worker (`dispel_one`, smart mode: positive buffs of enemies, negative of the others) on every unit within `Area` of the point; friends are skipped when `DataF` ("Ignore Friendly Buffs") is set; `DataE` ("Summoned Unit Damage", 180) goes to the worker's damage input; each unit that lost a buff or was hurt gives the caster `DataA` life and `DataB` mana, each buff removed `DataC` / `DataD` more | UNSURE |
| Agyd | CAbilityGraveyard (CAbility) | every `Cool` seconds (15) a dead `UnitID` corpse (ghoul) is made on a ring of radius `DataB` (200) around the unit when fewer than `DataA` (5) lie within `DataC` (250), built and made dead as `CreateCorpse` does | UNSURE |

## Area effects and hero passives (Afla, Ablp, Abli, Aamk)

Tests: `crates/sim/tests/unit_abilities_a2_area.rs`.

| Code | Class (parent) | Model | Label |
|---|---|---|---|
| Afla | CAbilityFlare (SimpleSpell), order `flare` 852060 (getter) | point cast (`Rng` 99999 not gated); after `DataB` ("Effect Delay", 0.8 s) a visible fog modifier of radius `Area` opens for the caster's player and closes after `Dur` (15 s), the reveal of `item_abilities` (`pub(crate) reveal`). `DataA` ("Detection Type", 3) is not modelled | LIKELY (reveal), UNSURE (detection) |
| Ablp / Abli | CAbilityBlightPlacement / CAbilityBlightGrowth | once the unit is complete, blight grows around it by `DataA` ("Expansion Amount", 64) every `Dur` (0.08 s) up to `Area` (350 for Ablp, 768 for the Necropolis/Graveyard alias Abgs whose code is `Abli`); `DataB` ("Creates Blight") blights or clears the 32-unit cells (`Game::blight_circle`). Blight is not removed when the building dies | UNSURE (the cadence is the row read as a step interval) |
| Aamk | CAbilityAttributeModSkill (Passive) | a hero with it carries `F2I(DataA)` agility, `F2I(DataB)` intelligence and `F2I(DataC)` strength more in its base counters (the same adders and derived-bonus refresh as the attribute bonus items, `change_hero_attributes`); a level change or losing the ability adjusts the difference | LIKELY |

## Still not modelled (reason)

| Code | Reason |
|---|---|
| Aall | CAbilityAllied (campaign "Allied" marker: activation radius, interaction type, show buttons): no simulation effect |
| Aawa, Alea, Aapl, Adta, Acha, Abar | no AbilityData row (Alea, Aapl, Adta, Acha, Abar) or a bare marker (Aawa); the numbers are not in the data |
| Aimp, Agyb | no data fields in the rows (Impaling Bolt, Gyro Bombs): nothing to compute |
| Alit | Lightning Attack: graphic delay and duration only (presentation) |
| Aesn | Sentinel: the owl is an effect (`XEsn`) with no `UnitID`; its lifetime and sight rules are not in the row |
| Aexh | Exhume Corpses needs the Meat Wagon's corpse cargo (`Amtc`, `Amel`), which `unit_abilities_m.rs` only lists |
| Aclf | Cloud of Fog: "Attacks Prevented" / chance to miss / speed modifiers on units in an area; the silence class does not state which attacks are stopped |
| Adef | Defend: needs a damage-taken-by-attack-type sink (piercing 50 %, spells, 30 % deflect) that the damage path does not have |
| Abof | Balls of Fire (Demolisher): the trigger and shape of the periodic area damage are not in the row |
| Achl | Chaos Cargo Load: only a `UnitID` (the chaos peon), the conversion table is not in the row |
| Ahid | Hide: owned by the invisibility module (S44) |

## Unicorn

None run in this batch: every number is a level-record field used as the editor labels name it, and the cores reused
(`unit_damage_target`, the buff framework, the income changer, `dispel_one`, the Rejuvenation tick) are the ones already
checked. All labels are therefore LIKELY or UNSURE, none CONFIRMED.
