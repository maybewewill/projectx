# Unit movement, pass 6: the per-walk step is a velocity, positions are evaluated (Game.dll 1.26a)

Status: reverse engineering for a clean-room port (interoperability), 2026-10-11. Written from IDA/Hex-Rays and capstone
disassembly (nothing from them is committed), and **checked by running the real routines in Unicorn**
(`tools/emu/movement6_emu.py`: 30,000 kernel vectors and 4,000 whole walks, 0 mismatches). Image base `0x6F000000`.
Labels: CONFIRMED = read in code **and** reproduced bit for bit in Unicorn; CONFIRMED (read) = read in the disassembly only;
LIKELY = consistent with the code, role inferred; UNSURE = not resolved.

## 0. Result in one paragraph

The 0.03 s movement walk (`0x6F483300` -> `0x6F493410` -> `0x6F490FC0` -> `0x6F4907F0`) **does not move the unit by a
step**. A mover keeps a position in cells, a **velocity `V` in cells per second** and a time snapshot, and the position at
time `t` is evaluated as `cell + V * (t - snapshot)` (this is what `GetUnitX/Y` read, the normaliser of `unit-position.md`).
The walk only **steers**: it re-bases the position at the walk's time and replaces `V` by `speed * (cos h, sin h)` with a
new speed and heading. So the earlier contradiction of `movement-5.md` section 8 (a cap of `speed/32` cells "per walk")
disappears: `U+0x88 = min(speed, 522.67) / 32` is a cap in cells **per second**. `crates/sim` now moves units this way
(`crates/sim/src/mover.rs`, `crates/sim/src/movement.rs`); the placeholder `speed * 0.03` per walk is gone.

## 1. The mover object (U) (CONFIRMED where marked)

| Field | Meaning | Evidence |
|---|---|---|
| `U+0x14` bit 31 | selects clock layer A (`world+0x14`) or B (`world+0x68`) | `0x6F483270` (CONFIRMED read) |
| `U+0x70` / `+0x74` | snapshot of the layer's time (float) and row (int) at the last re-base | `0x6F497440` (Unicorn, `ST`) |
| `U+0x78` / `+0x7C` | position in cells at the snapshot | Unicorn `VS`, `ST` |
| `U+0x80` / `+0x84` | velocity V, cells per second | Unicorn `VS`, `SV` |
| `U+0x88` | speed cap, cells per second (FLT_MAX = none) | Unicorn `CS`, `SK` |
| `U+0x8C` | facing (radians) | Unicorn `SV` |
| `U+0xB0` | arrival radius, cells | `0x6F474D40` (read), Unicorn `WK` |
| `U+0xB4` | acceleration step (cells per second added per walk) | Unicorn `TA`, `WK` |
| `U+0xB8` | turn cap per walk, radians | Unicorn `TA` |
| `U+0xBC` | propulsion window, radians | Unicorn `TA` |
| `U+0xC0` / `+0xC4` | speed the walk set; `wrap(facing - heading)` | Unicorn `SV` |
| `U+0xC8` / `+0xCC` | facing target and angular velocity of the facing smoother `0x6F494200` | read only |
| `U.98+0x40` bit `0x20000000` | set while V is non-zero and the cap positive | Unicorn `VS` |
| record R `+0x18/+0x1C` | goal (cells); `R+0x20` speed; `R+0x24` heading; `R+0x28` flags | read, Unicorn `WK` |

**Clock layers.** A layer is an "Agile" timer queue (`Agile.cpp`, 84 bytes, constructor `0x6F46C590` with the horizon
`300.0` at `0x6FAAE594`): `+0x40` its time (float), `+0x44` a row counter, `+0x48` the horizon. The pump
(`0x6F46E520`) advances the time; at the horizon it re-bases the queue (`0x6F469D40`: row + 1, time to 0). Elapsed time is
`snap(T - T0) + horizon * (row - row0)` (`0x6F483E70`, `crate::heroes::cell_term`). During a timer callback the queue time
is the timer's expiry. `crates/sim` evaluates positions at `Game::game_time` (set to the expiry before every callback) with
rows fixed at zero; the 300 s restart is not modelled (UNSURE: only the rounding of very late times differs).

