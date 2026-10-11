# Vision: fog-of-war grid, writers, queries (Game.dll 1.26a)

Status: pass 3 (2026-10-11, section 11.9 onwards). Written from disassembly and decompiled structure of the 1.26a
`Game.dll` (SHA1 c5d8624b…). Tags: CONFIRMED = read from the code and/or checked in Unicorn; LIKELY = the reading is
consistent with the code but not run; UNSURE = open. Addresses are image VAs.

Verification (pass 1 and pass 2):

| Item | Address | Cases | Mismatches |
|---|---|---|---|
| state table | `0x6f00e7a0` | 1,000 (250 per flag pair) | 0 |
| span writer, mode 1 / 2 / 4 | `0x6f406920` / `0x6f4069f0` / `0x6f406850` | 1,000 | 0 |
| cell writer | `0x6f3b0e20` | 1,000 | 0 |
| rectangle writer | `0x6f3b0e90` | 1,000 | 0 |
| grid constructor (sizes, words) | `0x6f408d10` | 300 rectangles | 0 |
| grid scale (soft-float quotient) | `0x6f408d10` | 40 vectors checked in Rust | 0 |
| span table | `0x6f408fd0` | 158 radii (3..160) | 0 |
| sight circle writer | `0x6f3ba480` | 300 grids (half wider than 256 cells), 26,544 changed words | 0 |

Harnesses: `tools/emu/vision_emu.py` (pass 1: writers and table) and `tools/emu/vision_grid_emu.py` (pass 2:
constructor, span table, circle; it hooks the Storm heap, the CRT memset and `fs:[0]`, see its header).

## 0. Summary of pass 2 (what changed from pass 1)

- The grid is the playable area, not a fixed 1/128 grid: `width = trunc(range / 128)`, and the scale is
  `width / range` (section 11.1). The "1/128 assumption" of pass 1 was right only for ranges that are multiples of 128.
- The span table of the sight circle is decoded and checked (section 11.2). The circle writer is checked
  end to end (section 11.3).
- The single-cell writer reads its coordinates as bytes: columns and rows wrap modulo 256 (section 11.3).
- The refresh is not on the movement timer: it has its own repeating timer, message 0x80269, period 0.4 s (section 11.4).
  `lockstep.md` section 11.2 says the refresh is called from 0x8026A; that is wrong (see the correction there).
- The visible-layer counter `DAT_6fab6a34` has no writer and is zero: the visible layer is cleared on every refresh
  (section 11.4).
- The unit sight path is only partly decoded. The source applier `0x6f40a740` pushes three arguments into the
  four-argument circle writer (section 11.5), so it cannot be the unit path as written.
- `IsUnitVisible` / `IsUnitFogged` / `IsUnitMasked` run the unit's slot-0xFC method (section 11.7).

## 1. The grid object

- The game global `[0x6FAB65F4]` (the same object that holds the local player id at `+0x28`) holds the fog grid pointer at
  `+0x34`. Every query and writer reads `[[0x6FAB65F4]+0x34]` (CONFIRMED, natives `IsFogEnabled` `0x6f3bb200`,
  `SetFogStateRect` path, `0x6f3c98f0`).
- Grid object fields (CONFIRMED from `0x6f00e7a0`, `0x6f26d0c0`, `0x6f192620`, the writers, the flag setters and the
  constructor `0x6f408d10`):

| Offset | Meaning |
|---|---|
| `+0x10` | `FogMaskEnable` flag (the `a` bit of the state table) |
| `+0x14` | `FogEnable` flag (the `b` bit) |
| `+0x28` | allocation base of the four arrays |
| `+0x2c` | pointer to the **mask** array (ushort per cell, "unexplored" bits, initial `0xFFFF`) |
| `+0x30` | pointer to the **visible** array (ushort per cell, initial 0) |
| `+0x34` | blight sample layer (section 11.1) |
| `+0x38` | elevation layer (section 11.1) |
| `+0x3c` / `+0x3e` / `+0x40` | local player's own mask / shared mask / allied mask (section 11.1) |
| `+0x44` / `+0x48` | y origin / x origin of the cell mapping (world units, the playable minimum) |
| `+0x4c` / `+0x50` | playable maximum y / x (copied from the playable rectangle) |
| `+0x60` / `+0x6c` | width / height in cells |
| `+0x64` | row pitch (`1 << shift`, the smallest power of two at least the width, at least 2) |
| `+0x68` | shift: a cell's index is `(row << shift) + col` |
| `+0x70` / `+0x74` | x scale / y scale (cells per world unit) |

- Index: `(row << shift) + col`, with `col` from x and `row` from y (CONFIRMED, `0x6f26d0c0`).

## 2. Point to cell, and the words

### 2.1 Mapping (`0x6f192620`, CONFIRMED by disassembly)

For an axis with origin `o` and scale `s`, limit `n` (width for x, height for y):

    cell = clamp( f2i( mul( sub(v, o), s ) ), 0, n - 1 )

