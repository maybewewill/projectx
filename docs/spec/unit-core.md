# Unit core: creation, CUnit layout, accessors and lifecycle natives (WC3 1.26a Game.dll)

Scope: the natives that create, read, move, kill, remove, show, pause and re-own units, the CUnit fields they touch, and the order of effects. Plain-English spec for a clean-room reimplementation. Addresses are Game.dll VAs (image base 0x6F000000). Confidence tags:

- CONFIRMED: read directly in the decompile or disassembly, control flow or constants visible.
- LIKELY: strong structural evidence, meaning not fully proven.
- UNSURE: guessed from names or position, or the code was not read.

Method: natives were read in `decomp/natives.c`, helpers in the chunk files, and the short thunks and accessors were checked with capstone on the DLL bytes. Virtual slots were mapped to addresses from the `.?AVCUnit@@` vtable (118 entries, slot = byte offset / 4). The file-based `vfunc_names.tsv` was used only as a cross-check. Several constants are zero in the file and filled at runtime, so their values are not verified here.

---

## 0. Key findings (summary)

1. Units are CAgent-derived objects reached through handle ids. Every unit native first resolves the handle through one shared resolver. It returns NULL (and the native returns its default) when the handle is invalid, when the object's kind tag is not 0x2B61676C, or when the removed flag at +0x20 is set. CONFIRMED.
2. Position, facing and speed do not live on CUnit directly. They live in a movement sub-object at CUnit+0x164. Life lives in a stat sub-object at CUnit+0x98 (with its smart pointer at +0xA0). Mana is read through the smart pointer at CUnit+0x08. CONFIRMED.
3. CreateUnit validates the player before creating anything. CreateUnitByName also looks up the type name first. The handle id is allocated last, after the object is fully built and initialized. CONFIRMED.
4. KillUnit does not remove the unit. It sets life to 0.0 through the life setter and fires CEvent 0xD01E6. RemoveUnit is a separate call that sets the removed flag at +0x20. CONFIRMED.
5. SetUnitState only handles state 0 (life) and state 2 (mana). States 1 and 3 (max life, max mana) are silently ignored. CONFIRMED.
6. Owner is stored at CUnit+0x58 as a player index, not a pointer. Changing owner moves food-used and food-made accounting from the old player to the new one. CONFIRMED (structure), LIKELY (meaning of each bit).
7. RemoveUnit does not release the handle id. The id is released when the agent's reference count reaches zero (see handles-2.md). LIKELY.
8. No RNG draw was found in the creation path code that was read. The ability factories and FUN_6F2A0E30's sub-calls were not read in full. UNSURE.

---

## 1. Native-to-code map

| Native | Address | Resolves through | Core helper / slot |
|---|---|---|---|
| CreateUnit | 0x6F3C5230 | player resolver FUN_6F3BD4D0 | FUN_6F24F1F0 -> FUN_6F29F990 -> slot 0x1AC (107) -> slot 0x8C (35) |
| CreateUnitByName | 0x6F3C52A0 | name lookup FUN_6F4C4630 + FUN_6F3A8D10 | same as CreateUnit |
| CreateUnitAtLoc | 0x6F3C52D0 | location resolver FUN_6F3BD3B0 | CreateUnit with location x (+0x24) and y (+0x28) |
| CreateUnitAtLocByName | 0x6F3C5320 | name lookup + location | same as CreateUnitAtLoc |
| GetUnitX | 0x6F3C5510 | FUN_6F3BDCB0 | slot 0xB8 (46) -> unit+0x164, FUN_6F4743A0 |
| GetUnitY | 0x6F3C5550 | same as GetUnitX, other axis | same |
| SetUnitX | 0x6F3C5970 | FUN_6F3BDCB0 | slot 0xB8, FUN_6F4743C0 -> FUN_6F474250 |
| SetUnitY | 0x6F3C59E0 | same | same |
| SetUnitPosition | 0x6F3C5A50 | FUN_6F3BDCB0 | FUN_6F2A4B80(1), then slot 0x180 (96) -> FUN_6F2A5D50 |
| SetUnitPositionLoc | 0x6F3C5AB0 | location, then SetUnitPosition | same |
| GetUnitFacing | 0x6F3C5790 | FUN_6F3BDCB0 | slot 0xB8 (46), then mover slot 0x1C |
| SetUnitFacing | 0x6F3C5AF0 | FUN_6F3BDCB0 | slot 0xB8, FUN_6F6EEE20 (mul), FUN_6F474830 |
| SetUnitFacingTimed | 0x6F3C5B30 | FUN_6F3BDCB0 | if duration <= 0.0 then SetUnitFacing, else timed path (not read) |
| GetUnitTypeId | 0x6F3C5910 | FUN_6F3BDCB0 | reads +0x30 directly |
| GetUnitState | 0x6F3C5400 | FUN_6F3BDCB0 | FUN_6F27AE90 (switch on state) |
| SetUnitState | 0x6F3C5EA0 | FUN_6F3BDCB0 | FUN_6F27AF20 |
| GetOwningPlayer | 0x6F3C8190 | FUN_6F3BDCB0 | FUN_6F2834E0, then FUN_6F430C80 (handle) |
| SetUnitOwner | 0x6F3C5ED0 | unit + player resolver FUN_6F3BD4D0 | FUN_6F2A4BE0 |
| KillUnit | 0x6F3C8040 | FUN_6F3BDCB0 | slot 0x124 (73) with 0.0 (= SetLife) |
| RemoveUnit | 0x6F3C8060 | FUN_6F3BDCB0 | slot 0x84 (33) -> FUN_6F291790, then FUN_6F2A63B0(1,1) |
| ShowUnit | 0x6F3CE730 | FUN_6F3BDCB0 | enumerates a list (FUN_6F2AF2F0), slot 0xD0 (52) per other unit (partial) |
| PauseUnit | 0x6F3C7340 | FUN_6F3BDCB0 + mover check | FUN_6F2A81C0 (pause) or FUN_6F28CA50 (unpause), then FUN_6F332700 |
| IsUnitType | 0x6F3C7E90 | FUN_6F3BDCB0 | switch on type kind (case 0 -> FUN_6F03FDC0; jump table not recovered) |
| GetWidgetLife | 0x6F3C4660 | widget resolver FUN_6F3BE5B0 | slot 0x12C (75) |
| SetWidgetLife | 0x6F3C4690 | widget resolver | slot 0x124 (73) = SetLife |

