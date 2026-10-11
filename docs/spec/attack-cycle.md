# Attack cycle in Warcraft III 1.26a Game.dll (clean-room behaviour spec)

Status: static analysis of the decompile export (decomp/chunk_*.c, natives.c, vtables.tsv, vfunc_names.tsv) and the existing docs. The original was not run in this pass. No Unicorn test was run for this document (listed in section 9 as the next step). The damage formula itself is in docs/spec/attack-damage.md and is not repeated here.

Tags: CONFIRMED = read directly in the decompile or a table; LIKELY = strong inference from names, call shape or constants; UNSURE = open or contradicted by a second reading.

Addresses are 1.26a. Names like FUN_x are decompiler names; the meaning given here is our reading.

Important caveat on the decompile: several call sites pass a float by pointer (for example the timer helper receives the delay as a pointer, and the constant 0.1 appears as the address of the float). Some operands in the schedule helper are shown as unrecovered registers. Where that matters the claim is tagged UNSURE.

---

## 0. Summary (plain English)

1. A unit's attack is one object, CAbilityAttack (vtable 0x6F8B252C, 195 slots). It is driven by messages: the object reads a message id at offset +8 and dispatches it through slot 3 (0x6F0D16E0). Attack orders, timers and sub-tasks all arrive as message ids in the 0xD0xxx range.
2. Starting an attack (order 0xD000F) validates the target, then either starts the swing (if in range and ready) or sends a move sub-task (follow the target, or move to the point).
3. The swing is gated by three checks in 0x6F0CD350 and 0x6F0CF660: a cooldown value read from the attack's timer sub-object at +0x1D8 (must be zero or less), a range test (0x6F4766E0), and a facing test (0x6F476580 against the Misc key "AttackHalfAngle").
4. Timing is done with agent timers (0x6F4778F0, float seconds, see AGENTS 4.3h). Two timer messages matter: 0xD01B0 (cooldown end / re-arm) and 0xD01B1 (execute the hit). The execute timer is capped by a 0.01 s constant (LIKELY).
5. Between "in range" and "hit" there is a random reaction delay (message 0xD0163 starts it, 0xD01AF ends it). It multiplies a uniform real by the Misc key "ReactionDelay" (LIKELY).
6. The execute step (0x6F0CF660) rolls the damage (GetDamageAmount, 0x6F0C6940) and then switches on the per-weapon phase at +0xDC + 4*weapon. Phase 1 applies damage at once through the target's slot 0x120. Other phases create a missile (0x6F0408B0 family) and damage is applied on impact.
7. Missiles are CMissile objects (vtable 0x6F89147C, base CBullet/CBulletBase). Impact is checked by CMissile slot 45 (0x6F2C42D0). The per-tick move is CMissile slot 47 (0x6F2BECB0). The impact time arithmetic is NOT decoded.
8. Damage enters the target through UnitDamageTarget (0x6F3C8850) and the target's slot 0x120, which is 0x6F2A40D0 (CONFIRMED). The attacking ability is itself a damage handler: slot 62 (0xF8) returns its category and slot 63 (0xFC, 0x6F0CEA80) is the counter-attack (retaliation) check.
9. Target acquisition for idle units was NOT found. What is known: acquire range is stored on the attack object (+0x244), the retaliation gate is 0x6F0CEA80, and the generic spatial helpers are used only by group natives or filter builders.

---

## 1. Object and message layout

