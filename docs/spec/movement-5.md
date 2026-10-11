# Unit movement, pass 5: the step vector, the per-walk cap and the speed sink (Game.dll 1.26a)

Status: reverse engineering for a clean-room port (interoperability), 2026-10-11. Written from disassembly (capstone,
scripts in the scratchpad, not committed), from the decompile export (scratchpad, not committed) and from the constants
dumped at start-up (`runtime_consts.tsv`). Image base 0x6F000000. Labels: CONFIRMED = read from the code or a constant;
LIKELY = consistent with the code, role inferred; UNSURE = not resolved.

Question: how far does a unit move per walk (every 0.03 s of agile clock)? Result: **not determined**. The chain that
delivers the speed to the walk is now mapped end to end, and the arithmetic of two of its steps is confirmed (one of
them is a divide by 32). But one operand (the value of `mgr+0x80`, the upper bound in the speed sink) and the rule that
produces the per-walk step vector are not known, so no formula is confirmed. `crates/sim/src/orders.rs` is **not**
changed by this pass.

Terms used below (CONFIRMED meaning of the offsets, see section 2):
- **U** = the unit object (`CUnit`). The walk record `R` (44 bytes, in a cluster) has its owner at `R+0x14` = U.
- **Z** = the mover/position object returned by `U.vtable[0xB8]`. Its class is the one with `Position::vf6`.
- **mgr** = the object at `[0x6FAB7368]`. Its `+0x80` is the upper bound used by the speed sink. Its value is not known.

---

## 1. Summary of the chain (speed to walk)

```
SetUnitMoveSpeed 0x6F3C5C40
  -> 0x6F26DE00 (unit+0x1EC = Amov ability; return if none)
  -> 0x6F202F30 (holder = Amov+0x6C; holder.vt[0](speed, 1))
  -> 0x6F201D50 (Amov):
        eff = 0x6F201C60 (effective speed, clamped, section 3)
        Z   = owner.vt[0xB8]()          owner = Amov+0x30 (or 0x6F472890)
        Z.vt[0x18](&eff)                = Position::vf6 0x6F4747B0   (section 4)
  -> U.+0x88 := min(eff, mgr+0x80) / 32       (CONFIRMED arithmetic, LIKELY owner U, section 4)

walk (every 0.03 s; section 5) for each record R with owner U:
  V = (U.+0x80, U.+0x84)                  step vector, written elsewhere (section 6)
  d = |V|                                  0x6F4907F0 (CONFIRMED)
  R.+0x20 := d (0.0 if |d| < 4e-7)         CONFIRMED
  R.+0x24 := heading of V (or U.+0x8C if d == 0)     CONFIRMED
  path step 0x6F495320 (writes R fields; section 7)
  if R.+0x20 > U.+0x88 then R.+0x20 := U.+0x88       CONFIRMED (disasm 0x6F490990-0x6F4909AD)
```

So the walk distance is **min(|V|, U.+0x88)**, both in cells. CONFIRMED.
What is open: how V is made, and whether `U.+0x88` (the cap) is a per-walk amount. Section 8 explains why both matter.

---

## 2. Field meanings (CONFIRMED, from the stores and loads in the code)

| Object | Field | Meaning | Evidence |
|---|---|---|---|
| U | +0x78, +0x7C | position in cells (x, y) | 0x6F4A6FD0 and 0x6F474D40 store there; groups-events-2 agrees |
| U | +0x74 | a float set by the move routine (store CONFIRMED, meaning UNSURE; see section 7) | 0x6F497080 `mov [esi+0x74], eax` |
| U | +0x80, +0x84 | step vector V (x, y), cells | written by 0x6F4A70D0 (section 6); read by 0x6F4907F0 |
| U | +0x88 | per-walk cap (cells); earlier notes called it impactZ, that was the type-record copy at creation | 0x6F4A73F0 writes it; 0x6F4907F0 reads it |
| U | +0x8C | facing (angle) | 0x6F4907F0 fallback heading; 0x6F491250 |
| U | +0xB0, +0xB8 | scratch values saved and restored around the walk (0.49 and an override) | 0x6F4907F0 |
| U | +0x98 | pointer to a flag block (bit 0x40000000 is cleared/set per walk) | 0x6F490FC0 |
| U | +0xA8 | pointer to a block whose byte +0x8B bit 0 gives the 0x80000 flag | 0x6F4907F0 tail |
| R | +0x14 | owner U | 0x6F4907F0 |
| R | +0x18 | target point (x, y) | 0x6F4907F0 passes `R+0x18` to 0x6F495320 |
| R | +0x20 | step length d (cells), after the cap | 0x6F4907F0 |
| R | +0x24 | heading of V | 0x6F4907F0 |
| R | +0x28 | flags (0x200000 = skip this tick, 0x10000, 0x20000, 0x80000) | 0x6F4907F0 |