## 2. The kernels (all CONFIRMED in Unicorn unless noted)

| Routine | Address | What it does | Vectors |
|---|---|---|---|
| set velocity | `0x6F493A40` -> `0x6F4A73B0` | `delta = target - V`; re-base with zero delta; `V += delta` (softfloat) | `VS` 3000 |
| velocity add | `0x6F4A70D0` | `|V|^2 < 2e-7` -> V = 0, flag off; else `len = sqrt(V.x^2+V.y^2)`, `cap < len` -> `V = V * (cap * recip(sqrt))` (`0x6F07B870`); flag = `cap > 0` | `VS` |
| re-base | `0x6F4A6FD0` | `cell = (cell + V*v) + delta`; snapshot = layer (time,row) | `VS`, `ST` 3000 |
| set cap | `0x6F4A73F0` | stores the cap; a cap below `|V|` re-bases and rescales V to the cap (same add as above, delta 0) | `CS` 3000 |
| speed sink | `0x6F4747B0` | cap = `min(speed, mgr+0x80) / 32` (exponent - 5, zero flush) through `0x6F4A73F0` | `SK` 3000 |
| `mgr+0x80` | written by `0x6F46B850` | `32.0 / 0.03 * 0.49 = 522.67` (bits `0x4402AAAB`), both constants from the call site `0x6F01E100` | asserted in `speed_sink_matches_the_original` |
| accel + turn | `0x6F493FA0` | speed `+= U+0xB4` when `!(window <= |err|)` and not forced, else 0; heading `+= clamp(err, +-U+0xB8)` | `TA` 3000 |
| heading of a vector | `0x6F4B2DE0` | `acos(vx/r)`, `2pi - a` when `vy < 0`; 0 under `1e-5` | `HD` 3000 |
| heading error | `0x6F493D90`/`0x6F493D20`/`0x6F496630` | signed `target heading - current` wrapped into `[-pi, pi]`, 0 under `2e-7` | `AD` 3000 |
| angle fold | `0x6F493910`, `0x6F6EFE50`, `0x6F6EF2D0` | `[0, 2pi)`; `fold(x,m) = frac(x * recip|m|) * |m|` with two fix-ups (new port) | `NA` 3000 |
| angle wrap | `0x6F47C420` | `x - trunc(x/2pi)*2pi` (and `- 2pi` for negative x): used by the turn and window setters | `WP` 3000 |
| velocity writer | `0x6F494160` (+ `0x6F10E370`) | stores speed and `wrap(facing - heading)`; speed > 0: `V = speed*(cos h, sin h)`; else `V = 0` and `facing = fold(heading)` | `SV` 3000 |
| **the whole walk** | `0x6F4907F0` + `0x6F495320` + `0x6F493010`/`0x6F492EC0` | see section 3 (the real code ran with the path follower stubbed) | `WK` 4000 |

## 3. The walk (CONFIRMED, `WK` 4000 walks: 271 arrivals, 630 blocked, 1381 in motion afterwards)

For one record R of unit U at layer time T (solo record, no group effects):

1. `pos = cell + V * v` (`0x6F497460`), `s = sqrt(V.y^2 + V.x^2)`, `R.d = s` (0 when `|s| < 4e-7`).
2. `R.h = facing` when `R.d == 0`, else the heading of V (`0x6F4B2DE0`).
3. Arrival test (`0x6F493EA0`) against the **goal** `R+0x18`: `within = (U+0xDA bit 0) or radius >= dist`;
   `arrived = within and |heading error| <= 0.2` (the error is `0x6F493D20` of `R.h` against `goal - pos`).
   - arrived: `R.d = 0`.
   - within, not aligned: turn in place: `R.d = 0`, `R.h += clamp(err, +-turn cap)`.
   - otherwise the follower (`0x6F49E020`) answers 0 (open: steer toward its **waypoint**) or non-zero (blocked: speed 0,
     heading still turns toward the goal): `R.d += accel` when the error is inside the window, else 0; `R.h += clamp(err)`.
