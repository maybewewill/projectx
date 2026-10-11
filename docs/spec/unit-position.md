# Unit position storage (GetUnitX/Y, SetUnitX/Y, the cell model)

Status: pass 1, 2026-10-11. Supersedes the "goal 2 NOT confirmed" verdict of `heroes.md` 8.8.
Harness: `tools/emu/unit_position_emu.py` (Unicorn, real code of the cell setter `0x6F474250` and the normaliser
`0x6F4743A0`, grid A, synthetic origin and grid values). Rust check: `crates/sim/tests/unit_position_vectors.rs`
(`UNIT_POSITION_VECTORS=<file>`). Model: `crates/sim/src/unit_position.rs`. Labels: CONFIRMED = read in disassembly
and/or checked in Unicorn; LIKELY = strong inference, not run; UNSURE = not read or not checked.

## 1. Storage

| Field | Meaning | Source |
|---|---|---|
| unit+0x78 / +0x7C | position x / y in cells | CONFIRMED (`0x6F4A6FD0` writes them; `0x6F4737D0` and `0x6F4A6FD0` read them) |
| unit+0x70 / +0x74 | cell term snapshot: grid origin (float) and grid row (int) | CONFIRMED (`0x6F483430` copies G+0x40/+0x44 into them; `0x6F497440` calls it) |
| unit+0x80 / +0x84 | step vector (cells) | CONFIRMED that it is read by `0x6F4A6FD0`/`0x6F38B0B0`; writers in `movement-5.md` 6 |
| unit+0x14 bit 31 | selects grid A (`mgr+0x14`, clear) or grid B (`mgr+0x68`, set) | CONFIRMED (`0x6F483270`) |
| W+0x6C / W+0x70 | world origin for x / y (W = `[0x6FAB7368]`, the agile world) | CONFIRMED that the normaliser and the setter subtract them (x with +0x6C, y with +0x70) |

## 2. Read: GetUnitX/Y = the normaliser (CONFIRMED)

`GetUnitX` `0x6F3C5510` and `GetUnitY` `0x6F3C5550` return the normaliser `0x6F4743A0` of the unit's position
reference (unit+0x164). Its arithmetic (heroes.md 8.7, unchanged):

- `v` = cell term: `snap(G+0x40 - U+0x70)` plus `G+0x48 * i2r(G+0x44 - U+0x74)` when the rows differ (`snap` gives +0 for
  |x| < 1e-4).
- `X = U+0x78 + U+0x80 * v`, `Y = U+0x7C + U+0x84 * v` (softfloat).
- `out.x = 32*X + W+0x6C`, `out.y = 32*Y + W+0x70`, where `32*` adds 5 to a non-zero exponent.

**Consequence (CONFIRMED by the vectors):** every position write refreshes the snapshot U+0x70/+0x74 from the grid it
uses (section 3). So for a unit on grid A, `v` is +0 at read time, whatever the grid values are, and whatever the
step vector is. Then `GetUnitX = 32*U+0x78 + W+0x6C` exactly. The grid values and the step vector drop out of the
getters. The model therefore stores only the cells (and the cached world value, which is that output).

## 3. Write: the cell setter and its tail (CONFIRMED by disassembly and by the vectors)

`SetUnitX` `0x6F3C5970`, `SetUnitY` `0x6F3C59E0` read the current position through the normaliser, replace one
coordinate with the JASS real and call `0x6F4743C0` → `0x6F474250` with the pair `{x, y}` (a pointer to two floats).

1. `0x6F474250` (CONFIRMED): for each coordinate `P = scale(sub(c, origin))`, where `scale` adds -5 to the exponent
   and gives +0 when the exponent is below 6 (the sign-bit test of `0x6F474250` is "magnitude < 0x03000000").
   x uses `W+0x6C`, y uses `W+0x70`. It then calls `0x6F4737D0` with the pair of `P`.
2. `0x6F4737D0` (CONFIRMED): `F = 0x6F38B0B0(v)` = `(U+0x78 + U+0x80*v, U+0x7C + U+0x84*v)` (two multiplies and two
   adds, CONFIRMED in disassembly); `delta = sub(P, F)` per coordinate; then `0x6F4A7380` → `0x6F4A6FD0`, and the
   notification (vtable slot +0x54) when the flag is 1.
3. `0x6F4A6FD0` (CONFIRMED): `r = add(U+0x78, U+0x80*v)` (y likewise), `U+0x78 = add(r, delta.x)`, `U+0x7C = add(r', delta.y)`,
   then `0x6F497440` (snapshot refresh from grid A or B), then the pathing projection `0x6F4A6D70` and gate `0x6F4A6E40`
   (not modelled, see section 6).

Net effect for the model (`v` = +0 and step = +0 at rest, which is the state the model keeps):

    F      = add(old, +0)
    P      = scale(sub(world, origin))
    delta  = sub(P, F)
    stored = add(F, delta)

`stored_cell` in `unit_position.rs` is exactly this. Two points matter and were found by the vectors:

- The stored cell depends on the **old** cell, not only on the new world value: `delta` is formed against the old
  position, and the add then rounds at the old magnitude. A set that puts the unit back at the same world point can
  therefore store a different cell (a few ulps of the cell, which is 32 world units times that per world ulp).