Note: the decompile of 0x6F4907F0 shows the sqrt result as `local_34` and loses the value (softfloat returns in
registers). The disassembly was used instead (section 9).

---

## 3. The effective speed (CONFIRMED structure; units LIKELY world units per second)

- 0x6F201C60 (CONFIRMED): if the counter at `Amov+0x7C` is nonzero it returns 0.0. Otherwise it calls 0x6F2019F0.
- 0x6F2019F0 (CONFIRMED): clamps the stored speed (Amov+0x70 based) between a lower and an upper bound read from
  data. Upper bounds `0x6FAB4440/48/50/58` = 522.0 and lower bounds `0x6FAB443C/44/4C/54` = 1.0 (CONFIRMED values in
  runtime_consts.tsv). The owner's values `0x6FAB3AD0` = 522.0 and `0x6FAB3AD4` = 100.0 are applied only under
  conditions (unit flags at +0x20 and +0x194 and an ability found through 0x6F0787D0); which bound each one is for
  is UNSURE. The choice between the families uses an input flag `ebx` (not decoded).
- The function reads the Amov object's fields (+0x70, +0x78) and an ability through 0x6F0787D0 and 0x6F29C540/580
  (type data, not decoded). No multiplication by a time or by a scale appears in it. CONFIRMED (full disassembly read).
- So the number that reaches the sink is the **clamped speed in world units per second** (LIKELY from the 522 maximum,
  which is the usual WC3 maximum speed).

---

## 4. The speed sink: Position::vf6 (CONFIRMED code, LIKELY owner)

- 0x6F4747B0 is `Position::vf6` (vfunc map; its vtable is at 0x6F877DF4). It is `ret 4` with one float pointer.
- It takes the unit handle stored in Z (`Z+8`, `Z+0xC`), resolves it with 0x6F03FA30, and gets U.
- It computes `x = min(arg, mgr+0x80)`: `fld [mgr+0x80]`, `fld [arg]`, `fcompp`; if `arg > mgr+0x80` the value is
  `mgr+0x80`, otherwise `arg`. CONFIRMED.
- Then it applies the bit operation (CONFIRMED disassembly):
  `r = (x - 0x03000000) ^ x` sign test; if the sign changes the result is 0.0, otherwise `r = x + 0xFD800000`.
  Since `0xFD800000 = -0x02800000 = -(5 << 23)`, this subtracts 5 from the float exponent, which is **division by 32**
  for normal values (checked in Python: 522.0 -> 16.3125, 270.0 -> 8.4375, 1.0 -> 0.03125). It is the same
  world-to-cell scale used by the pathing code (pathing.md, x32 factor). CONFIRMED.
- The result goes to `U+0x88` through 0x6F4A73F0 (CONFIRMED: `mov [esi+0x88], ecx`).
- So: **U.+0x88 = min(eff_speed, mgr+0x80) / 32** (cells). CONFIRMED arithmetic; the owner U is LIKELY (the handle).

**The value of `mgr+0x80` is not known.** The constructor of the object at `[0x6FAB7368]` (0x6F470A60, created at
0x6F4719B8, size 0x8C) writes `+0x84` and `+0x88` to zero but not `+0x80`. Whether `+0x80` is set later (for example
by 0x6F46DA10, which is also called there) is open (UNSURE). If it stayed zero the cap would be zero and no unit could
move, so it must be set to something at run time. This is the first thing to read dynamically.

Consequence: with `mgr+0x80` at least the speed, the cap is `speed/32` cells, which is 16.3 cells for 522 units/s.
A cap of that size only makes sense if one walk is one second, or if V is much smaller than the cap. Both conflict
with a 0.03 s walk. This is the main open contradiction (section 8).

---

## 5. The walk timer (CONFIRMED, from movement 4 and lockstep 11.2; not changed here)

- The walk is a repeating agent timer with period 0.03 f32 (`0x6FAB7370`, CONFIRMED value). Its message is 0x8026A,
  dispatched to 0x6F4675C0 and then 0x6F483300 (the mover walk).
- 0x6F483300 walks the cluster list and the mover list and calls 0x6F493410 per cluster, which calls 0x6F490FC0, which
  calls 0x6F4907F0 once per record (CONFIRMED, decompile and disassembly).
- No multiplication by 0.03 appears in 0x6F4907F0 or its direct callees (CONFIRMED, movement-4 section 2, re-checked:
  the five absolute uses of `0x6FAB7370` are timers and pushes).