- CAbilityAttack vtable at 0x6F8B252C; slot n is at the n-th entry (line n+1 of the vtable list). CONFIRMED.
- The attack object holds: unit back-pointer at +0x30 (CONFIRMED read in many functions), per-weapon arrays indexed by weapon w in {0,1}: phase at +0xDC+4w, timer at +0xF4+4w with count at +0xF0, damage param arrays at +0x88, +0x94, +0xA0, +0xAC (CONFIRMED, see attack-damage.md), range at +0x258+8w, second range-like float at +0x26C+8w, flags at +0x20, current weapon at +0x2B8, target handle at +0x2C0/+0x2C4 region (LIKELY), acquire range at +0x244 (CONFIRMED read by 0x6F276A00), timer sub-object pointer at +0x1D8, sub-task objects at +0x1AC, +0x1C4, +0x1D8, +0x1EC, +0x200, +0x240, +0x248, +0x278, +0x280, +0x294, +0x2A8, +0x2B0, +0x2C8, +0x3D8 (LIKELY: task/vtable objects; their roles are not named).
- Message dispatch: slot 3 = 0x6F0D16E0 (CONFIRMED). Cases (message id -> handler, CONFIRMED from the switch):
  - 0xD0002, 0xD0003 (smart), 0xD000F (attack) -> 0x6F0CC090 (attack order start)
  - 0xD0010 -> 0x6F0C8710; 0xD0011 (attack once) -> 0x6F0CC610
  - 0xD0013 and 0xD014B -> 0x6F0C5450
  - 0xD0144 (stop) -> 0x6F0CB3C0
  - 0xD0148 / 0xD0149 -> 0x6F0CB760(0) / (1)
  - 0xD014A -> 0x6F0CCD50
  - 0xD014C / 0xD014D -> 0x6F0CB720(0) / (1)
  - 0xD014E -> 0x6F0CB4A0; 0xD014F -> 0x6F0CB7D0
  - 0xD0161 -> 0x6F0CC910; 0xD0163 (reaction delay start) -> 0x6F0CB340
  - 0xD0165 -> 0x6F0CD330; 0xD0166 -> 0x6F0C8A20; 0xD0168 -> 0x6F0CC770; 0xD0169 -> 0x6F0CC620
  - 0xD016A -> 0x6F0CF010; 0xD0196 -> 0x6F0CF5C0(1)
  - 0xD01A2 -> 0x6F0CD230; 0xD01A4 -> 0x6F0D0710; 0xD01A5 -> 0x6F0CF300; 0xD01A6 -> 0x6F0CD150
  - 0xD01AD -> 0x6F0C8B70; 0xD01AE -> 0x6F0CB520; 0xD01AF (reaction delay end) -> 0x6F0C8900
  - 0xD01B0 (swing timer) -> 0x6F0D0C70
  - 0xD01B1 (execute timer) -> 0x6F0D0D30, which calls 0x6F0CF660 (CONFIRMED)
  - 0xD01B2 -> 0x6F0CD7B0; 0xD01BD -> 0x6F0C5600; 0xD01C1 -> 0x6F0D1380; 0xD01C4 -> 0x6F0C9E30
  - 0xD02A3 -> 0x6F0CB7A0; 0xD02A4 -> 0x6F0CCE20
- Sub-task primitives used by the attack start (names are LIKELY, behaviour CONFIRMED by the call pattern): 0x6F295EE0 (start a sub-task with a message id and a target), 0x6F295CC0 (start a sub-task with a point), 0x6F295BC0 (wait for id X, cancel on id Y), 0x6F295AC0 (cancel a listener), 0x6F296200, 0x6F2963F0 (set a bool/state), 0x6F29ED60 (drain the order queue).
- Sub-task ids used by the attack start: 0xD0170 (follow target), 0xD016B (move to point), 0xD016E, 0xD016F, 0xD0178, 0xD0144 (stop), 0xD0148, 0xD0149, 0xD014A..D014E, 0xD0161, 0xD0163, 0xD0166, 0xD0168, 0xD016A (all CONFIRMED as ids passed to these primitives; meanings LIKELY from the listing in section 2).

## 2. Attack start (order 0xD000F): 0x6F0CC090

