# Order system (WC3 1.26a Game.dll) - pass 1

Status: analysis notes for a clean-room reimplementation (interoperability). Plain-English behaviour
with addresses and constants. No decompiled code is copied here.

Labels used on every claim:
- CONFIRMED: read in the disassembly (capstone) and consistent with the decompile, or read straight from data.
- LIKELY: decompile only, or the meaning is inferred from names, strings or a consistent pattern.
- UNSURE: not checked, or the argument mapping is ambiguous in the decompile.

Conventions: "unit+0xNN" is a byte offset into the unit object; "order+0xNN" into an order object.
Addresses are virtual addresses in Game.dll 1.26a. Natives are the entry points named in natives.c.

---------------------------------------------------------------------------------------------------
## 0. Pipeline in one paragraph

A JASS native (or a player command) gives a unit, an order id and optional point/target. The native
resolves its handles (unit, widget, location, player), turns an order string into an id when needed,
runs a validator, then builds an order object of one class (immediate, point, target, instant, build).
That object goes to the unit's dispatcher FUN_6F2A4AB0, which either clears the current orders,
appends to the queue, or starts the order at once. Starting calls the order's class handler, looks up
the ability that owns the order (FUN_6F279A90), and calls that ability's start slot (vtable slot 3).
For move, attack, patrol and similar orders the ability is CAbilityMove, whose start handler switches
on the order id and calls FUN_6F203D60. That routine sets up the move or attack sub-tasks and asks the
mover for a path (see docs/spec/movement*.md and pathing*.md).

---------------------------------------------------------------------------------------------------
## 1. Order ids and names

Name registration: a block of calls FUN_6F3B0730(id, name) fills the id/name tables. The block
containing "attack" starts at FUN_6F229A00 (chunk_011 around line 33279). "smart" is registered from
the function at FUN_6F3B0750 (id loaded from data slot 0x6F92EDD8, value 0xD0003). The name for 0xD0004
is the 4-byte packed string at 0x6F92B9B0. CONFIRMED for every row below unless marked otherwise.

| Id | Name (registered) | Handled by / notes |
|---|---|---|
| 0xD0002 | (not named yet) | CAbilityMove start: calls FUN_6F2053B0 with flags (0,1). |
| 0xD0003 | smart | CAbilityMove start: FUN_6F2053B0 (0,0). |
| 0xD0004 | stop | Not in the CAbilityMove start switch; owned by another ability (not found). Validator special case, see 4.1. |
| 0xD0005 | (not named yet) | Immediate validator always accepts it (returns 0), see 4.1. |
| 0xD000C | setrally | Not checked. |
| 0xD000D | getitem | CAbilityMove start: FUN_6F2053B0 (1,0). |
| 0xD000F | attack | CAbilityMove start: FUN_6F203D60 (1,0). |
| 0xD0010 | attackground | Not checked. |
| 0xD0011 | attackonce | Not checked. |
| 0xD0012 | move | CAbilityMove start: FUN_6F203D60 (0,0). |
| 0xD0014 | AImove | CAbilityMove start: FUN_6F203D60 (1,0). |
| 0xD0016 | patrol | CAbilityMove start: FUN_6F204560. |
| 0xD0017 | (not named yet) | CAbilityMove start: FUN_6F2055C0. |
| 0xD0019 | holdposition | CAbilityMove start: FUN_6F201300. |
| 0xD001A.. | build, humanbuild, orcbuild, nightelfbuild, ... | Build family (registered at FUN_6F229A00). |
| 0xD0021 | (drop item point) | Named by the items spec (docs/spec/items.md). Not re-checked here. |
| 0xD0022+slot | drop item slot | Named by the items spec. Not re-checked here. |
| 0xD0032 | harvest | Registered at FUN_6F229A00. |
| 0xD004B | (not named yet) | CAbilityMove start: FUN_6F2038B0. |
| 0xD0144 | internal, not a player order | "Stop/cancel the current orders" notification (see 3.4). |

