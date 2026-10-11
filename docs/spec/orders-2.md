# Order system - pass 2

Same rules as orders.md: plain English, addresses, CONFIRMED / LIKELY / UNSURE. No pasted code.
Read-only on /home/user/projectx. Binary: Game.dll 1.26a (capstone trace and decompile).

Corrections to pass 1 (orders.md)
- C1. Zero-flag point natives: pass 1 said they build a point-class order. The disassembly shows the
  zero-flag branch of FUN_6F3B2F30 builds the TARGET-class constructor (FUN_6F294D40). CONFIRMED.
- C2. Point natives pass arg2 = 0 to the shared path, not 1. CONFIRMED (trace in section 4). The
  dispatcher call always gets (order, 1, 1) from the shared path, so replace = 1, drain = 1 stays.
- C3. Pass 1 said the ability validators compare the order id. They compare arg2 (the constant 0 for
  point natives) with each ability's slot 40. See section 1.

---------------------------------------------------------------------------------------------------
## 1. Point-move and attack validation (question 1) - RESOLVED for move, mostly for attack

### 1.1 How the call reaches the abilities (CONFIRMED, disassembly)
- IssuePointOrderById (0x6F3C8940) and IssuePointOrder (string, 0x6F3C8DA0) call
  FUN_6F3B2F30 with stack arguments: arg1 = the unit's slot-25 value (unit+0x58, read through the
  unit vtable slot 25 at 0x6F28B1E0), arg2 = constant 0, arg3 = y. The order id is in the register that the
  shared path reads as its id (EDI), x in EBX, the unit in ESI. CONFIRMED.
- The shared path (FUN_6F3B2F30) reads the flags word for the id (0x6F372C30), masks it with 0x3FF,
  and tests bits 0x2, 0x4 and 0x8 in that order.
- FUN_6F279560 is called as thiscall on the unit with five stack words pushed in this order: y, x, arg2,
  arg1, id. So its parameters are: id, arg1 (the subject), arg2 (= 0), x, y. CONFIRMED.
- FUN_6F279560 loops over the unit's ability list and, for each ability, compares its slot-40 value with
  its parameter 4, which is arg2 = 0. An ability whose slot 40 returns 0 therefore matches.
  CONFIRMED (code path); see 1.2 for which abilities return 0.
- For a match it calls the ability's slot 151 (+0x25C) with (id, subject, x, y, 3). CONFIRMED.

### 1.2 Slot 40 default (CONFIRMED)
- CAbility base slot 40 (0x6F052250) returns 0. CAbilityMove and CAbilityAttack do not override it
  (vtable scan). Spell and item classes do (for example the family at 0x6F069030), so they match only
  when arg2 is their own id, which in the point path never happens.
- So base abilities (move, attack, and the rest of the CAbility family that do not override slot 40)
  all match the point validator. The check that decides is slot 151 on each match.

### 1.3 CAbilityMove slot 151 (0x6F201FB0), CONFIRMED structure
1. Calls FUN_6F029470 (base point gate, see 1.4). If it returns 0:
   - if id is 0xD0002, 0xD0003 or 0xD000F, it runs a subject check: the owner's slot 59 (+0xEC) on the
     subject, then FUN_6F3A3530. A non-zero check gives 0 (accept), otherwise 0xDD. Other ids return 0.
   - for the other ids (move 0xD0012, AImove 0xD0014, patrol 0xD0016, 0xD0017, ...) it returns 0.
2. If FUN_6F029470 returns non-zero, it returns that code.
So for move (0xD0012), the point validation result is simply the result of FUN_6F029470.

### 1.4 FUN_6F029470 (base point gate, CONFIRMED structure)
1. Calls the ability's slot 192 (+0x300) with the id. CAbilityMove slot 192 is FUN_6F200A10: it returns 0
   for ids 0xD0012, 0xD0014, 0xD0016, 0xD0017, 0xD0002, 0xD0003, 0xD000F, and 0xDD for every other id.
   CONFIRMED.
2. If slot 192 returns non-zero, return that code (family check failed).
3. If slot 192 returns 0, call the ability's slot 154 (+0x268) with argument 0. If that returns non-zero,
   the result is 0 (accept). If it returns 0, the result is 0xB8 (gate closed). CONFIRMED.
So a move point order is accepted iff the id is in the movement family AND the slot-154 gate is open.

### 1.5 Slot-154 gate (base FUN_6F052C30), CONFIRMED partially
The gate needs all of these (in order):
- an execution context object from FUN_6F03FA30 (a handle table lookup); it must exist;
- its tag at +0xC must not equal 0x2B61676C ("lag+"), a guard the same as GetUnitCurrentOrder uses;
- its field at +0x20 must be 0;
- the ability's slot 0x1E0 / 0x1E4 checks must both be 0 (meaning UNSURE, probably availability or
  cooldown); and the ability's field at +0x54 must be non-null (it links to the ability's data record);
