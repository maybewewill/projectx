# Vision: fog-of-war grid, writers, queries (Game.dll 1.26a)

Status: pass 1 (2026-10-11). Written from disassembly and decompiled structure of the 1.26a `Game.dll`
(SHA1 c5d8624b…). Tags: CONFIRMED = read from the code and/or checked in Unicorn; LIKELY = the reading is
consistent with the code but not run; UNSURE = open. Addresses are image VAs.

Verification: `tools/emu/vision_emu.py` (Unicorn, the user's Game.dll). Results, seed 7, 1,000 cases per item:

| Item | Address | Cases | Mismatches |
|---|---|---|---|
| state table | `0x6f00e7a0` | 1,000 (250 per flag pair) | 0 |
| span writer, mode 1 / 2 / 4 | `0x6f406920` / `0x6f4069f0` / `0x6f406850` | 1,000 | 0 |
| cell writer | `0x6f3b0e20` | 1,000 | 0 |
| rectangle writer | `0x6f3b0e90` | 1,000 | 0 |

The first run (200 cases, span writer) exposed the odd-start quirk in section 3.3 (38 mismatches), which
the model now reproduces. The first rectangle run exposed the row pitch (`+0x64`), which the harness sets.

## 1. The grid object

- The game global `[0x6FAB65F4]` (the same object that holds the local player id at `+0x28`) holds the fog
  grid pointer at `+0x34`. Every query and writer reads `[[0x6FAB65F4]+0x34]` (CONFIRMED, natives
  `IsFogEnabled` `0x6f3bb200`, `SetFogStateRect` path, `0x6f3c98f0`).
- Grid object fields (CONFIRMED from `0x6f00e7a0`, `0x6f26d0c0`, `0x6f192620`, the writers and the flag setters):

| Offset | Meaning |
|---|---|
| `+0x10` | `FogMaskEnable` flag (the `a` bit of the state table) |
| `+0x14` | `FogEnable` flag (the `b` bit) |
| `+0x2c` | pointer to the **mask** array (ushort per cell, "unexplored" bits) |
| `+0x30` | pointer to the **visible** array (ushort per cell) |
| `+0x44` / `+0x48` | y origin / x origin of the cell mapping (world units) |
| `+0x60` / `+0x6c` | width / height in cells |
| `+0x64` | row pitch of the rectangle writer (LIKELY `1 << shift`) |
| `+0x68` | shift: a cell's index is `(row << shift) + col` |
| `+0x70` / `+0x74` | x scale / y scale (cells per world unit) |

- Index: `(row << shift) + col`, with `col` from x and `row` from y (CONFIRMED, `0x6f26d0c0`).
- The cell scale value is not read yet (UNSURE). The circle code multiplies the radius by `+0x70`, which
  makes the scale a 1/128 of a world unit if the cell is 128 wide. The Rust model assumes 1/128 (UNSURE).

## 2. Point to cell, and the words

### 2.1 Mapping (`0x6f192620`, CONFIRMED by disassembly)

For an axis with origin `o` and scale `s`, limit `n` (width for x, height for y):

    cell = clamp( f2i( mul( sub(v, o), s ) ), 0, n - 1 )

`sub`, `mul`, `f2i` are the soft-float operations of `crates/softfloat` (`0x6f6eeef0`, `0x6f6eee20`,
`0x6f6eec20`; checked there). The clamp is written with an unsigned compare: values above `n-1` (including
negatives seen as large unsigned) go to the nearest end. Mapping the two corners of a rectangle uses the same
function (`0x6f3b76e0`).

Before the mapping, the point queries clamp the point into the playable area with `0x6f469490` (CONFIRMED
that it clamps; the bounds are the world rectangle, minimum at world `+0x6c`/`+0x70` and maximum from the
constant 32.0 at `0x6FAAE564`, LIKELY, not run).

### 2.2 Words

- `vis` word = `[+0x30][index]`, read as `| 0xF000` by the query path (CONFIRMED, the high nibble is not a
  player bit).
- `msk` word = `[+0x2c][index]`, read as `& 0xFFF` (CONFIRMED). Player bits are 12-bit in the query path; the
  player index is `slot` (the player object's byte at `+0x30`) and the bit is `1 << slot`.

## 3. Writers

Three modes. The mode is the fog state constant (CONFIRMED: the JASS constants 1, 2, 4):

| Mode | Value | `msk` bit | `vis` bit | Routine |
|---|---|---|---|---|
| masked | 1 | set | clear | `0x6f406920`, `0x6f3b0e20` (branch 1) |
| fogged | 2 | clear | clear | `0x6f4069f0`, `0x6f3b0e20` (branch 2) |
| visible | 4 | clear | set | `0x6f406850`, `0x6f3b0e20` (branch 4) |

Other modes write nothing (CONFIRMED).

### 3.1 Cell writer `0x6f3b0e20`
Stores one cell with the mode and the player mask. No clamp: the caller passes byte coordinates (CONFIRMED).

### 3.2 Span writer `0x6f406920` / `0x6f4069f0` / `0x6f406850`
One row, columns `[x0, x1]`. Row outside `[0, height)` writes nothing. The start is clamped to 0, the end to
`width-1` (CONFIRMED). The store pattern is pairwise (one dword per two cells):

1. if `x0` is odd, store `x0` alone;
2. from the next even cell, store pairs up to `x1-1` (or `x1` when `x1` is odd);
3. if `x1` is even, store the cell after the last pair alone.

For `x0 <= x1` this is exactly the inclusive span. For `x0 > x1` steps 1 and 3 still store, so up to two cells
outside the span are written (CONFIRMED, Unicorn: example `[11, 6]` writes 11 and 12). The Rust model
reproduces it (`vision::span_cells`). Cells past the grid's columns land in the flat array (a column past
the row pitch wraps to the next row): not modelled beyond the row pitch (UNSURE, very rare).

### 3.3 Rectangle writer `0x6f3b0e90`
Arguments: mode, mask, `x0`, `y0`, `x1`, `y1`. The start is clamped to 0, the end to `width-1` / `height-1`.
The loops then run over **`[x0, x1) x [y0, y1)`: the end is exclusive** (CONFIRMED, Unicorn: 1,000 cases with
the exclusive model, 0 mismatches). Consequence: the last column and row of the grid are never written by a
rectangle, and a rectangle whose max lies in cell `k` does not write cell `k`.

### 3.4 Rectangles in the callers
- `0x6f3b76e0` converts a rectangle object (min corner at `+0`/`+4`, max at `+8`/`+0xC`, as `y0,x0,y1,x1`)
  with `0x6f192620` and calls the rectangle writer (CONFIRMED).
- Fog-state rectangles (`SetFogStateRect`) and rect modifiers use it, so their cells are
  `[cell(min), cell(max))` (CONFIRMED structure).

## 4. Shapes: the sight circle `0x6f3ba480`

Arguments: the grid, mode, player mask, centre point (world), radius (world float).

1. The centre cell comes from `0x6f192620` (CONFIRMED).
2. The radius in cells is `R = f2i( mul(radius, +0x70) )` (CONFIRMED).
3. `R = 0` writes nothing (CONFIRMED). **With a 1/128 cell, a radius below 128 units does nothing.** This is
   the most important UNSURE item: it depends on the scale.
4. `R = 1`: the rectangle `[cx-1, cx+1) x [cy-1, cy+1)` through the rectangle writer, so a **2x2 block**
   `{cx-1, cx} x {cy-1, cy}` (CONFIRMED from the call arguments; the exclusive end is CONFIRMED in 3.3).
5. `R = 2`: the same 2x2 block, plus the four single cells `(cx±2, cy)` and `(cx, cy±2)` (CONFIRMED).
6. `R >= 3`: rows `[cy-R, cy+R)` (end exclusive, CONFIRMED from the loop bound `iVar4 = cy+R`, the row `cy+R`
   is not written). Each row takes its span from a table of 2R+1 entries built by `0x6f408fd0` (CONFIRMED
   that it is a span table of `(min, max)` per row).
   - The table comes from `0x6f408f80` (cache per radius) and `0x6f408800`, which is an **integer midpoint
     circle** (CONFIRMED by reading the code: start `d = 3-2r`, east step `4x+6`, south-east step
     `4(x-y)+10`, records `x` at every south-east step, stops when `x > y`). The count is `r/2 + 1`.
   - The expansion from the octant table to the 2R+1 row spans (`0x6f408fd0`) is **not decoded** (UNSURE).
     The Rust model uses `x*x + dy*dy <= R*R` per row (UNSURE stand-in, `vision::VisionGrid::circle_shape`).

## 5. Queries

| Native | Address | Rule | Status |
|---|---|---|---|
| `IsVisibleToPlayer(x, y, p)` | `0x6f3c98f0` | state `== 4` | CONFIRMED |
| `IsFoggedToPlayer(x, y, p)` | `0x6f3c9980` | state `== 2` | CONFIRMED |
| `IsMaskedToPlayer(x, y, p)` | `0x6f3c9a10` | state `== 1` | CONFIRMED |
| `IsLocation*ToPlayer` | `0x6f3c9940` / `0x6f3c99d0` / `0x6f3c9a60` | same, from the location | CONFIRMED (structure) |

The state is the table of section 6 for the player's bit. An invalid player returns false without a read
(CONFIRMED: `0x6f3bd4d0` returns null).

### 5.1 The state table `0x6f00e7a0` (CONFIRMED, Unicorn 1,000 cases)

Let `a = FogMaskEnable`, `b = FogEnable` (as 0 or 1), `row = a + 2b`:

| `a`, `b` | row | if `vis & bit` | else if `bit & ~msk` | else |
|---|---|---|---|---|
| 0, 0 | 0 | 4 | 4 | 4 |
| 1, 0 | 1 | 4 | 4 | 1 |
| 0, 1 | 2 | 4 | 2 | 2 |
| 1, 1 | 3 | 4 | 2 | 1 |

In words: with both flags on, a visible cell is visible; an explored but not visible cell is fogged; an
unexplored one is masked. With fog off, everything not masked is visible. With the mask off, nothing is
masked, and the unseen cells are fogged. With both off, every cell is visible.

## 6. Who writes the words

### 6.1 Explicit states (`SetFogState*`)
`0x6f3c1a30` (rect), `0x6f3c1ab0` (radius), `0x6f3c1b20` (location radius) (CONFIRMED structure): the player
mask comes from `0x6f408110(slot, useShared)`, which is `1 << slot`, or the player's shared mask word at
player `+0x2E0` when `useShared` is set (CONFIRMED structure; the shared mask's content is LIKELY the
allied players for alliance type 5, see `natives/fog.rs`). The writer is the one of section 3 with the given
state: the explicit state is an immediate write (CONFIRMED structure).

### 6.2 Fog modifiers
- Create: `0x6f3d0e70` (rect), `0x6f3d0f90` (radius), `0x6f3d10f0` (location radius). The modifier object
  has flags at `+0x20`: `0x8` rect, `0x10` radius (point), `0x20` location radius; `mode = flags & 7`; the
  player words at `+0x24`; the shape fields at `+0x2C..+0x4C` (CONFIRMED structure, `0x6f3e6f80`).
  Bits `0x80`/`0x100` come from the create arguments (UNSURE which argument is which).
- Start `0x6f3c1bc0`: sets flag `0x40` (CONFIRMED). Stop `0x6f3c1be0`: clears it (CONFIRMED). Destroy
  `0x6f3c1ba0`: calls the object's destroy slot (CONFIRMED).
- Apply `0x6f40a7b0`: writes the modifier's shape with `mode = flags & 7` for each player in its word (the
  word is the owner's, or all players of the object when the argument is `0xFFFF`) (CONFIRMED structure). The
  call at creation uses `0xFFFF` (`0x6f3e6f80`).

The Rust model writes a started modifier immediately, and rebuilds on stop (section 7). The original applies it
at the next refresh (section 6.3). The difference is in timing only and is UNSURE for queries between a start
and the next refresh.

### 6.3 Refresh `0x6f40a8f0` (per call; CONFIRMED structure, cadence LIKELY)
Called from `0x6f4299e0`, which `0x6f42fc80` calls. `0x6f42fc80` is the handler of the movement message
`0x8026A` in `movement.md`, so the refresh runs on each movement pass (LIKELY):

1. For each of the 16 player slots, `DAT_6fab6a14[p] = 0x6f473170(p) | 0x6f3df1c0(p)`: the player's
   **vision mask** (own bit plus the shared-vision players, LIKELY; the two helpers are not decoded).
2. If `DAT_6fab6a34 & 3 == 0`, the **visible words are cleared** for every cell (CONFIRMED). The counter is
   read only here; its writer is not found (UNSURE). So the visible layer is rebuilt on every fourth refresh,
   and the explored layer is never cleared by the refresh.
3. Sight sources are applied (see 6.4); then the active modifiers (flag `0x40` set, `0x100` clear) are applied
   with `0x6f40a7b0`.

### 6.4 Sight sources `0x6f40a740` (CONFIRMED structure)
For a source object with radius `+0x30` (float) greater than zero:
- player mask = `DAT_6fab6a14[source owner index]` (section 6.3);
- mode from the source's field at `+0x28` (index 0 gives visible `4`; 2 gives masked `1`; any other gives
  fogged `2`);