Correction to docs/spec/pathing-2.md section 6.2: it says 0xD0014 is patrol. The registration table
names 0xD0014 "AImove" and 0xD0016 "patrol" (CONFIRMED). The move dispatch groups 0xD000F and 0xD0014
together, and that matches "attack" and "AImove" rather than patrol.

Internal ids that are not orders (they look like the order ids but are notifications or sub-tasks):
0xD0144 (stop notification), 0xD0148, 0xD014A, 0xD0161, 0xD0165, 0xD0166, 0xD016B (move to point),
0xD0170 (move to / follow a unit), 0xD0171, 0xD0173, 0xD0174, 0xD0179, 0xD017A, 0xD029C, 0xD029D,
0xD02A5 (queued notification), 0xD02A6 (started / cleared notification). LIKELY for all of these.

Order id from a string: FUN_6F3A8DF0 (name to id, -1 if unknown). Id to name: FUN_6F3A8E20.
OrderId(string) returns 0 when the name is unknown (CONFIRMED: the native maps -1 to 0).
OrderId2String(id) returns the null string when there is no name or the name is empty (CONFIRMED).
AbilityId(string) uses a second table (FUN_6F3A8E60), same pattern (LIKELY).

Order flags table: FUN_6F372C30(id) returns a 32-bit flags word for an id, or 0 when the id is not in
the table (CONFIRMED that it returns 0 when missing). The flags word is read from entry+0x18 of a keyed
table. Bits used by the point/target natives: 0x2 (point-capable), 0x4 (target-typed), 0x8 (see 3.3), and
bit 0x80 (sign bit, tested as negative). Exact meaning of each bit: LIKELY.

---------------------------------------------------------------------------------------------------
## 2. Unit fields used by orders (CONFIRMED offsets unless marked)

| Offset | Meaning | Confidence |
|---|---|---|
| +0x5C flags | bit 0x100 = locked: the unit accepts no new orders (dispatcher, append, enqueue all return silently; stop is refused). Bits 0x1 and 0x2 are used by the drain loop. Bit 0x100000 causes an extra cleanup in the clear-all path (FUN_6F2A49C0). | CONFIRMED for 0x100; LIKELY for the rest |
| +0x174 / +0x178 | pending list: orders accepted but not started; the drain loop FUN_6F29ED60 walks it. | LIKELY |
| +0x19C / +0x1A0 | current list: orders that have started. The head is the current order. | CONFIRMED head use (GetUnitCurrentOrder) |
| +0x1A8 / +0x1AC | queue list: shift-queued orders waiting behind the current one. | LIKELY |
| +0x198 | count that the dispatcher reads as "busy" (non-zero). What increments it is not located. | UNSURE |
| +0x1B4 | total order count. A new order is refused when the count is greater than 500. | CONFIRMED |
| +0x1DC / +0x1E0 | list of the unit's abilities (validators iterate it). | CONFIRMED |
| +0x3C8 of the game object (from FUN_6F300710) | order subsystem object. Point natives return 0 when it is null. | CONFIRMED |

Queue capacity: both the append path (FUN_6F2A0650) and the enqueue path (FUN_6F2A0510) refuse when the
count is above 500, so the unit holds at most 501 orders (CONFIRMED from both code paths). A refused
order is dropped silently. The native still returns true, because the dispatcher returns nothing
(CONFIRMED). The older docs mention a 16-entry queue (AGENTS.md section 4.3 and an OpenRealm note);
the binary says 500. Treat the 16 as wrong for 1.26a.

Order object fields: +0x20 flags (0x80 = not yet started; cleared when the order starts, LIKELY;
0x2 = accepted into the unit, set by FUN_6F2A0770; 0x4 = in progress, checked by FUN_6F283340;
0x8 = checked in the shift path, meaning not yet known); +0x24 order id (CONFIRMED); +0x2C / +0x30 list
links, -1 = empty (LIKELY). Vtable slot 7 is the class tag getter (CONFIRMED use); vtable slot 23
(+0x5C) is cancel or dispose (LIKELY).

---------------------------------------------------------------------------------------------------
## 3. Natives: what they do

