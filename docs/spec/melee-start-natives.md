# Melee start and creep camp natives (lane D)

Image addresses of the 1.26a `Game.dll`. Plain-words findings from the disassembly (capstone) and the user's own
`Storm.dll`; no decompiled text is copied. Labels: CONFIRMED (read in the disassembly or run in Unicorn), LIKELY,
UNSURE. Code: `crates/sim/src/natives/melee_start.rs`; tests: `crates/sim/tests/melee_start.rs`,
`tools/emu/unit_order_growth.py`.

## 1. CreateBlightedGoldmine (0x6F3CD630)

Blizzard.j uses it in `BlightGoldMineForPlayerBJ`: the script reads the old mine's position and gold, calls
`RemoveUnit` on it, calls this native, then `SetResourceAmount(newMine, gold)`.

- CONFIRMED order. Player check (`0x6F3BD4D0`); null result when the handle is not a player. The facing is converted
  once (degrees times pi/180, the constant at `0x6FAAE5E8`). The unit builder `0x6F24F1F0` (same builder `CreateUnit`
  uses; it allocates no handle) is called with the immediate raw codes:
  1. `'ngol'` (0x6E676F6C, the neutral gold mine) for **player slot 15** (`ecx = 0xF`). No handle id is allocated for it.
     Its vtable slot 0x84 is then called with no arguments; that is the slot `ShowUnit(false)` calls (0x6F3CE730 calls
     slot 0x84 for false, 0x80 for true), so the neutral mine is **hidden**.
  2. `'ugol'` (0x75676F6C, the haunted mine) for the argument player (slot read from player+0x30), same position and
     facing. A failed build returns null and leaves the hidden mine in the world.
  3. The `Abgm` ability (`0x6F197400` returns 0x4162676D) of the new mine is looked up; when present, the link routine
     `0x6F1ADF30` runs on it with the hidden mine: it finds the hidden mine's `Agld` ability (0x41676C64), copies its
     amount (`+0x6C`) into the haunted mine's ability (`+0x6C`), sends the ability-added message 0xD01A0 and stores a
     reference to the hidden mine at `+0x84` of the ability.
  4. The handle id of the haunted mine is allocated **last** (`0x6F430C80`).
- CONFIRMED: neither the native nor the link routine calls the blight writer `0x6F193470`. The native paints no
  blight. Blight appears through other callers of the writer (`0x6F0EF3E3`, a blight-spot object at `0x6F1939E0` with a
  radius that grows); which of them the haunted mine's construction triggers is UNSURE and not modelled. There is no
  blight radius, owner or facing parameter in the native: the owner is the argument player, the facing is the argument.
- Model: `unit::build` (the shared builder, optional handle). The hidden mine is a unit of player 15 with `hidden`
  set and no handle. The new mine is registered as a resource node holding the hidden mine's amount if the host
  registered one, else 0 (the `Agld` default amount is ability data the model does not load; the melee script calls
  `SetResourceAmount` right after). Support: LIKELY.

## 2. GetWorldBounds (0x6F3D0D60)

- CONFIRMED: the rect is a 16-byte copy of `0x6F73F500` (called on the global map object `[0x6FAAE790]`) stored at
  rect +0x24 (min y), +0x28 (min x), +0x2C (max y), +0x30 (max x) (the offsets the `GetRectMin/Max` natives read);
  the handle is allocated last. No clamp is applied (the rect is not built through `Rect`).
- CONFIRMED: `0x6F73F500` takes the first and the last 28-byte record of an array at map object +0xE4 (count at +0xE0).
  In each first dword, bits 14 to 22 and bits 23 to 31 are two 9-bit tile indices; each index `i` becomes the float
  `128 * i - 32768` (`0x6F739EB0`: the float `65536 + 128 * i` built from the index bits, plus the constant -98304.0 at
  `0x6FADB404`, runtime value checked). So the four numbers are tile-aligned map corners: the first record gives the
  minimum corner, the last record the maximum corner.
- The playable-area getter `0x6F73F5A0` builds the playable rect from this same rect by moving each edge by 128 times
  four integers at map object +0x80C, +0x810, +0x814, +0x818 (LIKELY the four w3i camera complements). This
  agrees with the engine's confirmed Circumvention playable area, which needs the world bounds to be the whole map:
  corner (-6144, -6144) to (6144, 6144) (w3e center -6144, 97 x 97 corner points = 96 tiles of 128).
- Model: `Game::set_world_bounds`; the engine sets it to the map corner and corner + (tiles * 128), the tiles being
  playable size plus complements (`engine::map::map_tiles`). Checked on the Circumvention numbers in the test. The
  writer of the array (the terrain loader) was not traced, so the edge convention (corner point, not tile origin) rests
  on the playable-area agreement: LIKELY. Without a host input the rect is all zeros (UNSURE).

## 3. ChooseRandomCreep (0x6F3B3660) / ChooseRandomNPBuilding (0x6F3B3680)

Filters and draw were CONFIRMED earlier (natives-pending.md section 10); this pass closes the order and the tileset
test.