4. `R.d = min(R.d, U+0x88)`; the group limit is the minimum cap of the records (`0x6F4900A0`), so a solo record has
   `limit = U+0x88`; `speed = limit > R.d ? R.d : limit`.
5. `0x6F494160`: `V = speed * (cos h, sin h)` as a **delta against the old V**, re-based at T (kernel `SV`).

So a unit at rest accelerates to its cap in one walk (`U+0xB4` is set to the cap by the move issue) if it faces the target
within the window; otherwise it turns in place by at most `U+0xB8` per walk (0.03 s) until it is inside the window.

### Move issue (`0x6F474D40`, CONFIRMED by disassembly; the arithmetic is the kernels above)

`cap = B4 = div32(min(speed, 522.67))` (through `0x6F4A73F0`), the stop sequence `0x6F4956C0` (V = 0, speed fields 0,
path object invalidated: `P.88 |= 0x100000`), `U+0xB0 = max(0.49, div32(radius))`, and the goal pair into the record
(`0x6F48F790`). `SetUnitMoveSpeed` goes through the Amov chain to the same sink `0x6F4747B0` and changes only the cap.
`SetUnitTurnSpeed` (`0x6F474B30`): `U+0xB8 = max(0.001, wrap(x))`; `SetUnitPropWindow` (`0x6F474BA0`): `U+0xBC = wrap(x)`; the
natives pass the JASS real unchanged (radians; `Blizzard.j` converts degrees).

## 4. What changed in `crates/sim`

- `mover.rs`: the kernels above (`Kin`, `steer`, `heading_of`, `heading_error`, `normalise_angle`, `fold`, `wrap_positive`,
  `speed_bound`, `speed_to_cap`, `arrival_radius`, `turn_cap`, `prop_window`) and `walk`.
- `movement.rs`: `MoverRt` per unit, `Game::game_time`/`set_game_time`, the clock layer, `mover_issue`, `mover_halt`,
  `mover_speed_changed`, the cached position refresh (`UnitObj::x/y` of a unit in motion follow the game time).
- `orders.rs`: `step_unit` runs `mover::walk` (waypoint advance inside 0.49 cells, LIKELY); a route start issues the mover
  (zero V, cap), a dropped route / completed order / dead unit halts it. `store_unit_position` re-bases a moving unit with
  `delta = P - F` (the original setter, vectors `ST`).
