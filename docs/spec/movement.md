# Unit movement per simulation step (Game.dll 1.26a)

Status: pass 3, reverse engineering for a clean-room port (interoperability). Written from the decompiled
listing (scratchpad/decomp) and from the disassembly of the functions marked "asm". Image base 0x6F000000.
Labels: CONFIRMED = read directly from code or constants; LIKELY = consistent with the code but the role is
inferred; UNSURE = not resolved in this pass.

Helper calls use the calling convention in docs/spec/softfloat.md: ECX = result pointer, EDX = operand A,
stack = operand B (binary), or ECX = result, EDX = operand (unary). Decompiled single-argument calls hide one
operand, so every operand order below was taken from the disassembly where noted.

---

## 0. Corrections to earlier notes (docs/spec/pathing-2.md and AGENTS.md 4.3h/4.3j)

- The 0.1 constant at 0x6FAAE4A0 is NOT read by the mover. A byte scan of .text for the absolute address finds
  only two readers: FUN_6F480E90 (passes &0.1 to FUN_6F4A6890, LIKELY as a period) and FUN_6F4AB390 (stores it at
  object+0xC0 as a default interval). CONFIRMED (scan). So "the mover uses 0.1 per tick" (pathing-2 section 6.2)
  is wrong. The step length in the mover is not multiplied by 0.1 anywhere in 0x6F48xxxx to 0x6F4Bxxxx.
- The value 0x6FAAE4F4 (the start value of the segment walk FUN_6F49BD10 and read about 400 times) is zero in the
  file and no immediate store to it was found. It is NOT 1.0 as pathing-2 guessed. UNSURE.
- Constants verified by immediate stores in the static initializers (CONFIRMED):
  0.001 at 0x6FAAE47C, 0.01 at 0x6FAAE480, 0.1 at 0x6FAAE4A0, 0.2 at 0x6FAAE4B0, 0.49 at 0x6FAAE4CC,
  0.5 at 0x6FAAE4D0, 0.75 at 0x6FAAE4E8, 1.2 at 0x6FAAE4FC, 4e-7 at 0x6FAAE618 (f32 0x34D6BF95),
  FLT_MAX (0x7F7FFFFF) at 0x6FAAE624 (used as "no override" sentinel), pi/4 at 0x6FAAE5F8, 3pi/2 at 0x6FAAE604,
  2pi (6.2831855) at 0x6FAAE608, zero-threshold 1e-5 as the integer bit pattern 0x3727C5AC at use sites.

---

## 1. Per-step entry and order over units

### 1.1 Who starts a step (CONFIRMED edges, LIKELY timing)

- FUN_6F3B0750 calls FUN_6F426BF0(1), which posts message 0x8026A into the queue at DAT_6FAB7370
  (through FUN_6F4778F0). FUN_6F3A2EC0 (the per-frame driver) calls FUN_6F426BF0(0), which does not post
  but calls FUN_6F477D30 (cancel, UNSURE). So the step is driven by a posted message, not by a fixed loop.
- FUN_6F42FC80 is a message switch. Message 0x8026A goes to FUN_6F426C30. CONFIRMED.
- FUN_6F426C30 calls FUN_6F4675C0, then FUN_6F53F160 (not read), and FUN_6F443480 if that returns nonzero.
- FUN_6F4675C0 is an 11-byte stub: ECX = the world object pointer read from 0x6FAB73D8, then JMP FUN_6F483300.
  CONFIRMED (asm).
- LIKELY: one message = one simulation tick, and the tick rate is 10 per second (the 0.1 s the game clock
  uses, pathing-2 section 4.3h). NOT CONFIRMED in this pass, since the mover does not read 0.1.

### 1.2 Per-tick unit loop: FUN_6F483300 (CONFIRMED order)

1. The world tick counter at world+0x538 is incremented. If it wraps to 0, it is set to 0x400.
2. FUN_6F49AC40 (not read).
3. List at world+0x38C: FUN_6F48E4C0 for each node (next pointer read before the call).
4. If the list at +0x38C is not empty: for each node of the list at world+0x3B8 whose +0x7C is nonzero,
   FUN_6F4920F0 (not read).