CONFIRMED structure, LIKELY meaning:
1. Reads the target from the order payload (order record +0x0C..), then clears attack flag 0x40000 at unit+0x20.
2. Tests the target: if the target is invalid or flagged (target flag bit at +0x3 & 0x2), the attack is refused. The vtable call 0x13C on the target and flag 1 at +8 are checked.
3. Asks a helper (0x6F271600) whether the attack can proceed; if it returns zero, it calls 0x6F0CBD70 on the target (a validity check that returns zero when the target is usable).
4. If the order was a direct attack on a unit (order id 0xD000F and a target), it tries to issue an attack-related sub-order through the unit's order list (0x6F020C00 and 0x6F0C5E30 with 0xD0003, meaning a smart order with the same target). If that succeeds the function returns.
5. Otherwise it posts the sub-task list: wait for 0xD0166 and 0xD000F, cancel 0xD014A, then follow the target (0xD0170) when the target is a unit that moves, or move to the target's point (0xD016B) when not. The "swing" state then waits for 0xD0148.
6. If the target is already in range and ready, the code posts 0xD0148 / 0xD014C / 0xD014E and the swing machinery (0xD0168, 0xD016A, 0xD016F, 0xD0163 for the reaction delay, 0xD014C, 0xD014E, 0xD0178, 0xD0144) takes over.
7. Priority and class: the unit's class and the flag 0x100 (unit+0x5C bit) decide whether a weapon-specific request is used (LIKELY).

## 3. Readiness, range and facing gate

### 3.1 Cooldown gate (CONFIRMED structure)
- Function 0x6F0CD350 ("try to start the swing") first calls the virtual slot 0x18 of the timer sub-object at +0x1D8. The call returns a pointer to a float holding the remaining cooldown in seconds.
- If that float is greater than 0.0 the function returns at once. So a value of zero or less means ready; a positive value means still cooling down. CONFIRMED.
- The same timer sub-object is used by 0x6F0CB9E0 (schedule helper, section 4). LIKELY: it is the attack's cooldown timer.

### 3.2 Range (LIKELY, polarity CONFIRMED from the call site)
- Range argument: 0x6F0C6B30 returns the weapon range for weapon w. Its value comes from this+0x258+8w, or from the +0x2CC override when 0x6F4786B0 / 0x6F089000 say so (CONFIRMED read; the exact condition is LIKELY a per-unit range override).
- 0x6F0CF660 and 0x6F0CD350 raise the range floor: the range used is max(weapon range, DAT_0x6FAB006C) where DAT_0x6FAB006C = 32.0 (runtime constant table, CONFIRMED value). That is one pathing cell (32 world units). LIKELY, but the pointer-selection code is unusual; see UNSURE list.
- The per-weapon float at +0x26C+8w is added to that range (CONFIRMED add through 0x6F6EF000; LIKELY the "RngBuff" bonus).
- Target radius: the target's virtual slot 0xB8 (CUnit slot 46) returns a float that is passed as the second argument of the range test. LIKELY the collision size.
- The range test is 0x6F4766E0 (args: range pointer, target radius, flag 1). It returns nonzero when the attacker may hit now. CONFIRMED by the call site in 0x6F0CF660: a nonzero result skips the move path and goes straight to the execute path.
- Inside 0x6F4766E0 (LIKELY): the attacker's and the target's collision values are read at +0x90 of both units; the larger one, floored at 0.49 (DAT_0x6FAAE4CC = 0.49, runtime value CONFIRMED), is used; the squared centre distance is computed with 0x6F6EF3B0 (softfloat dist2, CONFIRMED routine in AGENTS 4.3g) in either world units (+0x78) or cell units (flag 1 path, through 0x6F497460 and 0x6F38B0B0); the result is compared with the squared range. The final "approx zero" test uses the epsilon DAT_0x6FAAE47C = 0.001 (runtime value CONFIRMED). The exact inequality (strict or not, which side gets the collision size) is UNSURE.