### 3.1 Common helpers

- FUN_6F3BDCB0: unit handle to unit object; 0 when null (CONFIRMED).
- FUN_6F3BE5B0: widget handle to object; 0 when null (CONFIRMED use).
- FUN_6F3BD3B0: location handle to (x, y) (CONFIRMED use).
- FUN_6F3BD4D0: player handle to player object; 0 when null (CONFIRMED use).
- FUN_6F3BAD70: order name to id; the native then compares with -1 (CONFIRMED use).

### 3.2 Immediate orders (no target)

- IssueImmediateOrder (string) at 0x6F3C8D80, IssueImmediateOrderById at 0x6F3C8900.
  String form: id = name lookup, -1 returns false. Then unit lookup (null returns false). Then the
  shared path FUN_6F3B2EF0: validator FUN_6F2844D0 (see 4.1). Accept when the result is 0 or 0xDC; else
  false. On accept, build an immediate order (FUN_6F294A40), dispatch it (FUN_6F2A4AB0 with replace=1,
  drain=1) and return true. CONFIRMED.

### 3.3 Point and target orders

- IssuePointOrder (string) 0x6F3C8DA0, IssuePointOrderLoc 0x6F3C8DC0, IssuePointOrderById 0x6F3C8940,
  IssuePointOrderByIdLoc 0x6F3C8980. Location forms take x and y from the location (CONFIRMED).
  The shared path is FUN_6F3B2F30. Steps (LIKELY, from the decompile; argument mapping UNSURE):
  1. Get the order subsystem object. If it is null, return false.
  2. Read the flags word for the id.
  3. If the flags word is 0, run the point validator FUN_6F279560. On accept (0 or 0xDC), build a
     point-class order and dispatch it. Return true.
  4. If bit 0x2 is set, run FUN_6F279560 as well. If accepted, build a point order (bit 0x4 clear) or
     a target-typed order (bit 0x4 set), dispatch it, return true.
  5. If bit 0x2 is clear and bit 0x8 is set, run FUN_6F279560, and on accept dispatch a point order and
     return 0. This is a return-value quirk: an order is issued but the native reports false (LIKELY,
     decompile only; the return value on that branch is the constant 0).
  6. Otherwise return false.
- IssueTargetOrder (string) 0x6F3C8E20, IssueTargetOrderById 0x6F3C89D0: the widget must be non-null,
  then the shared path FUN_6F3B3090 (flags word again, with validators FUN_6F279630 / FUN_6F279560 and
  the same class choices). Details UNSURE.
- IssueInstantPointOrder (string) 0x6F3C8E40, IssueInstantPointOrderById 0x6F3C8A20: validator
  FUN_6F279720 (vtable slot 149). Accept on 0 or 0xDC; then build an order with the widget as optional
  target (FUN_6F294E40), dispatch (replace=1, drain=1), return true. CONFIRMED.
- IssueInstantTargetOrder (string) 0x6F3C8E60, IssueInstantTargetOrderById 0x6F3C8AB0: validators
  FUN_6F279800 (vtable slot 150) and FUN_6F279720, with a second pass for the second widget.
  Accept on 0 or 0xDC; build (FUN_6F294E40), dispatch, return true. CONFIRMED structure.

### 3.4 Build orders

- IssueBuildOrder (string) 0x6F3C8E80, IssueBuildOrderById 0x6F3C8BE0: first the build check
  FUN_6F2A50C0; accept on 0 or 0x45. Then point validator FUN_6F279560; on accept, build a point-class
  order (FUN_6F294B30), dispatch (replace=1, drain=1), return true. CONFIRMED.

### 3.5 Neutral orders

- IssueNeutralImmediateOrder(ById) 0x6F3C8EA0 / 0x6F3C8CA0, IssueNeutralPointOrder(ById) 0x6F3C8EC0 /
  0x6F3C8CE0, IssueNeutralTargetOrder(ById) 0x6F3C8EE0 / 0x6F3C8D30. The player handle must resolve
  (FUN_6F3BD4D0). The immediate and target forms then call the normal path. The point form passes the
  player's index byte (player+0x30) where the normal point path passes the unit's +0x58 value. Effect
  of that difference: UNSURE. Owner is not changed by these natives (LIKELY).