5. MAIN MOVER LOOP: list at world+0x3B8. For each node, read the next pointer (+8) first, then call
   FUN_6F493410(node). The node is the movement state of one moving group or unit (LIKELY the path object at
   unit+0xA8 or the group at +0x3B8; the link fields are at +4 and +8). The order is the list order.
6. List at world+0x440: FUN_6F494200 for each node (not read).
7. The toggle at world+0x53C flips between 0 and 1. For the list at world+0x51C, FUN_6F4961C0 (with argument
   world+0x540) is called on alternate elements, the parity set by the toggle. CONFIRMED from the control flow.

Order rule: units are processed in list order, one after another, and each one's position is updated before the
next unit is processed. There is no two-phase update of positions for the whole list.

Insertion order of the list (LIKELY): FUN_6F493810 links a node directly after the head, so the most recently
added node is first. The membership test (node+4 == 0 and node+8 == 0) is in FUN_6F495320. UNSURE which list
head is used there.

### 1.3 Per-mover update: FUN_6F493410 (asm and decompiled, CONFIRMED structure)

1. FUN_6F493070: if the object's flag 0x1000 is set and FUN_6F4924C0 returns 0, return at once (skip).
2. FUN_6F492C10: for each unit of the group, in array order, if the unit's owner (FUN_6F495940) is this group,
   call the unit's virtual at slot 0x54 with the unit's position (FUN_6F497460 and FUN_6F38B0B0 build the
   position). Then FUN_6F492B90. The return value is not used reliably (the decompiled function is void).