- `script.rs`: the pump sets the game time to the expiry of each callback and to the tick time when it ends.
- `engine/src/data.rs`: the unit type's `propWin` (degrees in the SLK) is converted to radians (LIKELY, section 6).
- Tests: `crates/sim/tests/mover_vectors.rs` (10 tests; embedded samples of the table-free kernels only, full sets with
  `MOVER_VECTORS=<dir>` and `GAME_DLL`: no output of the game's lookup tables is committed), `order_natives` golden positions re-pinned (explained there), the fixtures of `order_natives`,
  `combat_natives` and `engine/tests/pathing` now give the footman its real turn rate (0.6) and window (60 degrees): **a
  unit with a zero propulsion window never accelerates** (window `> |err|` is strict), which the old placeholder did not
  model.

Observable differences from the placeholder: positions advance on every tick (not at the walks); a unit stops inside 0.49
cells (15.7 world units) of the goal once aligned (the old model 6.4); a move due east drifts in y by about 0.01 world
units because the heading of V comes from the softfloat arccosine near 1 (3.4e-4 rad) and the steering corrects it back
(real engine behaviour as far as the kernels go); a unit that is not facing the goal turns in place first.

## 5. Corrections to earlier notes

- `movement-5.md` 8 (the contradiction) and `movement-4.md`: `U+0x80/0x84` is **velocity in cells per second**, `U+0x88`
  the cap in cells per second; the walk period 0.03 is not a multiplier anywhere in the chain, consistently, because the
  position is evaluated against the clock. The `522.67` bound is `32 / 0.03 * 0.49`: the speed at which a walk moves 0.49
  cells.
- `movement.md` 4.2 / `movement-2.md` 5: `0x6F493FA0` is the **acceleration and turn limiter** (`R.d += U+0xB4`,
  `R.h += clamp(err, U+0xB8)`), not a plain clamp; the turn is gradual (`U+0xB8` radians per walk).
- `movement.md`: the arrival radius is `U+0xB0 = max(0.49, radius/32)` cells; `0.2` is the heading tolerance (radians) of the
  arrival test, `0.49^2 = 0.2401` the squared radius for passing a path waypoint.
- `unit-position.md` 6 item 3: the step vector is the velocity; its writers are `0x6F4A70D0` (add + clamp) via
  `0x6F4A73B0`, called by the walk (`0x6F494160`), the stop (`0x6F4956C0`) and `0x6F4A73F0`.
- The cell setter is **not** lossless for a unit in motion: `F = cell + V * v` enters `delta` (`ST` vectors).

## 6. Open (UNSURE) and found on the way

1. **Facing of a unit in motion.** `U+0x8C` changes only when the unit stops (`0x6F494160`) and in the facing smoother
   `0x6F494200` (per class table `0x6FAB7440`: acceleration `T0`, cap `T1`, a pow shaping `T2/T3`; threshold 0.001; works on
   `U+0xC8`/`+0xCC`). Its target `U+0xC8` writer is not found. The sim sets `facing = heading` while moving.
2. **Path follower** (`0x6F49E020`/`0x6F49DAC0`/`0x6F49DF00`/`0x6F49C3B0`): the state machine (return codes 0/1/3/4/`0x100000`,
   wait counter at least 20 on a blocked path, waypoint pop when `dist^2 < 0.2401`) is only modelled as "open, next waypoint
   at 0.49 cells".
3. **Groups.** Cluster speed matching (`0x6F4900A0`: minimum cap), the leader slow-down (flag `0x800`), the `0x200`/`0x20000`
   flags and `U+0xB0` override are not modelled (solo records only).
4. **Unit push-apart found**: `0x6F4961C0` (called from `0x6F483300`, every second walk for alternating records) applies a
   separation vector (`R+0x18/0x1C`) through `0x6F494EE0` -> `0x6F4737D0`. Not modelled.
5. **Layer rollover** at 300 s (section 1), and whether the layers A and B have the same time (assumed).
6. **Propulsion window units.** SetUnitPropWindow takes radians; the type value (`propWin` 60) is assumed converted to radians
   at load (the conversion site is not located); `UnitObj::prop_window` is radians in the sim (engine data converts).
7. The radius argument of the move issue (`a9`) is 0 in the sim (arrival radius 0.49 cells); what the original passes for a
   plain move order is not traced. The speed given to the speed sink is the raw `move_speed`; the original passes the
   effective speed clamped by `0x6F2019F0` (1..522).
8. `Position::vf6`'s owner U and the unit's `Amov` ability are assumed identical (`movement-5.md`).
9. Dynamic confirmation on the running original was **not** possible here: the available replays are not valid in-game files
   (the game refuses them), so no live trace was taken; the checks are Unicorn runs of the real code.

## 7. Reproducing

```
GAME_DLL=C:\path\Game.dll python tools/emu/movement6_emu.py KIND OUT.txt 3000 1   # KIND = VS CS TA AD HD SV NA WP SK ST WK
MOVER_VECTORS=<dir with vs.txt cs.txt ta.txt ad.txt hd.txt sv.txt na.txt wp.txt sk.txt st.txt wk.txt> GAME_DLL=C:\path\Game.dll \
  cargo test -p sim --test mover_vectors -- --nocapture
```

`WK` runs the real `0x6F4907F0` and `0x6F493010` on a synthetic group/record/unit with three callees answered by the harness
(`0x6F49E020` follower, `0x6F49B470` path validity, `0x6F49B5A0` release) and the notification of vtable slot `0x50` stubbed.