### 3.3 Facing (CONFIRMED call, LIKELY semantics)
- In 0x6F0CD350, after the range test passes, if the helper 0x6F278170 returns nonzero the code reads the Misc key "AttackHalfAngle" (CONFIRMED string) and calls 0x6F476580 with the target position. A zero result means "not facing" and the swing is not started (it jumps to the not-ready exit 0x6F0CB9E0 / 0x6F0CCB00). The condition under which 0x6F278170 is nonzero is UNSURE (probably "the unit must turn to face the target before striking").
- Facing data for movement is in docs/spec/movement.md (heading from the step vector, instant turn).

## 4. Swing timing: schedule helper and timer messages

### 4.1 The schedule helper 0x6F0CB9E0 (LIKELY)
- Called as schedule(delay, flag) with the delay passed by pointer. Call sites: 0x6F0CC620 (delay constant 0.1), 0x6F0CC770 (delay 0.1), 0x6F0CD330 (delay constant DAT_0x6FAAE4D0 = 0.5? see below), 0x6F0CD350 (delay from the attack timer), 0x6F0D0C70 (delay 1.4e-45 = integer 1 as a float bit pattern, flag 1), 0x6F1DB0A0 (in another class).
- It reads the attack timer's remaining value X through +0x1D8 slot 0x18 (CONFIRMED call). If the delay is below X, it schedules message 0xD01B0 at X (CONFIRMED timer message id), or runs at once if X is not positive (LIKELY). This is the "swing finished" timer.
- If the flag is nonzero, it schedules message 0xD01B1 (execute) at min(delay, DAT_0x6FAAE480) (CONFIRMED constant 0.01 s; the min is LIKELY from the clamp pattern and UNSURE in direction). The execute is therefore never later than 0.01 s after the call when flagged, unless the delay is smaller.
- Timer primitive: 0x6F4778F0(float *delay, message id, object, 0, 0), a CAgentTimer start (AGENTS 4.3h, CONFIRMED that timers take f32 seconds and order by expiry then sequence).
- When the attack timer is not yet expired the code does not schedule the execute (it returns or uses 0x6F477D30 for the immediate case).

### 4.2 Swing end (0xD01B0 -> 0x6F0D0C70) (LIKELY)
- If the reaction delay flag 0x20000 (unit object +0x20) is clear and the "ready/ in-range" flag 0x200 is set: compute the target point (0x6F26F8E0), set the point (0x6F281580), call 0x6F0C8930 and 0x6F0C8D70 (weapon bookkeeping), then re-arm the execute with 0x6F0CB9E0(..., 1), i.e. the execute is scheduled again.
- If the flag 0x200 is clear but the unit has a valid weapon mask, it calls 0x6F0CF5C0(0), which re-selects the weapon / target (0x6F0CBD70, then 0x6F0CD350 on success or 0x6F0CCB00 to stop).
- If the reaction delay flag is set, nothing happens (the reaction end handler will finish the job).

### 4.3 Reaction delay (0xD0163 -> 0x6F0CB340; end 0xD01AF -> 0x6F0C8900) (CONFIRMED structure)
- Sets flag 0x20000 at +0x20. Reads the Misc key "ReactionDelay" (CONFIRMED string, value not in the container). Draws a real in [0,1) with 0x6F2853D0 (RNG; the stream used is UNSURE; the other per-stream real helpers are in rng.md and combat is stream 2 per AGENTS). Multiplies by ReactionDelay with the softfloat multiply 0x6F6EEE20 (CONFIRMED helper). Schedules 0xD01AF after that time.
- The end handler clears 0x20000 and calls 0x6F2840A0 and 0x6F29ED60 (drain queue). LIKELY it resumes the swing.
- So: an attack is delayed by a random reaction time whose scale is the Misc key. LIKELY, but whether the draw happens on every attack or only when the attacker was idle is UNSURE.