`sub`, `mul`, `f2i` are the soft-float operations of `crates/softfloat`. The clamp is written with an unsigned compare:
values above `n-1` (including negatives seen as large unsigned) go to the nearest end. Mapping the two corners of a
rectangle uses the same function (`0x6f3b76e0`). The point queries clamp the point into the playable area first
(`0x6f469490`).

### 2.2 Words

- `vis` word = `[+0x30][index]`, read as `| 0xF000` by the query path (CONFIRMED, the high nibble is not a player bit).
- `msk` word = `[+0x2c][index]`, read as `& 0xFFF` (CONFIRMED). The player index is `slot` (the player object's byte at
  `+0x30`) and the bit is `1 << slot`.

## 3. Writers

Three modes. The mode is the fog state constant (CONFIRMED: the JASS constants 1, 2, 4):

| Mode | Value | `msk` bit | `vis` bit | Routine |
|---|---|---|---|---|
| masked | 1 | set | clear | `0x6f406920`, `0x6f3b0e20` (branch 1) |
| fogged | 2 | clear | clear | `0x6f4069f0`, `0x6f3b0e20` (branch 2) |
| visible | 4 | clear | set | `0x6f406850`, `0x6f3b0e20` (branch 4) |

Other modes write nothing (CONFIRMED).

### 3.1 Cell writer `0x6f3b0e20`
Stores one cell with the mode and the player mask. **Both coordinates are read as bytes** (`movzx` of the stack
arguments): the row is `row & 0xFF`, the column is `col & 0xFF`, and the index is `((row & 0xFF) << shift) + (col & 0xFF)`
(CONFIRMED by disassembly; a column of -1 is column 255). No clamp.

### 3.2 Span writer `0x6f406920` / `0x6f4069f0` / `0x6f406850`
One row, columns `[x0, x1]`. Row outside `[0, height)` writes nothing (unsigned compare). The start is clamped to 0,
the end to `width-1`. The row and column are full integers (CONFIRMED by disassembly and by the wide-grid circle runs,
section 11.3). The store pattern is pairwise (one dword per two cells):

1. if `x0` is odd, store `x0` alone;
2. from the next even cell, store pairs up to `x1-1` (or `x1` when `x1` is odd);
3. if `x1` is even, store the cell after the last pair alone.

For `x0 <= x1` this is exactly the inclusive span. For `x0 > x1` steps 1 and 3 still store, so up to two cells
outside the span are written (CONFIRMED, Unicorn: example `[11, 6]` writes 11 and 12).

### 3.3 Rectangle writer `0x6f3b0e90`
Arguments: mode, mask, `x0`, `y0`, `x1`, `y1`. The start is clamped to 0, the end to `width-1` / `height-1`. The loops
run over **`[x0, x1) x [y0, y1)`: the end is exclusive** (CONFIRMED). The indices are full integers (CONFIRMED,
section 11.3).

### 3.4 Rectangles in the callers
- `0x6f3b76e0` converts a rectangle object (min corner at `+0`/`+4`, max at `+8`/`+0xC`, as `y0,x0,y1,x1`) with
  `0x6f192620` and calls the rectangle writer (CONFIRMED).
- Fog-state rectangles (`SetFogStateRect`) and rect modifiers use it, so their cells are `[cell(min), cell(max))`.

## 4. Shapes: the sight circle `0x6f3ba480`

Arguments: the grid (ECX), mode, player mask, a pointer to the centre point, a pointer to the radius (`ret 0x10`,
CONFIRMED). The centre cell comes from the mapping of section 2.1. The radius in cells is
`R = f2i( mul(radius, +0x70) )` (CONFIRMED). The shapes are in section 11.3.

## 5. Queries

| Native | Address | Rule | Status |
|---|---|---|---|
| `IsVisibleToPlayer(x, y, p)` | `0x6f3c98f0` | state `== 4` | CONFIRMED |
| `IsFoggedToPlayer(x, y, p)` | `0x6f3c9980` | state `== 2` | CONFIRMED |
| `IsMaskedToPlayer(x, y, p)` | `0x6f3c9a10` | state `== 1` | CONFIRMED |
| `IsLocation*ToPlayer` | `0x6f3c9940` / `0x6f3c99d0` / `0x6f3c9a60` | same, from the location | CONFIRMED (structure) |

The state is the table of section 5.1 for the player's bit. An invalid player returns false without a read (CONFIRMED).

### 5.1 The state table `0x6f00e7a0` (CONFIRMED, Unicorn 1,000 cases)

Let `a = FogMaskEnable`, `b = FogEnable` (as 0 or 1), `row = a + 2b`:

| `a`, `b` | row | if `vis & bit` | else if `bit & ~msk` | else |
|---|---|---|---|---|
| 0, 0 | 0 | 4 | 4 | 4 |
| 1, 0 | 1 | 4 | 4 | 1 |
| 0, 1 | 2 | 4 | 2 | 2 |
| 1, 1 | 3 | 4 | 2 | 1 |

In words: with both flags on, a visible cell is visible; an explored but not visible cell is fogged; an unexplored one is
masked. With fog off, everything not masked is visible. With the mask off, nothing is masked, and the unseen cells are
fogged. With both off, every cell is visible.