---

## 6. Writers of the step vector V = (U.+0x80, U.+0x84) (CONFIRMED list; the non-zero producer is partly open)

The writer is 0x6F4A70D0 (CONFIRMED): `U.80 := v.x`, `U.84 := v.y`; if `|v| < 2e-7` (constant `0x6FAAE614` = 2.0e-7,
CONFIRMED) both are set to 0.0 and the flag at `U.98+0x40` bit 0x20000000 is cleared, then a virtual call on U
is made when the second argument is nonzero. Otherwise the code continues (not read in full).

Callers of 0x6F4A70D0 go through 0x6F4A73B0 and 0x6F493A40 (= "set vector and notify", CONFIRMED):

| Caller | What it passes | Meaning |
|---|---|---|
| 0x6F4969CD (in 0x6F496980) | zero vector | stop the walk (CONFIRMED) |
| 0x6F4956DF (in 0x6F4956C0) | zero vector, and clears 0xC0/0xC4 | stop (CONFIRMED) |
| 0x6F494160 (only caller 0x6F491285 in 0x6F491250) | `FUN_6F10E370(speed)`: V = (speed·cos θ, speed·sin θ) if speed > 0, else zero | the speed-based vector builder; its only caller passes `speed = 0.0` (CONFIRMED) |
| 0x6F496A2B (in 0x6F4969F0) | the vector the caller gives | set the walk vector (CONFIRMED) |
| 0x6F4941D8, 0x6F4AAB47 | not read | LIKELY stop/reset (UNSURE) |

0x6F4969F0 (CONFIRMED) is the entry used by the move-target routine 0x6F497080:
1. `U.+0x88 := 0x6FAAE624` = FLT_MAX (the "no override" value, movement.md; CONFIRMED value), through 0x6F4A73F0;
2. two more setters (0x6F4AA840, 0x6F4AA850) with the same constant;
3. `0x6F493A40(U, v, 1)` = set V to `v`.

So the move-target routine sets the cap to FLT_MAX and then sets V. This looks like "no cap for this order"; it means
the cap set by the speed sink is **overwritten** at each move issue (LIKELY, not verified: the order of calls
between the speed setter and the move issue is not known).

---

## 7. The move-target routine 0x6F497080 (CONFIRMED structure; semantics partly UNSURE)

- Called from 0x6F47A650 (the move issue for a point, branch `param_10 == 0`) and from the wrapper 0x6F4972B9
  (CONFIRMED call edges). 0x6F47A650 itself is called from 0x6F2BCC50 (ability-level move; the arguments are
  `unit+0x8C` as facing and a float from `param+0x54`, LIKELY speed-related, UNSURE).
- Parameters (`ret 0x14`, five stack arguments): the caller 0x6F47A650 passes a point, a point, a point,
  `&0x6FAAE56C` (= **60.0**, CONFIRMED value) and a float pointer. The routine reads the float pointer's slot not at
  all in the first 420 bytes (CONFIRMED). The mapping from stack slots to these names is UNSURE (section 7 below).
- It stores `*p2` to U+0x74 and `*p4` to U+0x78 (CONFIRMED stores at 0x6F4970A6/0x6F4970AB). With this caller
  `*p4` = 60.0, which conflicts with U+0x78 being the cell position (section 2). So the mapping of the stack
  parameters is UNSURE, and so is everything below that uses p4.
- It computes a distance with softfloat (0x6F6EEE20 mul, 0x6F6EF000 add, 0x6F6EF750 sqrt). A compare of that
  distance with the value at p4 follows (CONFIRMED compare, UNSURE meaning). Then it writes U+0x58 (distance) and
  U+0x5C (heading, 0x6F4B2DE0) (LIKELY from the store pattern). Then a vector is built at U+0x50 and passed to
  0x6F4969F0 (section 6).
- So **V is probably a direction vector toward the target, scaled by a length taken from the 60.0 constant or from the
  distance** (UNSURE which). If the length is the distance, V is the whole remaining path and not a per-walk step. If it is
  60.0, V is a path segment. Either way the per-walk step must then come from the path step (0x6F495320 / 0x6F493FA0 /
  0x6F494200), which runs every walk and rewrites R.+0x20 and R.+0x24 (UNSURE which one writes V each walk).

---

## 8. Why no formula is confirmed

Three things are needed; the first two are open:

1. **`mgr+0x80`** (the bound in the speed sink, section 4). Without it the cap `U.88` is unknown. Dynamic read: break on
   the store of the speed, or read the object at `[0x6FAB7368]` after start-up in the running game.
