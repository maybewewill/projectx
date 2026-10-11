# Real map boot (lane I)

`w3boot --dll game.dll --mpq war3 --mpq War3x --mpq War3xLocal --mpq War3Patch --steps 600 <map>` over every map file
of the user's install (`C:\Program Files (x86)\Warcraft III\Maps`, all subfolders) and the two melee maps that only
exist inside the archives (`(1)TheDeathSheep.w3m`, `(4)WarChasers.w3m`). 600 steps = 12,000 ticks (60 s of game time).
Result of 2026-10-11 after the fixes below: 0 panics; every shipped melee/scenario map boots, runs `config`, starts
`main` and finishes the steps.

## Results

| Group | Maps | Boot ok | Notes |
|---|---|---|---|
| Maps\ (RoC `.w3m`, w3i v18) | 43 | 43 | needed the new format-18 `war3map.w3i` reader (was: "unsupported format version 18" on all of them) |
| Maps\ (user custom: `DotA v6.83sf`, `iCCup DotA 454`) | 2 | 0 | `war3map.j` line 1 is `ICCUP_MAP_SIGNATURE_V1: ...` (not JASS), see open items |
| Maps\FrozenThrone (TFT `.w3x`, v25) | 107 | 107 | |
| Maps\FrozenThrone\Scenario | 9 | 8 | `(6)BomberCommand`: type error `return null` in an `integer` function (open item) |
| Maps\Scenario | 2 | 2 | |
| archives only (`(1)TheDeathSheep`, `(4)WarChasers`) | 2 | 2 | extracted to a scratch dir first |
| Maps\Download (user custom maps) | 15 | 2 | see open items |
| Maps\Download\deprotected | 1 | 0 | w3i version 33 (a newer game format, out of scope for 1.26a) |

All booted maps report `config: ok`, `main: started as thread 2`, and the same unbound natives (the set the lane A-E
work left): `CreateCorpse, GetLocalizedHotkey, GetRescuer, GetSellingUnit, IssueBuildOrderById, SetUnitUseFood,
TriggerWaitForSound, UnitSuspendDecay` (153 of 164 booted maps). The 11 others add some of these (all referenced only by
custom or scenario scripts): `GetUnitLevel, IsUnitAlly, IsUnitEnemy, ReviveHeroLoc, SelectHeroSkill, SetUnitPathing,
SetUnitLookAt, UnitSetConstructionProgress, UnitSetUpgradeProgress, GetHeroProperName, GetOrderTargetDestructable,
GetUnitName, IsUnitSelected`. No new native was needed for the shipped melee maps.

Per-map table for the shipped maps: every row of the groups above with "Boot ok" equal to "Maps" has the same line:
boot ok, 8 unbound natives (the list above). The failing rows are listed individually:

| Map | Boot ok | Reason |
|---|---|---|
| `Maps\DotA v6.83sf.w3x`, `Maps\iCCup DotA 454.w3x`, `Download\` DotA/iCCup/garik (7 files) | no | `war3map.j` begins with the line `ICCUP_MAP_SIGNATURE_V1: <hash>`; our lexer stops at `:`. UNKNOWN how the original handles it (not investigated: custom maps) |
| `FrozenThrone\Scenario\(6)BomberCommand.w3x` | no | `function GetAirportTrainingPlane takes integer ... returns integer / return null`. Our checker rejects it. The original compiler (Unicorn, entry `0x6F458900`, p4/p5 in {0,1}) rejects it as well: it reports an error and emits no output. So either the real load path is more lenient than that entry or the map does not compile in 1.26a. OPEN |
| `Download\101 Rounds`, `Download\Legion TD X1-X4` | no | obfuscated scripts that declare both `O` and `o` (and `S`/`s`, ...) as globals. The original compiler is case-insensitive for identifiers (Unicorn: two globals `O`, `o` give an error), so these maps were not made for 1.26a. Not a loader problem |
| `Download\RATD v2.50.w3x` | no | `duplicate function or native` (23 type errors), same kind of cause (not investigated) |
| `Download\deprotected\iCCup DotA 500_deprotected.w3x` | no | `war3map.w3i` version 33 |

## Fixes made for this table

* `w3map`: `war3map.w3i` format 18 (Reign of Chaos). Layout from the real files: no loading-screen model, no game data
  set, no fog/weather/sound/light/water fields; campaign number + three loading texts, a prologue number + three
  texts, then the same player and force records as version 25. Version 25 and 18 are accepted, others still fail with
  `UnsupportedVersion`.
* `fdf`: a trailing `f`/`F` after a number is accepted (`Height 0.090f,` in `Glue\CampaignMenu.fdf`).
* `gamedata`: `Profile::parse_lenient` (stray lines such as `/ Stuffed Penguin` in `ItemFunc.txt` and `Purchase Medusa
  Pebble` in `ItemStrings.txt` are skipped instead of dropping the file) and case-insensitive key lookups
  (`get_ci`, `get_list_ci`).

## Object text and data wired at boot (`engine::objdata`)

Checked against the original in IDA (`game126a`):

| Table | Source files | Original |
|---|---|---|
| `lane.localized` (`GetLocalizedString`) | the `StringList` blocks of every FDF listed in `UI\FrameDef\FrameDef.toc` (`GlobalStrings.fdf`, `NetworkStrings.fdf`, `DateStrings.fdf`, ...), keys case-insensitive | `0x6F3BACF0`: `TRIGSTR_nnn` substitution, then the string table lookup `0x6F5C9650` -> `0x6F5C8ED0`; a miss returns the input |
| `lane.object_names` (`GetObjectName`) | `Name` of the 45 profile files in the pointer table at `0x6F936B10` (`Units\*Strings.txt`, `*Func.txt`; later file wins per key) | `0x6F3BB6B0` -> `0x6F32DBE0`: index 0 of the record's name list, null when empty |
| `lane.ability_art` (`GetAbilityEffectById`) | keys `effectArt, targetArt, casterArt, specialArt, areaEffectArt, missileArt, lightningEffect` (types 0..6) | `0x6F3BCE40` -> `0x6F3B40D0` switch -> getters `0x6F32E2D0..0x6F32E450`; index clamped to the last entry |
| `lane.ability_sound` (`GetAbilitySoundById`) | keys `effectSound`, `effectSoundLooped` | `0x6F32E4F0`, `0x6F32E510` |
| `shops.item_stock` | `Units\ItemData.slk` columns `stockMax`, `stockRegen` | shop natives read the type record |
| `shops.unit_stock` | `stockMax`, `stockRegen` of the unit type records (`UnitBalance.slk`) | same |
| gold mine start amount | `Units\AbilityData.slk`, `Agld`, level 1 `DataA` (12500 on the real data); `Game::set_ability_resource_amount`, a unit built with the `Agld` ability becomes a resource node with this amount (`CreateBlightedGoldmine` copies the hidden mine's amount to the haunted mine) | UNSURE: the ability class that reads it was not traced |

Real data (Banditridge-style melee map): 1,264 global strings, ~2,000 names, 1,582 art lists, 116 sound lists, 273 item
stock rows. UNSURE / not modelled: the map's own object data (`war3map.w3u/w3a/w3t`, `war3mapSkin.txt`) that overrides
these records; whether `Name` is split on commas (no shipped `Name` contains one); the exact FDF set the original loads
for the string table (we load the whole TOC; several template FDFs with top-level `Texture`/`String` blocks fail in the
fdf parser and are noted in the boot report, they hold no `StringList`).

## Tests

* `crates/engine/tests/objdata.rs`: object text and data from a synthetic archive; a format-18 map boots and gets the tables.
* `crates/w3map/tests/synthetic.rs`: `reign_of_chaos_map_info_version_18`.
* `crates/fdf/tests/parser.rs`: float suffix. `crates/gamedata/tests/profile.rs`: lenient parse and case-insensitive keys.
* `crates/sim/tests/melee_start.rs`: gold mine units start with the `Agld` amount.
