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
4. **SetUnitPosition / SetUnitPositionLoc** go through the move path (slot 96 = `0x6F2A5D50`, core `0x6F2AC220`). The
   clamp of that path is CONFIRMED and modelled (section 8.1). The notify in slot 55 (`0x6F2A5FB0`) is not modelled.
5. **Creation placement.** NOT resolved: the placement record does not reach the cells in the code read (section 8.2).
   `0x6F474D40` is the move-issue routine (events 0xD0196/0xD0198), not the placement.
6. **Pathing hooks** `0x6F4A6D70` (projection on unit+0x94) and `0x6F4A6E40` (gate on unit+0x98) are not modelled; the
   harness stubs them (they write no position field, from their bodies). The vtable +0x54 notification is not modelled.
7. **Map origin** (section 5): LIKELY, not checked on a real map.

## 7. Tests

- Unit tests (no Game.dll): `unit_position.rs` (zero-origin round trip exact, exponent flush, far-origin loss).
- Vector test: `crates/sim/tests/unit_position_vectors.rs`, skipped without `UNIT_POSITION_VECTORS`. Vectors are generated
  by `tools/emu/unit_position_emu.py GAME_DLL...` and not committed (they are outputs of the user's own Game.dll).

## 8. Pass 2 (2026-10-11): move path clamp, creation, walk, grid B

Labels as above. Addresses are Game.dll VAs. Harness and vectors: `tools/emu/unit_clamp_emu.py` (new), `tools/emu/unit_position_emu.py`.

### 8.1 SetUnitPosition clamps into the playable area (CONFIRMED, fixed in the sim)

- `SetUnitPosition` `0x6F3C5A50` → `FUN_6F3BDCB0` (unit) → `FUN_6F2A4B80(1)` → vtable slot 96 (`0x180`) = `CUnit::vf96`
  `0x6F2A5D50` with (x*, y*, 1, 1, -1, 0, 0, 0, -1, 0). `SetUnitPositionLoc` `0x6F3C5AB0` reads the location and calls
  `0x6F3C5A50`, so it takes the same path.
- `vf96` → core move `0x6F2AC220` (CONFIRMED call, all arguments passed through). Its first step, before any other
  use of the pair: copies `*x`, `*y` into a local pair and calls **`0x6F469490`** with ECX = that pair (CONFIRMED,
  disassembly at `0x6F2AC242`). Then, when its 4th argument is non-zero, it calls vtable slot 55 (`0x6F2A5FB0`, a
  notify that also writes globals `0x6FAB425C/4260/4264` and may read the pair; NOT modelled, UNSURE). Then the
  mover's cell setter `0x6F474250` on the (clamped) pair, flag = (9th argument == 0) = 1 for SetUnitPosition.
- `0x6F469490` (CONFIRMED by disassembly, and by 3,000 Unicorn vectors, `tools/emu/unit_clamp_emu.py`, seed 1: Rust
  `unit_position::clamp_to_playable`, 3000/3000 match, `playable_clamp_matches_the_original_file`):
  x is clamped to [W+0x6C, W+0x74 - 32.0]; y to [W+0x70, W+0x78 - 32.0]. The lower bound is tested first and, when it
  applies, the upper bound is not tested. The constant 32.0 is `0x6FAAE564`. The compares are x87 `fcom`, so NaN is kept.
- Sim change: `natives/unit.rs::set_position` (SetUnitPosition and SetUnitPositionLoc only) now clamps through
  `Game::clamp_unit_move` when a playable area is set. **SetUnitX/SetUnitY do not clamp** (CONFIRMED: `0x6F3C5970`
  → `0x6F4743C0` → `0x6F474250`, no clamp call). Before this pass the sim stored the raw point for all of them.
- The playable rectangle is W+0x6C..W+0x78 (min x, min y, max x, max y) as used by `0x6F469490`. This matches
  `location.rs`. The map loader still has to fill it (see section 5, UNSURE).

Also seen in `vf96`/core move, still open (UNSURE): `0x6F32D0B0` (a flag from the object at `[0x6F938C6C]`, field
+0xCC) decides whether a second clamp `0x6F3E0B50` runs (see 8.2).

### 8.2 Creation placement (goal 1): the route into the cells is NOT found

Facts (CONFIRMED by disassembly):
- `CreateUnit` `0x6F3C5230` → `0x6F24F1F0` (the wrapper: `0x6F29F990` with flags 0x502, then vtable slot 35 at `0x8C`,
  which is the handle/owner step; no position write in the wrapper) → `0x6F29F990` (the body).
- `0x6F29F990`: resolves the type (`0x6F29F3E0`), computes the corner mode from the facing (0.0 / 90.0 / 180.0 tests),
  and, when `0x6F32D0B0` is non-zero, clamps the pair with `0x6F3E0B50` (which first calls `0x6F469490`, then adds a
  signed inset `±[0x6FAB68A8]`). Then it builds a 0x174+-byte placement block on the stack and calls the init
  `CUnit::vf107` `0x6F2A0E30` with it as the template argument.
- `0x6F3E0B50`'s inset reads `[0x6FAB68A8]`, which is in `.data`, zero in the file, and **written at run time by
  `0x6F3DB4F0`** (`i2r` of a field, used as a scratch slot). Its value at a creation is therefore whatever the last call
  left. This is a state dependency on the call history; not modelled (UNSURE).
- `vf107` (`0x6F2A0E30`): writes no unit cell field itself (no store to +0x78/+0x7C/+0x80/+0x84 in the function body).
  Its only position step is the read-back at the end: normaliser of `vf46` (`0x6F2A1E74`), then the setter with ECX =
  `unit+0x164` (`0x6F2A1E9E`), flag 1. So the setter is a read-back of whatever the cells are when `vf107` runs.
- `0x6F29F3E0` (the type copy, called before `vf107`) writes into the new unit: `+0x78` ← `FUN_6F29C2D0()`, `+0x7C/+0x80/+0x84`
  ← three floats of `FUN_6F29C790()`, `+0x88` ← `FUN_6F29C830()`. These getters read the unit-data table at `0x6FAB445C`
  by key. Their meaning is not decoded; they are the only cell-range writes on the creation path that were found.

Consequence (UNSURE, important): **the placement record never reaches the cells in the code read so far.** Either
`+0x78/+0x7C` are not the position at creation (the type copy then matters for the first read), or the placement is
written by a function not yet found. The sim's `place_new_unit` (store the placement, then read it back and store
again, the `vf107` pattern) is therefore **unverified**: it reproduces the read-back but not the initial write. Next
step: a Unicorn run of `0x6F29F990` with a stubbed `vf107` (record the pair at `0x6F2A1E9E`), to see the cells that
the read-back sees.

