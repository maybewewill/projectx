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