- centre from the source's position getter (vtable slot `0x78`, LIKELY the unit's position);
- shape = the circle of section 4.

The units are not decoded: the iteration that calls `0x6f40a740` for each unit (through `0x6f46d1b0`, not
read) is UNSURE.

### 6.5 Flags `FogEnable` / `FogMaskEnable`
- `FogEnable(v)` `0x6f3bb1e0` → `0x6f407e60`: stores `v` at `+0x14`; when `v` is false it clears the **visible**
  array (CONFIRMED).
- `FogMaskEnable(v)` `0x6f3bb210` → `0x6f407dd0`: stores `v` at `+0x10`; when `v` is false and the second
  argument is set, it clears the **mask** array (CONFIRMED for the memset condition; the argument mapping of
  the native is UNSURE).
- Both then call the same update routines (`0x6f406cc0`, `0x6f300710`, `0x6f392e70`, `0x6f00fac0`,
  `0x6f016cd0`, not read).
- `IsFogEnabled` `0x6f3bb200` reads `+0x14`, `IsFogMaskEnabled` `0x6f3bb230` reads `+0x10` (CONFIRMED).
- The Rust natives for these two are in `natives/presentation.rs` and are not wired to the grid yet (see 9).

## 7. Unit visibility (partial)

- `IsUnitVisible` `0x6f3c7af0`, `IsUnitFogged` `0x6f3c7d00`, `IsUnitMasked` `0x6f3c7d80`: each resolves the
  unit handle, then calls a unit virtual method (slot `0xFC`) with the player slot, `0` and a state mask
  (`4`, `2`, `1`) (CONFIRMED). The method itself is not read: it presumably tests the cell of the unit's
  position (LIKELY), and it may account for the unit's own sight and `invisible` (UNSURE).
