# Lane E natives (selection, players, strings, timed turn, presentation)

Readings of the 1.26a natives, from the decompile (IDA) and disassembly. CONFIRMED = read directly, LIKELY = inference,
UNSURE = guessed. Code: `crates/sim/src/natives/lane_e.rs`, `group.rs` (GroupEnumUnitsSelected), `commands.rs`
(selection shared with the replay commands).

## Selection
- `SelectUnit 0x6F3C7910`, `ClearSelection 0x6F3BBAA0` act on the LOCAL player (world+0x28), CONFIRMED. Add is refused when
  the selection already has 12 units (CONFIRMED `< 0xC`); duplicates are not added; remove drops the unit. The original also
  requires bit 2 of the unit flag word (+0x20) to add (meaning UNSURE, not modelled). Both end in a "realize" that updates
  the mirror copies (no simulation state).
- `SyncSelections 0x6F3BB740`: realizes slots 0..11, then `0x6F44B2F0(0.0, 0)`, the same call `TriggerSleepAction` ends in.
  So it is `TriggerSleepAction(0.0)` (minimum timer 1e-4 s). CONFIRMED.
- `GroupEnumUnitsSelected 0x6F3CE590`: group and player must resolve; the group is cleared; each selected live unit
  (selection order, LIKELY append order) is the filter unit; null filter or a true result adds it. No limit, no type or
  owner test.
- Replay: select commands edit the same per-player list (`Game::lane.selections`); `CommandState` copies it in and out per
  command. `RemovePlayer` clears the player's selection (the async clear at the start of `0x6F41F220`).

## Players
- `SetPlayerUnitsOwner 0x6F3C12F0`: the player resolves, then the callee `0x6F40B1D0` is a bare `retn 4`. No effect.
  CONFIRMED.
- `CripplePlayer 0x6F3C1310`: stores the force's player mask (16 bits) at player+0x328 when the flag is true, 0 when false,
  then notifies observers (message 0x2B7A9E75, UI). Nothing in the simulation was found reading it (not searched
  exhaustively, UNSURE).
- `RemovePlayer 0x6F3C9DC0` -> `0x6F41F220(result)`: clear the selection; if the slot state (+0x270) is 1 and the game
  result word (+0x40) is NEUTRAL (3): store the result, and a non-victory result makes the slot state 2. The victory/defeat
  messages (0x8000D / 0x8000E) are posted on a branch whose guard (+0x268) is not identified. The non-playing branch
  (words +0xF0 and +0xE0) is not modelled. LIKELY.
- `CachePlayerHeroData 0x6F3C9DE0` -> `0x6F41B8E0`: heroes are the player's units whose type id first byte is 'A'..'Z', not
  removed, not illusions; sorted descending by `(level*100000 + xp)*10` plus three terms from `0x6F2777B0/7850/78B0` (not
  decoded, taken as 0), ties by lower handle id; the cache keeps (type id, level, xp). LIKELY.
- `GetPlayerScore 0x6F3C9B70`: 25 cases; an unknown index or a null player gives 0. Mapping (CONFIRMED): 0 trained = sum of
  record +0x1C over unit-class records; 1 killed = sum of +0x2C over unit records; 2 structures built = +0x1C over structure
  records whose two type properties (`0x6F29B800 + 0x6F29B830`) are not both zero; 3 razed = +0x2C over structure records; 4
  tech percent (100 when the race has no tech types); 5..9 player words +0x248..+0x258; 10 = 11 + 12; 11 = word+0x130 / 10;
  12 = +0x150 / 10; 13 = +0x170 / 10; 14 = +0x1B0; 15 = +0x190; 16 = +0x140 / 10; 17 = +0x160 / 10; 18 = +0x180 / 10;
  19 = +0x1C0; 20 = +0x1A0; 21 = sum over unit and structure records of weight * (+0x1C + +0x2C); 22 = 10 * sum of hero
  xp; 23 = 11 + 16; 24 = 22 + 23 + 21. The writers of the economy words and of the +0x2C counts are not in the model: a
  host feeds them with `LaneE::add_stat` and `type_kills`; type weights default to 1 (UNSURE).

