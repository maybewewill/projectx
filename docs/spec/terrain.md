# Terrain queries: cell level, ground height, GetLocationZ (Game.dll 1.26a)

Status: reverse engineering for a clean-room port (interoperability), S13, 2026-10-12. Read with IDA/Hex-Rays and
capstone (nothing from them is committed) and **checked by running the real routines in Unicorn**
(`tools/emu/terrain_emu.py`; vectors `crates/sim/tests/terrain/vectors.rs`; Rust `crates/sim/src/terrain.rs`).
Image base `0x6F000000`. Labels: CONFIRMED = read in code and reproduced bit for bit in Unicorn; CONFIRMED (read) =
read only; LIKELY; UNSURE.

## 0. Result in one paragraph

The original keeps one **28-byte record per `w3e` tile corner** in a terrain object (singleton getter `0x6F01F5A0`,
slot `[0x6FAAE790]`). Everything below reads that array. The **cell level** used by the missile uphill miss
(`0x6F0128C0`) is the low nibble of the "misc" byte (the w3e *layer*, i.e. the cliff level) of the corner nearest to
the pathing cell, raised to a ramp-object level when a rectangle of that list covers the cell. The **ground height**
behind `GetLocationZ` is a piecewise bilinear surface over the four corners of the tile, where a corner's height is
`(h14 - 8192) / 4 + 128 * (layer - 2)` plus a ramp offset, and `GetLocationZ` returns the **larger of the ground and the
water surface** at the point. All three are reproduced bit for bit (3 queries x about 3,500 synthetic cases on grids with
cliffs, ramps of every variant, water and edges; plus 700 cases on the real `(2)Circumvention` terrain, 0 mismatches).

## 1. The corner records and their loader (CONFIRMED read, loader `0x6F771020` -> `0x6F756CA0`)

`0x6F771020` checks the magic `W3E!` (`0x6F981760`) and the version (3 to 11), then `0x6F756CA0(stream, version)`
builds the array. For version 11 (the 1.26a files) each point is read by `0x6F740610` after the defaults of `0x6F73B680`:

| Record offset | Content | Source |
|---|---|---|
| `+0x00` | bits 0-13 ground height `h14`; bits 14-22 y position index; bits 23-31 x position index | file u16 `& 0x3FFF`; index = `trunc((f32(i*128 + center) + 32768) / 128)`, 9 bits |
| `+0x04` | bits 0-17 point index; bits 18-31 water level (14 bit) | file u16 `<< 18` |
| `+0x08` | low nibble ground texture (high nibble 0xE default) | file byte |
| `+0x09` | variation byte | |
| `+0x0A` | **misc byte**: low nibble = **layer = cliff level**, high nibble = cliff texture | file byte |
| `+0x0C` (u16) | flags: `0x02` ramp, `0x04` blight, `0x10` water, `0x40` boundary, `0x80` map edge | file flag byte `0x10/0x20/0x40/0x80`, water word bit 14 |
| `+0x10..0x18` | normal (0, 0, 1) | default |

The terrain object keeps: `+0xB4` = width-1, `+0xB8` = height-1 (in tiles), **`+0xBC` = 4 * (width-1), `+0xC0` = 4 *
(height-1)** (the extent in 32-unit pathing cells, set by `0x6F74D730`), `+0xE0` count, `+0xE4` the array, and the map
extent floats `+0xC4` min y, `+0xC8` min x, `+0xCC` max y, `+0xD0` max x taken from the first and the last record's
position bits (`0x6F73F430`, `128 * index - 32768`). The record index is **row-major**, x fastest. CONFIRMED (read).