- The per-unit relation classifier `0x6f3a1460` (and `0x6f3a1520`, the variant for a single cell) reads twelve
  masks at unit `+0x1F8` and reports, for a cell, which of the twelve relation sets see it (visible bits) and
  which see it fogged (CONFIRMED structure). Its callers are in the unit AI and order code (`0x6f38e880`,
  `0x6f38ea30`, `0x6f392a10`, `0x6f398b30`, `0x6f399d00`, ...), not the attack scan (UNSURE what they decide).

## 8. Target acquisition does not read vision (CONFIRMED at the direct-call level)

The attack-cycle functions (`attack-cycle.md`) call no vision routine directly:

- scan timer callback `0x6f0cf210`: callees `0x6f0c71c0`, `0x6f0ccfa0`, `0x6f2a0210`, `0x6f2a3270`, `0x6f472890`;
- candidate callback `0x6f0ccfa0`: callees include `0x6f2a4ab0` (order dispatch) and `0x6f4766e0` (range), no
  vision routine;
- group builder `0x6f2a3270`: callees are the spatial dispatcher `0x6f467570` and the cell helpers, no vision
  routine;
- swing gate `0x6f0cd350` and execute `0x6f0cf660`: no vision routine among the callees.

So acquisition takes the first enemy in range order whatever its visibility (LIKELY for the whole cycle,
since the callees were read one level deep only). The unit AI's relation classifier (7) is the only place
that could make a decision on vision, and it is not in the attack path.