Both CreateUnit natives return the handle from FUN_6F430C80 (the allocator in handles-2.md). The decompile of the CreateUnit native body shows no return value, which looks like a decompiler artifact. The JASS signature returns a unit handle. LIKELY.

---

## 2. Shared handle resolution (every unit native)

- The resolver is FUN_6F3BDCB0. It takes a handle id and calls FUN_6F428050 (handle-table lookup, see handles-2.md) to get the table entry. It then takes a smart pointer (FUN_6F038450) to the agent. CONFIRMED.
- A valid unit must pass all three checks. CONFIRMED structure:
  1. The agent pointer is non-null (FUN_6F03FA30 gives the agent behind the smart pointer).
  2. The agent's kind tag at +0x0C equals 0x2B61676C. The same tag check appears in the mover checks. LIKELY that this tag identifies unit agents.
  3. The removed flag at +0x20 is zero. Any non-zero value means the handle is dead.
- If any check fails the resolver returns NULL. Natives then return 0, 0.0 or do nothing, depending on the native. CONFIRMED.
- Player and location resolvers (FUN_6F388950 / FUN_6F3B8FA0 path, used by FUN_6F3BD4D0 and FUN_6F3BD3B0) use the same structure with different kind tags. LIKELY.
- Each resolver call also runs FUN_6F3A8060, a one-time lazy initialiser guard. It sets up world context on first use and does nothing afterwards. LIKELY.
- The same resolver is used by reads and writes alike. Reads are not "pure": they can take and release smart-pointer references. LIKELY, low impact on determinism.

---

## 3. Unit creation path (CreateUnit, CreateUnitByName, CreateUnitAtLoc)

### 3.1 Order of effects (CreateUnit, CONFIRMED unless noted)

1. FUN_6F3B28F0: UI pump. If more than 100 ms of GetTickCount has passed since the last call, it calls a host callback (DAT_6FAB6694) with 0. This is wall-clock dependent but only feeds UI. It must not affect simulation state. LIKELY harmless.
2. Player validation: FUN_6F3BD4D0 resolves the player handle. If invalid, the native returns immediately. Nothing is allocated and no handle is created. CONFIRMED.
3. Facing conversion: FUN_6F6EEE20 (softfloat multiply) multiplies the angle argument by the constant at 0x6FAAE5E8. LIKELY that this is degrees to radians. The constant value is not verified (zero in file, runtime-filled).
4. FUN_6F24F1F0 calls FUN_6F29F990 with flags 0x502 and extra args (1, -1). Then, if the result is non-null, it calls slot 0x8C (35) with (4, 0, 0). CONFIRMED.
5. FUN_6F430C80 allocates the handle id for the new object (see handles-2.md, base 0x100000, LIFO reuse of freed ids). CONFIRMED.

### 3.2 Order inside FUN_6F29F990 (the creation core)