### 4.4 Execute (0xD01B1 -> 0x6F0D0D30 -> 0x6F0CF660)
- 0x6F0D0D30 reads the weapon from +0x2B8 and the target from the current target handle and calls 0x6F0CF660(target, weapon, flags, 1, 1, 1). CONFIRMED.
- 0x6F0CF660 sets this+0x2B8 to the weapon, rolls the damage through 0x6F0C6940 (GetDamageAmount; the roll itself is in attack-damage.md), stores the result, then switches on the phase at +0xDC+4w. The damage roll happens BEFORE the phase switch, so the amount is fixed at execute time for every phase. CONFIRMED order inside the function.
- Phases (CONFIRMED switch values; meanings LIKELY):
  - Phase 1: instant hit. Uses FUN_6F2BEAC0 (a check) and then the damage call through the target's slot 0x120 (UnitDamageTarget path, see section 6). If the target is not valid after the hit, it calls 0x6F2764C0 / 0x6F0774E0 to decide the path.
  - Phase 2: ranged missile. Creates a missile record (0x6F0408B0, then 0x6F040830 for the parameters), 0x6F0C54C0 and 0x6F0C5510 set the spawn point, 0x6F0C60A0 gets the homing or launch object, then 0x6F276560 checks the launch and 0x6F0C77A0 / 0x6F0CDEC0 / 0x6F2C3F70 clean up.
  - Phase 3: missile with ground or splash targeting. Uses 0x6F08D280, 0x6F0C71C0, 0x6F476670, 0x6F022190 and 0x6F07C450 (a flag check that sets the fStack value to 0.0). If the unit has flag 0x200 it uses the variant with the point from 0x6F26F8E0 (0x6F0409A0 / 0x6F040A60), otherwise 0x6F0409F0 / 0x6F040AA0. Then 0x6F2767E0 checks the launch.
  - Phase 4: no hit. Sets the status float to 1.4e-45 (a flag that changes the damage table column, see attack-damage.md, LIKELY "ethereal / magic-immune" target).
  - Phase 5: missile variant via thunk to 0x6F0408B0 with 0x6F09A730 pre-call; launch check 0x6F276600; cleanup 0x6F0C7800 and 0x6F0CDF40.
  - Phase 6: missile with the unit's per-weapon fields (+0x100, +0xBC, +0x238 pointer, +0x28 of that object) through 0x6F040900; launch check 0x6F2766A0; cleanup 0x6F0CDFC0. Extra flag 0x8000 is set when the target is ethereal (0x6F285AC0 / 0x6F285B80).
  - Phase 7: missile variant through 0x6F040950 (per-weapon offsets +0xBC, +0x134, +0x11 slots), launch check 0x6F276740, cleanup 0x6F0CE040.
  - Phase 8: missile with point from 0x6F26F8E0 or the default (0x6F040A60 / 0x6F040AA0), launch check 0x6F276880, cleanup 0x6F0CE140.
- After the switch the function calls 0x6F4C48F0 (cleanup), and then releases the target reference (the refcount at +4 of the target object).
- Phase numbers are stored per weapon at +0xDC; the unit-type record decides them (attack-damage.md section 2: weapType/atkType; mapping to phase numbers UNSURE).

## 5. Projectiles (CMissile)

### 5.1 Classes and vtables (CONFIRMED)
- CMissile vtable: 0x6F89147C (50 slots). Base classes in the RTTI: CBulletBase and CBullet (slots 3, 14, 15, 39, 41, 43, 44, 48, 40, 8, 9). Per-missile-type subclasses (CMissileDeathCoil, CMissileDrain, CMissileForkedLightning, ...) have their own tables (0x6F082B50 .. 0x6F0A1810 etc.).
- Factory: InstanceGenerator for CMissile at 0x6F932510 (CONFIRMED).