### 3.6 Group orders

- GroupImmediateOrder(ById), GroupPointOrder(ById, Loc), GroupTargetOrder(ById) at 0x6F3C4300 to
  0x6F3C4440. Not read in detail. LIKELY: loop over the group calling the single-unit natives. Return
  value rule (any or all): UNSURE.

### 3.7 Query natives

- GetIssuedOrderId 0x6F3C2C80: reads the current event kind through FUN_6F3BB2C0 (which reads the
  current event context at index 6, FUN_6F4280B0). Kinds 38, 39, 40 (0x26, 0x27, 0x28) need FUN_6F3C2BA0
  and then FUN_6F40B620 to succeed; kinds 75, 76, 77 (0x4B, 0x4C, 0x4D) need FUN_6F3C2BE0. Then the
  current order-event data object is fetched (FUN_6F26FCB0, which returns 0 when the list is empty) and
  its order id is at data+0x24. If the event data is missing, the native returns 0xD0142. Other event
  kinds return 0. CONFIRMED structure; the meaning of 0xD0142 as a default is LIKELY.
- GetOrderPointX 0x6F3C2D00 and GetOrderPointY 0x6F3C2D50: only event kinds 39 and 76 (point orders)
  return a value, read from the event data (FUN_6F26FD30). Other kinds return the constant 0.0 (CONFIRMED).
- GetOrderTarget 0x6F3C2DE0 (and the Destructable, Item, Unit variants 0x6F3C2F10, 0x6F3C3040, 0x6F3C3170):
  target from the event data for the target kinds (LIKELY; not read in detail).
- GetUnitCurrentOrder 0x6F3C7780: unit to the current list. If the unit's current list is not empty, it
  returns the id of the head order (+0x24). Otherwise 0. There is also a guard on an execution-context
  object (FUN_6F03FA30). The meaning of that guard is UNSURE. CONFIRMED structure.

Event numbers (JASS constants). ConvertPlayerUnitEvent(i) and ConvertUnitEvent(i) are the identity
at 0x6F3B26C0 and 0x6F3B26D0 (CONFIRMED), so the constant is the number itself. From the OpenRealm
common.j copy (third-party reference, verify against the user's own common.j):
- EVENT_PLAYER_UNIT_ISSUED_ORDER = 38 (0x26), EVENT_PLAYER_UNIT_ISSUED_POINT_ORDER = 39 (0x27).
  The target variant is 40 (0x28) (LIKELY from the sequence; not in the copy).
- EVENT_UNIT_ISSUED_ORDER = 75 (0x4B) (the copy confirms this one). The point and target unit variants
  are 76 (0x4C) and 77 (0x4D) (LIKELY).
These numbers match the switch in GetIssuedOrderId and the point test in GetOrderPointX, so the mapping
is LIKELY confirmed by the binary.

---------------------------------------------------------------------------------------------------
## 4. Validators (before an order is accepted)

### 4.1 Immediate validator FUN_6F2844D0 (unit, order id)

- Id 0xD0005: returns 0 (always accept). CONFIRMED.
- Id 0xD0004 (stop): returns 0xDD when the unit is locked (unit+0x5C bit 0x100), otherwise 0. CONFIRMED.
- Any other id: walks the unit's ability list and calls FUN_6F0762A0 for each ability (an accept
  predicate). The first ability that returns 0 makes the result 0. Otherwise the result is the
  smallest non-zero code, or 0xDD when no ability answered. CONFIRMED structure; FUN_6F0762A0 not read.
- Accept rule at the natives: 0 or 0xDC. CONFIRMED.

### 4.2 Point, instant and build validators

