# Movement 8: the follower's straight-line check and detour draw (R6)

Ported and checked in Unicorn (`tools/emu/follow_emu.py`, vectors `crates/sim/tests/data/follow_w.txt` 500 cases,
`follow_a.txt` 600; 6,000-case runs also 0 mismatches): `crates/sim/src/follow.rs`.

* Walk step `0x6F49DF00` (reach test `0x6F49C3B0`, not-reached branch), straight-line check `0x6F49D8D0`: the direction to
  the waypoint is shortened to one cell (`0x6F49B050`), `floor(pos+step)` is the end cell, `0x6F49ABB0` gives a direction
  code, and the trace of the footprint class (`0x6F38ADA0`: radius <0.5/<1/<1.5/else = 0..3) visits cells
  (`0x6F4A2070/2180/2360/2550` over `0x6F4A1EA0/1FA0/1FE0/2020`). A cell is a null entry when outside the grid or when
  `mask & word` has a top-byte bit; an occupant (kind 1, node flag bit 24, stamp not dead/equal, `node+0x40 & 0x8FFFFFFF == 0`,
  `mask & flags & 0xFFFFFF != 0`) adds its mover if the mover tag `0x60706375` matches, else a null. The walker's own node
  (`P+0xA0`) has `+0x40` raised for the trace. The list holds at most 32.
* Blocker scan `0x6F49D770`: skips null, invalid units, movers with speed^2 == 0 and movers already waiting for a live mover;
  marks the first other mover as `P+0xA8/0xAC` with wait 4 unless it is slower and equal-class (then it is told to wait 20 for us).
* Blocked branch `0x6F49DE90`: only when no live blocker and wait == 0; `0x6F49B340` draws `7 + ((step >> 22) & 1)` from the
  global generator when `dist2(P+0x24, pos) > 144.0` or `unit+0x38 > 1`, else 2; `0x6F49DBE0(0,0,0,0)` empties the waypoint
  array and sets the index to -1; the retry counter drops by one; result 1. Next walks: the region/cell re-request is rate limited by
  `0x6F49ACA0` (10 walks, `request_gate`, Unicorn 600 cases).
* Footprint nodes (`0x6F4A7570`: node A `U+0x94` push, node B `U+0x98` path with bit 24; rebased by `0x6F4A6FD0` -> `0x6F4A6D70`
  and `0x6F4A6E40`): node B box = floor(pos - lo) .. floor(pos + hi) + 1 with (lo,hi) = (0,0),(1,0),(1,1),(2,1) by class. `U+0x90`
  = max(collision-like value, 1.0)/32 (`0x6F4766A0`).

Sim wiring (`follow_game.rs`, `orders.rs::step_unit`) is **opt-in** (`SIM_FOLLOW=1` in `engine::pathing::install`) because it overdraws:
sim cumulative draws at records 2094 / 5497 are 527 / 8013 (flags 0x01000006) against real 348 / 1315 and baseline 320 / 923.
Causes (UNSURE): the detour search `0x6F49BB90 -> 0x6F4A3CC0` (cell A*, `0x6F4A3AD0`, `0x6F4A2EA0`, `0x6F4A2E00`, `0x6F4A2F30`) is not
ported (a BFS stand-in did not help), node flags low bits and `node+0x40` of units are unknown (with flags 0x01000000 units never
block: 1014 / 1203), static structures are absent from the grid, the request token `P+0xA4` is not modelled, `unit+0x38` is assumed > 1.