## 6. Who writes the words

### 6.1 Explicit states (`SetFogState*`)
`0x6f3c1a30` (rect), `0x6f3c1ab0` (radius), `0x6f3c1b20` (location radius) (CONFIRMED structure): the player mask comes
from `0x6f408110(slot, useShared)`, which is `1 << slot`, or the player's shared mask word at player `+0x2E0` when
`useShared` is set (CONFIRMED structure). The explicit state is an immediate write (CONFIRMED structure).

### 6.2 Fog modifiers
- Create: `0x6f3d0e70` (rect), `0x6f3d0f90` (radius), `0x6f3d10f0` (location radius). The modifier object has flags at
  `+0x20`: `0x8` rect, `0x10` radius (point), `0x20` location radius; `mode = flags & 7`; the player words at `+0x24`; the
  shape fields at `+0x2C..+0x4C` (CONFIRMED structure, `0x6f3e6f80`). Bits `0x80`/`0x100` come from the create arguments
  (UNSURE which argument is which).
- Start `0x6f3c1bc0`: sets flag `0x40` (CONFIRMED). Stop `0x6f3c1be0`: clears it (CONFIRMED). Destroy `0x6f3c1ba0`:
  calls the object's destroy slot (CONFIRMED).
- Apply `0x6f40a7b0`: writes the modifier's shape with `mode = flags & 7` for each player in its word (CONFIRMED
  structure). When the modifier's `0x80` flag is set, its player word is first expanded to the union of the players'
  masks through the table `DAT_6fab6a14` (CONFIRMED: a loop over the 16 bits, OR of the table entries).

The Rust model writes a started modifier immediately, and re-applies the active modifiers and the unit pass on every
refresh (section 7). The original applies a modifier at the next refresh (section 11.4). The difference is in timing only
(UNSURE for queries between a start and the next refresh).

### 6.3 Refresh `0x6f40a8f0` (CONFIRMED structure; cadence in section 11.4)
Called from the message handler `0x6f4299e0` (section 11.4). The body, in order (section 11.5 has the details):
player masks, clear of the visible layer, the source walker, the active modifiers, the unit pass.

### 6.4 Sight sources (`0x6f40a740`, section 11.5)
The source applier takes a source object: radius `+0x30` (float, applied only when above 0.0, CONFIRMED), mode `+0x28`
(0 gives visible 4, 2 gives masked 1, any other gives fogged 2, CONFIRMED), player `+0x24` (its vision mask is
`DAT_6fab6a14[player]`, CONFIRMED), position through the object's vtable slot `+0x78` (LIKELY the position getter). It
then calls the circle writer with three stack arguments (section 11.5).

### 6.5 Flags `FogEnable` / `FogMaskEnable`
- `FogEnable(v)` `0x6f3bb1e0` → `0x6f407e60`: stores `v` at `+0x14`; when `v` is false it clears the **visible** array
  (CONFIRMED). Wired in the Rust model (`VisionGrid::set_fog_enabled`).
- `FogMaskEnable(v)` `0x6f3bb210` → `0x6f407dd0`: stores `v` at `+0x10`; when `v` is false and the second argument is set,
  it clears the **mask** array (CONFIRMED for the memset condition; the argument mapping of the native is UNSURE). The Rust
  model stores the flag only (UNSURE, the mask clear is not wired).