## Timed turn
`SetUnitFacingTimed 0x6F3C5B30`: if NOT (0.1 < duration) the native is `SetUnitFacing`. Otherwise target = facing * pi/180,
delta = wrapped shortest difference (`0x6F496630`, exact wrap UNSURE), steps = duration / 0.03, rate = |delta / steps|; if
steps or rate are below 2e-7 nothing happens; else the mover gets the target (`0x6F475230`) and the turn cap (mover+0xFC =
rate, `0x6F48E5E0`). Model: each movement tick (0.03 s) the facing moves toward the target by at most the rate (UNSURE: the
mover's own turn stepping `0x6F494200` was not read). Needs the soft-float division tables.

## Strings and names
- String fetch helper `0x6F3BAA40`: null or empty gives null; text starting with `TRIGSTR_` gives atoi of the rest, looked up
  in the map string table (`""` when missing); anything else is the text itself. `StringLength` and `StringCase` use it.
- `GetLocalizedString 0x6F3BACF0`: helper above, then a lookup of the text in the global string table (`0x6F5C8ED0`, hash
  lookup; key case folding UNSURE, ours is ASCII case-insensitive); a miss returns the text. Host table `LaneE::localized`.
- `GetLocalizedHotkey 0x6F3B2D80`: raw string (no TRIGSTR), then `0x6F009F90` -> `0x6F4D1F80`: key text to virtual-key code
  through input configuration tables. NOT implemented (tables unread).
- `GetObjectName 0x6F3BB6B0` = name getter `0x6F32DBE0(code, 0)`; null when absent or empty. Host table
  `LaneE::object_names`.
- `GetAbilityEffectById 0x6F3BCE40`: effect type 1..6 selects targetArt, casterArt, specialArt, areaEffectArt, missileArt,
  lightningEffect (0 and others: effectArt); the index is clamped to the last entry; an empty list gives null.
  `GetAbilitySoundById 0x6F3BCEA0`: sound type 1 (looping) or other, first entry. Host tables `ability_art`,
  `ability_sound`. The real data is the ability art columns; the engine does not load them yet.
- Engine: `war3map.wts` is parsed (`parse_wts`) into `LaneE::map_strings`; the boot report resolves the map name and author.

## Presentation-only
`SetUnitAnimation`, `...WithRarity` (flags 16, or 32 for RARITY_RARE), `...ByIndex`, `SetUnitBlendTime` (animation
component), `SetUnitColor` (stores a colour byte in the unit's UI record, refreshes the model), `UnitAddIndicator` (posts an
indicator message): all need a live unit and are logged as `PresentationEvent::Command`. No simulation state reads them.
`UnitShareVision 0x6F3C81C0`: sets the (unit, player) shared-vision flag through `0x6F296910`, which refreshes the unit's
vision sets. Recorded in `LaneE::shared_vision`; the fog model has no unit vision sources, so it has no effect there yet
(UNSURE).

## Level, save, load, sound wait
- `ChangeLevel 0x6F3BBC30`: no mode gate; hands the raw name and the score-screen flag to the campaign loader `0x6F3A4C70`.
  Recorded as `HostRequest::ChangeLevel`.
- `LoadGame 0x6F3BBE60`, `SaveGame 0x6F3BBEE0`: gate `0x6F3B28C0` = single-player and not a replay (`0x6F53E670`,
  `0x6F53F160`); a replay or network game does nothing. Save also rejects names containing ".." or ":" and allows one save
  at a time (flag at game+0x3E4 until the save finishes). Recorded as host requests.
- `TriggerWaitForSound 0x6F3C3F20`: `0x6F44B2F0(offset, sound)` puts the thread into wait state 1 with the sound as wait
  object; the sound system resumes it. Not implemented (no playback; lengths come from audio files).