- FUN_6F279560 (point, vtable slot 151 = +0x25C), FUN_6F279630 (point variant, slot 152 = +0x260),
  FUN_6F279720 (instant point, slot 149 = +0x254), FUN_6F279800 (instant target, slot 150 = +0x258).
  Each walks the unit's ability list. For an ability whose order-id getter (vtable slot 40 = +0xA0)
  equals the requested id, it calls the ability's check. Result 0 means accept. Otherwise the code is
  0xB9 (blocked by an ability state; meaning LIKELY) or 0xDD (no match). The overall result is the
  smallest code seen, or 0xDD for an empty list. 0xDC is also seen as "accept with a notice" (one
  variant sets a global flag). CONFIRMED structure.
- Important: the base ability class returns 0 for slot 40, and CAbilityMove and CAbilityAttack do not
  override it (CONFIRMED from the vtables). Spell and item classes do override it (for example the
  family at 0x6F069030). So point validators never match move or attack through slot 40. How a point
  move or attack order passes validation is UNSURE and is the main open question for this subsystem.
- FUN_6F2A50C0 (build check): accepts 0 or 0x45 (CONFIRMED from the build native).

### 4.3 Blocked check FUN_6F2832E0 (unit, order)

- Returns 0 when the current list is empty (CONFIRMED).
- Otherwise finds the ability that would handle the order (FUN_6F279A90), and returns that ability's
  slot-139 result (+0x22C). Base class returns 0 (not blocked). CAbilityAttack overrides slot 139; the
  meaning of its result is UNSURE.

---------------------------------------------------------------------------------------------------
## 5. Dispatcher FUN_6F2A4AB0 (unit = this, arg1 = order object, arg2 = replace flag, arg3 = drain flag)

Calling convention: thiscall with the unit in ECX and three stack arguments (CONFIRMED from the
disassembly). Natives always pass arg2 = 1 and arg3 = 1 (CONFIRMED). So arg2 = 1 means "replace" (not a
shift-queued order), and arg2 = 0 means "queued behind the current order" (LIKELY, from the flow).

Steps, in order:
1. If the unit is locked (+0x5C bit 0x100): return silently (CONFIRMED).
2. If the busy count (+0x198) is greater than 0:
   a. arg2 = 0: append the order to the queue through FUN_6F2A0650 (append mode, drain = arg3).
   b. arg2 != 0: clear the current orders (FUN_6F2831A0), then enqueue the new order (FUN_6F2A0510).
3. Otherwise (not busy):
   a. Run the blocked check (4.3). If it reports blocked:
      - arg2 != 0: clear the current orders (FUN_6F2831A0) and drop the new order.
      - arg2 = 0: drop the new order.
      (CONFIRMED flow; LIKELY that this is "a repeat or conflicting order is refused"; UNSURE which.)
   b. Not blocked, arg2 != 0: clear everything (FUN_6F2A4A10: fire the stop notification, cancel each
      current order, reset the lists and the count to 0), then enqueue the new order (FUN_6F2A0510).
   c. Not blocked, arg2 = 0: fire the stop notification (FUN_6F2964E0), optionally FUN_6F283340 when the
      current head has flag 0x8 (meaning UNSURE), then append the order at the front of the current
      list (FUN_6F2A0650, append = 0, drain = arg3).

Enqueue FUN_6F2A0510 (refuse if locked or count > 500): insert the order into the queue list and the
current list (LIKELY), count + 1. If the pending list is non-empty, fire notification 0xD02A5 and stop.
Otherwise start the order at once (FUN_6F29DFF0 with drain = 1). CONFIRMED structure.

Append FUN_6F2A0650 (refuse if locked or count > 500): if the current list is empty, insert the order at
the head of the current list and the queue (LIKELY). If the current list is not empty and append mode is
set, insert after the last element. Count + 1. If the pending list is empty, start the order
(FUN_6F29DFF0 with drain = arg3). Otherwise the order waits in the pending list. CONFIRMED structure.

Clear FUN_6F2831A0: if the current list is not empty, walk it and fire notification 0xD02A6 for each
element, calling each element's cancel slot (+0x5C). Then reset the count and move the current list into
the queue head, detaching its first element (LIKELY: the interrupted order stays as the queue head). The
exact effect on the queue is UNSURE.