### 8.3 Mover walk (goal 3): the per-record step and where the walk ends (CONFIRMED structure, step arithmetic UNSURE)

- `0x6F4907F0` (per record R, owner U = `R+0x14`): reads V = (U+0x80, U+0x84); `d = |V|` (softfloat mul/add/sqrt);
  `R+0x20 = d` (0 if `d < 4e-7`); `R+0x24` = heading `0x6F4B2DE0(R+0x20)`, or U+0x8C when d = 0. Then either
  (flag `R+0x28 & 0x200000` clear) `0x6F38B0B0(start, 0x6F497460(...))`, `0x6F495320(start, R+0x18 (target),
  R+0x20, R+0x24, ...)`, then `if U+0x88 < R+0x20: R+0x20 = U+0x88`; or (flag set) `0x6F4940C0`, which is the same
  without the path step. This re-confirms movement-5 section 9.
- `0x6F495320` (path step): calls `0x6F493EA0` (a validity/arrival test), then `0x6F493FA0` (or `0x6F49E020` for the path
  node). `0x6F493FA0` (read from the decompile this pass): if the input is non-zero, the output is
  `sign(x) * min(U+0xB8, |x|)` (the turn-rate cap of movement.md, field `+0xB8`). **It writes an angle-like scalar,
  not a position.** `0x6F4940C0` calls `0x6F493FA0` the same way.