## 9. Rust model (crates/sim)

- `vision.rs`: `VisionGrid` (sparse cells, width/height default 256, origin 0, scale 1/128, both flags on),
  `table_state`, `span_cells`, `cell_index`, the writers, `rect_shape`, `circle_shape`. Unit tests cover the
  table, the quirks, the exclusive ends and the circle sizes.
- `natives/fog.rs`: the count model is replaced. Starting a modifier writes its shape at once; stopping or
  destroying it rebuilds the visible layer from the remaining active modifiers (in start order, UNSURE);
  explicit states are immediate writes. Shapes use the grid (`rect_shape`, `circle_shape`), so the circle
  radius below 128 writes nothing.
- `natives/misc.rs` still uses the old inclusive helpers (`rect_cells`, `circle_cells`, `cell_of_size`) for
  blight and terrain; they are kept unchanged.
- Not wired: `FogEnable`/`FogMaskEnable` (presentation), the unit visibility queries (`unit_attr.rs`), the
  unit sight sources (no units feed the grid yet), the refresh cadence (the Rust grid is rebuilt only by
  modifier stop/destroy).

## 10. Open items

1. The grid's real size, origin and scale (`+0x60`, `+0x6c`, `+0x44/+0x48`, `+0x70/+0x74`): find the constructor
   of the grid object (not read). This decides whether radii below 128 units write anything.
2. The circle's row spans: decode the expansion in `0x6f408fd0` (and `0x6f408f80`'s cache) and check it in
   Unicorn (the allocator stubs `0x6f407a30`, `0x6f408800` callees `0x6f34c1e0`, `0x6f35c2d0`).
3. The writer of `DAT_6fab6a34` (the refresh counter), and the refresh call sites with their cadence.
4. The unit sight sources: how `0x6f46d1b0` walks the units, and the position and sight fields of the source.
5. The player vision mask helpers `0x6f473170` and `0x6f3df1c0` (shared vision and alliance types).
6. The modifier create arguments that set `0x80` and `0x100`, and the initial values of the two layers.
7. `IsUnitVisible` through vtable slot `0xFC` (the unit's method), and invisibility/detection.
8. The exact timing of a modifier's first write (immediate versus the next refresh).