Clear-all FUN_6F2A4A10: clear (FUN_6F2831A0), extra cleanup if flag 0x100000, stop notification
(FUN_6F2964E0), cancel each current order (slot +0x5C), set the current and queue lists to empty and
the count to 0. CONFIRMED.

Stop notification FUN_6F2964E0: fires internal kind 0xD0144 to the unit (through its notify slot), then
builds a second event of a kind not decoded, calls the unit's cancel slot, and clears the pending list
(CONFIRMED for the pending-list reset; the second event is UNSURE). CONFIRMED that 0xD0144 is the first notification; CAbilityMove's start switch handles
0xD0144 by FUN_6F205B80, which cancels move sub-tasks of kinds 0xD0170, 0xD0171, 0xD0173, 0xD0174,
0xD016B and 0xD029D (LIKELY that this is "cancel move").

Drain FUN_6F29ED60: while the pending list is non-empty, it takes the first pending order, sets unit flag
0x1, notifies the unit with that order, and then either clears flag 0x2 handling or runs a removal helper
(FUN_6F2840A0). The loop ends when the flag is cleared by the handler or the pending list is empty, then
it calls FUN_6F29E110. Flags 0x1 and 0x2 meaning: UNSURE.

---------------------------------------------------------------------------------------------------
## 6. Start, ability lookup and events

### 6.1 Start FUN_6F29DFF0 (unit, order, drain flag)

1. Clears order flag 0x80 (CONFIRMED).
2. Dispatches by the class tag of the order (vtable slot 7 compare):
   - immediate class: FUN_6F299A00;
   - point class: FUN_6F299C40;
   - target class (or the second target type, checked by FUN_6F271680): FUN_6F299E90.
   CONFIRMED that these are the three class handlers; their bodies were not read.
3. If FUN_6F26F8E0 returns 0 (LIKELY: "not already handled"), call FUN_6F279FE0. That function finds the
   owning ability (FUN_6F279A90) and calls its slot 3 (start) with an event object: event+0x8 = order
   id (copied from order+0x24), event+0xC = order (CONFIRMED).
4. If FUN_6F279FE0 finds no ability, call FUN_6F26E670 (not read). Then fire notification 0xD02A6 to the
   unit (CONFIRMED).
5. If drain = arg3 is non-zero, call FUN_6F29ED60 (the drain loop).

Note on step 4: 0xD02A6 is fired after the ability has taken the order, so "started" is the meaning
LIKELY for this site. The same kind is fired by the clear path (6.3) too, so its meaning is UNSURE.

### 6.2 Order to ability lookup FUN_6F279A90 (unit, order)

- The order's class tag is read through vtable slot 7 (+0x1C). Tags seen: 0x2B6F7263, 0x2B6F7264,
  0x2B6F7463, 0x6F722E32, 0x6F722E33, 0x6F72642E, 0x6F726474, 0x6F727432. (CONFIRMED values;
  names not decoded.)
- For each tag, the function walks the unit's ability list (+0x1DC) and calls an accept predicate
  (FUN_6F0C5E30, FUN_6F0762A0, FUN_6F275FF0, FUN_6F275F10, FUN_6F276060 or FUN_6F146120 depending on tag).
  For each ability that accepts, it reads the ability's rank through slot 0x248 (+0x248 = slot 146).
  The result is the accepting ability with the smallest rank; the first one wins on a tie.
- CONFIRMED structure. The predicates and the rank meaning: UNSURE.
- Slot 135 (+0x21C) of an ability is an order-id accept test: CAbilityMove returns 1 for ids 0xD0019,
  0xD0012 and 0xD0016, and 0 otherwise (CONFIRMED from the disassembly). It is used by FUN_6F27A070
  (a helper that finds the ability for an id). Its use in the main path is LIKELY, not confirmed.

### 6.3 Events and notifications