- The zero-sign behaviour of softfloat (a zero first operand returns the second operand) decides the sign of a stored
  zero. The `+0` step term in `F` is needed for that (one vector case, `old = -0`, needed it).

## 4. Vectors (CONFIRMED, Unicorn, 0 mismatches)

Each case: random origin `W+0x6C/+0x70` and grid values (`hero_normaliser_emu.set_case`), grid A selected, snapshot
forced to grid A, a random earlier write (so the old cells come from the setter), then one of: SetUnitX (other y read
back through the normaliser), SetUnitY, or the raw pair. Output: the normaliser for x and y, and the cells.

| Set | Cases | Mismatches (cells and world bits) |
|---|---|---|
| seed 1 (2,000) | setX 657, setY 678, pair 665 | 0 |
| seed 7 (3,000) | setX 1,000, setY 991, pair 1,009 | 0 |

Findings from the same runs (CONFIRMED):
- The other coordinate is re-rounded by SetUnitX/SetUnitY (the pair carries the normalised other value). Its stored cell
  changed in 52 of 1,657 SetUnitX cases and 47 of 1,669 SetUnitY cases (5,000-vector run). Earlier notes (heroes.md 8.8)
  said it never moves; that was too coarse a test.
- Round trips are not the identity. Read-back equals the set value in 658/1,657 SetUnitX cases (40%), 664/1,669 SetUnitY
  cases (40%), and 293/1,674 raw pair sets (18%, both coordinates). Differences are a few world ulps, once about 1e-4
  (`2^-13`, the rounding of a sum at the old position's magnitude). The model reproduces every stored value exactly.

## 5. Mapping to the sim

- `Unit.cell_x/cell_y` hold the cells; `Unit.x/y` hold the cached normaliser output (what the getters return). Every
  write goes through `Game::store_unit_position` (SetUnitX/Y, SetUnitPosition(Loc), the mover, creation). The getters are
  unchanged.
- Origin: `Game::world_origin()` = the playable minimum `(min x, min y)`, or zero without a playable area. LIKELY that
  W+0x6C / W+0x70 hold the playable minimum: the wpm reader stores the playable-bounds floats there, with the rect
  layout (y0, x0, y1, x1) (`pathing-build.md` 1.1, CONFIRMED stores; the layout is LIKELY). NOT checked against a map.
  `set_playable` recomputes the cached positions.
- Creation (`natives/unit.rs::create`): the placement is stored, then the init's read-and-write-back of the same
  position (`place_new_unit`). The vf107 init `0x6F2A0E30` does read through `vf46` and write with `0x6F474250`
  (CONFIRMED); that the placement itself is written through the same setter is LIKELY.
- Mover (`orders.rs::step_unit`): the step is computed in world units as before and written through the cell setter.
  LIKELY that the original's walk writes through `0x6F4A6FD0`, but the walk arithmetic in cells is not traced (movement-5),
  so the per-step rounding of the walk is a model. This changes the walk's bits relative to the previous world-only model.
- Region and fog code read `Unit.x/y` (the cache), so they see the same values as JASS.

## 6. Open (UNSURE)

1. **Grid values.** The layer objects `mgr+0x14` / `mgr+0x68` (+0x40 origin, +0x44 row, +0x48 scale) have no writer found
   (the world object is built by `0x6F468B30` → `0x6F489750`, which is the 0x978-byte engine object; the layer fields are
   not written there). They drop out of the getters while the snapshot is refreshed from grid A, so this is not needed
   for grid A units. They would matter for grid B (unit+0x14 bit 31 set) and for any stale snapshot.
2. **Grid B and the bit.** Who sets unit+0x14 bit 31 is not found. The sim never sets it, so the model is grid A only.
   With a non-zero `v` the step term `U+0x80*v` would enter the getters; the model has no `v`, so units with bit 31 set
   are not representable (UNSURE).
3. **Step vector writers during a walk** (`0x6F4A70D0` callers, movement-5 section 6): the value of the step term only
   enters through `U+0x80*v` and `F`, so it is invisible while `v` is +0. Its real values during a walk are not needed
   for the getters, but the walk's own arithmetic is still open.
4. **SetUnitPosition / SetUnitPositionLoc** go through the move path (slot 96, `0x6F2A9CA0`), not through `0x6F474250`
   directly. The model assumes the same setter arithmetic (LIKELY); the move path was not run in Unicorn.
5. **Creation placement.** Whether the placement is written by the setter (the model assumes so, LIKELY) or by another
   writer such as `0x6F474D40` (also a writer of unit+0x78, per movement-5) is not traced.
6. **Pathing hooks** `0x6F4A6D70` (projection on unit+0x94) and `0x6F4A6E40` (gate on unit+0x98) are not modelled; the
   harness stubs them (they write no position field, from their bodies). The vtable +0x54 notification is not modelled.
7. **Map origin** (section 5): LIKELY, not checked on a real map.

## 7. Tests

- Unit tests (no Game.dll): `unit_position.rs` (zero-origin round trip exact, exponent flush, far-origin loss).
- Vector test: `crates/sim/tests/unit_position_vectors.rs`, skipped without `UNIT_POSITION_VECTORS`. Vectors are generated
  by `tools/emu/unit_position_emu.py GAME_DLL...` and not committed (they are outputs of the user's own Game.dll).
