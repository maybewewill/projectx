# Unit movement, second pass (Game.dll 1.26a)

Builds on movement.md (pass 1). Same labels: CONFIRMED / LIKELY / UNSURE. Read-only; nothing in
/home/user/projectx changed. Unicorn tests used the original Game.dll code through tools/emu/emu.py, with the
start-up constant stubs replayed first (scratchpad/mov2_run.py: replays the C7 05 immediate stores and the
I2R stubs in 0x6F84D000-0x6F84E800, 77 stores + 53 I2R calls).

## 0. Corrections and new facts

- 0x6FAAE4F4 = 1.0 (CONFIRMED). The start-up stub at 0x6F84DE80 is I2R(1) into that slot. This resolves the
  pass-1 open item (it was not 0.0 or 1.2). 0x6FAAE504 = 2.0 (stub at 0x6F84DEC0). 0x6FAAE478 = 1e-4 (stub at
  0x6F84DC90), used as the zero snap in the cell-to-world helper (section 2).
- Softfloat is NOT IEEE round-to-nearest. Against Python float32 arithmetic on 300 random pairs each:
  SUB 38 mismatches, ADD 63, MUL 167. Any port must use the original rounding (crates/softfloat), never f32 ops.
  0.49*0.49 is 0.24009999 (softfloat result, equal to the IEEE product for this input).
- Turn and prop setters are reached through CUnit virtuals (CONFIRMED): SetUnitTurnSpeed (0x6F3C5CB0) calls
  vtable slot 0xC0 (= CUnit::vf48, 0x6F28B0F0); that does ECX += 0x164 and JMPs to 0x6F474B30. SetUnitPropWindow
  (0x6F3C5CD0) calls slot 0xC4 (vf49), which JMPs to 0x6F474BA0. The 0x164 sub-object holds an object handle
  id at +8 and +0xC (CONFIRMED read in 0x6F474B30); 0x6F03FA30 resolves the handle to the object (CONFIRMED
  table lookup). The writes land in that resolved object (LIKELY the unit or its movement state).
- SetUnitMoveSpeed is 0x6F3C5C40 (name table confirmed by natives.c). It calls 0x6F26DE00 with the speed on
  the stack, and that only forwards to the ability object at unit+0x1EC (LIKELY) via 0x6F202F30. It does not
  write a unit field itself.
- Pass 1 said the mover loop 0x6F493410 walks the list at world+0x3B8. Pass 2 found that the list that units
  join is different (section 3). See 3.3.

## 1. Setters: which field each native writes (CONFIRMED)

| Setter | Writes | Clamp | Source |
|---|---|---|---|
| 0x6F4937C0 | object+0xB8 (turn cap, per-step magnitude limit of 0x6F493FA0) | none | asm |
| 0x6F4937D0 | object+0xB0 (the 0.49 margin is written here during a step) | none | asm |
| 0x6F4937E0 | object+0xBC (propulsion window; read by 0x6F493FA0 as U+0xBC < |v|) | none | asm |
| 0x6F4937B0 | object+0xB4 (the value added in 0x6F493FA0 when the test holds) | none | asm |
| 0x6F474B30 (turn) | calls 0x6F4937C0 | input clamped to at least 0.001 (0x6FAAE47C) | asm |
| 0x6F474BA0 (prop) | calls 0x6F4937E0 | none | asm |
| 0x6F474B10 / 0x6F474B80 | getters: return +0xB8 / +0xBC of 0x6F03FA30(handle) | - | decompiled |

So the per-step turn cap is object+0xB8 and the propulsion window is object+0xBC. Both are set by natives.
The per-step speed cap (object+0x88, pass 1 U+0x88) is NOT written by these setters.

## 2. Cell-to-world helper 0x6F483E70 (CONFIRMED by Unicorn)

Signature (CONFIRMED): __thiscall, ECX = object G, five stack arguments (out, pa, i, pb, j), ret 0x14.
Behaviour: out = snap(a - b) + s * (i - j), where a = *pa, b = *pb (float32), i, j are signed ints, and
s = G+0x48 (float32). snap(x) = 0 when |x| < 1e-4 (0x6FAAE478). The subtraction is the softfloat SUB
0x6F6EEEF0, the product is MUL 0x6F6EEE20 with I2R 0x6F6EF350 (for i-j), and the sum is ADD 0x6F6EF000.
The term s*(i-j) is added only when i != j.
Test: 400 random cases (including the snap and i == j branches), comparing against the same primitives run
in Unicorn: 0 mismatches. (A plain f32 model fails, because of the rounding noted in section 0.)

