# Unit movement, pass 7: facing, separation, groups, the path follower (Game.dll 1.26a)

Status: reverse engineering for a clean-room port (interoperability), 2026-10-12. Written from IDA/Hex-Rays and capstone
disassembly (nothing from them is committed) and **checked by running the real routines in Unicorn**
(`tools/emu/movement7_emu.py`, harness of `movement6_emu.py`): 24,000 vectors in total, 0 mismatches. Image base
`0x6F000000`. Labels as in `movement-6.md`: CONFIRMED = read in code **and** reproduced bit for bit in Unicorn;
CONFIRMED (read) = read in the disassembly only; LIKELY = consistent with the code, role inferred; UNSURE = not resolved.

## 0. Result

| Item | Routine | Result | Unicorn |
|---|---|---|---|
| facing of a unit in motion (closes `movement-6.md` 6.1) | velocity notification `0x6F494620` -> `0x6F4A74A0` | `U+0x8C` = heading of `V`, set by the speed writer's notify flag | `SN` 3000, `WN` 4000 (whole walk) |
| facing smoother (`U+0xC8/+0xCC`) | `0x6F494200`, finish `0x6F4954F0` | decoded, ported | `FS` 4000 |
| `SetUnitFacing` | `0x6F3C5AF0` -> `0x6F474830` -> `0x6F4945F0` -> `0x6F4A7480` | folds the angle into `[0, 2*pi)`, joins the smoother list | `NA` of pass 6 |
| path follower waypoint test | `0x6F49C3B0` | reached = `dist2(wp, pos) <= 0.49*0.49` (tie counts), result codes 0/2/4 | `WR` 3000 (257 exact ties) |
| path follower state machine | `0x6F49E020` and callees | return codes and counters read, not ported | - |
| push-apart record | `0x6F4961C0` (+ `0x6F494EE0`, `0x6F494DE0`, `0x6F4B7FE0`) | decoded, ported; neighbour gather stays a host service | `PA` 3000 (195 RNG draws) |
| group speed limit and gate | `0x6F4900A0`, `0x6F490C30` | decoded, ported, not wired (no cluster model in the sim) | `GL` 3000 |

`crates/sim` changes: section 7. Tests: `crates/sim/tests/movement7_vectors.rs` (8 tests), 2 tests in
`crates/sim/tests/order_natives.rs`.

## 1. The velocity notification: the facing of a unit in motion (CONFIRMED)

`0x6F4A70D0` (velocity add, `movement-6.md` section 2) ends with `if (notify) vtable[+0x50](U)`. The slot is `0x6F494620`:
it calls `0x6F4A74A0` and links the mover into the world's smoother list (section 2) when `U+4` and `U+8` are both zero.
`0x6F4A74A0` reads the velocity after the add/clamp:

1. `s2 = add(Vx*Vx, Vy*Vy)`; `d = sub(s2, 0.0)`;
2. if `|d| < 4.0e-7` (`0x6FAAE618`) nothing happens; otherwise (also NaN)
3. `U+0x8C = heading(V)`: `len = sqrt(s2)` and the heading routine `0x6F4B2DE0` (`movement-6.md`: `acos(vx/len)`, mirrored to
   `2*pi - a` for `vy < 0`; zero under `1e-5`), through `0x6F1D2AB0`.

Who passes the notify flag (`a3`):

| Caller | flag |
|---|---|
| speed writer `0x6F494160` -> `0x6F493A40` (both its paths, so every walk) | 1 |
| stop sequence `0x6F4956C0` (`V = 0`) | 1 |
| cap setter `0x6F4A73F0` -> `0x6F4A73B0` | 0 |

So while a unit walks, `GetUnitFacing` (which reads `U+0x8C` through `Position` vtable slot 7 `0x6F474810`, times 180/pi) is the
heading of the velocity **as the softfloat arccosine gives it** (a move due east reads about 0.005 rad off, as `movement-6.md`
section 4 found). On the stop path (`speed <= 0`) the walk sets `facing = fold(heading)` after the notification, which is
the `U+0x8C` of `movement-6.md`. `GetUnitFacing` does **not** read `U+0xC8`.

## 2. The facing smoother `0x6F494200` (CONFIRMED, `FS` 4000 vectors)

World list `world+0x438` (head `world+0x440`, intrusive links `U+4`/`U+8`): the movement walk `0x6F483300` calls the smoother
for every mover in it **after** the walks of all clusters. Members join through the notification (section 1), `SetUnitFacing`
(`0x6F4945F0`), the walk head `0x6F495320` and `0x6F4940C0`; the constructor `0x6F494650` leaves them out and sets
`U+0xC8 = U+0xCC = 0`, `U+0xD8 = 0`. Fields: `U+0xC8` smoothed angle, `U+0xCC` angular velocity, class row
`(U+0xD8 >> 8) & 0xF` (the writer of those bits is not found; the constructor zero is used).