2. **The per-walk producer of V.** The move issue sets V from a direction and a length that is either the distance or
   60.0 (section 7). If it is a whole path segment, a unit would move that far in one walk, so something (probably the
   path step) must shorten it to the per-walk amount each walk. Which function does that and what it multiplies by is
   open. The candidates are 0x6F495320 (writes R.20/R.24 and calls 0x6F493FA0) and 0x6F493FA0 (writes through its output
   pointer; it reads `U+0xBC` against a value, the "prop window" of movement.md, UNSURE).
3. **The speed-to-walk factor.** If the walk is 0.03 s, a per-walk amount for 522 units/s is 15.66 world units =
   0.489 cells. Notice that **0.49 (`0x6FAAE4CC`, CONFIRMED value 0.49) is the path-validity radius** (movement-3
   section 4) and not a speed. A factor of 0.03/32 = 0.0009375 per (speed unit) would be the exact conversion from
   world units per second to cells per walk; no such constant was found in the chain (CONFIRMED: no `0x3CF5C290`
   multiply, section 5).

### The contradiction to resolve first

As read, the cap is `speed/32` cells, applied as a per-walk limit. At 522 units/s that is 16.3 cells per walk, i.e.
about 540 cells per second at 30 walks per second, which is far too fast. So at least one of these is wrong:
(a) the cap is not a per-walk amount, (b) `mgr+0x80` is a per-walk bound (a value near 0.0009 * speed would fix it),
(c) V is always much smaller than the cap, so the cap never binds, and the per-walk step is V itself. The data read
so far fits (c) only if V is made per walk at the right size, which is open.

---

## 9. Soft-float operation sequence (CONFIRMED for the parts read)

Per record in 0x6F4907F0 (disassembly, registers as in the code):

1. `x = U.80`, `y = U.84` (loads at 0x6F4907FD and 0x6F490812).
2. `s = sqrt(add(mul(y,y), mul(x,x)))` through 0x6F6EEE20 (mul), 0x6F6EF000 (add), 0x6F6EF750 (sqrt).
3. `d = s` if `|s| >= 4e-7` (`0x6FAAE618`), else 0.0. Stored at R+0x20.
4. heading = atan2-like 0x6F4B2DE0 of V (or U+0x8C when d = 0). Stored at R+0x24.
5. path step (0x6F495320), then `if R.20 > U.88 then R.20 = U.88` (fld/fcompp; CONFIRMED).

The per-walk move vector is therefore not computed in 0x6F4907F0; the sequence above is the only arithmetic there.
The softfloat helpers used are checked (crates/softfloat, 540k vectors, 0 mismatches; softfloat.md).

---

## 10. Confidence table

| Item | Label |
|---|---|
| Walk distance per record = min(|U.80,84|, U.88), cells | CONFIRMED |
| U.88 = min(eff speed, mgr+0x80) / 32 (the ÷32 arithmetic and the min) | CONFIRMED |
| Speed chain SetUnitMoveSpeed -> ... -> Z.vf6 | CONFIRMED (edges); Z = unit.vt[0xB8] LIKELY |
| Position::vf6 = 0x6F4747B0, owner U = unit via handle | CONFIRMED code, LIKELY owner |
| eff speed in world units per second, clamp to 1..522 | CONFIRMED values, units LIKELY |
| Step vector writers and the zero-vector rule (|v| < 2e-7 -> 0) | CONFIRMED |
| Move issue sets U.88 = FLT_MAX and then V (0x6F4969F0) | CONFIRMED |
| V = direction * (distance or 60.0) at move issue (0x6F497080) | UNSURE (structure CONFIRMED, scaling not) |
| Per-walk shortening of V and the per-walk factor | UNSURE (not found) |
| Value of mgr+0x80 | UNSURE (not set in the constructor) |
| Walk = 0.03 s; no 0.03 multiply in the mover | CONFIRMED (movement-4, re-checked) |
| Per-second to per-walk conversion | UNSURE, no formula |

---

## 11. Next steps (in order)

1. Dynamic (Wine + user's game files, as in AGENTS.md): on a move order at a known speed, log the value at
   `[mgr+0x80]` and `U.88` after each walk, and `U.80/84` before and after 0x6F495320. This settles items 1 and 2 of
   section 8 at once.
2. If `U.80/84` are rewritten by 0x6F495320 or 0x6F493FA0, read their stores (disassembly of those two functions,
   which were not read in full in this pass: 0x6F493FA0 is ~200 instructions).
3. Only then write the formula into `crates/sim/src/orders.rs` (`step_unit`), with the cell scale of section 4.

No change was made to code in this pass. The `speed * 0.03` model in orders.rs stays a placeholder (its comment already
says so).