Caller: 0x6F4885A0 (out, unit, unit+0x70, unit+0x74) calls 0x6F483270 for the grid object G and then 0x6F483E70
with a = G+0x40 (float), i = G+0x44 (int), b = unit+0x70 (float), j = unit+0x74 (int). The unit position is
therefore a pair: float offset +0x70 and integer cell index +0x74. 0x6F497460 wraps it and returns the result.
So: world coordinate = (grid origin float - unit offset) + (grid cell - unit cell) * s.
- CONFIRMED: the formula above (structure and rounding of the sum).
- LIKELY: the unit's world position is (cell index)*s + offset, with s = G+0x48 being the cell size.
  The value of s at run time was not read (it is set on the grid object at run time). If s = 32.0 that is
  the x32 in pathing-2. UNSURE until the grid object's initialisation is found (0x6F483270 target).

## 3. Mover list order

### 3.1 Insertion (CONFIRMED)
0x6F493810 (ECX = head H, stack = node N): N.prev = H, N.next = H.next, H.next.prev = N, H.next = N.
That is LIFO insertion right after the head. It is called from six sites: 0x6F493D90 (+89), 0x6F4940C0 (+1D),
0x6F4945F0 (+26), 0x6F494620 (+21), 0x6F495320 (+22), 0x6F495FC0 (+172). In all six the head is
ECX = [0x6FAB73D8] + 0x438, i.e. the sentinel at world+0x438 (CONFIRMED from the instructions before each call).
Membership test: a node joins only when its own +4 and +8 are both 0 (CONFIRMED).
So: units join the list headed at world+0x438 (the first real node is at world+0x440) with the most recent
first.

### 3.2 Walkers (CONFIRMED)
- 0x6F483300 walks world+0x440 (list order) and calls 0x6F494200 per node. (Pass 1 did not state this.)
- 0x6F483300 walks world+0x3B8 (list order) and calls 0x6F493410 per node.

### 3.3 The 0x6F493410 list has no writer found (UNSURE, important)
No store of the form [reg+0x3B8] or [reg+0x3B0] exists in .text (capstone scan), and no insertion call names
head world+0x3B0. So either the 0x6F493410 loop receives its nodes some other way that was not located, or
it is dead in this build. Since 0x6F495320 (the path step) is reached only from 0x6F4907F0 -> 0x6F490FC0 ->
0x6F493410, the path step depends on this unlocated feeder. This is the first thing to resolve in pass 3.
Meanwhile 0x6F494200 is the per-unit integrator that runs on the world+0x438 list (section 5).

### 3.4 Sort 0x6F48F2D0 (CONFIRMED by Unicorn)
Calling convention: one stack argument (pointer to the record). Record layout: word 0 = count n; words 1..n =
payload (uint); words 0xD+2k = key1 (float) and 0xE+2k = key2 (float) for k = 0..n-1.
Algorithm (selection, from the end): for p = n-1 down to 1, find m in [0, p] with the smallest key1, scanning
downward from p-1 with a strict less-than (so among equal minima the highest index is chosen; ties
starting from the pivot p keep p), then swap payload, key1 and key2 of m and p.
Result: key1 is in DESCENDING order (index 0 largest), and the lowest key is at index n-1.
The consumer 0x6F490C80 pops from the end, so the unit with the smallest key is processed first (CONFIRMED
from its loop, LIKELY meaning: lowest key first).
Test: 300 random arrays (n = 0..10, keys drawn from a few values to force ties) against a Python port of the
same loop, 0 mismatches.

## 4. Waypoints (partly resolved)

Path object P (the movement state; LIKELY the object at unit+0xA8, pass 1). Fields (CONFIRMED use):
- P+0x40: waypoint array, 8-byte entries (x, y) floats.
- P+0x50: count of waypoints.
- P+0x74: current waypoint index. Reset to -1 by 0x6F49C990 after advancing (CONFIRMED store of 0xFFFFFFFF).
- P+0x78: segment counter, used by 0x6F49C2E0 (which subtracts 2 per advance, LIKELY) and 0x6F49C100 (LIKELY
  recomputes the count).
- P+0x94: wait counter (set to at least 20 on blocked results, CONFIRMED stores of 0x14).

Functions:
- 0x6F49C930 (CONFIRMED): if index >= count, check the goal with 0x6F49BB90 (returns 0 when not reached);
  otherwise write the waypoint at P+0x40 + index*8 and return 1.
