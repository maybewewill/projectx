# Invisibility and detection (S44)

Code: `crates/sim/src/invisibility.rs`; tests `crates/sim/tests/invisibility.rs`. Method: IDA on the user's `game.dll`
(no Unicorn run: the model is a counter and a mask test, no new numeric kernel). Labels as in the other specs.

## Original (read from the machine code)

| piece | address | what |
|---|---|---|
| increment | `0x6F28D7D0(unit, &fade)` | unit counter `+0x114` ++; on 0 -> 1 with fade 0 the flag setter `0x6F284950` ORs `0x1000000` into unit `+0x5C`; with fade > 0 a timer (message 852436) sets it later. CONFIRMED |
| decrement | `0x6F28D9C0(unit, &fade)` | counter --; at 0 the flag is cleared at once, then the fade-in is started. CONFIRMED |
| PermInvis `Apiv` | slot `0x12C` `0x6F118250` | increments with fade = the level `Dur` (2.0 in the data) when its level counter `+0x3C` is <= 0. `Agho` (Ghost) is a `CAbilityPermInvis` with the same slot. CONFIRMED structure |
| Invisibility `Aivs` | effect `0x6F13C080` | buff `Binv` for `Dur` / `HeroDur` (the shape of the potion effect `0x6F13C180`). CONFIRMED structure |
| viewer gate | `0x6F3A15F0(unit, viewerMask, flag)` | invisible unit is seen iff owner in the viewer's mask (player `+0x2E0`) or unit detection mask (`0x6F27A380(1)` = sub-object `+0x24`) & mask. Not invisible: always passes. CONFIRMED |
| natives | `0x6F3C7CC0` IsUnitInvisible, `0x6F3C7C80` IsUnitDetected, `0x6F2E07C0` UnitInvis | IsUnitInvisible = unit slot `0xFC`(player,0,4) == 0; IsUnitDetected reads the per-player counters (`+0x74`). LIKELY mapping |
| detectors | `Adet`, `Adts`, `Atru`, `Agyv` | `CAbilityDetector` family (init `0x6F055070`); radius = level `Rng` 900 (LIKELY) |
| Shadow Meld `Ashm` | `CAbilityShadowMeld < CAbilityButton` | 21 overrides; init `0x6F1ECB20` reads a day/night state of the world and sets/clears the disable flag `0x100`; the level numbers 1.5 / 2.5 / 0.5 are stored in three property cells (`0x6F1DCD70`). The cast order is not a factory getter (no id in `getters.tsv`). UNSURE |

## Model

* Counter derived on query: each `Binv` / `BOwk` buff, each `Apiv` / `Agho` ability, Shadow Meld while it is night and
  the unit has no order. Removal of a buff or ability needs no hook.
* Detection mask derived on query: owners of live detectors whose radius covers the unit.
* `Game::invisibility_gate(unit, viewer)`: not invisible, or viewer is the owner / not an enemy of the owner, or the
  viewer's vision mask contains the owner, or the detection mask intersects the viewer's mask.
* Users: Wind Walk (`BOwk` buff already applied by `orc.rs`), the potion (`Binv` by `item_abilities`), Sorceress
  Invisibility (order 852069), Ghost / Permanent Invisibility, Shadow Meld.
* Effects: `IsUnitInvisible`, `IsUnitDetected`, `IsUnitVisible` (gate first) natives; the idle acquisition scan skips
  units the scanning owner cannot see.

## Open / UNSURE (before S50)

* The fade (Apiv `Dur`, Wind Walk `DataA`): the unit is invisible at once here.
* Whether the viewer mask word contains allies (taken: owner, non-enemies, shared vision).
* Attack orders and spell targets against unseen units are not refused; chasing units keep their target when it
  becomes invisible.
* What breaks invisibility: only Wind Walk's own first-strike rule (orc.rs); attacking does not break `Binv` / `Apiv`
  (LIKELY).
* Shadow Meld: "idle at night" is a stand-in; the fade (DataA 1.5) and the order are not decoded.
* `UnitInvis` (AI native) still returns false.

## S50: fade-in, ghost hooks, dropped orders

* **Fade-in** (`invisibility_note`, `is_invisible`): the 0 -> positive step of the counter is recorded with its fade (the
  largest of: Permanent Invisibility `Dur` 2.0, Ghost `Dur`, Wind Walk `DataA` "Transition Time" 0.6, Sorceress
  Invisibility `DataA`, Shadow Meld `DataA` "Fade Duration" 1.5); the flag counts `fade` seconds later (CONFIRMED
  structure: `0x6F28D7D0` sets the flag at once only for fade 0, otherwise through a timer, message 852436). Hooks:
  `sync_neutral_spells` (abilities), `apply_invisibility_buff`, the potion path, Wind Walk. A 0.1 s scan runs while a record
  or a Shadow Meld unit exists (it stops by itself). A source added without a hook is invisible at once.
* **Ghost / Aeth**: `Agho` (`unit_abilities_a::is_ghost`) and `Apiv` (`m_perm_invisible`) each add one. `Aeth` is the
  *visible* ghost form (editor `ETH1/2` "Immune to Morph Effects", "Does Not Block Buildings"): it adds no invisibility and
  sees nothing; the S46 note "lets the unit see ghosts" was wrong. Ghost `DataA` is "Auto-Acquire Attack Targets": with
  zero a ghost does not acquire targets while invisible (`invisibility_allows_acquire`, checked in `scan_target`).
* **Shadow Meld** (`m_shadow_meld_active`): idle (no orders) at night for `DataC` 0.5 s, then the fade 1.5 s; `DataB`
  "Day/Night Duration" 2.5 s is not used (UNSURE).
* **True Sight** (`m_true_sight_reveals`): the data row carries the radius in `Rng` (900); the query read `Area` (0) and
  never revealed. Fixed. Detectors now honour `DataA` ("Detection Type", 3 = invisible + burrowed): a detector whose type
  has no invisible bit (2) reveals nothing.
* **Ultravision** (`Ault`): the night sight equals the day sight while the unit has it (UNSURE, no data row).
* **Orders**: `dispatch_order` drops an order that targets an enemy unit the issuer's player cannot see (invisible, not
  detected, not shared vision): the natives still answer true. The cast core refuses a unit target the caster's player
  cannot see (`spell_target_unseen`).

Tests: `crates/sim/tests/invisibility.rs` (fade of Permanent Invisibility, Shadow Meld with the action duration and the
fade, Ghost vs Aeth, detection type, dropped attack order, Ultravision).