1. Reads x and y from the caller's pointers (float, softfloat-safe copies). Builds a stack-local placement record (x, y, z, facing). CONFIRMED that x and y are copied. The exact record layout is UNSURE.
2. FUN_6F29F3E0 fills a large creation template from the unit type database. It writes about 70 dwords, including copies of type values at +0x38, +0x3C, +0x40 and +0x58. Returns 0 if the type record is missing (then the whole call fails). CONFIRMED (writes and early return), field meanings UNSURE.
3. Ground and terrain queries: FUN_6F32D0B0 (pathing footprint related), then the placement helper FUN_6F3E0B50 (adjusts the requested point to a valid spot in some modes). The mode argument comes from two float constants (0x6FAAE470 / 0x6FAAE570) compared against an epsilon (0x6FAAE614). UNSURE, because all three constants are zero in the file.
4. Terrain height: FUN_6F0126F0(x, y, 0) gives the z value that is stored with the placement record. LIKELY that this is terrain height at (x, y).
5. Object construction (CUnit is allocated somewhere between these steps; the exact allocator call was not isolated). UNSURE.
6. The placement record facing is set from the caller's facing value (CONFIRMED the copy).
7. Virtual init: slot 0x1AC (107) = CUnit::vf107 at 0x6F2A0E30, called with (type id, template pointer, 1, -1). This is the main init. See section 3.3.
8. Post-init: FUN_6F3A1650 (player index to player object, LIKELY), FUN_6F40B1E0 (ability/food setup), FUN_6F2E52F0, FUN_6F2833E0 (flag check), then per-owner counter updates and food accounting depending on flags (section 3.4).
9. Returns the new object. FUN_6F24F1F0 then calls slot 35 (0x8C) with (4, 0, 0), which stores 4 at CUnit+0x2E0 and calls FUN_6F26B860 on +0x2D8. The meaning of 4 is UNSURE (likely an initial animation or action state).

### 3.3 Init body (CUnit::vf107 at 0x6F2A0E30, 4286 bytes)