- 0x6F49C990 (LIKELY advance): when P+0x78 != 0, optionally test the position with 0x6F49C8D0 against the
  radius 0.49 (0x6FAAE4CC) when its flag argument is 0; then 0x6F49C2E0 (segment advance); if that fails, set
  P+0x94 to at least 20 and return 2; else set P+0x74 = -1 and return whether a segment was loaded.
- 0x6F49C3B0 (CONFIRMED structure): compares its distance argument with MUL(0.49, 0.49) = 0.2401 (softfloat;
  checked). Returns 0 when the distance is larger (not reached / path is off), 2 when a segment is pending and
  blocked (sets P+0x94 >= 20), 4 otherwise. So 0.49 is a squared radius (0.49^2 = 0.2401), used to decide
  whether the path is still valid, NOT an arrival radius.
- 0x6F493EA0 (CONFIRMED constant 0.2, CONFIRMED test): arrival test returns 1 when the flag holds and
  |residual| <= 0.2 (not squared). This is the arrival radius (in the path units, cells per pathing-2) LIKELY.
- 0x6F49C2E0 (LIKELY): pops one segment pair. Returns 1 on success and writes *out = 1 when a next point exists.

Open: the exact rule for choosing the next index (P+0x74 incremented where?), and the meaning of P+0x78. Not
resolved in this pass. The rule is "advance after the 0.2 arrival test passes", LIKELY, not verified.

## 5. Per-unit integrator 0x6F494200 (partly read; LIKELY turn/steer step for the 0x438 list)

- Class: (unit+0xD8 >> 8) & 0xF gives a row index; the row is 16 bytes at 0x6FAB7440 + row*16 (four float32
  values: T0 at +0, T1 at +4, T2 at +8, T3 at +0xC). The table values are small (about 2e-7 to 2e-6, LIKELY
  rates per tick). Their meaning is not decoded here.
- The heading difference is wrapped by 0x6F496630 with pi at 0x6FAAE600 = 3.1415927 (CONFIRMED value), and
  the magnitude is compared with 0x6FAB7540 = 0.001 (0x3A83126F; **correction 2026-10-11, runtime-constants audit**: the earlier value 3.4e-8 was wrong, the start-up value is 0.001). Below it, the function calls the
  unit's virtual at slot 0x58 (no turn, LIKELY "arrived").
- Above it: the magnitude is clamped against T1 and T2 with DIV (0x6F6EFE10) and pow (0x6F6F0810), then
  0x6F493910 is called and the result written to the output. Full formula UNSURE.
- So the turn rate per tick comes from the class table and the pow/DIV step, not from the setter values alone.

## 6. Speed: per-second value and writer of unit+0x88 (NOT resolved)

- GetUnitMoveSpeed (0x6F3C5810) -> 0x6F26DE20 -> 0x6F201C60 -> 0x6F2019F0 (CONFIRMED chain). 0x6F2019F0 returns
  the effective speed in the native's units (per second, LIKELY) from the ability fields (+0x70, +0x78 and the
  0x6F077590 lookup), clamped to per-class ranges read from data 0x6FAB4440/0x6FAB4448 (and 0x6FAB443C/
  0x6FAB4444 for the other class), with defaults 0x6FAB4450/0x6FAB4458 and caps 0x6FAB3AD0/0x6FAB3AD4
  (CONFIRMED: the code reads these; the values were not read).
- The per-tick conversion (per-second value -> object+0x88 step length) is NOT in 0x6F2019F0, and no store to
  +0x88 exists in 0x6F20xxxx-0x6F4Fxxxx (capstone scan). So the writer is outside this range or is reached
  through a virtual (LIKELY: the call 0x6F201D50 -> vtable slot 0x18 of the object from 0x6F472890).
  UNSURE. This is the first open item for pass 3.
- Acceleration: none found. Speed is recomputed each tick from the effective value (LIKELY).

## 7. Collision and priority (pass 1 section 5, unchanged)

- 0x6F490C80 builds the keys and sorts them with 0x6F48F2D0 (section 3.4). Processing order: lowest key first.
- Skip flag 0x200000 for lower-priority units near higher ones (CONFIRMED structure).
- No push-apart found (CONFIRMED absence in these functions).

## 8. Open items for pass 3 (in order)

1. The feeder of the list at world+0x3B8 (section 3.3). Without it the 0x6F493410 path is not reached.
2. The writer of object+0x88 and the per-second to per-tick conversion (section 6). Start at the object from
   0x6F472890 and its vtable slot 0x18.
3. The grid object G initialisation (0x6F483270) to get s = G+0x48 (is it 32?).
4. The next-index rule in 0x6F49C990 and 0x6F49C2E0 (section 4).
5. The full turn formula in 0x6F494200 (section 5).
