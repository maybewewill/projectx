# Abilities and buffs in Warcraft III 1.26a (Game.dll): framework spec, pass 1

Scope: the ability base class, the rawcode factory, per-ability fields, level changes, removal, cooldown (partial), the buff base class and the buff natives, timed life, and invulnerability (ability 'Avul'). Individual spells are not covered.

Labels: CONFIRMED = read from the decompile/disassembly and the code path is clear. LIKELY = strong evidence, not checked end to end. UNSURE = a guess or an open question.

Sources: decomp chunk files and natives.c in scratchpad/decomp, vfunc_names.tsv and vtables.tsv (slot tables written to scratchpad/slots_CAbility.txt, slots_CBuff.txt and slots_CUnit.txt during this pass). Disassembly via capstone. No runtime checks were run in this pass.

Conventions used throughout:
- Vtable slot number = byte offset / 4 (a call through offset 0x2E4 is slot 185).
- Raw codes are four ASCII characters read big-endian (0x4176756C = 'Avul').
- Calling convention notes come from the register use in the disassembly, not from the decompiler's parameter list, which is often wrong for these thiscall/fastcall functions.

---

## 0. Corrections to the goal statement

- Invulnerability is not at FUN_6F10FA90. That address is a 6-byte thunk that returns the constant 'Avul'. The native that adds or removes it is SetUnitInvulnerable at 0x6F3C72F0, which forwards to unit vtable slot 42 (CUnit::vf42 at 0x6F07B4A0). CONFIRMED.
- The natives UnitAddAbility, UnitRemoveAbility, UnitCountBuffsEx, UnitHasBuffsEx, UnitRemoveBuffs and UnitRemoveBuffsEx exist in 1.26a. There are no plain UnitCountBuffs or UnitHasBuffs natives in the native list. CONFIRMED.
- The factory dispatch function is FUN_6F24CDF0 and it is __fastcall: ECX is the unit, EDX is the raw code. The decompiler shows these as an argument list with the wrong names. CONFIRMED.

---

## 1. CAbility base

### 1.1 Objects and tables