One step, with `err = wrap(U+0x8C - U+0xC8)` (the signed wrapped difference of `0x6F496630`), `T0..T3` the class row:

1. `|err| < 0.001`: finish (vtable `+0x58` = `0x6F4954F0`): `U+0xC8 = U+0x8C`, `U+0xCC = 0`, unlink. Nothing else.
2. `rate = add(rate, sign(err) * T0)`; `mag = |rate|`.
3. `m = (T1 > |err|) ? |err| : T1`; `q = (T2 > |err|) ? div(|err|, T2) : 1.0`, clamped to `[0, 1]`; if `q != 1.0` then
   `m = mul(m, pow(q, T3))`.
4. `if m < mag then mag = m`; `rate = sign(rate) * mag` (sign taken after step 2).
5. `U+0xCC = rate`; `U+0xC8 = fold2pi(add(U+0xC8, rate))` (`0x6F493910`).

Class rows (`0x6FAB7440`, built at start-up by `0x6F85FA40` from decimal literals through `S2R`; rows 9..15 repeat row 0):

```
row  T0 (accel)  T1 (cap)  T2 (window)  T3 (shape)      row  T0     T1    T2   T3
 0   0.07        0.2       0.2          0.25             5   0.05   0.18  1.2  0.6
 1   0.03        0.1       0.2          0.25             6   0.1    0.3   0.8  0.4
 2   0.015       0.4       0.8          0.25             7   0.003  0.08  1.2  0.5
 3   0.005       0.1       1.2          0.5              8   0.001  0.05  1.4  0.5
 4   0.04        0.15      1.2          0.6
```

The Rust side keeps the decimal literals (`mover::FACING_TEXT`) and their `S2R` values (`mover::FACING_BITS`); a test parses the
text with the game's tables and compares (the `0.1` of row 1 is `0x3DCCCCCE`, the soft divide's value).

**Role (LIKELY).** `U+0xC8/+0xCC` are in the state fold `0x6F4946D0` (it mixes `+0x9C, +0xA0, +0xB0, +0xBC, +0xB4, +0xB8,
+0xC0..+0xD4` byte-wise: `x ^ (31277 * x)` per byte), so they are simulation state for the checksum, but nothing in the
gameplay path reads them (the attack facing gate `0x6F476580` goes through `0x6F4A7230`, not this field; UNSURE for the
renderer). `crates/sim` keeps them in `MoverRt` and runs the smoother every walk.

## 3. `SetUnitFacing` (CONFIRMED by disassembly; the fold is the checked `NA` kernel)

`0x6F3C5AF0`: `rad = mul(degrees, 0x6FAAE5E8 = pi/180)`, `Position::set` (`0x6F474830`): vtable `+0x4C` = `0x6F4945F0` ->
`0x6F4A7480` (`U+0x8C = fold2pi(rad)`) and the list link; its second argument (immediate) is 0 from JASS, so the finish
routine does not run. The sim stored the raw angle before: `SetUnitFacing(450)` read back 450; now 90 (fixed, test in
`order_natives.rs`).

## 4. The path follower (read; partly ported)

`U+0xA8` is the follower object `P` (`0x6F49E020` is its step, called by the walk head `0x6F495320`). Fields (CONFIRMED read):
`P+0x40` pointer to the waypoint array (8-byte cell pairs), `P+0x74` index of the waypoint being walked to (the array is
consumed **downward**), `P+0x78` stored count, `P+0x7C`/`+0x80` growth, `P+0x88` flags (`0x100000` blocked/wait, `0x200000`
use the pathing request, `0x2000000`, `0x20000000`), `P+0x94` wait counter, `P+0x98` retry counter, `P+0xB4` size class
(`0x6F38ADA0`), `P+0xA4` token compared by the walk.

`0x6F49E020(pos, target_copy, U)` returns: `0x100000` when `P+0x88 & 0x100000` is set (waiting); `1` while `P+0x94 != 0`
(it decrements the counter first); otherwise it publishes the size class and the world frame to the search globals
(`0x6F49B8E0`), calls the re-plan step `0x6F49DAC0`, and, if that returns 0, the walk step `0x6F49DF00`.