- Stores type id at CUnit+0x30 (param_2). CONFIRMED. GetUnitTypeId reads this same offset.
- Calls FUN_6F2AB1E0 and FUN_6F474C10 (set up sub-objects, not read in full).
- If the type record's kind value (FUN_6F29C240) equals 2, sets CUnit+0x5C bit 0x20000000. CONFIRMED.
- Copies type-record flag bits into CUnit+0x5C. Type bit 0 set clears 0x1000, bit 0 clear sets 0x1000. Type bit 1 set sets 0x200, bit 1 clear clears 0x200. CONFIRMED.
- Zeroes a set of fields at these byte offsets: +0x54, +0x150, +0x154, +0x158, +0x15C, +0x160, +0x198, +0x1B4, +0x1B8, +0x1BC, +0x1C4, +0x1C8, +0x1D4, +0x1D8, +0xE8, +0x114. CONFIRMED (offsets), meanings UNSURE.
- Stores 0.0 through FUN_6F477350 (LIKELY the initial life and mana values, later overwritten from the template). CONFIRMED call, LIKELY meaning.
- Emits a fixed sequence of setup notifications through the base CAgent virtual slot 2 (0x6F62A9A0). IDs in order: 0xD0153, 0xD0154, 0xD0155, 0xD0156, 0xD0157, 0xD0158, 0xD0159, 0xD015A, 0xD0164, 0xD0148, 0xD014C, 0xD0162, 0xD0145, 0xD0146, 0xD0147, then conditionally 0xD015D, 0xD015B, 0xD015C (only when the type id's top byte is an uppercase letter A to Z, i.e. type id >> 24 in 0x41..0x5A, CONFIRMED), then 0xD019D and 0xD01D4. CONFIRMED order; meaning of each ID LIKELY "set attribute X from the template".
- Writes movement and combat fields from the template into sub-objects (sub-object at +0x1FC, +0x20C, +0x210, +0x24C, +0xE4). CONFIRMED that these copies occur; field meanings UNSURE.
- Sets +0x240 and +0x244 to -1 (no-target sentinels, LIKELY).
- Ability loop: for each ability slot of the type record, FUN_6F29D2F0 says whether the slot is present, FUN_6F29C1B0 gives the ability kind, and a switch calls one of six factories (kinds 1, 2, 3, 4, 0xB, default). The created ability is attached to the unit through the ability's own virtual slot 0x84 (33) and then released through FUN_6F079990. CONFIRMED that abilities are created from the type's list in list order. The exact loop nesting (the break after first hit) and factory internals are UNSURE. Also UNSURE whether the factories draw RNG.
- Calls FUN_6F3A8060 and FUN_6F430DF0 (handle or registry insertion, LIKELY), FUN_6F20AA10, FUN_6F079990 (ref-count helper), FUN_6F4C3F50, FUN_6F24CDF0, FUN_6F027BD0 and FUN_6F0787D0 (trigger or event dispatch with several args, LIKELY used for "unit created" triggers), FUN_6F407570 (owner-side list, LIKELY), FUN_6F078970 (value from template +0x174), and a game-constant read by name "BuildingPlacementNotifyRadius".

### 3.4 Owner and food accounting at creation (CreateUnit flags 0x502)

- Flag bit 0x2 selects the per-owner counter path FUN_6F40FD30 and sets +0x64 bits 0x10 and 0x2. CONFIRMED structure.
- Flag bit 0x100 calls FUN_6F40F5E0 with the unit's food-used value (FUN_6F29BAF0) and sets +0x64 bits 0x100 and 0x200. CONFIRMED.
- Flag bit 0x400 calls FUN_6F03F970 with the food-made value (FUN_6F29BB20) and sets +0x64 bits 0x400 and 0x800. CONFIRMED.
- Flag bit 0x10000 sets CUnit+0x5C bit 0x40000000. Flag 0x20000 sets CUnit+0x5C bit 0x2000000. CONFIRMED.
- Flag bit 0x1 (not in 0x502) would use FUN_6F40FE30 and set +0x64 bits 0x9.

### 3.5 CreateUnitByName and CreateUnitAtLoc

- CreateUnitByName: resolves the name first (FUN_6F4C4630 then FUN_6F3A8D10). If the name is not found it returns 0 before the player check. Otherwise the same order as CreateUnit. CONFIRMED.
- CreateUnitAtLoc: resolves the location (FUN_6F3BD3B0). Uses location X from +0x24 and location Y from +0x28 (floats). Then calls the CreateUnit body. CONFIRMED.
- Neither native draws RNG on its own.

---

## 4. CUnit layout (fields touched by the natives)

Offsets are byte offsets from the CUnit object start. The object is also a CAgent (vtable at +0).

| Offset | Meaning | Confidence |
|---|---|---|
| +0x00 | vtable pointer (CUnit vtable at 0x6F931934, 118 slots) | CONFIRMED |
| +0x08 | smart pointer to the mana stat agent (read via FUN_6F4773A0 with ECX = unit) | LIKELY |
| +0x0C | kind tag 0x2B61676C is read here by the resolvers and the mover checks (which object exactly holds it: unit or a sub-agent is UNSURE) | LIKELY |
| +0x14 | flag word (bit 31 chooses one of two global objects for speed scaling) | LIKELY |
| +0x20 | removed flag. Non-zero (bit 0 set by RemoveUnit) means the handle is dead | CONFIRMED |
| +0x28 | selection-related pointer (non-zero triggers selection refresh) | LIKELY |
| +0x2C | ushort used by visibility mask code (FUN_6F2AD7E0 reads it) | LIKELY |
| +0x30 | unit type id (4-byte rawcode, big-endian characters) | CONFIRMED |
| +0x58 | owner player index (small integer, compared against the new owner index in SetUnitOwner) | CONFIRMED (index), LIKELY (meaning) |
| +0x5C | unit flag word (see below) | CONFIRMED |
| +0x64 | ownership and food counter flags (see below) | CONFIRMED |
| +0x98 | life stat sub-object (CAgentPtr at +0xA0) | CONFIRMED (offset), LIKELY (meaning) |
| +0xA0 | smart pointer (agent) for the life stats; its +0x84 field is max life | LIKELY |
| +0xE4 | template copy (purpose UNSURE) | UNSURE |
| +0x164 | movement sub-object (position, facing, speed, pathing state) | CONFIRMED |
| +0x1E8 / +0x1EC | flags that gate SetPosition effects | LIKELY |
| +0x248 | flag word (0x10, 0x40 are checked on owner change; 0x200 gates position updates; 0x40 used by PauseUnit and show code) | CONFIRMED (bits read), LIKELY (meaning) |
| +0x2E0 | state value set by the (4,0,0) call after creation | CONFIRMED (offset), UNSURE (meaning) |

CUnit+0x5C bits used by the natives (CONFIRMED where marked):
- 0x8: set by RemoveUnit's flag routine (FUN_6F28DBE0). CONFIRMED.
- 0x100: when set, the removal path skips some cleanup. CONFIRMED.
- 0x200: set from type bit 1. Used in the position path (LIKELY).
- 0x1000: inverse of type bit 0. LIKELY.
- 0x2000000 and 0x40000000: set from creation flags 0x20000 and 0x10000. CONFIRMED.
- 0x20000000: set when the type record's kind value is 2. CONFIRMED (test), meaning UNSURE.

CUnit+0x64 bits (per-owner accounting, CONFIRMED structure):
- 0x1 and 0x8 with 0x10 and 0x2: per-owner counters for the two flag classes (applied on owner change, via FUN_6F40FE30 / FUN_6F40FD70 / FUN_6F40FD30 / FUN_6F40FDF0).
- 0x100 and 0x200: food used. 0x100 means applied to owner, 0x200 means currently counted. SetUnitUseFood sets or clears both as a pair (0x300).
- 0x400 and 0x800: food made. Same pattern (0xC00).

---

## 5. Getters

### GetUnitX / GetUnitY (CONFIRMED structure, LIKELY math)
- Resolve the unit. If invalid, return the default float (the zero constant at 0x6FAAE470). CONFIRMED.
- Get the movement sub-object: CUnit vtable slot 46 (0x6F28B0E0) is a one-line thunk that returns unit+0x164. CONFIRMED.
- Read the position from the movement object (FUN_6F4742F0 reads a pair of values through the smart pointer at +8 and computes the world coordinate). The world coordinate is a snapped softfloat computation (add, multiply) using the world scale constants. LIKELY that this is the cell-to-world conversion documented in movement-2.md.
- So X and Y are derived from stored movement data, not stored as plain world floats on CUnit. LIKELY.

### GetUnitFacing (CONFIRMED structure)
- Resolve the unit. Then get the mover agent and check its kind tag (0x2B61676C) and its removed flag (+0x20 must be 0). If either check fails, return the default 0.0. CONFIRMED.
- Call the mover's slot 7 (+0x1C) to read facing in radians. Multiply by the constant at 0x6FAAE60C (radians to degrees). LIKELY.

### GetUnitTypeId (CONFIRMED)
- Returns the 4-byte value at CUnit+0x30. Returns 0 if the handle is invalid.

### GetUnitState (CONFIRMED)
- States 0 to 3 are valid. Anything else returns 0.0.
- 0 (life, current): slot 0x12C (75) with thunk ECX += 0x98, then the stat read. CONFIRMED.
- 1 (life, max): slot 0x130 (76) with thunk ECX += 0x98, then the agent's +0x84 field. CONFIRMED (path), LIKELY (meaning).
- 2 (mana, current): reads through the smart pointer at CUnit+0x08. CONFIRMED (path).
- 3 (mana, max): reads the agent at CUnit+0x08 and returns its +0x84 field. CONFIRMED (path).

### GetOwningPlayer (CONFIRMED structure)
- Resolve the unit, then FUN_6F2834E0 gives the owner (player object from the owner index at +0x58, LIKELY), then FUN_6F430C80 gives the handle id for that player. Returns 0 if the unit is invalid.

### IsUnitType
- Resolves the unit, then switches on the type-kind argument. Case 0 calls FUN_6F03FDC0. The jump table at 0x6F3C7EC8 was not recovered. UNSURE for all other cases.

### GetWidgetLife (CONFIRMED)
- Resolves the widget (FUN_6F3BE5B0). Calls slot 0x12C (75), so it is the same as GetUnitState(0). If invalid, returns 0.0.

---

## 6. Setters and lifecycle

### 6.1 SetUnitX / SetUnitY (CONFIRMED structure)
- Resolve the unit. Read the current movement vector through slot 46 and FUN_6F4743A0.
- Build a new temporary vector with the new coordinate in the chosen axis. For SetUnitX the other horizontal axis is copied from the current value and the third component is 0.0 (zero constant). The temporary layout used by SetUnitY is muddled in the decompile and UNSURE.
- Store it through FUN_6F4743C0, which calls FUN_6F474250 with flag 1. LIKELY flag 1 means "recompute z from terrain".
- This path does NOT call slot 96 (SetPosition). So it does not run the full move path (no spatial index update, no movement-event dispatch). LIKELY.

### 6.2 SetUnitPosition / SetUnitPositionLoc (CONFIRMED structure)
1. Resolve the unit.
2. FUN_6F2A4B80(1): a pre-move wrapper. It calls slot 59 (0xEC) with (0, 1, 1) and then FUN_6F294A40 and FUN_6F2A4AB0. LIKELY that this begins a position change (pathing bookkeeping).
3. Call slot 96 (0x180) = CUnit::vf96 = FUN_6F2A5D50 with (x*, y*, 1, 1, -1, 0, 0, 0, -1, 0). The -1 in the z slot means "compute z from terrain". CONFIRMED that -1 selects the automatic path (decompile test on param == -1). LIKELY on meaning of the other args.
4. Inside FUN_6F2A5D50 (CONFIRMED skeleton):
   - Test the gate: FUN_6F26DE50(1) nonzero, or CUnit+0x248 bit 0x200 set. If set, call slot 83 (0x14C) with (0, 1), FUN_6F283BC0(0), and save the old cell (scratch globals at 0x6FAB425C, 0x6FAB4260, 0x6FAB4264), and compute z if -1.
   - Defaults: if the callback argument is 0, substitute the global default (0x6FAB425C) or the code default (0x6F2A5C90).
   - Core move: FUN_6F2AC220 with the arguments (not read in full). UNSURE.
   - Slot 104 (0x1A0) is called around FUN_6F2781F0, then FUN_6F283A50 and FUN_6F2ADA60 (LIKELY spatial index update), then if +0x28 is non-zero FUN_6F3334C0 (selection refresh).
   - If the gate is set or +0x1EC is zero: slot 82 (0x148) with (1, 1, 1) and FUN_6F283BC0(1). If the gate is set also FUN_6F28E170. If +0x28 and not +0x1EC: slot 0xE0 and FUN_6F4E82D0/FUN_6F4E8360.
   - If +0x1E8 is non-zero: FUN_6F0C6840.
   - Finally FUN_6F110BC0 and FUN_6F0787D0 (trigger/event dispatch, LIKELY "unit moved" style), and FUN_6F12D520 if that returns non-zero.
- Ordering to preserve: the old cell is recorded before the core move, and the trigger dispatch happens last. LIKELY.

### 6.3 SetUnitFacing (CONFIRMED structure)
- Resolve the unit, get the movement object (slot 46 thunk), multiply the angle argument by the constant at 0x6FAAE5E8 (LIKELY degrees to radians), then FUN_6F474830 gets the mover agent and calls its slot 19 (0x4C) with the radian value. If the value is non-zero it also calls slot 22 (0x58). LIKELY that slot 19 is "set facing" and slot 22 is "apply/commit".
- Facing is set instantly here. Turn-rate limits belong to the mover (movement.md). LIKELY.

### 6.4 SetUnitFacingTimed (CONFIRMED partial)
- If the duration argument is <= 0.0 (the zero constant at 0x6FAAE4A0), call SetUnitFacing. Otherwise the timed path begins by multiplying the angle by 0x6FAAE5E8 (rest not read). UNSURE for the timed behaviour.

### 6.5 KillUnit (CONFIRMED)
- Resolve the unit. Call slot 73 (0x124) = CUnit::vf73 = SetLife with a pointer to 0.0.
- SetLife (CONFIRMED):
  1. Clamp: if the new life is <= 0.0 (includes -0.0 and negatives), store +0.0.
  2. Store into the life stat block (ECX = CUnit+0x98) via FUN_6F477350.
  3. Fire CEvent with ID 0xD01E6 on the unit through the base dispatch (CAgent slot 4 = 0x6F629A90). The event object is built on the stack with the CEvent vtable and the ID.
- KillUnit does not remove the unit and does not release the handle. CONFIRMED.
- Whether the JASS "unit dies" trigger fires from event 0xD01E6 or from a later step is UNSURE.

### 6.6 SetWidgetLife (CONFIRMED) and SetUnitState(0) (CONFIRMED)
- Both call slot 73 (SetLife), so they clamp and fire 0xD01E6 exactly as KillUnit does.

### 6.7 SetUnitState (CONFIRMED)
- Valid only for state index 0 to 3 and a valid unit. Then FUN_6F27AF20 is called:
  - State 0 (life): calls slot 73 (SetLife with clamp and event).
  - State 2 (mana): calls FUN_6F477350 directly on the unit. No clamp, no event.
  - States 1 and 3 (max life, max mana): ignored. A reimplementation must reproduce this quirk.

### 6.8 SetUnitOwner (CONFIRMED structure)
- Native: resolve the unit, resolve the player, then FUN_6F2A4BE0(new owner index = player+0x30 byte, changeColor flag B, fire flag 1). The owner index is read from the player object at +0x30. CONFIRMED.
- FUN_6F2A4BE0 (CONFIRMED skeleton):
  1. If new owner == current owner (+0x58), return with no effect.
  2. Look up old and new player objects from their indices (FUN_6F3A1650). Check the new player's field at +0x268 (LIKELY "is active" or slot counter).
  3. Clear visibility and selection for the old owner: slot 38 (0x98), which calls FUN_6F2AD7E0 and then slot 68 (0x110) with the mask.
  4. If the unit is not in a special state (+0x20 bit 0 clear and +0x5C bit 0x100 clear), run the normal owner-remove step, otherwise call FUN_6F2831A0.
  5. Undo the old owner's per-owner counters (+0x64 bits, listed in section 4).
  6. Food accounting around the switch: flag 0x800 subtracts food made, 0x200 subtracts food used, 0x100 adds food used, 0x400 adds food made (the code does exactly these four adjustments). Which owner each adjustment is applied to relative to the +0x58 write is UNSURE (the owner index is written in step 9).
  7. Visibility refresh if the old owner was the local player (checked via the global at 0x6FAB65F4).
  8. If the fire flag is set, FUN_6F29E190(old, new, ...). This is the owner-change event. LIKELY.
  9. Store the new owner index at +0x58. Then FUN_6F4749C0 and FUN_6F474CB0 (LIKELY mover and owner notifications).
- The changeColor flag (B) is passed through but its effect is only partly read. UNSURE.

### 6.9 RemoveUnit (CONFIRMED structure)
Order, all synchronous in one native call:
1. Resolve the unit.
2. Slot 33 (0x84) = FUN_6F291790:
   a. Slot 38 (0x98) = FUN_6F2AD7E0. Masks the unit's visibility bits against the global player mask, and calls slot 68 (0x110) with the remaining bits (hide-for-players). CONFIRMED.
   b. FUN_6F28DBE0(1, 0): if the removed flag (+0x20 bit 0) is not already set:
      - Set CUnit+0x5C bit 0x8. CONFIRMED.
      - Slot 83 (0x14C) with (1, 1), the unlink-from-world step (LIKELY).
      - If +0x5C bit 0x100 is clear: slot 40 (0xA0) with 0 and FUN_6F283BC0(0), FUN_6F26ECD0(0). CONFIRMED.
      - Set CUnit+0x20 bit 0 (the removed flag). CONFIRMED.
      - FUN_6F2AB310(-1, -1) (LIKELY the removal list or world update).
3. FUN_6F2A63B0(1, 1): post-remove cleanup, in this order (CONFIRMED):
   - If the type id's top byte is uppercase (A to Z) and +0x5C bit 0x40000000 is clear: FUN_6F26EBD0.
   - FUN_6F2A4A10, FUN_6F2A3CC0.
   - Fire event 0xD015A (second arg is 1).
   - Fire event 0xD0164.
   - Fire event 0xD0156 (third arg is 1).
   - Fire event 0xD0178.
   - Fire event 0xD0144.
   - FUN_6F2963F0(0), FUN_6F29ED60.
- Handle ids are not released here. After RemoveUnit, the resolver returns NULL for this handle (removed flag). LIKELY that the id is released later, when the object's reference count reaches zero (see section 7).

### 6.10 ShowUnit (PARTIAL)
- Resolve the unit. If the argument is true, enumerate a list (FUN_6F2AF2F0) and, for each other valid unit in the list, call slot 52 (0xD0) with 1. The true path is partly read. The false path and the meaning of the list are UNSURE. LIKELY that this refreshes visibility of attached or loaded units.

### 6.11 PauseUnit (CONFIRMED structure)
- Resolve the unit. Get its mover agent and check kind tag and removed flag. If invalid, do nothing.
- Pause (argument non-zero): FUN_6F2A81C0 then FUN_6F332700.
- Unpause: FUN_6F28CA50 then FUN_6F332700.
- The meaning of the paused state (whether it stops movement, timers or abilities) is UNSURE.

---

## 7. Handle id timing (see also handles-2.md)

- Creation: the id is allocated last, after the object is fully built (FUN_6F430C80). The first real id is 0x100001 (dummy entry 0 reserved, LIKELY per handles-2.md).
- Reads (GetUnitX, GetOwningPlayer, etc.) do not allocate new ids except GetOwningPlayer, which gets the player's existing handle (or allocates one if needed). LIKELY.
- KillUnit: no effect on the handle.
- RemoveUnit: sets the removed flag at +0x20. The resolver then rejects this handle. No id is freed here. LIKELY that the id is released later, when the handle-table reference count drops to zero and the agent's death message (0x80267, FUN_6F42DD80) runs (handles-2.md, "agent death releases the creation reference", LIKELY).
- Freed ids go on a LIFO stack and are reused first (handles-2.md). CONFIRMED.

---

## 8. Event IDs seen in this area

| ID | Where | Meaning (LIKELY unless noted) |
|---|---|---|
| 0xD0144 to 0xD0147 | init (vf107), remove cleanup | attribute setup or cleanup notifications |
| 0xD0148, 0xD014C | init | attribute setup |
| 0xD0153 to 0xD015A | init, remove | attribute setup, remove cleanup |
| 0xD015B, 0xD015C, 0xD015D | init, only for uppercase-initial type ids | class-specific setup |
| 0xD0162, 0xD0164 | init and remove | setup and remove cleanup |
| 0xD0156, 0xD0178 | remove cleanup | remove cleanup |
| 0xD019D, 0xD01D4 | init | setup |
| 0xD01E6 | SetLife (KillUnit, SetWidgetLife, SetUnitState(0)) | life changed (LIKELY), death detection happens in the handler |
| 0x80267 | agent destruction (handles-2.md) | engine message, releases creation reference |

The 0xD0xxx IDs go through the CEvent path (a CEvent object with the ID is dispatched). The init path calls the base CAgent slot 2 directly with the ID as the argument. CONFIRMED for the dispatch shape, LIKELY for the names.

---

## 9. Constants

| Name / address | Value | Status |
|---|---|---|
| 0x6FAAE470 (zero, default return) | 0.0 | LIKELY. Zero in the file, used as default return and zero compare. Runtime value not verified |
| 0x6FAAE4A0 | timed-facing threshold (0.0 in file) | UNSURE, likely 0.0 |
| 0x6FAAE5E8 | angle multiplier (degrees to radians) | LIKELY, value not verified (zero in file) |
| 0x6FAAE60C | angle multiplier (radians to degrees) | LIKELY, value not verified |
| 0x6FAAE570 | placement threshold | UNSURE |
| 0x6FAAE614 | epsilon for placement | UNSURE |
| 0x2B61676C | unit agent kind tag (checked at agent +0x0C) | CONFIRMED value |
| 0x100000 | handle id base | CONFIRMED (handles-2.md) |

Note: the file contains zeros at all these addresses (checked with pefile on the DLL). They are filled by startup code that was not located. A reimplementation must confirm the values from a live process or from the startup routine. This is the main blocker for bit-exact angle conversions and placement.

---

## 10. Ordering rules for a reimplementation

1. Resolve handle, validate kind tag and removed flag, then act. Any failure returns the native default with no side effects.
2. CreateUnit: validate player (no side effects on failure). Then build the object, init from the type record, run the setup event sequence, build abilities, apply owner and food counters, then allocate the handle last.
3. CreateUnitByName: look up the name before the player check.
4. SetLife (KillUnit, SetWidgetLife, SetUnitState 0): clamp to >= 0.0, store, then fire 0xD01E6.
5. SetUnitState: only states 0 and 2 are applied. States 1 and 3 are ignored.
6. RemoveUnit: hide, unlink, set removed flag, then cleanup events in order. The handle id is not freed here.
7. SetUnitOwner: no-op if the owner is unchanged. Otherwise remove old owner's counters and food, switch the owner index, re-apply counters and food for the new owner, then fire the owner-change event.
8. SetUnitPosition: record the old cell, do the core move, update spatial index, then dispatch the move trigger last.
9. SetUnitX/Y: direct movement-vector write, no move trigger. LIKELY.
10. SetUnitFacing: angle times constant, then store in mover, instant.

---

## 11. Open items and how to confirm them

Confirm these first, because they affect determinism:

1. The runtime values of 0x6FAAE470, 0x6FAAE5E8, 0x6FAAE60C, 0x6FAAE4A0, 0x6FAAE570 and 0x6FAAE614. Read them from a live process after startup (or trace the startup routine). This blocks bit-exact facing and placement.
2. Whether any ability factory or FUN_6F2AC220 (core move) draws RNG. Trace the RNG counter (stream 2 and others, see rng.md) across a single CreateUnit call in the emulator.
3. Whether the JASS unit-death trigger is dispatched from 0xD01E6 (SetLife) or from a later step. Trace the event dispatch from FUN_6F2AB460.
4. Exact layout of the creation template written by FUN_6F29F3E0 (about 0x1B0 bytes) and the placement record used by FUN_6F29F990.
5. The timed facing path (rest of SetUnitFacingTimed) and the jump table in IsUnitType.
6. ShowUnit false path and the list enumeration FUN_6F2AF2F0.
7. Meaning of the changeColor flag in SetUnitOwner and the body of FUN_6F2A4BE0 after the visible part.
8. When the handle id is actually freed (agent destruction versus refcount drop). Confirm with the emulator on RemoveUnit followed by reuse.

Suggested verification: run each native once in the Unicorn harness used for handles (tools/emu/handle_alloc_emu.py) and compare the write set (which offsets change) with this spec.

---

## 12. Not covered here

- GetUnitName (string), GetUnitFoodUsed and GetUnitFoodMade (partly), RemoveUnitFromStock, unit pools, SetUnitRescuable, SetUnitUseFood (only the flag handling is noted above).
- The movement turn formula (0x6F494200) and the speed chain (movement-2.md, movement-3.md).
- Attack and damage (attack-damage.md).