### 5.2 Creation
- 0x6F0408B0 (CONFIRMED): stores the source handle at missile +0x6C, zeroes +0x70 and +0x74 to the float 0.0, clears +0x78, then calls 0x6F040830.
- 0x6F040830 (CONFIRMED structure): reads the speed, arc and art values through the virtual getters on the ability (slots 0x2D0, 0x2CC, 0x2C4, 0x2C8), with 0x6F040760 as the art fallback when the parameter is 0. It then creates the object with 0x6F02E260. LIKELY: slot 0x2C4 = speed, 0x2C8 = arc, 0x2CC = homing, 0x2D0 = art (not confirmed by name).
- Getter names (CONFIRMED from the function index): CAbility::GetMissileArt 0x6F040760, GetMissileSpeed 0x6F0407B0, GetMissileArc 0x6F0407E0, GetMissileHoming 0x6F040810. Each first calls 0x6F02E640 and falls back to the unit or type value through 0x6F32C8E0.
- Strings present (CONFIRMED): "Missilespeed", "Missilearc", "Missileart". These are the unit-type/ability keys; the key-to-field mapping (speed in world units per second or per step) is UNSURE.

### 5.3 Per-tick update (partly CONFIRMED)
- CMissile slot 47 = 0x6F2BECB0: calls 0x6F2BBA80 (a get on the missile's owner or target, UNSURE which), then 0x6F2BE8B0 with the float 1.0 (LIKELY: advance the missile by one unit of its per-step motion), then calls slot 42 (0x6F2BD010) (CONFIRMED call; slot 42 calls 0x6F2AB2D0 and 0x6F2BCCC0).
- Travel time and homing arithmetic are inside 0x6F2BE8B0 and the per-class update functions. NOT decoded in this pass. So the per-tick impact time (distance / speed, arc height, homing turn) is UNSURE. Candidate: speed in world units per second times the 0.1 s step, as for the mover (movement.md), but unverified.

### 5.4 Impact
- CMissile slot 45 = 0x6F2C42D0 (CONFIRMED structure, LIKELY meaning): if the missile is not already marked (flags 0x20000000 and 0x10000000 clear) and flag 2 is set, it gets the target (0x6F2BBA80), compares the target's position kind (slot 0x1C) with 0x6F26C1C0, then checks whether the target is still valid (0x6F284EE0 with 4, 0x6F0774E0). On success it sets 0x20000000 (LIKELY "hit") and calls 0x6F2C2AF0 (apply the impact).
- Damage on impact: the rolled amount from section 4.4 is applied through the damage path (section 6) at this point, not at launch. LIKELY for missile phases; phase 1 is applied at once.
- Specific missile classes have their own damage method: CMissileDeathCoil::DealDamage 0x6F08EC30 (CONFIRMED name), CMissileCarrionSwarm::DamageTarget 0x6F1DD3A0 (CONFIRMED name). The generic impact path for plain attack missiles (phase 2..8) is not named.

## 6. Damage application and events

- UnitDamageTarget native 0x6F3C8850 (CONFIRMED in natives.c): it calls 0x6F2556E0 with the damage type and then the target's slot 0x120, which is 0x6F2A40D0 (CONFIRMED by the CUnit vtable, slot 72).
- UnitDamagePoint native 0x6F3C8770: schedules the same damage at the damage point through 0x6F088050 or 0x6F1D4440 depending on 0x6F01FD60 (CONFIRMED from attack-damage.md; the damage-point timer is the same timer machinery as section 4).
- 0x6F2A40D0 (target side) runs: invulnerability, then pass 1 over the handler list (slot 0xF8 returns a category, slot 0xFC applies), then pass 2 over categories 2 to 8, then armor, type table, ethereal, relation factor, then the final HP update through 0x6F29B730 and the target's slot 0x128 (CONFIRMED structure; attack-damage.md section 4).
- The attack ability is itself a damage handler. CAbilityAttack slot 62 (0xF8) = 0x6F0CE630 (a small getter, category) and slot 63 (0xFC) = 0x6F0CEA80 (CONFIRMED vtable slots; the category value is UNSURE).
- 0x6F0CEA80 (retaliation / counter-attack on damage, LIKELY): the first field of the damage event is the attacker. It returns 0 at once if: the unit's counter at +0x3C is positive; flag 0x100000 or 0x200000 (unit+0x5C) is set; the unit's pending count at +0x54 is positive; the attacker is null or flagged (+0xC & 2); the attacker fails 0x6F0C7240. Then it checks the unit's current order: if the unit has an order (flag 4 at +0x5C), only orders 0xD000F, 0xD0011, 0xD0012, 0xD0032 (with a check on a tag at +0x30) and 0xD0005 let it counter-attack; otherwise it returns 0. Then it may move (0x6F0C5300 setup, 0x6F021A00, 0x6F0CBD70, 0x6F26F300), tests range with 0x6F4766E0, and if in range and no blocking flag it sets the target (0x6F0CB1B0) and either 0x6F297C80 + 0x6F0CD900 (issue the attack) or 0x6F2A4B80(1). Flag 0x800000 or 0x400000 changes the path to a deferred attack. CONFIRMED structure; LIKELY meaning.
- Event firing: 0x6F2834E0 gates on a global flag (DAT_0x6FAB65F4) and calls 0x6F3A1650 with the unit type (CONFIRMED). This is called from the damage path. LIKELY it fires the "unit damaged" / "unit attacked" broadcast, but the event ids are NOT identified in this pass (UNSURE). The orders notifications 0xD0144 (stop), 0xD02A6 (order issued notification) and the JASS event ids 38-40 / 75-77 are listed in docs/spec/orders.md; they are not the damage events.
- The internal timer ids (0xD01AF, 0xD01B0, 0xD01B1) are NOT JASS event ids; they are message ids of this object.

## 7. Target acquisition

### 7.1 Acquire range (CONFIRMED storage, UNSURE initialisation)
- Acquire range of a unit is stored on its attack object (unit+0x1E8) at +0x244 as a float (getter 0x6F276A00 returns that value or 0.0 when there is no attack object; CONFIRMED).
- Native GetUnitAcquireRange 0x6F3C5D10 returns that value (0.0 when the unit is invalid). Native GetUnitDefaultAcquireRange 0x6F3C5DC0 reads a value from the unit type through 0x6F29C090 and 0x6F29A8C0 (+0x194 of the type record; LIKELY the default).
- The unit-type key "acquire" is read by the unit-type loader 0x6F263850 through 0x6F25BA00 into the type record at +0x5E4 (CONFIRMED key and offset from the loader call). The copy into the attack object's +0x244 is UNSURE (it is not in the functions read here).
- Native SetUnitAcquireRange 0x6F3C5CF0 calls 0x6F26D620 with the value. 0x6F26D620 only acts when the unit has an attack object AND the value compared with DAT_0x6FAAE568 (= 50.0, runtime value CONFIRMED) is in the direction "50.0 < value". It then calls 0x6F0C55C0, which calls the attack object's slot 0x240 with flag 1. CONFIRMED read. This is SURPRISING: it would mean an acquire range of 50.0 or less is never stored by the native. The compare direction may be mis-recovered by the decompiler. Needs a runtime test (set acquire to 10 and read it back).

### 7.2 Idle acquisition driver (NOT FOUND)
- Not found in this pass: the function that scans for targets when a unit is idle or on attack-move (AttackMoveXY 0x6F2DDC10 and AttackMoveKill 0x6F2E0A40 are AI natives and are the likely entry points, but they were not read).
- Spatial helpers seen: 0x6F467570 (spatial dispatcher) is called only from group natives (0x6F3E7C70 wrappers, 0x6F3C41A0 / 0x6F3C4230 for GroupEnumUnitsInRange and similar) and from filter builders (0x6F2A3270, 0x6F2AE9E0 through 0x6F476DC0 -> 0x6F47FAF0). CONFIRMED callers.
- 0x6F47EC80 (range enumeration, used by 0x6F47FAF0 when the range flag is set) accepts a unit iff squared distance <= squared radius (CONFIRMED in groups-events-2.md, 900/900 Unicorn). Cell walk x-major then y ascending (CONFIRMED, groups-events-2.md). So if idle acquisition reuses the group range query, the candidate order would be cell walk order, and ties would go to the first candidate in that order (LIKELY, not verified for acquisition).
- 0x6F26F020 is a boolean "any unit in a region" check (returns 0x47 when found). It is NOT the acquisition scan (CONFIRMED by its body and callers: it is used through a vtable table at 0x6F2A50C0).
- Scan period, selection rule (closest, priority, threat) and RNG use: UNSURE. No stream call was seen in 0x6F0CEA80. The candidate function (section 6) takes one candidate, not a list, so the selection happens in the caller, which was not found.

### 7.3 What the candidate check does (CONFIRMED structure)
- The retaliation check 0x6F0CEA80 (section 6) is the only per-candidate gate found. The range test there is the same 0x6F4766E0 as the swing gate.

## 8. Facts about the cycle that are CONFIRMED vs LIKELY (ordering rules)

Ordering rules that we believe the original follows (each with the tag):
1. Order accepted -> sub-task posts -> target valid. CONFIRMED that the start function checks validity before any sub-task post.
2. Ready check (cooldown <= 0) before range and facing. CONFIRMED order in 0x6F0CD350 (cooldown first, then range, then facing).
3. Range check before facing. CONFIRMED order in 0x6F0CD350.
4. Reaction delay after the ready/in-range decision, and before the execute timer. LIKELY (0xD0163 is posted from 0x6F0CC090 / 0x6F0CC770 flow).
5. Damage roll before the phase switch, so the amount is rolled at execute time. CONFIRMED.
6. Swing end (0xD01B0) re-arms the execute (0xD01B1) only if the 0x200 flag is set and no reaction delay is pending. LIKELY.
7. Missile damage is applied on impact (slot 45), not at launch, for missile phases. LIKELY.
8. Retaliation (0x6F0CEA80) runs as a damage handler after the target-side pass 1 category check. CONFIRMED that it is in the handler list; LIKELY the timing relative to armor (it runs before armor, since it is in the category passes).

## 9. Open items and the verification plan

1. Find the idle acquisition driver: start from AttackMoveXY (0x6F2DDC10) and the AI natives in natives_ai.c; look for callers of 0x6F0CEA80 through the vtable slot 0xFC from a non-damage context (the search so far only found the damage-path caller).
2. Decode 0x6F4766E0 exactly (strict inequality, which side gets the collision size, the 0.001 check). Unicorn test with fixed unit positions at +0x78/+0x7C and +0x90 values, comparing with a model: in range iff dist2(a,b) <= (max(rangeA, 32) + radius)^2. Requires building the unit fields; the harness is tools/emu/emu.py.
3. Read 0x6F0CB9E0 operands (the decompiler hid two floats). Check with Unicorn by calling it with a fixed attack-timer object and reading the timer messages it schedules.
4. Confirm the 0.01 clamp and 0.1 wait in the schedule helper and the cc770 / cc620 paths (Unicorn or dynamic test under Wine).
5. Identify the stream for ReactionDelay (0x6F2853D0 caller contexts) and the Misc default values (not in the container; read them from the user's Misc.txt).
6. Missile per-tick math: decode 0x6F2BE8B0 and the subclass update functions; check whether speed is per second times 0.1 s. Test with a straight flight case.
7. Set SetUnitAcquireRange to 10 and read it back (runtime test) to settle the compare direction at 0x6F26D620.
8. Identify the JASS event ids fired on damage (the broadcaster 0x6F3A1650 and its event constant) and on attack start / end.

## 10. What is NOT in this report

- No decompiled code is pasted here. Addresses are given for each claim.
- The damage formula, armor, dice and the target-side pass details are in docs/spec/attack-damage.md.
- Projectile per-class damage details beyond the names above are not covered.
- No Unicorn test was run for this document.