- `IsFogEnabled` `0x6f3bb200` reads `+0x14`, `IsFogMaskEnabled` `0x6f3bb230` reads `+0x10` (CONFIRMED).
- Map start (`0x6f3b0750`, CONFIRMED structure): from the game settings word at `+0x38` of the game object: bit `0x8`
  set leaves the defaults (both flags on); otherwise bit `0x4` alone gives both flags off, bit `0x1` or `0x2` gives both
  on (bit `0x2` also sets the object's `+0x3C0`). The names of the three settings are not read (UNSURE).

## 7. Unit visibility

- `IsUnitVisible` `0x6f3c7af0`, `IsUnitFogged` `0x6f3c7d00`, `IsUnitMasked` `0x6f3c7d80`: each resolves the unit handle
  and the player, then calls the unit's vtable slot `0xFC` with `(player slot, 0, state mask)` where the state mask is 4, 2
  or 1 (CONFIRMED by disassembly; CUnit vtable slot 63 is `0x6f285080`). Section 11.7 has the method.

## 8. Target acquisition does not read vision (CONFIRMED at the direct-call level)

The attack-cycle functions (`attack-cycle.md`) call no vision routine directly (scan timer `0x6f0cf210`, candidate
callback `0x6f0ccfa0`, group builder `0x6f2a3270`, swing gate `0x6f0cd350`, execute `0x6f0cf660`). So acquisition takes
the first enemy in range order whatever its visibility (LIKELY for the whole cycle).

## 9. Rust model (crates/sim)

- `vision.rs`: `VisionGrid`. Flat storage with pitch and shift as in the original. `from_playable(min_x, min_y, max_x,
  max_y, tables)` is the constructor (sizes and scale from the playable area, section 11.1). `span_table`, `octant_steps`
  (section 11.2), `circle_shape` / `write_circle` (section 11.3, shapes as flat index sets), `rect_shape`,
  `write_indices`, `clear_visible`, `set_fog_enabled`, the state table and the point mapping. Unit tests cover the table,
  the pitch, the byte wrap and the exclusive ends.
- `natives/fog.rs`: modifiers store flat index sets (`FogModifierObj::cells`). `Game::vision_refresh` (section 11.4):
  clear the visible layer, the active modifiers, then the unit sight pass. `Game::unit_fog_test` (section 11.7).
  `Game::is_night` and `Game::unit_sight_sources` (section 11.6).
- `natives/game.rs`: `Payload::Vision` (the 0.4 s timer), `Game::set_playable` builds the grid from the playable area
  when the softfloat tables are set (`Game::set_vision_tables`, called by `ScriptHost::with_game`). `UnitType` and `UnitObj`
  carry `sight`, `night_sight` (and `UnitObj` the `revealed` and `vision_extra` words). `Game::unit_objects` lists units in
  creation order.
- `script.rs`: arms the vision timer once per game (like the movement timer); `Payload::Vision` runs the refresh and does
  not count as a script event.
- `lockstep.rs`: `VISION_PERIOD_BITS` = `0x3ECCCCCE` (0.4 f32).
- `natives/unit_attr.rs`: `IsUnitVisible`, `IsUnitFogged`, `IsUnitMasked` registered (LIKELY).
- Tests: `vision.rs` unit tests (table values from the original, pitch, byte wrap, clear, flags); `tests/vision_grid.rs`
  (constructor vectors from the original, GAME_DLL-gated; sight reveal and clear; the 0.4 s timer; the three natives).

## 10. Open items (after pass 2)

1. The unit sight path (section 11.5): the walker's enumerator (`0x6f3daa60` returns a tag, not a list) and the source
   applier's missing fourth argument mean the walker is not the unit path. The unit pass `0x6f409e00` (per unit, records,
   shape kind at `+0x10C`) is what the unit loop calls; its radius rule is not decoded. The Rust model draws a circle of the
   sight radius per unit (UNSURE stand-in).
2. The unit list `0x6f39b6e0(0)`: which units it holds (all units, or the units of one player).
3. Day and night sight: the time-of-day rule (the 18:00–06:00 model is a stand-in) and whether the original reads
   `+0x190` at all.
4. The helpers `0x6f473170`, `0x6f3df1c0` (player vision masks, shared vision) and `0x6f284e80`, `0x6f26de50` (extra
   masks). The Rust model uses the player's shared-vision alliance row.
5. The unit cell: `0x6f474460` (position to a pathing cell through the map object) then `>> 2`. The Rust query uses the grid
   mapping; the two differ when the playable range is not a multiple of 128.