* **Waypoint reached (`0x6F49C3B0`, CONFIRMED, `WR`)**: `d2 = dist2(wp, pos)` (the fused `0x6F6EF3B0`, `(pos-wp)^2` summed),
  `c = mul(0.49, 0.49)` (`flt_6FAAE4CC`, soft multiply). Not reached when `c < d2` **or the compare is unordered**
  (`test ah,1`): the blocked state is cleared (`P+0x88 &= ~0x100000`, `P+0x94 = P+0x98 = 0`), result 0. Reached (so a tie
  counts, which the earlier sim model had as "not reached"): if `P+0x74 != 0` the same clear, then `0x6F49C380` picks the next
  waypoint (index from the skip scan `0x6F49BEE0`), result 0; else if `P+0x78 != 0` the re-plan step `0x6F49C2E0(1, pos)` runs,
  `P+0x94 = max(P+0x94, 20)` when it fails, `P+0x74 = 0xFFFFFFFF`, result 2; else result 4 (path finished).
* **Skip scan `0x6F49BEE0` (CONFIRMED read, semantics LIKELY)**: from index `P+0x74 - 1` downward it asks `0x6F49BD10` (a pathing
  cells walk between two points, size-class dependent) whether the waypoint is directly reachable and stops at the first that
  is not. So the follower **smooths the route at follow time**; the planner's path (`pathing-2.md`) has none.
* **Walk step `0x6F49DF00`**: `0x6F49C930` (current waypoint, fetching it from the array), `0x6F49C3B0` (above), then the
  straight-line check `0x6F49D8D0` (a size-class dependent search `0x6F4A2070/2180/2360/2550`, i.e. pathing levels 0..3) and, when
  blocked, `0x6F49DE90`: result 3 if `0x6F49C460` says blocked, else with `P+0x98 == 0` it asks `0x6F49B340` for a detour,
  `P+0x98 == 1` ends with 4, otherwise `0x6F49DBE0` and `P+0x98 -= 1`, result 1 (retry).
* **Re-plan (`0x6F49DAC0`)**: when the stored segment list is exhausted (`P+0x78 < P+0x70`) and flag `0x200000` is set it calls
  the pathing request `0x6F49B9C0` (the dispatcher `0x6F49A800` of `pathing-3.md`), result 2 when that fails; the request is
  gated by `0x6F49ACA0(1, world+0x538)`, a per-walk counter (`world+0x538` increments every walk and restarts at 1024 when it
  wraps): a request budget per walk (LIKELY). A failed re-plan leaves a wait of at least 20 walks (0.6 s).
* **Trailing nodes (`0x6F49C100`)**: from the end of the node list it drops nodes closer than `flt_6FAB7760` and stops at a
  flag node (`-128000.0`, `pathing-3.md`); result is the index of the first kept node.

Not ported: the follower object's arrays, `0x6F49BD10`, the search-level walkers and the budget need the pathing
request context (heap objects, the pyramid); only the reached test and the codes are in `crates/sim`
(`mover::follower_reach`, `mover::waypoint_reached`). The sim's route (`orders.rs`) still advances through waypoints that are
inside the radius and has no follow-time smoothing, no 20-walk wait and no retry counter.

## 5. Separation: the push-apart record `0x6F4961C0` (CONFIRMED except the gather; `PA` 3000 vectors)

The walk `0x6F483300` calls it for the records of the list `world+0x51C` (`R` = 44-byte record, `R+0x14` = the mover `U`),
**every second record, alternating**: the counter `world+0x53C` toggles 0/1 each walk, and `if (!result--) { call; result = 1 }`
processes records 0, 2, 4, ... on one walk and 1, 3, 5, ... on the next, so each record runs every second walk (0.06 s). The
second argument is the group object `world+0x540` that holds the neighbour list (array of 8-byte entries at `+0x0C`, count at
`+0x1C`; each entry's first dword is a node whose `+0x30` is the neighbour's mover).

Record fields: `R+0x18/+0x1C` separation vector of the last round, `R+0x20` flags: bits 0..15 cooldown, 16..19 table row, 20..27
and 28..31 two filter values handed to the gather.

Per call (`rec.flags`, `U+0xC0` = the speed the last walk set):

1. **Cooldown** (`0x6F494DE0`): if the low 16 bits are non-zero they are decremented and the call ends.
2. **Moving**: if `0 < U+0xC0` (a unit that walks): `R.vec = 0`, cooldown := 7 (`0x6F494FA0`), end.
3. **Apply the previous vector** (`0x6F494EE0`): if `add(vx*vx, vy*vy) != 0`: `target = pos + vec` (pos = `cell + V*v` as in
   `Kin::position`); if the placement test `0x6F494E10(R, target)` accepts (size-class / pathing check, not decoded) the position
   setter `0x6F4737D0(U, target, 1)` moves the unit there.