- CONFIRMED (Unicorn, `tools/emu/unit_order_growth.py`, 300 + 450 + 450 records, bucket mask growth 3 to 127, the
  real bucket initialiser `0x6F2A0BF0`, the real insert `0x6F2A6190`, the real growth `0x6F2A0C50` with a realloc
  stand-in for the Storm wrapper `0x6F6EB5CA`): the walk visits the records in **insertion order**, across growths.
  The earlier probe could not run growth because that realloc wrapper was not stubbed.
- CONFIRMED: insertion order = row order of `UnitUI.slk` (builder loop `0x6F2A7C30` counts the rows from 0 and calls the
  builder `0x6F2A6750` for each valid one). The SLK reader of `gamedata` keeps file order, so
  `gamedata::unittype::load_all_ordered` is the order; the previous `BTreeMap` order (sorted by code) would be wrong.
  Custom map units (w3u) would be appended by their own loader: not modelled (UNSURE).
- CONFIRMED against the user's `Storm.dll` (ordinal 571 at 0x15033C00): `strchr` (first match of the byte in the
  string, stops at the terminator). The tileset test is therefore "the tilesets text contains `*`, or contains the map
  tileset character"; a character of 0 skips the test. Ordinal 508 is a bounded string compare through a CRT import
  (case rule UNSURE, only the race text uses it).
- The map tileset character is the w3e main tileset byte (the original reads a byte at +0x1D0 of the map object): LIKELY.
- Model: `UnitTypes::set_pick_record` (insertion order), `Game::set_map_tileset`; draw = one step of stream 32
  (creep) / 34 (building), `high32(r * n)`, no draw for an empty list. Support: LIKELY.

## 4. Sleep natives (0x6F3C8450 .. 0x6F3C8590)

All resolve the unit through `0x6F3BDCB0` and look an ability up by code (`0x6F0787D0`).

- CONFIRMED codes: non-permanent sleep ability `ACsp` (0x41437370, `0x6F1D6180`), permanent `Asla` (0x41736C61,
  `0x6F1D6100`), alarm `Arla` (0x41616C72, `0x6F0226A0`).
- `UnitCanSleep` / `UnitCanSleepPerm`: the unit has the ability. CONFIRMED.
- `UnitAddSleep(u, add)` / `UnitAddSleepPerm`: add true and ability absent: create the ability through the factory
  (`0x6F1FC130` for `ACsp`, `0x6F1FBE90` for `Asla`) and attach (`0x6F079990`); add false and present: remove it
  (`0x6F079CC0`); otherwise nothing. CONFIRMED. The factories have two gates the model does not evaluate (UNSURE): a
  unit flag test (`0x6F26DE50`: unit flags +0x5C bit 16, with bit 7 and bit 27 when the argument is 1) and a data table
  test on the ability definition (+0x1F8 through `0x6F0098F0`: non-zero required for `ACsp`, zero for `Asla`).
- `UnitIsSleeping`: the `Asla` ability exists and bit 7 of its flag word (+0x20) is set. CONFIRMED. Bit 7 is the
  generic "listener registered" flag (`FLAG_LISTENER`). Its initial value after the factory is not known: the model
  starts a permanent sleeper asleep (UNSURE).
- `UnitWakeUp`: when `Asla` exists, calls its vtable slot 0xC0 (offset 0x300) with (1, 0). The slot body was not read;
  the model clears bit 7 (UNSURE).
- `UnitIgnoreAlarm(u, flag)`: when the unit has `Arla`, sets (true) or clears (false) bit 7 of its flag word and
  returns true; otherwise returns false. `UnitIgnoreAlarmToggled`: bit 7 of `Arla`, false without the ability.
  CONFIRMED, plain flag writes. (Blizzard.j does not use the sleep natives except through the BJ wrappers.)

## 5. Guard positions (0x6F3CB800, 0x6F3CB820, 0x6F3CB840)

Used by Blizzard.j in `MeleeStartingAI`: `RecycleGuardPosition(bj_ghoul[index])` for computer undead players.

- CONFIRMED structure: `RemoveGuardPosition` (unit) calls `0x6F2DCF00`: it takes the owner's AI object
  (`[player table]+0x2D4`, a smart pointer) and the unit's guard link (unit +0x304); when both exist it unlinks the
  record from the AI's lists (`0x6F2D9890`, `0x6F2EC850`, messages through `0x6F2A4B80`). `RemoveAllGuardPositions`
  (player) calls `0x6F2DCEC0`: the same AI object, then `0x6F2D97B0` clears the whole list. `RecycleGuardPosition` is
  remove, then a flag pass over the unit's abilities (`0x6F28E2C0`, argument 0) and the re-registration `0x6F2E52F0`
  (sends message 0xD01A0 to the AI objects of players 0 to 11).
- These records live in the creep AI object graph (linked lists with reference counts and message dispatch), not in a
  field the model has. The model does not run a creep AI and never creates a guard record, so the three natives are
  registered with their handle gate only and change nothing. That is exact while no record exists; it is UNSURE for the
  re-registration message of `RecycleGuardPosition` (the AI host does not receive it). Support: UNSURE.

## 6. Tests and tools

- `crates/sim/tests/melee_start.rs`: mine pair, handle order, world bounds on the Circumvention numbers, pick filters,
  stream/draw order, sleep and alarm state. `crates/gamedata/tests/unittype.rs`: `load_all_ordered` keeps row order.
- `tools/emu/unit_order_growth.py`: the order check in Unicorn (needs `GAME_DLL`).