- **No function on this chain writes U+0x78/U+0x7C.** The position writes found on the walk side: `0x6F4944A0` (a
  test-then-apply routine: `0x6F4A3810` test, `0x6F4A3FF0` apply, then `0x6F4737D0(P, 1)` on success; CONFIRMED calls;
  its purpose is a guess: a spot search for a placed object).
  So the walk's position update is not through `0x6F4907F0`; it is one of `0x6F494EE0`, `0x6F496EB0` or `0x6F497080`
  (`0x6F497080`, the move-target routine, calls `0x6F4737D0` in its body; not read in full). **Consequently the
  "old + step*v" model of orders.rs (`step_unit` → `store_unit_position` in world units) is NOT verified and the
  Unicorn walk comparison asked for could not be run: the per-step write is not found.**
- `[mgr+0x80]` (the cap bound in `Position::vf6` `0x6F4747B0`, CONFIRMED reads): no writer found in the decompile
  (grep of `DAT_6fab7368 + 0x80` gives reads only). The constructor `0x6F470A60` does not set it (movement-5). Open.

### 8.4 Grid B (goal 4): the selector is CONFIRMED, the writer of unit+0x14 bit 31 is NOT found

- `0x6F483270` (CONFIRMED, decompile): returns `mgr+0x14` (grid A) when `(unit+0x14) & 0x80000000 == 0`, else `mgr+0x68`
  (grid B). `0x6F483290` selects the same way between `mgr+0xBC` and `mgr+0x110`.
- Writers of bit 31 of the unit's `+0x14` word: not found by a decompile search (`0x80000000` masks in chunk_019 are
  reads, and the `|0x80000000` writes found are the 2-bit state words of the pathing grid, not this field). Not
  implemented. The sim keeps grid A only (unchanged).

### 8.5 Tests and counts (this pass)

- `tools/emu/unit_clamp_emu.py`: 3,000 cases (seed 1), 3,000/3,000 match the Rust `clamp_to_playable` (test
  `playable_clamp_matches_the_original_file` with `UNIT_CLAMP_VECTORS`); 64 cases embedded in the unit tests
  (`playable_clamp_matches_the_original_embedded`).
- `tools/emu/unit_position_emu.py`: 2,000 cases (seed 11), setter comparison `unit_position_vectors` passes.
- Unit tests in `unit_position.rs` (margin, inside points, below-min rule) pass.

## 9. Pass 3 (2026-10-11): creation placement, the vf107 frame, and the walk (partial; no sim change)

Harness: `tools/emu/unit_create_emu.py` (Unicorn, real `0x6F29F990` and real `vf107` `0x6F2A0E30`; stubs listed in
the harness; run with `GAME_DLL=... python3 tools/emu/unit_create_emu.py`). No vectors were committed and no sim code
changed in this pass. Labels as above.

### 9.1 What the placement body does (CONFIRMED by disassembly and by the run)

- `0x6F29F990` takes ECX = player/owner, EDX = unit type (`ebp`), stack: x*, y*, facing*, flags, p7, p8. The return
  value is the new unit object (`mov eax, esi` at `0x6F29FD26`), not a bool.
- The unit object is `[esp+0x14]` of the body. **No store to `[esp+0x14]` exists in the body**; the writer is inside a
  callee (UNSURE which one; the type copy `0x6F29F3E0` is the prime suspect). The harness stores the synthetic unit
  there at `0x6F29FB8F` and says so.
- The frame block passed to `vf107` is `body+0x54` (`lea edx,[esp+0x5c]` after two pushes). Inside `vf107`, with
  `ebp` = the block: `[ebp+0x60]` = x, `[ebp+0x64]` = y (read at `0x6F2A120F`/`0x6F2A1220` as the pointers passed to
  `0x6F477400`/`0x6F477350`), `[ebp+0x6C]`, `[ebp+0x70]`, `[ebp+0x74]` are read later (facing/z-like), `[ebp+0x78]`,
  `[ebp+0x7C]`, `[ebp+0x80]`, `[ebp+0x84]` are the type copy's cell and bound fields.