4. Row `T0..T4` of the table `0x6FAB75F8` (index `(R+0x22) & 0xF`, built by `0x6F860070` through `S2R`):
   `0: 5.0, 0.01, 0.5, 0.4, 0.7`; `1: 7.0 (0x40E00001), 0.01, 0.2, 0.4, 0.7`; `2: 8.0, ...`; `3: 9.0, ...`; `4: 10.0, ...`
   (rows 5..15 are all `0.0`, which divides by zero below: LIKELY never selected). `inv = div(1.0, T0)`.
5. Gather: `0x6F495D60(U, row, (flags >> 20) & 0xFF, flags >> 28)` + `0x6F496170` fill the neighbour list from the pathing-cell
   occupant lists of the rectangle `pos +- T0` (cells `floor` of the corners, end exclusive, walked column-major: outer loop over
   the first coordinate, `index = second + first * stride`; within a cell head to tail). A unit is taken once (stamp `unit+0x38`)
   if it passes `0x6F493B60`: node flags `(node+0x34 & 0xFF000000) == 0`, not the query's own unit, a class tag at `unit+0x10`,
   `unit+0x14 >= 0`, `0.0 < unit+0x90`, an approximate-equality test of `0.0` (`0x6F009AE0`, operand not resolved) and the unit's
   record `rec = unit+0xAC`: `(rec+0x20 >> 20) & 0xFF == filter byte` and `(rec+0x20 >> 28) & 0xF >= filter nibble`.
   The neighbour order matters (the sum below is not associative in softfloat). **Not ported**: the sim has no pathing-cell
   occupant lists, the record-to-unit-type filter bytes and the occupant order are unknown; `mover::PushHost::gather` is the seam.
6. For every neighbour `n` in order: `d = (pos.x - n.x, pos.y - n.y)`, `dist = sqrt(add(dx*dx, dy*dy))`;
   if `dist < 0.001` (`flt_6FAAE47C`) the direction is **random**: `0x6F4B7FE0` takes one step of the **global generator**
   (`rng::Rng::random_fraction`, the same `(low23 | 1.0) - 1.0` as `GetRandomReal`), `angle = mul(u, 2*pi)`,
   `d = (cos angle, sin angle)` (CONFIRMED order: x = cos, y = sin; 195 draws in the vectors) and `dist` is recomputed from it.
   If `T0 > dist`: `b = 1.0 - dist*inv`, `len = (T3 * b) * b`, `R.vec += scale_to(d, len)` where `scale_to` is `0x6F07B870`
   (`x * (len * recip(sqrt(x*x + y*y)))`).
7. `l = sqrt(add(R.x*R.x, R.y*R.y))`, `v = l * T4`. If `T1 > v`: `R.vec = 0`, cooldown := 7. Otherwise `v = min(v, T2)` and
   `R.vec = scale_to(R.vec, v)` (no cooldown change).

So two standing units on the same spot draw from the **global** random generator, which a replay-exact simulation must
reproduce (the draw is made in record order, every second walk); a unit that never overlaps does not draw.
`crates/sim` has the kernel (`mover::push_apart`) and the table rows but **does not run it**: wiring it needs the occupant lists
and the filter data of item 5 (a wrong gather would move units that the original leaves alone).

## 6. Groups (CONFIRMED kernels, not wired)

A movement group is a cluster of up to 12 records (`world+0x3B0` list, `movement-3.md`; a new cluster is linked at the head,
`0x6F4924D0`, whose constructor sets leader fields `+0x1C = 0.0`, `+0x24/+0x28 = FLT_MAX`). The 44-byte record array is at
`G+0x28`, the count at `G+0x38`. In `0x6F493010` the walk computes one limit per group and hands it to every record's speed
writer `0x6F492EC0`:

* **Gate `0x6F490C30` (CONFIRMED, `GL`)**: matching is off when `G+0x80 & 8`, or when any record has flags `R+0x28 & 0x210000`
  or its unit has `U+0xDB & 1`. Off means the limit is `FLT_MAX`, so each record keeps its own cap.
* **Limit `0x6F4900A0` (CONFIRMED, `GL`)**: the smallest `U+0x88` (cap, cells/s) over the records without `R+0x28 & 0x200000`,
  starting at `FLT_MAX`. A leader object (`G+0x7C`) records the minimum (`leader+0x24 = min(leader+0x24, limit)`) and, when its own
  `leader+0x20` is not `FLT_MAX`, that value replaces the result.