6. The gate `+0x3e0` of the fog manager and the flag from `0x6f4c34d0(0xD)` in `0x6f285080` (both taken as off/on, UNSURE).
7. The modifier create arguments that set `0x80` and `0x100`.
8. The mask-clear part of `FogMaskEnable` (the native's second argument).
9. Grids of width 0 (a playable area under 128 units): the constructor's behaviour is not run; the Rust model keeps one cell.
10. The layers `+0x34` (blight) and `+0x38` (elevation) are not used by the model (their readers are not read).

## 11. Pass 2 (2026-10-11)

### 11.1 The grid constructor `0x6f408d10` (CONFIRMED structure, CONFIRMED by Unicorn, 300 rectangles)

The constructor belongs to `CFogOfWarMap.cpp` (assert strings, line 795). Steps, in order:

1. The playable rectangle comes from `0x6f00f560` (a copy of four floats: min y, min x, max y, max x). They are stored
   at `+0x44`, `+0x48`, `+0x4c`, `+0x50`.
2. The cell size is 128 world units: the start-up constant `0x6fadb3e8` = 128.0 (CONFIRMED value; `0x6fadb3ec` = 64.0).
3. `width = f2i( div( max_x - min_x, 128.0 ) )` and `height` the same with y (`+0x60`, `+0x6c`). CONFIRMED on 300
   rectangles (`trunc(range/128)`).
4. The row pitch is the smallest power of two at least `width`, at least 2, and the shift its base-2 log (`+0x64`,
   `+0x68`). CONFIRMED structure.
5. The scale is `+0x70 = div( i2r(width), max_x - min_x )` and `+0x74` the same with the height. CONFIRMED bit-exact on 40
   vectors (the Rust test). So the scale is `cells / range`: a cell is `range / cells` units wide (about 128 to 256). The
   earlier 1/128 scale is only right when the range is a multiple of 128.
6. The four arrays are allocated as one block (`pitch * height * 8 + 0xC0` bytes, Storm allocation with the file name
   `CFogOfWarMap.cpp` line 795) and aligned to 32 bytes: `+0x2c` mask (filled with `0xFFFF`), `+0x30` visible (0), `+0x34`
   and `+0x38` the layers of section 11.1.1 (zero, then filled by the two helpers below). CONFIRMED by the Unicorn runs (mask
   and visible words checked on every cell).
7. `0x6f4089c0`, `0x6f4085a0` and `0x6f408cc0` reset small vectors (not geometry).
8. `0x6f408070` sets the local player's masks at `+0x3c` (own: `1 << slot`, or `0x0FFF` for an observer-like player, or
   `0xFFFF` when the grid flag bit 0 is set), `+0x3e` (the player's shared word at `+0x2E0`), `+0x40` (the OR of the
   allied players' shared words). CONFIRMED structure.

#### 11.1.1 The two extra layers (CONFIRMED structure)
- `0x6f4077c0` (`+0x34`): for each fog cell, 16 pathing probes (4 by 4 of the 32-unit pathing cells inside it, through
  `0x6f46c1e0` with the pathing bit `0x20`, blight). Any probe with the bit gives `0x7FFF` in the word. So a fog cell is 4 by 4
  pathing cells: 128 units, which is where the 128 comes from. The layer is not read by the visibility code.
- `0x6f406660` (`+0x38`): for each fog cell, the maximum of the 16 probe values (`0x6f0128c0`) shifted left by 7. Not read by
  the visibility code.

### 11.2 The span table `0x6f408fd0` (CONFIRMED by Unicorn, 158 radii, 0 mismatches)

The table of a radius `r` has `2r + 1` entries of `(lo, hi)` (8 bytes each). Entry `j` is the row `dy = j - r`.
Built from the octant list of `0x6f408800` (the midpoint circle, CONFIRMED by disassembly): start at `(0, r)` with decision
`3 - 2r`, east step `4x + 6`, south-east step `4(x - y) + 10`, and the x of every south-east step is recorded (the first
step is east when `3 - 2r < 0`, which holds for `r >= 2`).

Fill rules, in words (the octant list `o[0..n)`):
- Entry `r` (the centre row) is `(-r, r)`.
- For each octant index `k`: entries `k` and `2r - k` get `(-o[k], o[k])`.
- A running level starts at 1. When `level <= o[k]`, the rows that the level passes are filled: `o[k] - level + 1` row
  pairs, from the centre outwards (entries `r-1, r-2, ...` and `r+1, r+2, ...`), each with `(-(r-k), r-k)`. The level then
  becomes `o[k] + 1`.

Values checked (Unicorn, `run_span_table`): r = 3: `(-1,1) (-2,2) (-3,3) (-3,3) (-3,3) (-2,2) (-1,1)`; r = 4:
`(-1,1) (-3,3) (-3,3) (-4,4) (-4,4) (-4,4) (-3,3) (-3,3) (-1,1)`; r = 12 is in the Rust tests with its octant list
`[3, 5, 7, 8]`. The rows near the top are narrower than a true circle: the table is the original's shape, not a
rounded circle (CONFIRMED by the circle runs, section 11.3).

### 11.3 The sight circle `0x6f3ba480` (CONFIRMED by Unicorn, 300 grids, 0 mismatches)

For a centre cell `(cx, cy)` (mapping of 2.1) and a radius `R` in cells (`f2i(mul(radius, +0x70))`):
- `R = 0`: nothing.
- `R = 1`: the rectangle writer on `[cx-1, cx+1) x [cy-1, cy+1)`.
- `R = 2`: the same rectangle, and the four cells `(cx-2, cy)`, `(cx+2, cy)`, `(cx, cy-2)`, `(cx, cy+2)` through the byte-wide
  cell writer (so a coordinate of -1 is column 255, section 3.1).
- `R >= 3`: rows `cy-R` up to `min(cy+R, height)`, end exclusive (CONFIRMED). Row `y` takes entry `y - (cy - R)` of the
  span table; its span is `[max(cx+lo, 0), min(cx+hi, width))` through the span writer (section 3.2).

The Unicorn runs found two facts that the first model missed: the cell writer wraps bytes (a -1 column mismatched on the first
run), and the span and rectangle writers do not (grids wider than 256 cells with centres past column 255 mismatched when the
span writer was modelled as bytes). Both are in the Rust model. The Unicorn run also exercised the writers with centres
beyond column 255 (the 300-grid run, 0 mismatches).

### 11.4 The refresh cadence (CONFIRMED value and path; LIKELY the repeat)

- `0x6f426c50(1)` posts message `0x80269` with `0x6f4778f0`: a repeating agent timer whose timeout is the f32 at
  `0x6FAB6F00`. The start-up initialisers give `0x3ECCCCCE` = 0.4 (Unicorn, `tools/emu/runtime_consts.py`, CONFIRMED value).
  `0x6f426c50(0)` cancels it (`0x6f477d30`). Both `0x6f426c50(1)` and `0x6f426bf0(1)` (the movement timer) are called by the
  map start `0x6f3b0750` (CONFIRMED call sites).
- The message handler `0x6f42fc80`, switch case `0x80269`, calls `0x6f4299e0`: when the game flag `+0x290 & 2` is clear it
  re-posts the timer (`0x6f426c50(1)`), then calls the refresh `0x6f40a8f0` (CONFIRMED structure).
- So the refresh runs every 0.4 s of agile clock. **Correction of `lockstep.md` section 11.2**: the refresh is not called from
  the movement handler `0x8026A` (case `0x8026A` calls `0x6f426c30`, the mover); it is case `0x80269`.
- The counter `DAT_6fab6a34`: value 0 in the file, one reference in `.text` (the read in the refresh at `0x6f40a947`, found by a
  search of every absolute reference), no writer. So `(counter & 3) == 0` is always true and the visible layer is cleared on
  every refresh (CONFIRMED structure; LIKELY that the value is 0 at run time, since nothing changes it).

### 11.5 The refresh body `0x6f40a8f0` (CONFIRMED structure, the unit path UNSURE)

1. Player masks: for each slot `p`, `DAT_6fab6a14[p] = 0x6f473170(p) | 0x6f3df1c0(p)` (the helpers are not decoded; UNSURE).
2. The visible layer: `memset(vis, 0, (height << shift) * 2)` (CONFIRMED).
3. The source walker `0x6f46d1b0` with the callback `0x6f408eb0`, which calls the source applier `0x6f40a740` for each source
   (CONFIRMED call structure). The enumerator it is given, `0x6f3daa60`, returns the constant `0x2B66676D` (a class tag, not
   a list), and the caller pushes two stack arguments that the callee does not pop (CONFIRMED by disassembly). So the walker's
   sources are not decoded (UNSURE).
4. The source applier `0x6f40a740` reads radius `+0x30` (applied only when above 0.0), mode `+0x28` and player `+0x24`, takes the
   position from the object's vtable slot `+0x78`, and calls the circle writer `0x6f3ba480` with three stack arguments: mode,
   mask and the point pointer. The circle writer's `ret 0x10` expects four, and the fourth (radius pointer) is never pushed
   (CONFIRMED by disassembly, `0x6f40a79b`–`0x6f40a7a0`). So this path cannot draw a radius as written: the walker is not the
   unit sight path (LIKELY dead or unused in 1.26a; UNSURE).
5. The active modifiers (flag `0x40` set and `0x100` clear) are applied with `0x6f40a7b0`, with the player word expanded when the
   `0x80` flag is set (CONFIRMED).
6. `0x6f300710` (not read).
7. The unit pass: the list `0x6f39b6e0(0)` (count `+4`, pointers `+8`). For each unit: the handle is resolved through the
   handle table; the object must have the unit tag `0x2B61676C` and not be removed (`+0x20 == 0`); the unit's flag bit 0
   (`+0x20`) must be clear; the flags at `+0x5C`: `0x80000` needs `0x6f27a460`, `0x100` needs `0x20`. The owner is the
   unit's vtable slot `0xEC`. The mask is `DAT_6fab6a14[owner] | unit+0x14C` (u16), and when `0x6f26de50(1)` is zero the mask also
   takes `0x6f284e80()` (not decoded). Then the unit's own writer `0x6f409e00(unit, mask, 0, 0)` runs.
8. `0x6f409e00` writes the unit's cells in mode 4 (visible) with the mask: a shape from `unit+0x10C` (u16) for the small shapes
   (1: the 2x2 block; 2: the block with the arms; otherwise the record path), and for larger shapes a list of 20-byte records
   in the unit's object (the rows of its footprint). The radius rule behind the records is not decoded (UNSURE).

The Rust refresh does steps 2, 5 and a circle per live unit (step 7 with the radius of the sight value) in that order. The
modifiers come before the units, as in the original. The walker (steps 3-4) is not modelled.

### 11.6 Day and night (UNSURE)

The time of day is float game state 2 (`GetFloatGameState`; the JASS constant GAME_TIME_OF_DAY). The rule that selects the
night sight (`+0x190`) is not read. The Rust model takes night as `t < 6` or `t >= 18` (UNSURE, the usual convention) and uses
the night sight only when it is above zero (UNSURE). The default time is 0.0.

### 11.7 The unit visibility method `0x6f285080` (CONFIRMED structure, LIKELY the rule)

`IsUnitVisible` / `IsUnitFogged` / `IsUnitMasked` call the unit's vtable slot `0xFC` (CUnit vtable slot 63 = `0x6f285080`) with
`(player slot, 0, mask)`, mask 4, 2 or 1. The method:

1. Needs the fog manager (`[0x6fab65f4]`) and its word `+0x3e0` (non-zero; the meaning is UNSURE). Otherwise returns false.
2. A flag bit: `0x6f4c34d0(0xD)`'s object, its word `+0x10`, then `[+0x10]+4 & 0x200` (UNSURE what the object is). It is OR-ed into
   the flags as bit 0.
3. Calls `0x6f3a38f0(player, unit, flags, mask, 0)` (CONFIRMED argument order by disassembly):
   - the relation test `0x6f3a15f0` (only when flags bit 1 is clear): it fails only for units with the flag `0x1000000` whose
     owner is not in the player's shared word (`+0x2E0`) and not in `0x6f27a380(1)`. The model passes every unit (LIKELY for
     ordinary units);
   - when flags bit 0 is clear, the cell test: the unit's position goes through `0x6f474460` (the pathing-cell arithmetic
     through the map object, not decoded), then `>> 2` to the fog cell, then the state table `0x6f00e830` (= section 5.1). The
     result is `state & mask`. The Rust model uses the grid mapping for the cell (UNSURE, section 10.5).