3. FUN_6F492E90: returns the current path group (the object's +0x98) or 0.
4. FUN_6F492C90: the path request/ lookup. It throttles re-planning (see 5.3), and on failure the
   function takes the failure branch (FUN_6F490E60, FUN_6F491250) and returns.
5. On success: FUN_6F490FC0 (batch step over the group's records, section 2 onward), then FUN_6F493010
   (writes results back, LIKELY), then FUN_6F490E60 (wait counter, section 4.4).
6. Event: if a local flag is set, FUN_6F492440 (when the second flag is 0) or FUN_6F4912B0 (when nonzero).
7. The counter at object+0x68 (if nonzero) is decremented. A failed first check calls the object's
   virtual at slot 0x10 with 0 (LIKELY "stop" callback).

### 1.4 Batch over a group's records: FUN_6F490FC0 (CONFIRMED)

The group has a record array at +0x28 with count at +0x38. Records are 44 bytes (stride 0x2C). Each record R
has: R+0x14 = owner unit U, R+0x18/+0x1C = target position (vec2), R+0x20 = step length (speed), R+0x24 =
facing (angle), R+0x28 = flags.

Flags on R+0x28 (CONFIRMED where stated):
- 0x200000 = skip this tick (set by FUN_6F490C80, cleared at the start of each batch).
- 0x100000 = moving (set when R+0x20 > 0.01).
- 0x20000 = set when the step returns a "changed" value (from FUN_6F4907F0).
- 0x10000 = set when the step reports movement (cleared otherwise).
- 0x80000 = set when U+0xA8+0x8B bit 0 is set, otherwise 0x80000 is cleared.
- 0x40000 = set by FUN_6F490630 when the target was moved (path result changed).

Order inside FUN_6F490FC0:
a. Clear 0x200000 on every record.
b. If the count is 1, or the group flag 0x200 is set: call FUN_6F4907F0 for each record in array order, then stop.
c. Otherwise: if group+0x68 is 0 and group flag 0x2 is clear, call FUN_6F490BD0. If it returns 0, set
   group+0x68 = 0x42. Else call FUN_6F490C80 (priority resolution, section 5.1). Then FUN_6F490F80
   (sets 0x100000 on all records when group flag 0x20000 is clear or group+0x5C > 0x41).
d. For each owner U: set U+0x98's flag 0x40000000, then call FUN_6F4907F0 for each record in array order.
e. Clear the 0x40000000 flag again and set the moving bit (0x100000 on R) for every record with R+0x20 > 0.01.
   The loop is unrolled in groups of four, with a tail loop. The records are visited in array order.
   CONFIRMED (both loops and the constant 0.01 at 0x6FAAE480).

So within one tick, all records first get their step (d), then all records get the moving flag (e).
The moving flag used at the next tick's start is therefore the speed from this tick's step.

---

## 2. The per-record step: FUN_6F4907F0 (asm, CONFIRMED order)

Arguments: ECX = the group object (flags at +0x80 and +0x82, values at +0x6C and +0x74); stack = record R.
Owner U = R+0x14. Relevant unit fields: U+0x80 and U+0x84 (a vector, LIKELY the unit's current path delta),
U+0x88 (maximum step length), U+0x8C (idle facing), U+0x90 (a per-unit scalar), U+0xA8 (movement state),
U+0xB0 and U+0xB4 and U+0xB8 (scalars used by the integrator).

Order of operations:

1. If group flag 0x1 is set, call FUN_6F490630 (retarget: copies the path target into R+0x18/+0x1C, LIKELY).
2. Compute L = sqrt(U.x^2 + U.y^2) with MUL, MUL, ADD and SQRT (0x6F6EEE20, 0x6F6EEE20, 0x6F6EF000, 0x6F6EF750).
   The vector is (U+0x80, U+0x84) (asm: both are loaded from U). CONFIRMED operands from the asm.
3. Set R+0x20 (speed) from the value computed by the integrator (section 3). If |value| < 4e-7, the
   speed is 0.0. CONFIRMED threshold (asm: 4e-7 at 0x6FAAE618).
4. Facing (section 4.1): if R+0x20 == 0.0 exactly, R+0x24 = U+0x8C. Otherwise R+0x24 = heading from FUN_6F4B2DE0.
   CONFIRMED.
5. If group+0x6C is nonzero, save U+0xB0, set U+0xB0 = 0.49 for this step, and restore it after the step.
   CONFIRMED (the 0.49 constant at 0x6FAAE4CC).
6. If group flag 0x200 is set: save U+0xB8, and if group+0x74 != FLT_MAX, set U+0xB8 = group+0x74. Call
   FUN_6F493E80 (not read). The original U+0xB8 is restored after the step. CONFIRMED (asm at 6F490917-6F490936).
7. If R+0x28 bit 0x200000 is clear (the normal case): call FUN_6F497460 and FUN_6F38B0B0 to build the
   position vector, then call FUN_6F495320 (the path step, section 3). Then clamp R+0x20 to U+0x88:
   if U+0x88 < R+0x20, R+0x20 = U+0x88. CONFIRMED clamp.
   If bit 0x200000 is set (skipped): call FUN_6F4940C0 instead (the same integrator without the path step).
8. Restore U+0xB0 and U+0xB8 if they were saved. Set R+0x28 bit 0x20000 if the step returned a flag, and
   bit 0x10000 if the step reported movement. Set or clear 0x80000 from U+0xA8+0x8B bit 0. CONFIRMED.

Order summary (the important point for turning and moving): the facing (step 4) is computed BEFORE the path step
(step 7). The speed cap (step 7) is applied AFTER the path step.

---

## 3. Path step and speed: FUN_6F495320, FUN_6F493EA0, FUN_6F493FA0, FUN_6F49E020

### 3.1 FUN_6F495320 (LIKELY the per-step path advance)

- Called with ECX = unit U (asm: "mov ecx, esi" before the call, where esi = U). CONFIRMED.
- If U+4 and U+8 are both zero (node not linked), it calls FUN_6F493810 to link U into the list (section 1.2).
- Sets the output flag at the pointer passed as param_6 to the result of FUN_6F493EA0.
- If FUN_6F493EA0 returns nonzero (arrived, see 3.2): sets the output value to 0.0, calls FUN_6F493E90 and
  FUN_6F49B5A0, and returns.
- Otherwise: if U's path state flag at +0xA4 differs from the value passed in, a re-plan is forced (param_10 = 1).
  FUN_6F49B470 (throttle, section 5.3) is checked. If a re-plan is allowed: FUN_6F49AD30, FUN_6F49AD40,
  FUN_6F49DFB0 (reset the path to a single point, LIKELY).
- Calls FUN_6F49E020 (the path step, 3.3). Return codes: 0 = no move this tick, 1 = moved, 3 = blocked
  (continues with the else branch), 4 = repath needed (sets the output flag if the first check was set).
- Finally calls FUN_6F493FA0 once more, to clamp the value (3.4).

### 3.2 FUN_6F493EA0 (arrival test; asm and decompiled)

- Computes L = |v| with MUL, MUL, ADD, SQRT on the vector at the first pointer (+4 and +0 of the input).
- Calls FUN_6F493D20 with the waypoint vector and the input vector. The result is a scalar d (written to the
  output pointer param_6).
- Flag f = (U+0xDA bit 0 set) OR (step length <= the stored value). Return 1 when f is set AND |d| <= 0.2.
  CONFIRMED structure and the 0.2 constant at 0x6FAAE4B0 (found at 6F493EA0+0xD8).
- Meaning: the unit has arrived at the current waypoint when the projected residual |d| is within 0.2 units
  of the segment (LIKELY; the units of the waypoint vector are cells, from pathing-2, so 0.2 is in cells
  unless the input is already in world units; UNSURE).

### 3.3 FUN_6F49E020 (path step gate; CONFIRMED structure)

- Gate: if the flag word at the path object's +0x88 has bit 0x100000 clear, then:
  - if the wait counter at +0x94 is nonzero, decrement it and return 1 (waiting, no move);
  - otherwise call FUN_6F49B8E0, clear the path pointer at +0xA4 if its +0x38 is -1, call FUN_6F49DAC0
    (the step), and if that returns 0, call FUN_6F49DF00 (the fallback, 3.5).
- If bit 0x100000 is set, the function returns 0x100000 (the flag value itself, i.e. "moving; do nothing here"),
  which the caller reads as not handled (LIKELY).
- The path object here is the per-unit movement state U+0xA8 (LIKELY, from 490630 and 495320 usage).

### 3.4 FUN_6F493FA0 (clamp; asm, CONFIRMED)

- Input: a value v (the float at the pointer passed as the 4th argument, abs taken as |v|).
- If the flag argument is 0 and U+0xBC < |v| (an ordered compare on the asm), the output is ADD(U+0xB4, ...)
  (asm: FUN_6F6EF000 with operand U+0xB4); otherwise the output is 0.0.
- If |v| != 0: the magnitude is m = min(U+0xB8, |v|), and the output is +m or -m, with the sign taken from the
  sign of the output value computed above. The clamp limit is U+0xB8. CONFIRMED structure.
- Meaning: U+0xB8 is a maximum magnitude per step for the value it clamps (the step length or the turn value,
  UNSURE which), and it is replaced by group+0x74 for one step when the group has flag 0x200 (section 2, step 6).

### 3.5 FUN_6F49DAC0 and FUN_6F49DF00 (waypoints and fallback; partly read)

- FUN_6F49DAC0 gets the current path context from FUN_6F49B590. Its unit pointer is at context+0x1C, and the unit
  position is at unit+0x68 (vec2). It normalises the position with MUL and ADD (asm-level only).
  It then checks the segment count at path+0x70 against the index at path+0x78. If the index has reached the
  count and flag 0x200000 is clear, the path list is cleared (FUN_6F4987F0 and FUN_6F498770) and the index is
  reset to 0. If the flag is set, FUN_6F49B9C0 requests a new path, and a failure returns 2.
  The return value comes from FUN_6F49C990.
- FUN_6F49C990 returns 1 when a waypoint remains, and 2 when it is blocked (sets the wait counter to at least 20
  through path+0x94), after calling FUN_6F49C2E0 (path request). UNSURE in detail.
- FUN_6F49C930 (CONFIRMED): if the index (path+0x74) is at or past the count (path+0x50), it checks the distance
  to the goal with FUN_6F49BB90 and returns 0 if not reached. Otherwise it writes the current waypoint, the
  pair of floats at path+0x40 + index*8, and returns 1. So the waypoint array has 8-byte entries (x, y) and the
  current index is at path+0x74.
- FUN_6F49C3B0 (arrival, UNSURE): compares a distance against a threshold involving 0.49 (0x6FAAE4CC) and
  returns 0 (not yet), 4 (not on this path), or 2 (blocked; sets the wait counter to 20 minimum).
- FUN_6F49DF00 (CONFIRMED structure): calls FUN_6F49C930 (failure -> 2), then FUN_6F49C3B0 (0 -> try the
  search FUN_6F49D8D0, success -> 0; else wait -> 1 when FUN_6F03FA30 or the wait counter says so; 2 -> 2).
  Otherwise it restores the saved position and calls FUN_6F49DE90 (retry counter path+0x98: 0 -> FUN_6F49B340,
  if it is 1 -> return 4 (give up), otherwise decrement and return 1; 3 when FUN_6F49C460 returns nonzero).
- Mapping of the waypoint choice rule (which index advances, and when): UNSURE in this pass.

### 3.6 Speed in world units per second: not established

- R+0x20 is the step length of this tick (CONFIRMED that it is compared with 0.01 and with U+0x88 directly; no
  factor 0.1 is applied in the mover).
- So the mover's "speed" is in path units per tick. The conversion from the game's movement speed to U+0x88
  is not in the mover. UNSURE where it happens (probably the unit's speed setter or the order code).
- Acceleration: none found. R+0x20 is set each tick by the integrator (3.4), and there is no ramp toward a target
  speed in FUN_6F4907F0 or FUN_6F495320. The only limits are the magnitude cap U+0xB8 (3.4) and the speed cap
  U+0x88 (section 2, step 7). LIKELY: speed reaches the cap at once, subject to the clamp.
- Speed modifier hook: the sentinel override of U+0xB8 from group+0x74 (section 2, step 6) is the only
  per-step modifier found in the mover. FUN_6F474B30 writes it (clamped at 0.001, see 4.2).

---

## 4. Turning and facing

### 4.1 Heading from the step vector (CONFIRMED formula, asm of FUN_6F4B2DE0)

Inputs: a vector (vx, vy) and its length r (the step length R+0x20).
1. If |r| <= 1e-5 (bit pattern 0x3727C5AC): heading = 0.0.
2. Compute u = vx / r (FUN_6F6EFE10, DIV).
3. Compute a = acos(u) (FUN_6F6EFB00, ACOS).
4. If |a| <= 1e-5: heading = 0.0.
5. If vy >= 0: heading = a. Otherwise heading = 2*pi - a, with 2*pi = 6.2831855 at 0x6FAAE608.
   CONFIRMED from the branch at 6F4B2E41-6F4B2E73 (the sign test on the second component).

Heading is in radians on [0, 2*pi). In FUN_6F4907F0 the input is the step vector and r is the step length
(LIKELY; the vector object is the one in the ECX of the call, not resolved).

The facing written to R+0x24 is therefore the direction of the step vector, not an integrated turn. The
facing is set BEFORE the path step (section 2, step 4).

### 4.2 Turn rate

- FUN_6F474B30 clamps its input from below at 0.001 (0x6FAAE47C): if the input <= 0.001, it is set to 0.001.
  CONFIRMED clamp. It then calls FUN_6F4937C0 with the value (target field UNSURE; LIKELY the group's cap at
  group+0x74, which section 2 step 6 copies into U+0xB8).
- FUN_6F474BA0 stores through FUN_6F4937E0 with no clamp (CONFIRMED). Its target is the same kind of field,
  probably the "no override" value FLT_MAX (UNSURE).
- The per-step limit on a turn is the magnitude clamp of FUN_6F493FA0 with limit U+0xB8 (3.4). Whether that limit
  applies to the change of heading or to the absolute heading is UNSURE, since the decompiled code clamps the
  value it is given and the facing is set from the step vector right before the path step (4.1).
  So at this pass: the turn is instant (facing = step direction), and any turn-rate limit is only the clamp on
  U+0xB8. Gradual turning was NOT found.

### 4.3 Propulsion window (what blocks movement)

- A unit does not step while the path flag 0x100000 is clear and its wait counter at path+0x94 is nonzero
  (3.3). The counter is set to at least 20 by FUN_6F49C3B0 and FUN_6F49C990 when a path is blocked (CONFIRMED
  stores of 0x14).
- FUN_6F490E60 (CONFIRMED structure) sets a wait counter at group+0x64 (not path+0x94) when it is -1: it computes
  t = trunc(0.5 + k * d), with d = sqrt(a^2 + b^2) of two unit-derived values, and clamps t to 16..132
  (+165 when group flag 0x400 is set). The constants k = 0x6FAB7414 (3.8e-9) and 0x6FAB7430 (4.9e-8) are in
  .data, not immediate stores. UNSURE what the wait means in ticks.
- "Propulsion window" (a facing window in which the unit may move): NOT found. The mover moves whenever the
  step is not gated (3.3) and the flags are clear. LIKELY no facing window exists in the mover.

### 4.4 Turn versus move order (CONFIRMED within one call)

Facing (heading of the step vector) -> path step (waypoint advance, arrival test, FUN_6F49E020) -> integrator
clamp (FUN_6F493FA0) -> speed cap (U+0x88) -> restore U+0xB0 and U+0xB8.

---

## 5. Collision, blocking and re-planning

### 5.1 Priority and skip (FUN_6F490C80, CONFIRMED structure)

- Builds one key per record: a float pair from the owner unit's +0x90 scalar and its position (through
  FUN_6F497460). The records are sorted with FUN_6F48F2D0 (sort not read).
- The priority class of a unit is (U+0xD8 >> 12) & 0xF (CONFIRMED read).
- Records are then walked from the end of the sorted list. A record whose class is lower than the class of a
  neighbour that has a key in range is flagged 0x200000 (skip this tick). Records of equal or higher class are not
  flagged. The key test compares the keys of the neighbours.
- So: a unit is held back (skip, no move this tick) when a higher-priority unit is close by in key order. There
  is no positional push-apart in this batch.
- Tie-breaking: by the sort order and by class; the sort algorithm and its stability are UNSURE.

### 5.2 Separation / push-apart

- NOT FOUND. No code writes an offset to a unit's position for separation. CPoSeparate, CProximityMap and
  CPoPosCl (pathing.md) were not located in the step. The only other interaction in the batch is the 0.49 margin
  (section 2, step 5 and FUN_6F49C3B0). UNSURE whether 0.49 is a collision radius.

### 5.3 Re-plan throttle (FUN_6F49B470, CONFIRMED structure)

- If the unit's cell (rounded from the position with FUN_6F6EF170 and FUN_6F6EEC20, four values) is the same as
  before (per axis, with a shift), the re-plan is not needed: the out flag is set to 1 and the result is 0.
- Otherwise the re-plan is allowed only when the world tick counter (world+0x538) minus the unit's stamps at
  +0x7C and +0x80 is more than 9 for both (that is, at least 10 ticks since the last re-plan). LIKELY.
- A blocked unit re-plans only when this throttle allows it. Failed re-plans wait by the counters in 3.3 and 4.3.

### 5.4 Order in a tick for blocked units

Skip flag from 5.1 -> integrator without path step (FUN_6F4940C0, as step 7 in section 2) -> no movement.
Otherwise the path step (3.3) returns 0/1/2/3/4 as in 3.5, and 2 or 4 leads to a re-plan (5.3) or to giving up.

---

## 6. Constants and call-site table (all verified in this pass)

| Address | Value | Use (where read) |
|---|---|---|
| 0x6FAAE47C | 0.001 | turn setter clamp FUN_6F474B30; FUN_6F4961C0 and others |
| 0x6FAAE480 | 0.01 | moving threshold in FUN_6F490FC0 |
| 0x6FAAE4A0 | 0.1 | not read by the mover (period in FUN_6F480E90, FUN_6F4AB390) |
| 0x6FAAE4B0 | 0.2 | arrival tolerance in FUN_6F493EA0 |
| 0x6FAAE4CC | 0.49 | margin set into U+0xB0 in FUN_6F4907F0; FUN_6F49C3B0, FUN_6F49C990 |
| 0x6FAAE4D0 | 0.5 | rounding in FUN_6F490E60 and others |
| 0x6FAAE4E8 | 0.75 | cell centre offset (pathing-2, FUN_6F49A470) |
| 0x6FAAE4FC | 1.2 | segment walk and others (value confirmed, use UNSURE) |
| 0x6FAAE618 | 4e-7 | speed zero threshold in FUN_6F4907F0 |
| 0x6FAAE624 | FLT_MAX | "no override" sentinel for U+0xB8 |
| 0x6FAAE5F8 | pi/4 | (not checked in this pass) |
| 0x6FAAE604 | 3pi/2 | (not checked in this pass) |
| 0x6FAAE608 | 2pi | heading wrap in FUN_6F4B2DE0 |
| 0x3727C5AC | 1e-5 (bit pattern) | zero threshold in FUN_6F4B2DE0 |

---

## 7. Softfloat helpers named in the functions read here (AGENTS 4.3g)

| Address | Name | Where seen (examples) |
|---|---|---|
| 0x6F6EEE20 | MUL | FUN_6F4907F0 (x*x, y*y), FUN_6F493EA0, FUN_6F49DAC0, FUN_6F490E60 |
| 0x6F6EEEF0 | SUB (also used to copy a vector component, decompiled as a one-argument call) | FUN_6F4907F0, FUN_6F493EA0, FUN_6F4B2DE0 (SUB with 2pi) |
| 0x6F6EF000 | ADD | FUN_6F4907F0, FUN_6F493FA0 (U+0xB4 + ...), FUN_6F490E60 (+0.5) |
| 0x6F6EF750 | SQRT | FUN_6F4907F0 (length of U+0x80/+0x84), FUN_6F493EA0, FUN_6F490E60 |
| 0x6F6EFE10 | DIV | FUN_6F4B2DE0 (vx / r) |
| 0x6F6EFB00 | ACOS | FUN_6F4B2DE0 |
| 0x6F6EEC20 | F2I (truncating) | FUN_6F490E60 (wait counter), FUN_6F49B470, FUN_6F49D8D0 |
| 0x6F6EF170 | FLOOR | FUN_6F49B470, FUN_6F49D8D0 |
| 0x6F6EF350 | I2R | FUN_6F483E70 (grid index to real) |
| 0x6F6EF3B0 | not in the softfloat table | FUN_6F49C3B0 (one float argument; UNSURE) |
| 0x6F6EF990 | SINCOS pair (decompiled as a one-argument call here) | FUN_6F490C80 (zero init, UNSURE) |
| 0x6F6EEC70 | R2I | not used in this section |

---

## 8. Open items (in order)

1. Read FUN_6F493D20 (the projection used in 3.2) and FUN_6F495940 / FUN_6F4920E0 (unit-to-group ownership).
2. Read FUN_6F49C990, FUN_6F49C2E0 and FUN_6F49B9C0 to get the exact next-waypoint rule and the arrival
   threshold (0.2 or 0.49).
3. Read FUN_6F483E70 (cell to world conversion, called from FUN_6F4885A0 / FUN_6F497460) to confirm the
   cell-to-world factor and origin.
4. Find where U+0x88 and U+0xB8 are set from the unit's movement speed (per second) and turn rate. Start from
   the setter FUN_6F474B30 and its callers.
5. Read FUN_6F48F2D0 (the sort) and FUN_6F490250 for the tie-breaking rule.
6. Determine the target field of FUN_6F4937C0 / FUN_6F4937E0.
