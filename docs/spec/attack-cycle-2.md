# Attack cycle, pass 2 (acquisition, missile arithmetic, reaction-delay stream, range inequality)

Same rules as pass 1 (attack-cycle.md): plain English, addresses, tags CONFIRMED / LIKELY / UNSURE, no pasted code. Read-only: nothing under /home/user/projectx was changed.

Tools used: static decompile reads (fn.py over decomp/chunk_*.c), capstone disassembly of the binary (disasm_va.py), and a Unicorn harness built on tools/emu/emu.py (GAME_DLL = the 1.26a Game.dll, pylib from rngtools). Harness code: scratchpad/atk/range_emu.py, range_sweep.py, range_edge.py, range_flag1.py.

Corrections to pass 1 (found in this pass):
- Pass 1 said the range predicate floors the collision sum at 0.49. Measured in Unicorn: the boundary is the plain sum, also for sums of 0.2 and 0.6 (no floor visible). The 0.49 compare is in the code but its effect is not seen in these tests (UNSURE what it guards).
- Pass 1 said the reaction-delay stream is UNSURE. It is CONFIRMED as stream 9 (section 3).

---

## 1. Idle and attack-move target acquisition (LIKELY overall; key pieces CONFIRMED)

### 1.1 Who arms the scan
- The scan message is 0xD01C1. It is handled by the attack object's dispatcher (0x6F0D16E0 -> 0x6F0D1380). CONFIRMED.
- The message is armed as a timer on the unit by 0x6F04C350 with delay 1.0 s (the .data constant at 0x6FAAF0A0 = 1.0, CONFIRMED value) and flag 1 (LIKELY "repeat/ periodic" flag; the timer primitive's 4th argument, UNSURE meaning). CONFIRMED that the call exists; LIKELY that 0x6F04C350 is the unit idle/ready path (not named).
- Other arming sites: 0x6F035760 and 0x6F04AAA0 call 0x6F02D7D0 / 0x6F02D800 with message 0xD01C1 (generic message post, two variants, meaning UNSURE). CONFIRMED call sites.

### 1.2 The periodic tick 0x6F0D1380 (CONFIRMED structure)
1. Re-arms itself first: the timer for 0xD01C1 is started again with delay 0.5 s (constant 0x6FAB0048 = 0.5) when the unit has no active aux object (mask test at unit+0x6C/+0x70 gives -1), otherwise 1.0 s (constant 0x6FAB004C = 1.0). So the scan period is 0.5 s or 1.0 s. CONFIRMED values; LIKELY meaning.
2. Scan only when: the attack flag 0x2000 is clear (or it was set and cleared, with 0x4000 set), AND the unit's flag 0x200 (unit+0x20) is set (LIKELY "idle/ ready"), AND the attack's counter at +0x220 is below 1 (no pending attack), AND the busy helper 0x6F0D0920 returns zero.
3. If the busy helper returns zero it may run 0x6F0C9E70 (unit-owned case) and then calls the scan 0x6F0CF210.

### 1.3 The scan gate 0x6F0CF210 (CONFIRMED structure)
- Unit class from the unit's vtable slot 0xEC must be greater than 11 (0xB). LIKELY: this is the unit type/ priority class; the enum meaning is UNSURE.
- Unit flags at unit+8 bit 0 clear and unit+0x194 (piVar1[0x65], the unit state word) equal zero (LIKELY "not busy / not in a special order").
- Then: if FUN_6F2A0210(unit) > 6, OR the attack's acquire field at +0x244 is at least 300.0 (constant 0x6FAAE594 = 300.0, CONFIRMED value), the scan runs. So units with acquire below 300 scan only when FUN_6F2A0210 (a count: hero path = 0x6F473170 + 1, otherwise 0x6F29DCA0; meaning UNSURE, LIKELY a count of nearby enemies or a threat count) is above 6.
- The scan itself: 0x6F2A3270 is the unit-group filter builder. Callback = 0x6F0CCFA0. Context = {unit, attack object}. Flags 2 and 0x11. Radius value = the unit's vtable slot 0xEC called with arguments (10, 1500.0, position), where 1500.0 is constant 0x6FAB0058 (CONFIRMED value). So the scan radius is computed by the unit's own slot 0xEC; the exact formula is UNSURE (LIKELY it combines acquire with the unit's range).

### 1.4 Candidate callback 0x6F0CCFA0 (CONFIRMED structure, LIKELY meaning)
Return value: 1 = skip the candidate and continue; 0 = accept and stop (LIKELY; the builder's loop semantics not read).
Order of tests:
1. Attack flag 0x400000 on the attacker set -> skip.
2. The candidate's position via the attacker's slot 0xB8; then 0x6F476C40 with the constant 1500.0 and the leash/guard point (+0x2AC, +0x2B4 of the attack object): a region test. LIKELY: candidate must be within 1500 world units of the guard/ leash centre. Exact test UNSURE.
3. 0x6F0CBD70(attacker, 1, 0): the attack-validity check used everywhere else (same helper as the swing start). Nonzero -> skip.
4. Attack flag 0x2000000 clear and the attacker's order object at +0x1E8/ +0x1EC-area (slot 0x2FC) accepts order id 0xD000F for the candidate -> otherwise skip. LIKELY: "can this unit be ordered to attack this candidate".
5. Candidate flag 0x20000000 clear, or candidate field +0x200 greater than zero: then a metric from 0x6F4758D0 (wrapper to 0x6F475560 with id 0x8A, a point, and mode 3/2) is compared with 46 (0x2E). A larger value skips the candidate. LIKELY a path or distance check in cells (0x6F475560 not read; UNSURE).
6. Accept: attacker flag 0x800000 set, 0x6F1D6100 and 0x6F0787D0 build an event/ order record (LIKELY the "acquire" event), 0x6F2E22A0 notify, then the order object is built with 0x6F294D40 from the candidate's vtable slot 0xEC and issued with 0x6F2A4AB0 (replace = 1, same dispatcher as player orders, see pass 1 section 4.4 of orders.md). Return 0 (stop).

### 1.5 Selection rule
- First candidate (in the builder's enumeration order) that passes section 1.4 wins. No sort by distance, no priority by target type, no tie break. LIKELY. The enumeration order of the range query is the cell walk (x-major ascending, then y ascending; CONFIRMED for 0x6F47EC80 in groups-events-2.md). LIKELY the same order here, since the builder ends at 0x6F47FAF0 / 0x6F476DC0 which calls the same range routines.
- Ties: none needed (first wins).

### 1.6 RNG
- No RNG stream call in 0x6F0D1380, 0x6F0CF210 or 0x6F0CCFA0 (CONFIRMED by the bodies read). 0x6F1D6100 (called on accept) was not read, so RNG use there is UNSURE.
- Conclusion (LIKELY): idle acquisition is deterministic given the enumeration order and the timers.

### 1.7 Acquire field +0x244 (CONFIRMED readers and writers found)
Readers:
- 0x6F0CF210 (scan gate, section 1.3). CONFIRMED.
- 0x6F0D0EA0 (copies the acquire value into a record when the flag at +0x07C450 is set). CONFIRMED.
- Native GetUnitAcquireRange 0x6F3C5D10 through 0x6F276A00. CONFIRMED.
- Other readers in other classes: 0x6F17C6F0, 0x6F17CAC0, 0x6F203520, 0x6F349910, 0x6F48A520, 0x6F0CB660. Not all read; their class is UNSURE.
Writers:
- SetUnitAcquireRange 0x6F3C5CF0 -> 0x6F26D620. Stores only when the unit has an attack object and the value is above 50.0 (constant 0x6FAAE568 = 50.0, CONFIRMED). Then calls 0x6F0C55C0 -> attack slot 0x240 with flag 1. CONFIRMED (the coordinator confirmed the direction in the disassembly).
- 0x6F2CA800 copies the field into another object (CONFIRMED write at +0x244 of a second object; class UNSURE).
- 0x6F357D20 writes it under a condition (CONFIRMED write; class UNSURE).
- 0x6F556B10 writes it from a parameter (CONFIRMED write; class UNSURE).
- Initialisation from the unit type key "acquire" (pass 1) is still UNSURE.

### 1.8 What is still open for acquisition
- The exact body of 0x6F476C40 (region test), 0x6F4758D0/0x6F475560 (the id 0x8A metric and the threshold 46), 0x6F2A0210 (the count) and the unit's slot 0xEC formula.
- Where the first 0xD01C1 timer is armed for a freshly created unit (0x6F04C350 call sites).
- The attack-move (AttackMoveXY 0x6F2DDC10) entry: not yet connected to this scan.

---

## 2. Missile arithmetic

### 2.1 What the per-tick slot 47 really does (CONFIRMED structure, LIKELY meaning)
- CMissile slot 47 = 0x6F2BECB0 (CONFIRMED). It gets the missile's target object through 0x6F2BBA80 (returns a handle from the missile's mask fields +0xA0/+0xA4; CONFIRMED). If the target exists, it calls 0x6F2BE8B0(target-out, target, 1.0). Then it calls slot 42 (0x6F2BD010). CONFIRMED.
- So the per-tick slot is NOT a position update. It is an impact evaluation at the end of the path, using the constant 1.0 as the fraction of the path (LIKELY: fraction 1.0 = arrival point).
- 0x6F2BE8B0 (CONFIRMED structure):
  1. Gets a point from the missile through the missile's slot 0xAC with the fraction (through 0x6F2BCAC0, which also calls the target's slot 0x11C).
  2. If the missile flags 0x20000000 and 0x10000000 are clear, flag 2 is set, the target's slot 0x1C equals 0x6F26C1C0 (a kind check, tiny function, not read), then: 0x6F021A00 gets the missile's source ability; 0x6F284EE0 (canTarget, flag 4) must hold; and 0x6F0774E0 (ability slots 0x160 / 0x16C over the ability list; returns 1 when an ability's predicate holds) must hold. Then the result is 0.0 (the returned time) and the function returns without damage.
  3. Otherwise it calls the target's slot 0x120 (the damage entry, = 0x6F2A40D0 for units, CONFIRMED in pass 1) with the damage record built in step 1, and a flag from the missile flags bit 1.
- Reading: a missile whose ability predicate passes at arrival does not hit this tick (LIKELY: it is the "ability hits on arrival" path, e.g. a spell effect), and otherwise the target takes the damage. LIKELY; the predicate meanings are UNSURE.

### 2.2 Slot 45 (impact check) (CONFIRMED structure; LIKELY meaning)
- Slot 45 = 0x6F2C42D0. Same flag gate (0x20000000 and 0x10000000 clear, flag 2 set). Gets the target (0x6F2BBA80), requires the kind check (slot 0x1C vs 0x6F26C1C0), then 0x6F021A00 (source ability), 0x6F284EE0 (canTarget), 0x6F0774E0 (predicate), and sets 0x20000000 (LIKELY "hit done") and calls 0x6F2C2AF0 (launch / impact bookkeeping). Same predicate family as 2.1.
- So slot 45 is the arrival-side mirror of 2.1: it sets the "hit" flag so 2.1 does not run again. LIKELY.

### 2.3 Trajectory parameters (CONFIRMED pieces, arithmetic NOT decoded)
- 0x6F2C2AF0 (launch): reads the source point (slot 0xB8 of the source, with 0x6F29E6A0 or the ability slot 0xA0 selected by the arc/ homing flag), calls 0x6F2C1B50 (target), 0x6F2BEC80, 0x6F2C2AC0 (set the arc/ speed bookkeeping), then slot 0xE4 and slot 0x118 for the endpoint, and 0x6F47A880 with a flag selecting between two stored values through 0x6F2BCC50. CONFIRMED structure. The formulas are inside these helpers, not read.
- CBulletBase slot 43 = 0x6F2BB590: copies two floats, each computed as one softfloat multiply (0x6F6EEE20) of the input value with a parameter (CONFIRMED structure). LIKELY: position at fraction t = point * t with the missile's stored coordinates. Not Unicorn-tested.
- Projectile registration: 0x6F4E86C0 gets the callback 0x6F2BB4A0, which copies a point obtained from the missile's slot 0xA0 (CONFIRMED). So the engine moves the missile through its own projectile update (ProjectilePosition vf1 0x6F03BA70 is listed in the vtable table; not read).
- Speed, arc and homing: the getters 0x6F0407B0 (speed), 0x6F0407E0 (arc), 0x6F040810 (homing) and 0x6F040760 (art) are CONFIRMED present. Their values come from the ability / unit keys "Missilespeed", "Missilearc", "Missileart" (CONFIRMED strings). The units (world per second or per step) and the homing turn rate: UNSURE.
- Impact time per tick: NOT determined. The honest state: the engine's projectile system (0x6F4E86C0 family, ProjectilePosition) does the flight; this pass did not read it. Candidate model for a later test: impact at step n = ceil(distance / (speed * 0.1 s)), with the 0.1 s agile step (AGENTS 4.3h). UNSURE.

### 2.4 Unicorn status for missiles
- The missile helpers depend on the live object graph (vtables of missile and target, handle tables). Only CBulletBase slot 43's pure arithmetic was identified as self-contained, and it was NOT run. Missile arithmetic is therefore NOT confirmed in Unicorn.

---

## 3. Reaction-delay RNG stream (CONFIRMED)

- The reaction-delay starter 0x6F0CB340 calls the per-stream real helper 0x6F2853D0 with EDX = 9 (disassembly: mov edx, 9 immediately before the call, CONFIRMED). So the stream is 9.
- 0x6F2853D0 reads the stream state at the subsystem-13 array base + 4 + 8*k with k = its second argument (the EDX value). This matches rng.md section 1 (stream k at base + 4 + 8k). CONFIRMED.
- The real is multiplied by the Misc key "ReactionDelay" (softfloat multiply 0x6F6EEE20), and the result is the timer delay for 0xD01AF on the object at +0x1C4 (CONFIRMED disassembly: timer primitive 0x6F4778F0 with the object at this+0x1C4, message 0xD01AF, delay pointer = the product).
- Reaction delay therefore consumes one draw from stream 9 every time 0x6F0CB340 runs. Whether every attack or only some call it: UNSURE (the callers are 0x6F0CC090 / 0x6F0CC770 flows, per pass 1).

---

## 4. Range-test inequality in 0x6F4766E0 (CONFIRMED by Unicorn, flag 0 and flag 1)

### 4.1 Inputs (CONFIRMED by disassembly)
- ECX = pointer to a handle pair for the attacker (object at +8 and +0xC of the pair); arg 2 = pointer to a handle pair for the target; arg 1 = pointer to a float range value in world units; arg 3 = flag (0 or nonzero).
- Attacker object A and target object B come from the handle lookup 0x6F03FA30 (returns the object; hooked in the harness).
- Collision sizes: A+0x90 and B+0x90 (float). Positions: +0x78 (x) and +0x7C (y) in cell units (CONFIRMED from groups-events-2.md).

### 4.2 Measured rule (flag 0)
- The radius in cells is r/32 (the range value divided by 32; the code does this with an exponent subtract, CONFIRMED by the disassembly) plus the attacker's collision plus the target's collision.
- Accept (return 1) iff the squared centre distance D < T, where T = (r/32 + sA + sB)^2, strict.
- Measured boundaries along an axis, bisection to about 1e-7:
  - (sA, sB, range) = (0, 0, 320): boundary 10.0 (predicted 10.0)
  - (0, 0, 64): 2.0 (predicted 2.0); (0, 0, 32): 1.0; (0, 0, 640): 20.0
  - (1.5, 0.5, 320): 12.0 (predicted 12.0)
  - (0.25, 0, 96): 3.25 (predicted 3.25)
  - (2.0, 1.0, 160): 8.0 (predicted 8.0)
  - (0, 0.3, 10): 0.6125 (predicted 0.6125)
  - (0.5, 0.5, 1): 1.03125 (predicted 1.03125)
- Ties: at exactly 10.0 the result is 0 (rejected). Offsets of +0.0005 and +0.001 are also rejected. So the 0.001 tolerance in the code did not make a rejected tie accepted in these tests; the sense of that branch is UNSURE (it is reached only when D >= T and its effect was not observed).
- Diagonal check (3-4-5 triangle, D = 25): accepted at range 160 (radius 5) and rejected at 156.8 (radius 4.9). CONFIRMED consistent.
- Sum floor: with sA = sB = 0.1 and range 0 the boundary is 0.2 (the plain sum), and with sA = sB = 0 and range 0 nothing is accepted (D = 0 is not < 0). So the 0.49 compare in the code did NOT floor the sum in these tests. Correction recorded above.

### 4.3 Flag 1 path
- Flag 1 uses two extra helpers (0x6F497460 and 0x6F38B0B0) to make the two points. In the harness the world pointer is a zeroed stub, so those helpers run on dummy data. The measured boundaries for flag 1 equal flag 0 in all cases above, and writing +0x70/+0x74 on the objects did not change them. So in the harness the flag-1 point equals the +0x78 point.
- In the real game the flag-1 points may come from the pathing world (LIKELY, from the names). Not confirmed for real units: UNSURE.

### 4.4 Consequence for the attack code
- In 0x6F0CF660 a nonzero result skips the move path and executes the attack (pass 1 statement). This is consistent with "accept = inside range".
- The attack range argument is r = weapon range (floored at 32.0 per pass 1, UNSURE), so the attack reaches when the centre distance is below (range/32 + attacker collision + target collision) in cells.

---

## 5. Open items for the next pass
1. Region test 0x6F476C40 and the candidate metric 0x6F475560 (threshold 46): read and emulate.
2. Unit slot 0xEC formula (scan radius) and FUN_6F2A0210 (count).
3. The projectile flight in the engine (0x6F4E86C0 family, ProjectilePosition): read and emulate with a straight-line test; then the arc and homing.
4. 0x6F04C350: which unit event arms the 1.0 s scan timer, and whether a fresh unit gets one.
5. Flag 1 point helpers 0x6F497460 / 0x6F38B0B0 on a real pathing world (needs world data; not in container).