4. If the step-3 test is false, `0x6f26f9e0(unit, player)`: the player's bit in `unit+0x14C | unit+0x148` (the extra vision and
   the reveal words) makes it true (CONFIRMED).

So the Rust rule is: visible (or fogged, or masked, by the mask) when the cell state has the mask bit, or when the player's bit
is in `+0x148 | +0x14C`. The fallback ignores the mask, so a revealed unit also counts as fogged or masked (CONFIRMED by the code).

### 11.8 Verification

| Item | Runs | Result |
|---|---|---|
| Constructor sizes and initial words (`0x6f408d10`) | 300 rectangles (integer corners) | 300/300 |
| Scale words (soft-float quotient) | 40 vectors, Rust test with GAME_DLL | 40/40 bit-exact |
| Span table (`0x6f408fd0`) | radii 3..160 (158 radii) | 158/158 |
| Sight circle (`0x6f3ba480`) | 300 grids, half wider than 256 cells, 26,544 changed words | 300/300 |
| Circle, runs before the byte and index findings | 300 grids | 293/300 (-1 column), then 296/300 (wide spans) |
| Rust model: vision unit tests and integration tests | see the repository | pass |

Reproduce: `GAME_DLL=<Game.dll> PYTHONPATH=<unicorn, pefile> python3 tools/emu/vision_grid_emu.py --cases 300 --radii 160 --wide`.