**Cell origin (CONFIRMED read, corrects missiles.md 9.5 and the engine's earlier use of the playable minimum).**
`world+0x6C` (x) and `+0x70` (y), the origin of the cell conversion used by the miss gate, are written by the wpm reader
`0x6F46B850` from the rect `0x6F00F560(edx = 0)` = `0x6F73F500` = **the whole-map rectangle** (what `GetWorldBounds`
returns): the cell origin is the map's minimum corner (`w3e` center), not the playable minimum. The combat code now takes
`Game::world_bounds()` first.

## 2. Cell level `0x6F0128C0(ecx = cell x, edx = cell y, stack = out flag*)` (CONFIRMED, Unicorn)

1. `0x6F742820(x, y)`: if `x >= terrain+0xBC` or `y >= terrain+0xC0` (**unsigned**, so negative cells too) the base is
   **-1**. Otherwise the tile is `0x6F73AFA0`: `t = trunc((c + 2) / 4)` per axis (a cell belongs to the nearest corner;
   cells 0-1 -> corner 0, 2-5 -> corner 1, ...), the record is `array[28 * (width * ty + tx)]` and the base is
   `byte[+0x0A] & 0xF`.
2. `0x6F0118C0` on the ramp-object list (singleton `0x6F01F760`: `+4` count, `+8` array of 0x2C-byte entries): for every
   entry with `x0 <= x <= x1 and y0 <= y <= y1` (rect words at entry `+0x10 x0`, `+0x0C y0`, `+0x18 x1`, `+0x14 y1`) the
   value `ints[(x1-x0+1)*(y-y0) + (x-x0)]` (array pointer at entry `+0x24`) joins a running **signed max** (start -1); the
   found flag is set and written to the out pointer.
3. The result is `ramp_value` when found and `ramp_value > base` (signed), otherwise `base`.

The entries are created per object by `0x6F0152E0` (called from `0x6F019B00` <- `0x6F269400`): a footprint rect around a
unit-like object, with the level at the rect centre (`0x6F742820`) and an extra term. **Which objects do this is not
decoded**; the Rust model has the list (`RampRect`, `TerrainGrid::add_ramp_rect`) but the engine adds none (UNSURE: bridges
and similar walkable models; ordinary maps without them behave as modelled). Unicorn: 700 real-map + about 3,500 synthetic cases incl. 200+ ramp
hits, 0 mismatches.

## 3. Ground height (CONFIRMED, Unicorn)

`GetLocationZ` (`0x6F3C0E50`) -> `0x6F24F3E0(x, y, 1)` with `ecx = -1, edx = 0` -> `0x6F0126F0` -> terrain
`0x6F763F00(&{x,y,0}, -1)` -> `0x6F763D50(…, mode 7)`:

* Tile: `a = trunc((x - ox) / 128)`, `b = trunc((y - oy) / 128)` with `(ox, oy)` the first corner's position bits
  (float conversion `0x6F7E11E0`, the **truncating** branch, flag `0x6FAE759C` = 1 at run time). If `a < width-1 and b <
  height-1` (unsigned) it is used; otherwise an index equal to `width-1` / `height-1` is moved one back, and anything
  still outside gives **0.0**. A point just below the origin (-128 < x - ox < 0) truncates to tile 0 and then fails the
  local-coordinate range check, so it also gives 0.0.
* Corner height (`0x6F7523C0`, mode 7 = layer | ramp | deformation), all in f64 with an `f32` store after each step
  (the FPU is at 53-bit precision, `_control87(0x9001F)`):
  `z0 = f32(float(0x45800000 | h14 << 9) - 6144.0)` (= `h14 / 4 - 2048`);
  `z1 = f32(layer * 128.0 + (-256.0) + z0)` (`0x6FAA1760` = 128.0, `0x6FADB3E4` = -256.0 at run time);
  with the ramp flag: `z2 = f32(ramp_offset + z1)`. The deformation term (`terrain+0x790`, craters and ripples) is
  0 for a list that is empty or absent (a fresh game); **not modelled** (UNSURE).
* Ramp offset (`0x6F750AB0(tx, ty, 0)`): the corner kind `0x6F74BAE0` is a letter: no ramp flag -> `A`/`B`
  (cliff-top test false/true); ramp flag and cliff top -> `H` (offset 0); otherwise the class of `0x6F74B920` (0 -> `L`,
  1 -> `R`, 2 -> `S`, else `!`). Offsets: `R` = 64.0, `S` = 32.0 / 96.0 / 64.0 by the variant of `0x6F750860`
  (0 / 1 / 2, else 0.0), every other kind 0.0. The cliff-top test `0x6F747110`, the class `0x6F74B920` and the variant
  `0x6F750860` are integer/flag logic over the 3x3 neighbourhood (`0x6F743C60`) and are ported in full
  (`TerrainGrid::cliff_top`, `ramp_class`, `ramp_variant`); the cliff-top test starts its minimum from the constant at
  `terrain+0x988` = `trunc((MaxHeight + 256) / 128)` = 14 for Misc `[Terrain] MaxHeight = 1536` (`UI\MiscData.txt`; LIKELY that this is the file the profile read uses). The
  original caches the cliff-top result in the flag bits `0x400`/`0x800`; the result is a function of the grid only.
  Unicorn reaches every branch (`R`, `H`, `S0`, `S1`, `S2` counted by hooks; mutation of each offset fails the Rust test).
* Interpolation `0x6F741250(local dx, dy, corners c0..c3)` with the corners (x, y), (x+1, y), (x, y+1), (x+1, y+1)
  (`0x6F9815E0/F0` = 0,1,0,1 / 0,0,1,1): `vx = f32(dx / 128)`, `vy = f32(dy / 128)`; if `vx` or `vy` is outside
  `[-0.001, 1.001]` (constants `0x6F935D98`, `0x6F981A8C`; a NaN passes) the answer is **0.0**. Otherwise with `m` the
  minimum of the four (`m = c0; if m > c1 {m = c1}; if c2 < m ...`): `acc = f32(((c0-m)*(1-vy))*(1-vx) + m)`, then
  `acc = f32(acc + ((c1-m)*(1-vy))*vx)`, `acc = f32(acc + ((c2-m)*vy)*(1-vx))`, `acc = f32(((c3-m)*vy)*vx + acc)`, and
  the stored `f32(acc + 0.0)`. (It is **not** a triangle split: a plain bilinear blend written relative to the minimum.)

**Water.** `0x6F24F3E0` then asks `0x6F00F5D0 -> 0x6F7527F0` for a water surface: the point must lie strictly inside the
map extent (`0x6F73AC90`); the corner is the nearest one (`0x6F747450`: `a = f32((x - minx)/128 + 0.5)`, add 65536.0 in
f32, take bits >> 7 & 0x7FFF); the corner must have the water flag (`0x10`) and a water table must exist
(`terrain+0x30C`); and `0 < W - G'` with `W = f32(f32(float(0x45800000 | water14 << 9) - 6144.0) + off)` and `G'` the
corner's mode-7 height (`0x6F7526B0`). The result is `max` of ground and `W` (`W < G` keeps the ground, equal takes the
water). `off` is the float at `[water table + 0x108]`, the entry copied from the tileset's `TerrainArt\Water.slk` row
by `0x6F76A920` (the `height` column, field `+0x1C` of the descriptor). LIKELY (not decoded): `off = height * 128` (the
editor's well-known -89.6 for Lordaeron Summer is `-0.7 * 128`), the entry is the tileset's shallow-water row
`<tileset>Sha`; UNSURE which row is entry 0. The engine passes `Some(height * 128)` for that row; without Water.slk
the water is off.

Also decoded: the missile launch start z is `0x6F0126F0(-1, null, x, y, 0)` (ground, no ramp objects) and the spell
missile launcher `0x6F0ABDD0` uses `0x6F24F3E0(x, y, 1)` (the same function as `GetLocationZ`).

## 4. Rust model and wiring

* `sim::terrain::TerrainGrid` (corner grid, `cell_level`, `ground_height`, `location_z`), `Corner`, `RampRect`.
* `Game::set_terrain(Some(Arc<TerrainGrid>))` stores it and installs it as the missile miss gate's `CliffLevels`
  (`terrain::SharedTerrain`); `GetLocationZ` answers `location_z` (0.0 without a grid).
* `engine::terrain::{grid_from_w3e, water_offset, install}`; `Engine::boot` installs it when `war3map.w3e` parses.
* The miss gate's cell origin is `world_bounds` (map corner) before the playable minimum.
* Tests: `crates/sim/tests/terrain.rs` (+ `terrain/vectors.rs`, about 3,500 cases), `crates/engine/tests/terrain.rs` (700 real
  cases with `TERRAIN_W3E=<extracted war3map.w3e>`), `crates/sim/tests/combat_natives.rs::uphill_miss_from_a_terrain_grid`.

## 5. Open

1. Ramp-object list producers (`0x6F269400` callers) and the value written into the cells (`0x6F011800`): not decoded;
   the list is empty in the model. `GetLocationZ` with a non-empty list also runs `0x6F011780 -> 0x6F0116E0` (a ray test
   through the objects), not modelled.
2. Terrain deformation (`terrain+0x790`) in the height and the water row/offset of the tileset (section 3).
3. The nearest-corner position bits wrap at 9 bits (maps with a corner index above 511, impossible for the shipped
   32768-unit coordinate range).
4. (Closed in section 6: the unit z is wired.) The height function is
   there for it.

## 6. Unit height and the fly height (S18, 2026-10-12)

### 6.1 The unit's z (vtable slot `0xE4` = `0x6F281F50`) - CONFIRMED, 1,500 Unicorn vectors

`CUnit` vtable base `0x6F931934` (slot 72 = `0x6F2A40D0`). Slot `0xE4` takes four stack arguments `(point*, level, out_flag*,
unused)` and returns the height in st0 (the callers store it as a float). For an ordinary unit (flag word `+0x280` bit 3 clear,
not a corpse) the height is

    f32( fly + ground' )        (53-bit x87 add, stored as a float; not the soft float)

* `ground` = `0x6F0126F0(ecx = level, edx = out_flag*, x, y, 1)` = the terrain height of section 3 (ramp objects: none in the
  model); the launch asks with `level = -1`.
* `fly` = the unit's vtable slot `0xE8` (`0x6F29E850`): the **fly-height property** (agile value at `unit+0x214`) when the
  movement word `+0x1FC` is exactly 2 (fly) or the flag `+0x20 & 0x800` is set; the type field read by `0x6F29C710` (LIKELY the
  type's `moveHeight`) when the movement word is exactly 8 (hover); 0 otherwise.
* `ground'` = `water` when the movement word has a bit of `0x38` (hover, float, amph), a water surface exists at the point
  (`0x6F00F5D0` = `terrain::water_at`), the unit is not (movement exactly `0x20` without the swim flag `+0x280 & 0x20`) and
  `ground < water` (ordered); else `ground`.
* `*out_flag = (+0x280 >> 1) & 1` when the pointer is given.
* Not modelled (unreachable for a live shooter or target): the corpse branch (`+0x5C & 0x20000000` with `+0x200 <= 0`: a sinking
  interpolation between the ground at level `-3` and the ground, scaled by `fly / unit+0x210`), and the carried-unit branch
  (`+0x280 & 8`, cached point `+0x284..0x28C`).

Unicorn: `tools/emu/unit_z_emu.py` runs the real routine with `0x6F26DE50` (returns 0), `0x6F0126F0`, `0x6F00F5D0` and the
fly slot supplied; 1,500 random cases over ground, water present or absent, fly height, 12 movement words and the swim flag,
0 mismatches against `unitz::unit_height` (`tests/unit_z.rs`). The terrain pieces were checked in section 3. `Game::unit_z`
wires it (ground and water from `TerrainGrid`, fly height from `current_fly_height`); the missile launch reads it for the
shooter (launch z) and the target (aimed z).

### 6.2 Where z matters

* **Range checks are 2-D** (CONFIRMED by the decompile of `0x6F4766E0`): the swing predicate uses the squared distance
  `0x6F6EF3B0` of the two cell positions (x and y of `0x6F38B0B0`) and the collision radii (`+0x90`); no height enters.
* The missile **flight time is 2-D** (`0x6F496D10` uses `dx^2 + dy^2` only); the launch height, the impact height and the
  fly heights change only the z of the launch and aimed points (the arc terms, presentation).
* `GetUnitFlyHeight` (`0x6F3C5DA0`) returns the target field `unit+0x208`, not the climbing property; `GetUnitDefaultFlyHeight`
  the type's value.

### 6.3 `SetUnitFlyHeight(unit, height, rate)` (`0x6F3C5C60` -> `0x6F2835E0` -> `0x6F2783F0`) - LIKELY (read, not run)

* The height is `0.01` (`0x6FAAE480`) unless `0 < height` (strict). `0x6F2835E0(unit, &h, &rate, 1)`: the plain property object
  at `unit+0x204` (value at `+0x208`) takes `h` (so `GetUnitFlyHeight` answers at once); the running maximum `unit+0x210`
  rises to `h` if lower (consumer not found). Then, when `unit+0x200 == 0`, `0x6F2783F0` moves the climbing property:
  the target is `unit+0x208`; the property's modifier handle (`unit+0x224`) is cancelled; if the target equals the value
  now nothing else happens; otherwise (a type-dependent presentation offset update for a type with a positive
  `0x6F32D090` is skipped) the property limits become the two values (lower first), the rate is negated when
  descending (`value >= target`), and a zero rate sets the value at once (`0x6F477350`); any other rate is added as
  a modifier (`0x6F4774E0`) of slope `rate` lasting `|target - value| / rate + 1.0` s. The property is the agile one
  (section `agile.md` 2): `clamp(base + slope * w)`.
* Model: `Game::set_fly_height` / `fly_property` (`crates/sim/src/unitz.rs`, the unit fields `fly_from`, `fly_t0`, `fly_slope`,
  `fly_lo`, `fly_hi`, `fly_for`), evaluated against the game time (the agile clock); test `tests/unit_fly_height.rs`. UNSURE:
  which of `0x6F477460` / `0x6F477400` is the lower limit (the model assumes the order of the descending and ascending
  branches), and that the horizon is measured in the same clock.

### 6.4 Open

1. The corpse and carried-unit branches of the height; the flag `+0x20 & 0x800` (the Crow Form style flying state).
2. `0x6F29C710` (hover height) and the unit-side launch fields (`missiles.md` 11.5).
3. A run of `0x6F2783F0` with real agile objects (the harness `agile_engine_emu.py` has the property objects) to confirm the
   limit order and the horizon.