- Internal kinds, CONFIRMED where the code shows the firing site:
  0xD02A5 queued (fired by the enqueue path when the pending list is non-empty);
  0xD02A6 started/cleared (fired after a start, and once per element in the clear path);
  0xD0144 stop (fired first in the stop notification);
  broadcast set fired by FUN_6F0E1F90: 0xD02A5, 0xD0144, 0xD0179, 0xD029C, 0xD0161, 0xD017A.
- How these internal kinds are turned into the JASS events 38-40 and 75-77 is NOT located. The JASS
  event fires at some point after these internal events. Dispatch order relative to execution is
  therefore UNSURE for the JASS events.
- Per the OpenRealm research note (docs/research/openrealm/04-gameplay.md) issued-order events fire at
  submission, not at execution. That is a third-party claim; not verified in the binary (UNSURE).

---------------------------------------------------------------------------------------------------
## 7. Ability state machine as far as visible (move and attack)

Start (CAbilityMove start switch, then FUN_6F203D60 for move, attack and AImove):
1. Read the current sub-task (FUN_6F255150, FUN_6F271600). If it has none, read the ability's stored
   targets (FUN_6F2862F0, FUN_6F286390, FUN_6F286340). If a target is set and it is refused by the
   blocked test (FUN_6F284EE0 with flags), drop the target.
2. Clear some states (FUN_6F202EB0, FUN_6F2963F0, FUN_6F295BC0 with 0xD0166 / 0xD0012, FUN_6F295AC0
   with 0xD0148, 0xD014A, and 0xD0165 for non-attack orders).
3. For attack-type orders with a point or unit target, a special case (FUN_6F4773E0 for the order's
   range) can set the mode to 0.
4. When no target is current, create the move sub-task: point move (0xD016B, FUN_6F295CC0) or unit
   follow (0xD0170, FUN_6F295EE0). Each makes a task-order parameter object (LIKELY) and inserts it
   through FUN_6F28D5D0. Also FUN_6F0C7740 (LIKELY the path request to the mover).

So the target state machine is a stack of sub-task objects on the ability (LIKELY). Start pushes the
move or follow sub-task. Executing is the per-tick path the mover runs. Done is the point where the
sub-task is removed and the stop or clear notification fires.

Executing (per tick): not linked to the ability in this pass. The mover code is in docs/spec/movement*.md
(per-tick loop FUN_6F483300 and per-record step FUN_6F4907F0). How that loop reads the ability's
sub-task state is UNSURE.

Done: the sub-task is removed on arrival, or on cancel (stop notification 0xD0144 for move, clear
notification 0xD02A6 for the current orders). The arrival detection is UNSURE.

---------------------------------------------------------------------------------------------------
## 8. Rules for a scripted order vs a player command

- Scripted natives always pass replace = 1 and drain = 1 (CONFIRMED). So a script order replaces the
  current order, starts at once when nothing else is pending, and runs the drain loop.
- The shift (queued) path is replace = 0 (LIKELY from the dispatcher). It appends to the queue and
  does not stop the current order, but it does a stop notification when nothing is current.
- Both paths go through the same validator and the same dispatcher, so a scripted order and a player
  command produce the same dispatch as long as the validator accepts the same id and target.

---------------------------------------------------------------------------------------------------
## 9. Open questions (in order of impact)

1. How a point move or attack order passes validation, given that move and attack abilities do not
   override slot 40 (4.2). Needs an asm trace of FUN_6F3B2F30 and its callers to confirm the arguments
   reaching FUN_6F279560.
2. The map from internal kinds 0xD02A5, 0xD02A6, 0xD0144 to the JASS events 38-40 and 75-77, and the
   dispatch order against execution (6.3).
3. Meaning of the unit flags 0x1, 0x2, 0x8 (order object) and the busy count +0x198.
4. The blocked-check rule (4.3) and what CAbilityAttack slot 139 returns.
5. The per-tick hook between the ability sub-task and the mover (section 7).
6. Ability accept predicates FUN_6F0762A0, FUN_6F0C5E30 and the rank slot 0x248.
7. Stop owner: which ability handles 0xD0004.
8. GetUnitCurrentOrder guard FUN_6F03FA30.
9. Native return value of the group natives.