## 11.9 Pass 3 (2026-10-11): unit cell, unit sight writer, centre function

Harness: `tools/emu/vision_unit_emu.py` (centre and writer; runs on the JASS harness of `tools/emu/jasscomp`, which has
the TIB, the Storm and CRT import models and the start-up initialisers), `tools/emu/vision_unit_diag.py` (one case,
cell-by-cell map). Both are GAME_DLL-gated and write nothing from the original into the repository.

### 11.9.1 The unit centre `0x6f474460` (CONFIRMED by Unicorn, 200 of 200)

- Argument ECX = the unit's mover `unit+0x164` (the CUnit vtable slot `0xB8` is `lea eax,[ecx+0x164]; ret`,
  `0x6f28b0e0`, CONFIRMED). The function reads the handle at mover `+8` (the unit's handle), resolves it with
  `0x6f03fa30` (handle to object), and takes the unit's footprint rectangle at `unit+0x98`: `y0` at `+0x1c`, `x0` at
  `+0x20`, `y1` at `+0x24`, `x1` at `+0x28` (pathing cells).
- Fast path (both results `>= 0`): the output is `((x0 + x1) / 2, (y0 + y1) / 2)` in pathing cells, with the
  truncating halves of the signed division (`cdq; sub; sar 1`). The output pointer is the stack argument (`ret 4`).
- A negative half takes a slow path (`0x6f497460`, `0x6f38b0b0`, f2i) that was not run: UNSURE for off-map units.
- The result is stored as two ints, so the centre is a pathing cell, not a fog cell.

### 11.9.2 The unit fog cell (CONFIRMED structure, LIKELY for the footprint)

- `0x6f409e00` (the unit writer) and the unit query path `0x6f285080` -> `0x6f3a38f0` both take the centre of 11.9.1
  and shift it right by 2 (`sar 2`): a fog cell is 4 by 4 pathing cells = 128 world units, whatever the grid's own scale
  (section 11.1, `scale = cells / range`). On a playable area that is not a multiple of 128 the unit cell and the point
  cell (section 2.1) differ; both are what the original does (CONFIRMED structure).
- The footprint rectangle of a unit is not decoded (who writes `unit+0x98` is open). The Rust model takes the unit's own
  pathing cell as its footprint (LIKELY: a one-cell rectangle has the centre of that cell).

### 11.9.3 The unit sight writer `0x6f409e00(unit, mask, arg3, arg4)` (partly decoded)

Structure (CONFIRMED by disassembly):
1. Gate: the low 12 bits of `mask` must be non-zero, otherwise the owner (`unit`'s vtable slot `0xEC`) is read and the
   writer exits for owners `>= 12`. Bits 12 to 15 are written as well when the low bits are set (UNSURE meaning).
2. The centre (11.9.1, then `>> 2`), the radius kind `k = u16 unit+0x10C` (`arg4 != 0` gives 4 instead), and the shape:
   `k == 0` writes nothing; `k == 1` the 2x2 block, `k == 2` the block with the four arms (the circle of 0x6f3ba480 for
   radii 1 and 2, CONFIRMED by reading); `k >= 3` the shape object of `0x6f409d00(grid+0x78, k, pitch)`, a cache indexed
   by `k` whose builder `0x6f409120` makes a `(2k+1)^2` word block plus records.
3. The writes: `msk &= ~mask` and `vis |= mask` (mode 4, visible), with a per-record flag from an elevation compare
   (`grid+0x38` words against the unit level, `0x6f0128c0` or `0x6f00f600` for cliff data). So units can be blocked by
   cliffs (UNSURE for the magnitude, CONFIRMED that the path exists: the helpers read `TerrainArt\CliffTypes`).
4. For `k >= 3` the rows come from a span list at `grid+0x20` with count `grid+0x1C` (the layout of the span table of
   `0x6f408fd0`: `2k+1` pairs of i32). With count 0 the writer covers two cells or none; with a large count and zero pairs
   it covers nothing (CONFIRMED by Unicorn). Who builds this list per refresh is not decoded (UNSURE).

Unicorn results (`tools/emu/vision_unit_emu.py`, `vision_unit_diag.py`):

| Run | Cases | Result |
|---|---|---|
| centre `0x6f474460` (random rectangles) | 200 | 200/200 equal to `((x0+x1)/2, (y0+y1)/2)` |
| writer, flat terrain, grid+0x1C = 0 | 183 | 146 differ from the circle of section 11.3 (the writer covers only the top part near edges) |
| writer, grid+0x1C = 2k+1, grid+0x20 = span table of k | 92 | 72 differ from the circle |
| writer, interior centre, span table, k = 6 | 1 | the centre is covered; rows below the centre are wider than the circle, and row `cy+k` is included (the original's row end is inclusive for the unit path, UNSURE) |

So the circle of 0x6f3ba480 is **not** the unit sight shape for k >= 3. The Rust model keeps the circle as a stand-in.
Zero masks with the low 12 bits clear changed 7 of 8 cases (the writer is not a no-op there).

### 11.9.4 What the refresh writes, and does order matter (CONFIRMED by algebra and by the Rust tests)

- The sight pass only writes visible mode: `vis |= m`, `msk &= ~m`. Two such writes commute (same bits, same mode), so the
  order of the unit writers does not change the words. The modifiers (fogged or masked modes) are not commutative with
  sight; the refresh applies modifiers first and units last, as before (LIKELY the original's order, section 11.5).
- Rust test `sight_writes_only_set_visible_so_their_order_does_not_matter` (vision.rs).

### 11.9.5 Unit visibility query (updated model)

`IsUnitVisible` / `IsUnitFogged` / `IsUnitMasked` use the unit fog cell of 11.9.2 (`VisionGrid::unit_cell`), not the point
mapping. On the default 128-unit grid the two agree; on other playable sizes they differ (tests in vision.rs).

### 11.9.6 Time of day and night (partly decoded, UNSURE rule)

- The time of day is float game state 2 (`GetFloatGameState(GAME_TIME_OF_DAY)` = `0x6f3bc310`): the slot is the 16-byte
  record `game+8+16*2` = `game+0x28` (`0x6f427f40`, CONFIRMED). `SetFloatGameState` (`0x6f3bc2e0`) stores through
  `0x6f477350`, which clamps the value against the slot's limits (`+0x78`, `+0x7C`, `+0x80`, `0x6f477180`, CONFIRMED
  structure; the limits' values are not read).
- `SuspendTimeOfDay(flag)` = `0x6f429920` / `0x6f427e70` toggles bit `0x6c` of the game object... (CONFIRMED structure:
  `and [game+0x6c], ~flag`; when the bit set becomes zero the helper `0x6f429770` starts two agent timers and stores
  them at `game+0x70` / `game+0x74`). Its timer message is `0x80262` (pushed at `0x6f4298a3`).
- Message `0x80262` is case index 0 of the message switch of `0x6f42fc80` (`id - 0x80262` indexes a byte map at
  `0x6f42fd18`, then a jump table at `0x6f42fcfc`); the handler is `0x6f42fc9f`, which stores **0.0** into the time-of-day
  slot through `0x6f477350` (CONFIRMED by disassembly). So the message does not advance the clock by itself; the advance
  is open (UNSURE: the rate per tick; the scale `GetTimeOfDayScale` (`0x6f3bc4b0`) reads the object at `game+0x5c`).
- Night thresholds: none read yet. The Rust model keeps `night = t < 6 || t >= 18` and uses the night sight only when it
  is above zero (UNSURE). The night sight is the unit type's `nsight` (`+0x190` of the type record, `sight` at `+0x18C`,
  unit-types.md); its reader is not found (virtual getters `0x6f025d80`/`0x6f025d90` with no direct caller and no vtable
  entry in the data sections found by search).

### 11.9.7 Open items after pass 3

1. The unit writer's row shape for k >= 3 (11.9.3): the span list at `grid+0x20` and its row loop; who builds it.
2. The elevation / cliff compare and its level (11.9.3 step 3).
3. The footprint rectangle `unit+0x98` (who writes it; 11.9.2).
4. The unit radius kind `unit+0x10C`: the writer is not found (the copy at `0x6f02f990` and the constructors that zero
   it are the only sites seen; the three values `+0x104..+0x10C` come from `0x6f4c2d30` on the creation object,
   `0x6f053a58`-`0x6f053ae0`, UNSURE). Until then the Rust radius is `sight / 128` (UNSURE).
5. Time of day advance and the night threshold (11.9.6).