- CAbility is the ability class. Its vtable has 189 entries (slots 0 to 188). CONFIRMED (RTTI row).
- CBuff has a 213-entry vtable and uses the same early slot layout as CAbility. Its slot 7 returns 'buff' and CAbility's slot 7 returns 'abil'. CBuff overrides the scalar deleting destructor (slot 1 at 0x6F0678B0). LIKELY that CBuff is a subclass of CAbility, since CBuff's table keeps CAbility-named functions at most indices (for example 13, 16, 23, 34 to 40, 119 to 122). The RTTI shows 227 CBuff class names and 491 CAbility-family class names.
- CAbilityDatabase (RTTI at 0x6F92E0A8) is the ability definition store. The lookup path is FUN_6F265ED0 (keyed by raw code) into FUN_6F264D50. A missing definition calls FUN_6F2657A0. CONFIRMED path, LIKELY class.
- Definition record fields read by the code (CONFIRMED): definition +0x30 = raw code (the factory's switch key); +0x2C = valid flag (zero means no definition); +0x3C = maximum level (defaults to 1 when there is no definition, through FUN_6F02E570); +0x50 = number of level records; +0x54 = array of level records, each 0x68 bytes; +0x18 = sub-structure handed to init (slot 32).
- Per-level data: FUN_6F02E6A0(level index) returns the record at index times 0x68 plus the array base, or zero when the index is negative or past the count. CONFIRMED.

### 1.2 Vtable slots (CAbility)

| Slot | Address | What it does (read) | Status |
|---|---|---|---|
| 0 | 0x6F001F70 | shared base destructor (name collides with other classes) | UNSURE |
| 1 | 0x6F052A10 | CAbility destructor, deleting variant | LIKELY |
| 3 | 0x6F0017A0 | base hook that returns 0 and takes one argument. AGENTS says this is the start hook for an order | CONFIRMED body, LIKELY role |
| 7 | 0x6F021200 | type tag: returns 'abil' | CONFIRMED |
| 10 | 0x6F02E390 | returns this plus 0x0A after a helper call | UNSURE |
| 32 | 0x6F052B10 | init with (unit, definition sub-structure). Sets the definition pointer (+0x54), the level record (+0x4C), an owner handle (+0x34, from FUN_6F262A70), calls FUN_6F052A60, then clears flag 0x10 in +0x20 when FUN_6F02E540 returns zero | CONFIRMED |
| 34 | 0x6F020A00 | base no-op (takes one argument). The factory calls it with the unit | CONFIRMED |
| 35 | 0x6F020A10 | base no-op. The factory calls it on the path where no unit is given | CONFIRMED body |
| 37 | 0x6F020A20 | base no-op. The factory calls it after init on the unit path | CONFIRMED |
| 40 | 0x6F052250 | base returns 0 (order-accept default) | CONFIRMED (AGENTS agrees) |
| 58 / 59 | 0x6F020A60 / 0x6F020A70 | base no-ops. Used as pause / resume hooks on timed-life entries (see 3.4) | CONFIRMED body, LIKELY role |
| 106 | 0x6F0525F0 | notification hook: called on the removed ability and on every other list entry during removal (offset 0x1A8) | CONFIRMED call, body not read |
| 108 / 109 | 0x6F020AD0 / 0x6F052610 | hooks called after an ability is linked to a unit (offsets 0x1B0 / 0x1B4) | CONFIRMED call, role UNSURE |
| 118 | 0x6F052670 | magic-class predicate. Base returns 0. CBuff overrides and returns 1 | CONFIRMED body, role LIKELY |
| 119 | 0x6F052680 | physical-class predicate. Base returns 0 | CONFIRMED body, role LIKELY |
| 120 | 0x6F052690 | "negative / harmful" predicate. Base returns 0 | CONFIRMED body, role LIKELY (see 2.5) |
| 121 | 0x6F0526A0 | "positive / beneficial" predicate. Base returns 0 | CONFIRMED body, role LIKELY (see 2.5) |
| 122 | 0x6F0526B0 | "aura" predicate. Base returns 0 | CONFIRMED body, role LIKELY |
| 146 | 0x6F0527F0 | rank. Base returns 0 (lowest rank wins for order lookup) | CONFIRMED (AGENTS agrees) |
| 151 | 0x6F052820 | returns the constant 0xDD (221). AGENTS puts the order decision here | CONFIRMED body, meaning UNSURE |
| 185 | 0x6F041700 | level up: index +0x50 increments, then the level record is recomputed into +0x4C | CONFIRMED |
| 186 | 0x6F041720 | level down: index +0x50 decrements, then the record is recomputed | CONFIRMED |
| 192 | beyond the base table | only defined in subclasses (AGENTS: the CAbilityMove family) | LIKELY |

Unit vtable slots used by the ability natives (CUnit table, 118 entries):
- Slot 42 (0xA8, CUnit::vf42 at 0x6F07B4A0): the invulnerability worker. CONFIRMED.
- Slot 59 (0xEC, CUnit::vf59 at 0x6F28B1D0): used during timed-life application. CONFIRMED offset, meaning UNSURE.
- Slot 96 and 107 exist in the same table; slot 107 is the unit init (AGENTS). Not re-checked here.

### 1.3 Per-ability fields

These offsets are read from the code paths named in column 3 (CONFIRMED unless marked).

| Offset | Field |
|---|---|
| +0x20 | flags. Bit 0x08 = removed (set by the remove routine FUN_6F079CC0). Bit 0x10 = cleared in init when FUN_6F02E540 is zero. Bit 0x20 = owned by a unit (the factory sets it before attach). Bit 0x40 = attach refuses when set (FUN_6F079990). Bit 0x80 = event listener registered (cleared by FUN_6F1E7590). Bit 0x20000000 = checked only when the predicate argument is zero; the natives always pass one, so it has no effect on them. |
| +0x34 | owner handle, from FUN_6F262A70 at init. Meaning LIKELY (player). |
| +0x38 | permanence counter. UnitMakeAbilityPermanent adds or subtracts one. |
| +0x4C | pointer to the current level record (0x68 bytes). |
| +0x50 | current level index, zero-based. GetUnitAbilityLevel returns this plus one. |
| +0x54 | pointer to the definition (fetched lazily from the database by raw code). |

Unit-side fields (CONFIRMED from FUN_6F079990, FUN_6F0787D0 and the natives):
- +0x1DC and +0x1E0: the ability list. The pair (+0x1E0 AND +0x1DC) equal to all ones means empty. The list is walked with FUN_6F4786B0.
- +0x1E8: attack ability ('Aatk' 0x4161746B).
- +0x1EC: move ability ('Amov' 0x416D6F76).
- +0x1F0: hero ability ('AHer' 0x41486572).
- +0x1F4: build abilities (ids 'ANbu', 'AHbu', 'AEbu', 'AGbu', 'AObu', 'AUbu').
- +0x1F8: 'AInv' (0x41496E76).
- Lookup by raw code (FUN_6F0787D0): the six special ids above are answered from these fixed fields. Other ids scan the list and compare the slot-7 type tag or a subclass match. CONFIRMED.

### 1.4 Creation: the rawcode factory

Entry points:
- UnitAddAbility (0x6F3C82A0): the unit is resolved, then FUN_6F0787D0 checks whether the id is already present. If it is, the native returns false. Otherwise it calls the factory with the unit and the raw code, and the result (non-null) is the return value. CONFIRMED.
- SetUnitInvulnerable uses the same factory with 'Avul' (see section 5).
- The unit creation path (CreateUnit, 4.3n) builds abilities from the unit type's list, but that path was not traced in this pass.

Factory FUN_6F24CDF0 (CONFIRMED sequence):
1. Look up the definition with FUN_6F265ED0 (keyed by the raw code). A missing definition returns null.
2. Check the definition's registry index through FUN_6F0098F0. If it is invalid, return null.
3. Switch on the definition's raw code (+0x30). Each leaf calls one wrapper. A code with no leaf returns null, so UnitAddAbility returns false for it.
4. The wrapper calls a small registry lookup (FUN_6F001EC0, a hash keyed by the raw code and a second value) and builds the object.
5. Call slot 32 on the new object with (unit, definition sub-structure).
6. If a unit was given: set flag 0x20, call slot 34 with the unit, then call slot 37, then attach with FUN_6F079990(unit, object). Without a unit: call slot 35 instead and do not attach.
7. Return the object. The attach routine FUN_6F079990 links the object into the unit's list and stores special ids in the fixed fields (1.3). It also calls slots 108 and 109, and the event-registration helpers.

Factory table (417 leaves, 333 distinct codes):
- The full mapping is in scratchpad/specs/ability_factory_1.26a.tsv (raw code, wrapper address, ambiguity flag).
- Each wrapper is a 207-byte template. Its first call returns the wrapper's raw code as a constant. CONFIRMED.
- Examples: 'Avul' -> FUN_6F07ABC0 (CONFIRMED, used by SetUnitInvulnerable). 'Aloc' -> FUN_6F245A90. 'AHbz' -> FUN_6F243530. 'Abur' -> FUN_6F239520. 'AHre' -> FUN_6F246380. 'AInv' -> FUN_6F2458F0 (this code appears under several leaves, see below).
- 'Amov' and 'Aatk' do not have leaves. They are unit fixed fields, so UnitAddAbility with those ids should return false. LIKELY, not tested.
- Class names: NOT resolved. The wrappers do not contain a vtable store; the class is determined by the registry entry they look up, and that registration code was not found. So the table maps raw codes to wrappers, not to CAbilityXxx class names. UNSURE on every class name. The ~491 CAbility-family RTTI names exist, so the names can be matched later once the registry is found.
- Ambiguity: 97 leaf rows belong to 13 codes that appear under several leaves (for example 'AIgl', 'Arng', 'ANth', 'AIsa'). My nearest-comparison parse merged some nested branches, so those rows are likely artifacts. Treat them as unreliable until checked.

### 1.5 Level changes (CONFIRMED)

- GetUnitAbilityLevel (0x6F3C7290): returns the index plus one, or zero when the ability is absent.
- IncUnitAbilityLevel (0x6F3C7130): if the zero-based index is below the maximum (definition +0x3C, default 1), call slot 185 once. Returns the new level. Because the check is "below max" and not "below max minus one", the level can reach max plus one. This matches AGENTS' note (UNSURE, now read from the code).
- DecUnitAbilityLevel (0x6F3C70B0): if the index is above zero, call slot 186 once. Returns the new level.
- SetUnitAbilityLevel (0x6F3C71B0): clamps the requested level to at least 1, and to the maximum when the maximum is at or below the request. Then it calls slot 185 or slot 186 repeatedly until the level matches. Returns the new level.
- Each natives path requires a valid unit and a non-removed unit handle, then finds the ability with FUN_6F0787D0.

### 1.6 Removal (CONFIRMED where stated)

- UnitRemoveAbility (0x6F3C8310): finds the ability, calls FUN_6F079CC0, then FUN_6F332830, and returns true.
- FUN_6F079CC0 (partly read): if flag 0x08 is not already set, set it. Then call slot 106 on every other entry in the unit's list and on the ability itself. Then unlink it and update the head and tail pair. Whether the fixed unit fields (1.3) are cleared on removal was not confirmed (UNSURE).
- The object is not destroyed by this path. Its release is handled by a reference count (UNSURE; the release call was not traced).
- LIKELY: permanence (+0x38) does not block removal, because the remove routine does not read it. Only the first half of the routine was read.

---

## 2. Cooldowns (partial)

Status: NOT found. Only the entry points and the mechanism candidates are recorded.

- UnitResetCooldown (0x6F3C86D0) calls FUN_6F078C00 with the unit. That routine walks the unit's list, filters entries by a tag check, and calls per-entry work. CONFIRMED that the walk and the tag filter exist. The decompiled and disassembled argument use disagree at the tag call, so the filter tag and the per-entry action are UNSURE.
- Cooldown storage: not located. The level record (0x68 bytes, pointer at +0x4C) is the likely home for cooldown values, but no cooldown field offset was identified. The object-data field names are in a table in the unit-data loader (cooldownID and ignoreCD are seen for items and units), but the ability cooldown key was not found in a literal string.
- Per-tick decrement: not located. The candidates are the unit's update path or a CAgentTimer (section 4.3h of AGENTS). UNSURE.
- Cooldown start on cast: not located.
- Arithmetic: the soft-float routines are SUB at 0x6F6EEEF0 and ADD at 0x6F6EF000 (from docs/spec/softfloat.md). Whether cooldowns use them is not confirmed.

---

## 3. Buffs

### 3.1 Classes

- CBuff base: 213 vtable entries. Slot 1 is the deleting destructor (0x6F0678B0). Slot 7 is the tag 'buff' (0x6F0D1AC0). Slot 199 is apply, and it forwards to slot 200 with an extra constant 1 (0x6F0656A0, CONFIRMED). Slot 118 returns 1 (0x6F0656E0, CONFIRMED).
- CBaseBuffInvulnerable: a family of buff classes with its own overrides at slots 200, 203 and 204 (0x6F0D4CB0, 0x6F0656F0, 0x6F065700). Slot 203 returns 0; slot 204 calls slot 118 on the same object, so it is a dispellable test that reuses the magic test (LIKELY). The 'Avul' object is probably in this family, but that was not proven (UNSURE).
- CBuffTimedLife: the constructor is FUN_6F0D8570. It builds a CBuffProgressBar base, sets a FloatMini sub-object at +0xCC and a second sub-object at +0xD0, then sets its own vtable. CONFIRMED. Its tag (slot 7 at 0x6F0D2480) returns 'BTLF' (0x42544C46), CONFIRMED. Its slot 22 returns a name string pointer (0x6F0D2430).
- Other buff classes exist (227 RTTI names, e.g. CBuffSlow, CBuffStun, CBuffPoisonAttack, CBuffPhoenix, CBuffControlMagic, CBuffDarkMinion). Their creation paths were not traced.

### 3.2 Application

- The generic apply path (CONFIRMED): slot 199 on the buff object takes the unit and the duration and forwards to slot 200 (byte offset 0x320) with an extra constant one. The override used by the timed-life class (FUN_6F0D67C0, which calls FUN_6F0D28F0) first calls slot 200 with the unit, the duration and one, then calls slot 0 of a sub-object stored at +0xCC with the duration and one, then calls FUN_6F332810. The sub-object is probably the progress/float helper (LIKELY).
- Registration: FUN_6F2AB1E0 with flag one registers event 0xD01A1 for the buff object. LIKELY this is a "buff gained" event. The event id was not named.
- Stacking and replacement: NOT located. There is no evidence in the buff natives of a per-id stack limit. UNSURE.
- Periodic effects: NOT located. UNSURE.
- Duration timer: the timed-life sub-object may hold a timer, but this was not confirmed. The likely mechanism is CAgentTimer (its vtable is at 0x6F8784AC, 10 entries). UNSURE.

### 3.3 Predicates and the buff natives (CONFIRMED)

The four natives (all fixed-argument calls to shared helpers):

| Native | Address | Helper | Behavior |
|---|---|---|---|
| UnitHasBuffsEx | 0x6F3C8360 | FUN_6F076D00 | returns the helper's value (boolean), CONFIRMED |
| UnitCountBuffsEx | 0x6F3C83A0 | FUN_6F076E60 | counts matching entries, returns the count |
| UnitRemoveBuffs | 0x6F3C83E0 | FUN_6F079FE0 | remove, with fixed filters (see below) |
| UnitRemoveBuffsEx | 0x6F3C8410 | FUN_6F079FE0 | remove, with the caller's filters |

Argument mapping for the seven booleans of the Ex natives, in WC3 order: B1 removePositive, B2 removeNegative, B3 magic, B4 physical, B5 timedLife, B6 aura, B7 dispellable.

Predicate slots used by the helpers:
- B3 magic: slot 118, and the test is skipped when B3 is false.
- B4 physical: slot 119, skipped when B4 is false.
- B5 timedLife: slot 203, skipped when B5 is true.
- B6 aura: slot 122, skipped when B6 is true.
- B7 dispellable: slot 204 (called with zero), skipped when B7 is false.
- Polarity: slot 120 and slot 121 (names are LIKELY, see below).

Polarity rules for counting and checking (FUN_6F076E60 and FUN_6F076D00):
- An entry passes the polarity test when (slot 121 is zero, or B1, or not B2) and (slot 120 is zero, or B2, or not B1).
- With B1 and B2 both false, every entry passes. With B1 only, positive entries pass and negative entries fail. With B2 only, the reverse.

Polarity rules for removal (FUN_6F079FE0):
- An entry is removed only when it has polarity 121 and B1 is set, or it has polarity 120 and B2 is set. Then the other filters must pass.
- With both B1 and B2 false, nothing is removed. This differs from counting, where both false counts everything.
- Removal calls FUN_6F079CC0 on each match, which is the same routine as UnitRemoveAbility, so removal also sends slot 106 notifications.

Native-specific fixed filters:
- UnitRemoveBuffs passes (physical 0, magic 0, timedLife 1, aura 1, dispellable 0). So it removes timed-life and aura entries and applies no physical, magic or dispellable filter. CONFIRMED from the native call.

Names for slots 120 and 121 are LIKELY from the polarity behavior (120 = harmful, 121 = beneficial), not from a read of their bodies (both bodies are constant zero in the base table).

### 3.4 Timed life

- UnitApplyTimedLife (0x6F3C85D0): calls FUN_6F07B580 with the buff id and the duration. Then, if the unit flag 0x200 (unit +0x64) was set, it calls the unit's slot 59, adjusts a counter with FUN_6F40F5E0 by the negated value from FUN_6F29BAF0, and clears flags 0x100 and 0x200. The meaning of this adjustment is LIKELY supply/food accounting (UNSURE).
- FUN_6F07B580 dispatch (CONFIRMED): seven special buff ids select one of seven creators. The ids are 'BUan' (0x4255616E), 'BFig' (0x42466967), 'BEfn' (0x4245666E), 'Bhwd' (0x42687764), 'Bplg' (0x42706C67), 'Brai' (0x42726169), 'BHwe' (0x42487765). The creators are FUN_6F07AC90, FUN_6F07AD60, FUN_6F07AE30, FUN_6F07AF00, FUN_6F07AFD0, FUN_6F07B0A0, FUN_6F07B170. Any other id falls to the default creator FUN_6F07B240, which builds a CBuffTimedLife ('BTLF'). The 'BTLF' id itself is not compared; it simply takes the default branch.
- After the creator returns, the routine calls slot 199 on the buff with (unit, duration), then releases the buff with FUN_6F079CA0. CONFIRMED.
- The creators' class mapping (which of the seven is which class) was not resolved.
- UnitPauseTimedLife (0x6F3C8750) calls FUN_6F078E90 with the flag. That routine walks the unit's list, filters for the 'BTLF' tag, and calls slot 58 when the flag is true or slot 59 when it is false on each match. LIKELY (the tag filter argument is the same ambiguous pattern as section 2).
- Expiry: the buff's timer end is not traced. The expected result is a unit death that uses the same kill path as KillUnit (event 0xD01E6). UNSURE. The death event 0xD01E6 is emitted from the unit setup/teardown area (FUN_6F08A260 and FUN_6F2AB420 register or unregister it); the timer callback was not found.

---

## 4. Event and timer notes (for the timer agents)

- Unit event 0xD01E6 (death) is registered and unregistered through FUN_6F2AB420 (flag true registers, false unregisters). CONFIRMED.
- Event 0xD01A1 is registered by FUN_6F2AB1E0 for buffs. LIKELY "buff gained"; not named.
- CAgentTimer vtable (0x6F8784AC, 10 entries) and CAgentTimerExtended exist. Their use by buffs was not confirmed.

---

## 5. Invulnerability as ability 'Avul' (CONFIRMED except the class)

- The id is 'Avul' = 0x4176756C. It is returned as a constant by FUN_6F10FA90, a 6-byte thunk.
- SetUnitInvulnerable (0x6F3C72F0): resolves the unit, checks that the unit is valid and not removed, then calls unit slot 42 with the flag.
- CUnit::vf42 (0x6F07B4A0):
  - Looks up 'Avul' in the unit's list (FUN_6F0787D0).
  - If the state already matches the flag, nothing happens.
  - If the ability exists and the flag is false, it removes it (FUN_6F079CC0).
  - If the ability does not exist and the flag is true, it builds the ability: FUN_6F25F5C0 prepares the data, then FUN_6F07ABC0 (the 'Avul' factory wrapper) creates the object, then slot 32 initialises it with the unit. The attach step follows the factory sequence in 1.4.
- So "invulnerable" is "has ability 'Avul'", and that is the mechanism the 4.3q notes described. CONFIRMED.
- The class of the 'Avul' object is UNSURE. It is probably in the CBaseBuffInvulnerable family (LIKELY), because that family's overrides (slots 200, 203, 204) are the only ones that fit a protective flag. Whether it lives in the buff list or the ability list is not separated: both are the same unit list.
- Where the damage code reads invulnerability (AGENTS 4.3 attack damage) was not re-checked here.

---

## 6. Locust 'Aloc'

- 'Aloc' maps to FUN_6F245A90 in the factory table. CONFIRMED mapping, class UNSURE.
- Nothing about its behavior was read in this pass.

---

## 7. Open items (in priority order)

1. Find the registry that the wrappers look up (the hash fed by FUN_6F001EC0) and where it is filled. This gives the rawcode-to-class table, which is the main gap.
2. Find the cooldown field in the 0x68 level record and the code that starts a cooldown on cast. Read the update path that decrements it (unit update or a CAgentTimer).
3. Read CBaseBuffInvulnerable::vf200 (0x6F0D4CB0) for the attach step, and the stacking check it may contain.
4. Trace the timed-life timer end to the death call (expected to be KillUnit, event 0xD01E6).
5. Resolve the unit-level CUnit::vf59 (0x6F28B1D0) and the supply adjustment in UnitApplyTimedLife.
6. Decide the meaning of slot 151 (constant 0xDD) and of the list walk FUN_6F4786B0.
7. Runtime check (Unicorn or the original under Wine): UnitAddAbility('Amov') returns false; SetUnitAbilityLevel clamps at the maximum; IncUnitAbilityLevel reaches max plus one; the polarity table for count versus remove.

## 8. Files written

- scratchpad/specs/abilities.md (this file)
- scratchpad/specs/ability_factory_1.26a.tsv (raw code, wrapper address, ambiguity flag, status)
- Named vtable slot tables: scratchpad/slots_CAbility.txt, scratchpad/slots_CBuff.txt, scratchpad/slots_CUnit.txt.