- the data lookups FUN_6F40B490 and FUN_6F40FB70 must succeed, and a flag bit (ability +0x20 bit 0x10)
  may force a value of 1.
UNSURE: the exact meaning of each check and what the final success value is. The useful conclusion is
that the point gate depends on runtime context, so an order issued from a context with the lag tag or a
non-zero +0x20 is refused with 0xB8.

### 1.6 CAbilityAttack slot 151 (0x6F0C8160) and its slot 192
- Same shape as the move version: slot 192 (FUN_6F0CAED0) family check, then the gate, then the special
  subject check only for ids 0xD0002 and 0xD0003. For attack id 0xD000F the special branch is NOT taken,
  so the attack point result is the gate result. CONFIRMED for slot 151's code; the family set of
  FUN_6F0CAED0 is UNSURE (not read).

### 1.7 Where the target validator comes in (zero-flag and target branches)
- For flags 0 or bit 0x4 set, the shared path uses the target constructor (FUN_6F294D40) after the same
  point validator. So the table flags decide only the class of the built order, not which validator runs.
  CONFIRMED (branches in the trace).

### 1.8 Rank (slot 146) decides the owner (CONFIRMED values)
Move ability rank by id: 0xD0002 -> 3, 0xD0003 (smart) -> 16, 0xD000F (attack) -> 1, 0xD0012 (move) -> 1,
0xD00BD -> 13, anything else -> 0 (the default). Lower wins. So for a move point order, the move ability
(rank 1) beats smart (16) and 0xD0002 (3). Attack and move tie at rank 1; the first in the list wins.
The rank function is slot 146 (+0x248) of the ability (CONFIRMED use in FUN_6F279A90).

---------------------------------------------------------------------------------------------------
## 2. Where ISSUED_ORDER events fire (question 2) - NOT RESOLVED

What is confirmed:
- The internal kinds (0xD02A5 queued, 0xD02A6 started/cleared, 0xD0144 stop) are sent to the unit's
  notify slot (unit vtable slot 4, a generic function at 0x6F629A90), which calls the object's slot 5
  (0x6F62A7B0, a generic forwarder into the observer core FUN_6F62A5D0 / FUN_6F62A1C0). CONFIRMED.
- Trigger registration for player-unit events uses the raw number; the range check accepts 0x12 to
  0x32 (so 38, 39, 40 are in range). CONFIRMED.
- GetIssuedOrderId and GetOrderPointX switch on 38-40 and 75-77 (see orders.md 3.7). CONFIRMED.

What was searched and not found:
- Literal writes or comparisons of 0x26, 0x27, 0x28, 0x4B, 0x4C, 0x4D inside order functions
  (FUN_6F2A4AB0, FUN_6F29DFF0, FUN_6F279FE0, FUN_6F2A0650, FUN_6F2A0510, FUN_6F2831A0, FUN_6F2A4A10,
  FUN_6F29ED60, FUN_6F2964E0). None. The only hit for 0x26 in that area is inside FUN_6F02F360 (returns
  0x26 for id 0xD0032 when the ability check says 0xDD). LIKELY irrelevant.
- The literal 0x26 found elsewhere is in unrelated game-setting code (chunk_023) or a registration
  table (chunk_014). Not the event site.
- Writers of the current-event slot (offset +0xF4 of the context, read by FUN_6F4280B0 at index 6) that
  store order kinds: not located in this pass.

Hypothesis (UNSURE): the kinds are computed as base 0x26 (player-unit) or 0x4B (unit) plus a class index
(immediate 0, point 1, target 2), since the three values in each group are consecutive and match the
three class constructors. Not verified.

Dispatch order vs execution (UNSURE): FUN_6F29DFF0 fires 0xD02A6 after the ability start call, and the
enqueue path fires 0xD02A5 only when the order waits behind a pending one. If the JASS event is raised
from these internal notifications, it is raised after the ability has taken the order, i.e. at start,
not at submission. Not verified.

Next step to close this: break on the observer core FUN_6F62A1C0 (or its caller 0x6F62A5D0) with a
registered player-unit trigger, and trace which function writes the kind that reaches it.

---------------------------------------------------------------------------------------------------
## 3. Per-tick link: move sub-task -> path request -> mover (question 3) - PARTLY RESOLVED

Mover side (CONFIRMED, from docs/spec/movement.md section 1 and asm of the stub):
- The step is driven by a posted message 0x8026A, posted by FUN_6F426BF0(1) (called from FUN_6F3B0750).
  FUN_6F42FC80 is the message switch; 0x8026A goes to FUN_6F426C30 (chunk_018 line 18634), which calls
  FUN_6F4675C0 (an 11-byte stub that jumps to FUN_6F483300). CONFIRMED.
- FUN_6F483300 walks the mover list (world+0x3B0 and the lists at +0x440 / +0x3B8, see movement-2/3) and
  runs the per-record step FUN_6F4907F0 (movement.md). CONFIRMED.
- Tick rate: LIKELY one message = one step, 10 per second (0.1 s constant at 0x6FAAE4A0); not confirmed
  here.