- `vf107` writes the unit's position through three **lookup-based writers**, each of which first calls
  `0x6F03FA30` with the ECX of the caller (`[ecx+8]` index, `[ecx+0xC]` tag) to find an object:
  - `0x6F477400(ECX = unit+0xB8, [ebp+0x60])` (x) and `0x6F477350(ECX = unit+0xB8, [ebp+0x64])` (y);
  - `0x6F477460(ECX = unit+0x214, [ebp+0x74])` and `0x6F477400(ECX = unit+0x214, [0x6FAB4438])`;
  - `0x6F476250(ECX = unit+0x164)` → lookup + `0x6F49DFF0` (position reference init, not a cell write).
- `0x6F477400`/`0x6F477460` build the pair `(U+0x80, x)` / `(y, U+0x84)` and call `0x6F4A9390`, which sets the
  **step/bound pair** `U+0x80 = p0`, `U+0x84 = p1` and then **clamps** the cell `U+0x78` into `[U+0x80, U+0x84]`
  (`lo <= c <= hi` keeps `c`; `c < lo` gives `lo`; `c > hi` gives `hi`; then `0x6F4A9230` notifies). `0x6F4A92B0`
  (`0x6F477350` path) and `0x6F4A9320` do the same for the other axis / `U+0x7C`. (CONFIRMED by decompile; NOT yet
  checked in Unicorn.)
- `0x6F477180`/`0x6F4773A0` (the getters used by `0x6F477350`) read the bound pair with the same clamp rule.
- The read-back at the end (`0x6F2A1E9E`, `call 0x6F474250` with ECX = unit+0x164, after `vf46` `0x6F2A1E74`) is
  the setter of section 3; the harness stops there (`READBACK_RET = 0x6F2A1EA3`) and records the cells.

### 9.2 Result of the run (UNSURE as a model of the original, because of the stubs)

With the type copy stubbed to success (no block fields written), the read-back pair is `(0, 0)` and the stored cells
are `-0.0` (`0x80000000`) for a request of (100, 200) on a `[0, 4096]` playable rectangle. The placement coordinate
does not reach `U+0x78` in this run, because the clamp keeps `U+0x78` (zero) inside the bounds `[U+0x80, U+0x84]` that
the placement itself sets. **This is a stub artifact until the type copy is run for real**: the type copy writes
`U+0x78` (`block+0x78`, from `FUN_6F29C2D0`), `U+0x7C/80/84` (from `FUN_6F29C790`) and the bound fields, all of them
read from the unit-type table at `0x6FAB445C`. The current sim (`place_new_unit`, store then read back) therefore has
**no verified initial write**; section 8.2 still stands.

### 9.3 Stubs that the run depends on (UNSURE)

- Type copy `0x6F29F3E0` answered 1 (block fields not written).
- Lookup `0x6F03FA30` answered with the synthetic unit for every reference (the real lookup may resolve `unit+0xB8`,
  `unit+0x214` to other objects; this matters for which object the axis writers change).
- `vtable` of the unit: slot 46 = real `vf46`, slot 107 = real `vf107`, notify (+0x54) answered, others 0 (pop 0).
  Callees of `vf107` are stubbed with their own `ret imm` pop; `0x6F7E1059` (stack cookie) returns at once.
- Softfloat and math helpers (`0x6F6E0000..0x6F6FFFFF`), the cell setter chain, the normaliser and the clamps run for real.

### 9.4 Walk step (goal 1): not resolved in this pass

- `0x6F497080` (decompiled, `chunk_019.c`) is the **move setup**, not the per-step walk: it resets the mover record
  (`+0x40..+0x5C`, origin from the grid, the step length `min(|*param_5|, local_1c)` and a heading), and calls
  `0x6F4737D0(P, 1)` (the cell setter) only when `|*pfVar4| < DAT_6faae47c` (a near-zero speed case). It does not
  advance a unit per call.
- The per-call writes of the walk are therefore in `0x6F494EE0` / `0x6F496EB0` / `0x6F495320` (not run). No Unicorn
  vectors for the step were produced, so `orders.rs::step_unit` is unchanged and remains a model.

### 9.5 Tests

No sim code changed in this pass. The probe runs once per process; the results above are printed by
`tools/emu/unit_create_emu.py` and are not committed as vectors.