* **Per record `0x6F492EC0` (CONFIRMED read)**: `limit' = min(limit, R+0x20 speed field)`; with group flag `0x800` (`G+0x80`), `G+0x6C == 0` and
  `limit' > 0` it finds the leader record (`0x6F4924C0`), and if `limit' > leader.cap` it measures the distance to the leader
  (`dist2`, `0x6F38B140`, `0x6F020100`) and, when the unit is closer than a threshold (`unk_6FAB740C` factor of the leader's
  cap), replaces the limit by `leader.cap * unk_6FAB7410`: a leader slow-down (values not read). The result goes to the walk's
  speed writer `0x6F494160` with the facing of the record.
* In the walk `0x6F4907F0`: group flag `+0x6C` temporarily sets `U+0xB0` (arrival radius) to `0.49`; group flag `0x200` sets
  `U+0xB8` (turn cap) to the group's `+0x74` while it is not `FLT_MAX`; both are restored after the record; record flag
  `0x200000` selects the follow/formation step `0x6F4940C0` instead of `0x6F495320`.

The sim has no cluster concept: `Group*Order` natives issue orders unit by unit. The 12-unit cluster is very likely what the
"first 12 members" limit of the group order natives (`AGENTS.md` 4.3q) is; once clusters exist, `mover::group_limit` and
`mover::group_gate` give the speed matching. Not decoded: when records join a cluster, the leader choice, the formation step.

## 7. What changed in `crates/sim`

* `mover.rs`: `Kin::notify_facing`, `Kin::set_speed_heading_notify`, `FacingClass`/`facing_class`/`smooth_facing`/`FacingStep`,
  `Follower`/`follower_reach`/`waypoint_reached`, `GroupMember`/`GroupLeader`/`group_limit`/`group_gate`,
  `PushRow`/`push_row`/`PushRecord`/`PushHost`/`push_apart`/`scale_to`/`direction_of`. Lane H's kernels are untouched.
* `movement.rs`: `MoverRt` gains the smoother state (`face_smooth`, `face_rate`, `listed`, `face_class`); `Game::set_unit_facing`,
  `Game::facing_smoother`, `tick_facing_smoothers`; `mover_issue`/`mover_halt` list the mover (the stop notification).
* `orders.rs`: the walk is followed by `notify_facing` (the facing of a unit in motion is the heading of its velocity; the
  `kin.facing = trace.heading` stand-in is gone), the smoother pass runs after every movement walk, and the intermediate waypoint
  test is `waypoint_reached` (fused distance, inclusive).
* `natives/unit.rs`: `SetUnitFacing` goes through `Game::set_unit_facing`.
* Observable: `GetUnitFacing` of a walking unit is the velocity heading (not the record heading), `SetUnitFacing` folds the angle,
  a waypoint exactly on the 0.49 radius counts as reached.

## 8. Open

1. The writer of `U+0xD8` bits 8..11 (facing class row), the clock `U+0xC8` initial value for units created by a map script.
2. Follower: array growth, `0x6F49BD10`/`0x6F49B340`/`0x6F49DBE0`/`0x6F49C460`, the request budget `0x6F49ACA0`, flag meanings of
   `P+0x88`; wiring the 20-walk wait and the retry counter into the sim route.
3. Separation: the occupant lists of the pathing cells (order within a cell), the filter bytes of the unit type record, the
   placement test `0x6F494E10`, how records are created for units, the row selection (`R+0x22`).
4. Groups: cluster formation, leader choice, the leader slow-down thresholds `0x6FAB740C/0x6FAB7410`, the formation step.
5. Dynamic confirmation on the running original is still not possible (no valid replay in the environment), as in `movement-6.md`.

## 9. Reproducing

```
GAME_DLL=C:\path\Game.dll python tools/emu/movement7_emu.py KIND OUT.txt 3000 1     # KIND = SN FS PA WN WR GL
MOVER7_VECTORS=<dir with sn.txt fs.txt pa.txt wn.txt wr.txt gl.txt> GAME_DLL=C:\path\Game.dll \
  cargo test -p sim --test movement7_vectors -- --nocapture
```

`FS`, `WR` and `GL` have small embedded samples that touch no lookup table (`crates/sim/tests/data/mover7_*.txt`, only table-free
cases); `SN`, `WN` and `PA` run only with `GAME_DLL`. `PA` runs the real `0x6F4961C0` with the gather (`0x6F495D60`, `0x6F496170`),
the placement test (`0x6F494E10`) and the position setter (`0x6F4737D0`) answered by the harness; `WR` answers `0x6F49C380` and
`0x6F49C2E0`; the real notification (`0x6F494620`) and finish routine (`0x6F4954F0`) run in `SN`, `WN` and `FS`.