Path side (CONFIRMED edges):
- FUN_6F49DAC0 (the per-step update of a path object, CLrPath) calls the driver FUN_6F49B9C0 when flag
  0x200000 is set on the path; otherwise it appends a straight waypoint (FUN_6F498770). CONFIRMED.
- FUN_6F49DD00 also calls the driver FUN_6F49B9C0 (chunk_020 line 4856). CONFIRMED.
- The driver calls the dispatcher FUN_6F49A800 (pathing-2 section 4.1). CONFIRMED.
- FUN_6F475950 (move issue, nine arguments) calls FUN_6F49A800 directly (chunk_019 line 30270).
  CONFIRMED. Its callers are FUN_6F165CC0 and FUN_6F1661C0 (chunk_007 lines 10014, 10231). CONFIRMED.

Sub-task link:
- FUN_6F203D60 (move start) pushes sub-tasks 0xD016B / 0xD0170 with FUN_6F295CC0 / FUN_6F295EE0
  (orders.md section 7). CONFIRMED.
- FUN_6F0C7740 (called there) is only a container initialiser that calls FUN_6F0C5970, which is a
  reference-holder assignment (refcount swap). NOT a path request. CONFIRMED.
- The part of FUN_6F203D60 that was read does not call FUN_6F475950 or FUN_6F49A800 directly. So the
  sub-task reaches the path system through a call that was not found (UNSURE). Candidates: FUN_6F165CC0 or
  FUN_6F1661C0 (their callers and their callers are the unit command layer; their relation to the
  ability sub-task is UNSURE).

Who calls what each 0.1 s step (summary):
- Each step: message 0x8026A -> FUN_6F426C30 -> FUN_6F483300 -> per-unit FUN_6F4907F0 (mover, reads the
  unit's movement state at +0xA8).
- Path refresh: FUN_6F49DAC0 / FUN_6F49DD00 -> FUN_6F49B9C0 -> FUN_6F49A800 (only when a flag is set;
  otherwise the straight waypoint append).
- Order sub-task -> mover: NOT linked in this pass (UNSURE).

---------------------------------------------------------------------------------------------------
## 4. FUN_6F3B2F30 argument mapping (question 4) - CONFIRMED by disassembly

Entry (thiscall-style, with the unit and the id in registers set by the caller):
- Stack on entry: arg1 at +4, arg2 at +8, arg3 at +0xC (the caller pushed arg3 first).
- The caller IssuePointOrderById (0x6F3C8940) sets: ESI = unit object; EDI = order id; EBX = x. It then
  pushes y (arg3), then 0 (arg2), then the slot-25 value (arg1), and calls the shared path. CONFIRMED.
- The shared path pushes EBP = arg3 (y) and calls FUN_6F300710 (game object); the game object's +0x3C8
  must be non-null, else return 0. CONFIRMED.
- FUN_6F372C30(id) returns the flags word; the path masks it with 0x3FF; tests 0x2, 0x4, 0x8. CONFIRMED.

Branches (CONFIRMED from the disassembly):
- Flags 0 (id not in the table): call FUN_6F279560 with (id, arg1, arg2=0, x, y). If the result is 0 or
  0xDC, build the TARGET-class order (FUN_6F294D40) and dispatch it. Return 1. (Correction C1.)
- Bit 0x2 set: FUN_6F279560 as above. If the result is 0 or 0xDC: bit 0x4 clear -> build a point order
  (FUN_6F294B30) and dispatch, return 1; bit 0x4 set -> build a target-class order (FUN_6F294D40) and
  dispatch, return 1. Otherwise return 0.
- Bit 0x2 clear, bit 0x8 set: call the unit slot 25 again, run FUN_6F279560, accept 0 or 0xDC, build a
  point order (FUN_6F294B30) and dispatch, then return 0 (the quirk from pass 1, confirmed here).
- Bit 0x2 clear, bit 0x8 clear: return 0.

Constructor and dispatch argument shape (CONFIRMED):
- FUN_6F294B30 is fastcall: ECX = id, EDX = arg1 (subject), then four stack words: arg2 (0), x, y, 0.
- The dispatch call pushes the order object and the two words (1, 1) left on the stack, so the dispatcher
  receives (order, 1, 1): replace = 1, drain = 1. CONFIRMED.

What remains UNSURE: the meaning of arg1 (the slot-25 value, unit+0x58). The point gate in 1.5 and the
subject check in 1.3 both use it.

---------------------------------------------------------------------------------------------------
## 5. Open items after pass 2
1. Event-site search (section 2): find the writer of the kind that reaches the observer core, and the
   computed base-plus-index hypothesis.
2. Sub-task to path link (section 3): find the caller that connects CAbilityMove's sub-task with
   FUN_6F165CC0 / FUN_6F1661C0.
3. Meaning of unit+0x58 (arg1 of the point path) and of the gate checks in 1.5.
4. Family set of CAbilityAttack slot 192 (FUN_6F0CAED0).
