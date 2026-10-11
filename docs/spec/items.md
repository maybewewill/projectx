# Items: objects, pack (inventory), enumeration and random item selection (WC3 1.26a Game.dll)

Scope: the item natives (CreateItem, RemoveItem, position, owner, visibility, invulnerability, charges, flags),
the unit pack (UnitAddItem family, drop/use orders, UnitHasItem / UnitItemInSlot / UnitInventorySize),
EnumItemsInRect with GetEnumItem / GetFilterItem, and ChooseRandomItem / ChooseRandomItemEx.
Plain-English clean-room spec. Addresses are Game.dll VAs (image base 0x6F000000).

Confidence tags:
- CONFIRMED: read directly in the decompile or disassembly, or matches a constant read from code.
- LIKELY: strong structural evidence, not fully proven.
- UNSURE: guessed from names or position, or the code was not read.

Method: native bodies from `decomp/natives.c`, helper bodies from the chunk files (`extract_body.py`), argument
passing checked with capstone (`dis_fn.py`) where the decompiler hid it. Vtable slots mapped with the CItem
vtable (`.?AVCItem@@`, 110 entries, slot = byte offset / 4). Constants were read from the runtime-filled
values in `docs/spec/rng.md` and `tools/emu/runtime_consts.py` results.

Note on decompiler arguments: for __thiscall helpers the decompiler prints the explicit arguments and omits the
`this` pointer (ECX). Where a call site looked contradictory, the mapping below was checked against the
callee's own parameter use. Mapping rules used: `this` is the unit for pack helpers, `this` is the item for
CItem vtable methods.

---

## 0. Key findings (summary)

1. Items are CItem objects (CAgent-derived, vtable `0x6F9320B4`) reached through handle ids, the same way units
   are. The item resolver is FUN_6F3BEB50 (it resolves the handle, then the agent kind tag and removed flag at
   +0x20, like the unit resolver FUN_6F3BDCB0). CONFIRMED structure.
2. CreateItem builds the item object completely (type record load, placement, virtual init) and only then
   allocates the handle id (FUN_6F430C80, LIFO free-list rules from handles-2.md). No RNG draw was found on this
   path. CONFIRMED order; no RNG LIKELY.
3. A unit's pack is an array of N slots stored inside the unit object (N = unit+0x6C, 12-byte entries starting at
   unit+0x70). An empty slot holds the pair (-1, -1). Adding an item always takes the LOWEST free slot index.
   CONFIRMED.
4. Powerup items (flag 0x800) never occupy a slot; they are consumed at pickup. CONFIRMED (branch in the add path).
5. A full pack returns error code 0x11 on the add check and the add fails; the item is NOT placed in a slot.
   CONFIRMED (error and branch); where the item ends up after the failed UnitAddItemById is LIKELY (it stays on the
   ground at the unit position).
6. UnitRemoveItem and UnitRemoveItemFromSlot do not destroy the item. They take it out of the slot and place it on
   the ground at the unit position. CONFIRMED.
7. Drop natives (UnitDropItemPoint/Slot/Target) do not move the item directly. They issue a unit ORDER (order ids
   0xD0021 for point drop and 0xD0022 + slot index for slot drop). The order system does the move later.
   CONFIRMED that an order is issued; the order execution itself was not read.
8. ChooseRandomItem and ChooseRandomItemEx draw from RNG stream 35 (0x23) exactly once when at least one candidate
   exists, and pick candidates[floor(r * n / 2^32)] from a candidate list in the order of a global item-type
   linked list. CONFIRMED (stream, call structure, filter). The list order is LIKELY load-history based (see UNSURE
   section).
9. Several things are still open and listed in section 9 (event ids for pickup and drop, enum walker order, item
   pool path PlaceRandomItem, the exact pack-capacity source, the mechanism of SetItemVisible(true)).

---

## 1. Native-to-code map

| Native | Address | Core / helper | Notes |
|---|---|---|---|
| CreateItem(type, x, y) | 0x6F3BB9A0 | FUN_6F2B9940, then FUN_6F3A8060, then handle alloc FUN_6F430C80 | args: type in ECX, x as pointer in EDX, y as pointer on stack. CONFIRMED (disassembly) |
| RemoveItem | 0x6F3C4C30 | vf33 (hide/remove), then vf73 with 0.0 | CONFIRMED |
| GetItemTypeId | 0x6F3C4C60 | reads item+0x30 directly | CONFIRMED; returns 0 if item invalid |
| SetItemPosition(item, x, y) | 0x6F3C4C80 | holder removal (FUN_6F282E40) if carried, then FUN_6F2B6AE0 | CONFIRMED structure |
| GetItemX / GetItemY | 0x6F3C4D90 / 0x6F3C4DD0 | vf46 (0xB8) then FUN_6F4743A0 | CONFIRMED; invalid -> the constant at DAT_6FAAE470 (0.0) |
| SetItemDropOnDeath | 0x6F3C4D00 | flag 0x80 | CONFIRMED |
| SetItemDroppable | 0x6F3C4D30 | flag 0x400 | CONFIRMED |
| SetItemPawnable | 0x6F3C4D60 | flag 0x2000 | CONFIRMED |
| GetItemPlayer | 0x6F3C4E10 | vf100 (0x190) gives owner index, then FUN_6F3A1650, then handle | LIKELY |
| SetItemPlayer(item, player, changeColor) | 0x6F3C4EB0 | FUN_6F2B6AC0: writes owner index to item+0x74 | CONFIRMED write; LIKELY meaning |
| SetItemInvulnerable | 0x6F3C4EE0 | vf42 (0xA8) = FUN_6F077E20, flag 0x8 | CONFIRMED |
| IsItemInvulnerable | 0x6F3C4F00 | reads flag 0x8 | CONFIRMED |
| GetItemLevel | 0x6F3C4E50 | FUN_6F2B8C50 (reads item-type record +0x38) | CONFIRMED |
| GetItemType | 0x6F3C4E70 | FUN_6F2B8CB0 (item-type record +0x3C); returns 7 when invalid | CONFIRMED |
| SetItemVisible(item, flag) | 0x6F3C4F20 | vf33 (hide) when false, vf32 (show) when true | CONFIRMED dispatch; see UNSURE 9.4 |
| IsItemVisible | 0x6F3C4F70 | ~flags & 1 | CONFIRMED |
| IsItemOwned | 0x6F3C4F90 | holder key at +0x8C/+0x90 valid | CONFIRMED |
| GetItemCharges | 0x6F3C5050 | reads item+0x84 | CONFIRMED |
| SetItemCharges | 0x6F3C5070 | see section 4 | CONFIRMED |
| EnumItemsInRect(rect, boolexpr, action) | 0x6F3C5110 | rect resolver FUN_6F3BDB90, dispatcher FUN_6F4693D0 -> FUN_6F47FA90 | structure CONFIRMED; walk order UNSURE |
| GetEnumItem / GetFilterItem | 0x6F3BB3A0 / 0x6F3BB3C0 | enum slot 0xE / filter slot 0xD (FUN_6F430DB0) | CONFIRMED slots |
| UnitAddItem(unit, item) | 0x6F3C73F0 | FUN_6F282C60 check, FUN_6F282CA0 error check, FUN_6F282D30 add | CONFIRMED |
| UnitAddItemById(unit, id) | 0x6F3C74F0 | item created at unit position, flag 0x100, then add | CONFIRMED structure |
| UnitAddItemToSlotById(unit, id, slot) | 0x6F3C75B0 | FUN_6F282D90 to a given slot | CONFIRMED structure |
| UnitRemoveItem(unit, item) | 0x6F3C7660 | FUN_6F282E40 | CONFIRMED |
| UnitRemoveItemFromSlot(unit, slot) | 0x6F3C76A0 | FUN_6F282EC0: item from slot, then FUN_6F282E40 | CONFIRMED |
| UnitHasItem(unit, item) | 0x6F3C76F0 | FUN_6F26EC40 then FUN_6F0F8E80 (scan) | CONFIRMED |
| UnitItemInSlot(unit, slot) | 0x6F3C7730 | FUN_6F26EC20 (slot read) then handle FUN_6F430C80 | CONFIRMED |
| UnitInventorySize(unit) | 0x6F3C8F00 | FUN_6F26EC60 (slot count) | CONFIRMED |
| UnitDropItemPoint(unit, item, x, y) | 0x6F3C8F30 | order 0xD0021 via FUN_6F3C8A20 | CONFIRMED |
| UnitDropItemSlot(unit, item, slot) | 0x6F3C9010 | order 0xD0022 + slot via FUN_6F3C89D0 | CONFIRMED |
| UnitDropItemTarget(unit, item, widget) | 0x6F3C8FA0 | order via target (not read) | LIKELY |
| UnitUseItem / UseItemPoint / UseItemTarget | 0x6F3C9090 / 0x6F3C9100 / 0x6F3C9170 | checks same as drop, then order (not read) | LIKELY |
| ChooseRandomItem(level) | 0x6F3B3690 | FUN_6F2B9E10(cat=7, level, excl=0, flag4=1, flag5=0) | CONFIRMED |
| ChooseRandomItemEx(type, level) | 0x6F3B36B0 | FUN_6F2B9E10(cat=type, level, excl=0, flag4=1, flag5=0) | CONFIRMED |
| PlaceRandomItem(pool, x, y) | 0x6F3CB700 | pool path, not read | UNSURE |

Shared helpers (named in this spec):

| Helper | Address | Role |
|---|---|---|
| Item resolver | FUN_6F3BEB50 | handle -> item object, NULL if invalid. CONFIRMED |
| Unit resolver | FUN_6F3BDCB0 | handle -> unit, see unit-core.md. CONFIRMED |
| Item object builder | FUN_6F2B9940 | (type, &x, &y) -> new item object (not yet handled). CONFIRMED |
| Item type loader | FUN_6F2B90C0 | fills the item-type record for the new item. CONFIRMED |
| Item init (vf107) | FUN_6F2B7C60 | placement, flags, charges, owner. CONFIRMED structure |
| Pack capacity | FUN_6F0E4050 | min(unit+0x6C, FUN_6F6EEC20()). CONFIRMED structure; source UNSURE |
| Occupied count | FUN_6F0E3DB0 | counts slots with a valid key. CONFIRMED |
| Pack-full test | FUN_6F0F0E80 | occupied == capacity. CONFIRMED |
| Slot read | FUN_6F0F0F00(slot) | valid key at slot. CONFIRMED |
| Add-check | FUN_6F282CA0 | returns an error code, 0 = OK. CONFIRMED |
| Add (to unit) | FUN_6F282D30 | powerup -> consume, else first free slot. CONFIRMED |
| First free slot | FUN_6F0FD040 | lowest index with free key. CONFIRMED |
| Slot write | FUN_6F0F8C10 | writes slot key, holder, moves item. CONFIRMED structure |
| Remove from pack | FUN_6F0F0F80 | clears slot, detaches item abilities. CONFIRMED structure |
| Remove and drop | FUN_6F282E40 | remove from pack, place at unit position. CONFIRMED |
| Drop-by-slot | FUN_6F282EC0 | slot item then remove. CONFIRMED |
| Unit can carry | FUN_6F282C60 | unit flags and inventory check. CONFIRMED |
| Pickup permission | FUN_6F0F8F60 | returns 0 if allowed, else 0x55, 0xDD or 0xD3. CONFIRMED structure |
| Item ability setup | FUN_6F2B9600 | parses up to 4 abilities from the type string. CONFIRMED structure |
| Item-type field reads | FUN_6F2B8C50, 8CB0, 8C80, 8CE0, 8D10, 8D70 | record fields +0x38, +0x3C, +0x40, +0x44, +0x48, +0x70. CONFIRMED |
| Random selector | FUN_6F2B9E10 | described in section 7. CONFIRMED structure |
| Random pick | FUN_6F285320(stream, n) | high32(step(stream) * n). CONFIRMED |

---

## 2. Item object: fields touched

Offsets from the CItem object start (the object is also a CAgent, so the agent-level fields are shared).

| Offset | Meaning | Confidence |
|---|---|---|
| +0x00 | vtable pointer (CItem) | CONFIRMED |
| +0x0C | kind tag (same resolver check as units) | LIKELY |
| +0x14 / +0x18 | item key pair. Copied into a pack slot when the item is added. Slot empty value is (-1, -1). Initial values: +0x14 from a per-type lookup, +0x18 = 0 | CONFIRMED (copy), LIKELY (meaning) |
| +0x20 | flag word (see table below). Bit 0 is also the "removed/hidden" bit that the resolver reads | CONFIRMED (bits), LIKELY (bit 0 meaning) |
| +0x30 | item type id (rawcode) | CONFIRMED (GetItemTypeId reads it) |
| +0x54 | object used by the lifetime setter (vf73) | LIKELY |
| +0x74 | owner player index (byte written from the player object at +0x30) | CONFIRMED write, LIKELY meaning |
| +0x7C | dynamic value written by FUN_6F2B5D00 (0 on hide, 1 on show) | CONFIRMED write, meaning UNSURE |
| +0x84 | charges (int) | CONFIRMED |
| +0x88 | value set from the type record at creation (meaning UNSURE) | CONFIRMED write |
| +0x8C / +0x90 | holder key pair (the unit carrying the item). (-1,-1) when on the ground | CONFIRMED (read in IsItemOwned and SetItemPosition) |
| +0x9C .. +0xA8 | up to 4 ability ids from the type's ability string (written by FUN_6F2B9600) | LIKELY |
| +0xB8 / +0xBC | movement/position-related keys used by GetItemX/Y and SetItemPosition | UNSURE |
| +0x164 | mover sub-object (position), the same pattern as units | LIKELY |

Flag word at +0x20 (CONFIRMED bits, from the setters and the creation code):

| Bit | Name | Set by |
|---|---|---|
| 0x001 | hidden / removed | RemoveItem and SetItemVisible(false) (vf33) |
| 0x008 | invulnerable | SetItemInvulnerable (vf42) |
| 0x040 | (type template bit) | creation, from the type record |
| 0x080 | drop on death | SetItemDropOnDeath, creation from type |
| 0x200 | usable (has abilities) | creation from type; checked in FUN_6F2B9600 |
| 0x400 | droppable | SetItemDroppable, creation from type |
| 0x800 | powerup (consumed on pickup, never occupies a slot) | creation from type |
| 0x1000 | no charges (set when charges are 0) | SetItemCharges, creation |
| 0x2000 | pawnable | SetItemPawnable, creation from type |
| 0x40000000 | "not a unit-pack item" marker tested in pickup and drop paths | LIKELY |
| 0x100 | counted as "in the world" once (player counter at player+0x254 is incremented) | CONFIRMED (FUN_6F2B9600 path), meaning LIKELY |

---

## 3. CreateItem: order of effects

### 3.1 Native entry (CONFIRMED from disassembly)
- The native takes the item type in ECX as a plain integer, x as a pointer to a float32 in EDX, and y as a pointer
  to a float32 on the stack. It calls FUN_6F2B9940(type, &x, &y).
- After the builder returns, it calls the lazy-init guard FUN_6F3A8060, then FUN_6F430C80(result, 0) to get the
  handle id. If the builder returned 0, the handle call gets 0.

### 3.2 Builder FUN_6F2B9940 (CONFIRMED order)
1. Loads the item-type record (FUN_6F2B90C0). If this fails, the builder logs an error (FUN_6F4C48F0) and returns
   NULL. No handle is created for a NULL result.
2. Copies x and y from the pointers into a local placement record.
3. Creates the object: a smart pointer is taken (FUN_6F038890), the object is built (FUN_6F2B5AE0, FUN_6F4C8520,
   FUN_6F001EC0, FUN_6F472C20, FUN_6F48AB80, FUN_6F02A6A0). These were not all read in full.
4. Calls the virtual init, CItem vf107 = FUN_6F2B7C60 (see 3.3).
5. Builds the 3x3 orientation (FUN_6F2B6270: identity with a rotation from cos/sin of a fixed angle). No RNG.
6. Copies a 9-dword block of the result.
7. Returns the object.
No RNG call was observed in any of these (CONFIRMED for the read functions; LIKELY for the rest).

### 3.3 Init FUN_6F2B7C60 (vf107, CONFIRMED order)
1. Calls vf 0x164 on the object (a placement/path query with fixed arguments) and FUN_6F473D10 with the constant
   at DAT_6FAAE4F4 (1.0 at runtime). If the query succeeds the placement point is stored to the object (+0x14 side
   of the record and +0x18). LIKELY.
2. Sets z = terrain height at (x, y) through FUN_6F0126F0(x, y, 0). LIKELY (terrain height).
3. Inserts the object into the spatial structure (FUN_6F2ACDE0). LIKELY.
4. Calls vf 0x148(1,1,1) and FUN_6F2B71B0(1), FUN_6F333500.
5. Sets flag bits from the type record (see section 2 flag table), and sets 0x1000 when the template charge value is
   0. CONFIRMED.
6. Copies the template charge value into +0x84 and a second template value into +0x74 (owner index) and +0x88.
   CONFIRMED copies; meaning LIKELY.
7. Calls FUN_6F474A20, then vf 0x13C; if that is non-zero it calls FUN_6F4749E0(1). LIKELY that this is the
   "is a live object" hook.

### 3.4 Type record loaded by FUN_6F2B90C0 (CONFIRMED field writes)
- +0x0C: a float from the record builder (LIKELY the scale/radius value).
- +0x14: a per-type value from FUN_6F2B8FE0 (LIKELY the item id-key base, used as the item key at +0x14 of the
  object).
- +0x38 and +0x3C: both set to the same value from FUN_6F2B9050 (the item level at +0x38, the class at +0x3C).
- +0x40: default 0.0 (DAT_6FAAE470).
- +0x44: default 0.1 (DAT_6FAAE4A0). Nonzero means "pawn value/ droppable" (LIKELY).
- +0x48: default 0.
- +0x5C .. +0x74: values from FUN_6F2B8D40, 8E60, 8E90, 8EC0, 8EF0, 8DD0, 8E30 (type fields, meanings UNSURE).
- Returns 1 when the record was filled, 0 otherwise.

---

## 4. Charges, owner, visibility, invulnerability, flags (native-level behavior)

- GetItemCharges(item): returns item+0x84, or 0 if the item is invalid. CONFIRMED.
- SetItemCharges(item, n):
  - n < 0: sets flag 0x1000, sets charges to 0, then refreshes the UI (FUN_6F332700). CONFIRMED.
  - n == 0: same as n < 0. CONFIRMED.
  - n > 0: clears flag 0x1000, sets charges to n, refreshes the UI. CONFIRMED.
  - Invalid item: no effect. CONFIRMED.
- SetItemPlayer(item, player, changeColor): the owner index (the player object's byte at +0x30) is stored at item+0x74.
  If changeColor is non-zero, the UI tint for that index is updated. CONFIRMED write, LIKELY meaning.
- GetItemPlayer: reads the owner index via vf100, converts it to a player object, returns the handle for that
  player (allocated on demand). LIKELY.
- SetItemInvulnerable(item, flag): vf42 sets (flag != 0) or clears bit 0x8 of +0x20. IsItemInvulnerable returns
  bit 0x8. CONFIRMED.
- SetItemDropOnDeath / SetItemDroppable / SetItemPawnable: set or clear bits 0x80 / 0x400 / 0x2000. CONFIRMED.
- IsItemOwned: returns true when the holder key at +0x8C/+0x90 is not (-1,-1) and resolves to a live object.
  CONFIRMED.
- SetItemPosition(item, x, y):
  1. If the item is carried (holder key valid, holder is a unit with the expected kind tag and not removed), the
     item is first removed from that unit's pack and placed at the unit position (FUN_6F282E40). CONFIRMED.
  2. Then the new position is applied with FUN_6F2B6AE0(&x, &y, -1). LIKELY that this also updates z and the
     orientation matrix.
  - Invalid item: no effect. CONFIRMED.
- SetItemVisible(item, flag): if the item is currently carried (holder key at +0x8C/+0x90 valid and resolves to a
  live object), the call does nothing. CONFIRMED from the native body. Otherwise:
  flag false -> vf33 (hide); flag true -> vf32 (show). CONFIRMED dispatch; the effect on bit 0 is UNSURE (see 9.4).
- IsItemVisible(item): returns (~flags & 1). CONFIRMED.
- RemoveItem(item): vf33 (hide: sets bit 0 of +0x20, FUN_6F2B5D00(0), the spatial removal FUN_6F2AB310(-1,-1), and
  other calls), then vf73 with 0.0. vf73 (FUN_6F2B89E0) clamps the value to >= 0 and writes it into the object at
  +0x54 (a lifetime/expiry timer, LIKELY), then calls a threshold check. The item object is NOT freed by this native.
  Its handle id stays valid until the object's refcount is zero (handles-2.md). LIKELY.

---

## 5. Unit pack (inventory)

### 5.1 Layout (CONFIRMED)
- unit+0x1F8: inventory object pointer. Zero means "unit has no pack". Most pack natives return the default when
  this is zero.
- unit+0x6C: pack capacity field (the number of slots stored with the type).
- unit+0x70: slot array. Each slot is 12 bytes: (key lo at +0, key hi at +4, extra value at +8, not used in the
  checks). Slot i is at unit+0x70 + 12*i.
- Empty slot: both key words equal 0xFFFFFFFF. A slot is also treated as free when its key does not resolve to a
  live object (FUN_6F4786B0 returns 0).
- Slot key: the item's key pair (item+0x14, item+0x18) copied when the item is put into the slot. CONFIRMED.
- Capacity: FUN_6F0E4050 returns min(unit+0x6C, FUN_6F6EEC20()). The second value comes from a runtime helper that
  was not read. UnitInventorySize returns this value. CONFIRMED structure; source UNSURE.
- Occupied count (FUN_6F0E3DB0): number of slots whose key is not (-1,-1) and resolves to a live object.
  CONFIRMED.

### 5.2 UnitAddItem(unit, item) (CONFIRMED order)
1. Resolve unit and item. If either is invalid, return false (0).
2. The unit must have a pack and pass FUN_6F282C60 (unit flags and the pack's own check, vf 0x268 with argument 0).
   If not, return false.
3. Run the add-check FUN_6F282CA0(1, item is powerup, 0). It returns an error code, and 0 means OK:
   - unit flag 0x100 set -> code 0x98 (unit is in a removed/dead state) -> fail.
   - no pack -> 0x12 -> fail.
   - non-powerup item: if the pack is full (occupied == capacity) and (the extra flag argument is non-zero OR unit
     flag 0x4 is clear) -> code 0x11 (pack full) -> fail. The first argument 1 skips the other check.
   - powerup item: if the unit has no gold/lumber resource slot (FUN_6F0E3C50 == 0) -> code 0xD6 -> fail.
   - Pack capacity and item flags come from the calls above (see section 8 for the unresolved flag).
4. Add (FUN_6F282D30(item, 1)):
   - no pack -> return 0.
   - run the add-check again (the same result as step 3, so same failures).
   - powerup item (flag 0x800): the item is consumed immediately through FUN_6F0F0EA0 and the function returns 1.
     The item never gets a slot. CONFIRMED.
   - non-powerup item: the lowest free slot index is taken (FUN_6F0FD040, loop from 0 upward, first free index
     wins). The slot is written by FUN_6F0F8C10(item, slot, flag). Returns 1. CONFIRMED.
   - if no slot is free, FUN_6F0FD040 returns 0 and the add fails. The item keeps its old position.
5. The result is 1 on success, 0 on failure.

### 5.3 What FUN_6F0F8C10 does to the item when it is put in a slot (CONFIRMED structure)
- Writes the slot key from the item's key pair (item+0x14, item+0x18). If the item is not a live object, the slot is
  set to (-1,-1) instead.
- Recomputes the slot-related unit state (a floating value from the unit record, and FUN_6F2B7EF0 / 7F40 values,
  meanings UNSURE).
- Sets the item's holder key (item+0x8C/+0x90) to the unit's key. LIKELY (IsItemOwned reads it).
- If the flag argument is non-zero, and the unit is the local selected unit, a UI refresh is run (FUN_6F32F980).
  CONFIRMED structure.
- Hides the item from the ground (the path FUN_6F2B9600 with its first argument 0 calls vf33). LIKELY.
- Returns 1.
- If the unit's type flag 0x40000000 is clear, FUN_6F299610 is called (LIKELY a per-unit "item picked up" notice).

### 5.4 UnitAddItemById(unit, id) and UnitAddItemToSlotById(unit, id [, slot])
- Both resolve the unit, read its position (vf 0xB8, then FUN_6F4743A0), and create a new item of type id at that
  position (FUN_6F2B9940). The item is created before any check. CONFIRMED order.
- Item flag 0x100 is then set on the new item. CONFIRMED.
- The pack add check (FUN_6F282CA0) is run. If it fails the native returns 0 (the item stays as a ground item at the
  unit position). LIKELY.
- For UnitAddItemById the add is FUN_6F282D30(item, 1), and the native returns the item handle (FUN_6F430C80 on the
  item) regardless of the add result. LIKELY, because the return value of FUN_6F282D30 is ignored in the code read.
  Consequence (LIKELY): when the pack is full, the native returns a valid item handle for an item that sits on the
  ground at the unit position.
- UnitAddItemToSlotById: the add is FUN_6F282D90(item, ..., slot) which writes to the given slot index. If that slot
  is occupied, the write fails and the native returns 0. LIKELY.
- The handle id of the new item is allocated after the add (FUN_6F430C80 is the last call). CONFIRMED.

### 5.5 Remove and drop
- UnitRemoveItem(unit, item) -> FUN_6F282E40(unit, item):
  1. The unit must have a pack (unit+0x1F8 non-zero), otherwise return 0.
  2. FUN_6F0F0F80(unit, item): finds the slot holding the item (scan with the item's key). If not found, return 0
     and do nothing. If found:
     - clears the slot (both words set to -1),
     - detaches item-granted abilities from the unit (list at unit+0xC0, count at unit+0xBC, entries matched by the
       item's ability id at +0x70, LIKELY),
     - runs a charge/usable side effect (FUN_6F2B5CC0) when flags 0x200 and 0x1000 are both set, LIKELY,
     - runs the UI refresh (FUN_6F3329B0).
  3. The unit position is read (vf 0xB8, FUN_6F4743A0), and the item is moved to it: FUN_6F2B6AE0(&pos, -1). CONFIRMED.
  4. Returns 1.
- UnitRemoveItemFromSlot(unit, slot): if the slot holds a valid item, it is removed as above (FUN_6F282E40), then the
  unit position is re-applied to the item (FUN_6F2B6AE0). The item handle is returned. If the slot is empty or
  invalid, returns 0 (the handle 0). CONFIRMED structure.
- UnitHasItem(unit, item): pack exists, then the scan FUN_6F0F8E80 goes from the highest slot index down to 0 and
  returns 1 when a slot key equals the item's key. CONFIRMED.
- UnitItemInSlot(unit, slot): pack exists, the slot is read (FUN_6F0F0F00: slot < capacity and key not (-1,-1) and
  resolves to a live object). Returns the handle of the item (FUN_6F430C80). Returns 0 otherwise. CONFIRMED.

### 5.6 Drop and use natives (orders)
- UnitDropItemPoint(unit, item, x, y): requires unit (FUN_6F282C60), item valid, unit has pack, item is in the pack
  (FUN_6F26EC40 check). Then it calls FUN_6F3C8A20 with order id 0xD0021, x, y, and the item. That helper checks the
  order (FUN_6F279720 with the unit's owner index) and accepts when the code is 0 or 0xDC. Then it queues the order
  (FUN_6F294E40 + FUN_6F2A4AB0) and returns 1. CONFIRMED.
- UnitDropItemSlot(unit, item, slot): the same checks plus slot range (0 <= slot < capacity), then the order id is
  0xD0022 + slot index (FUN_6F3C89D0). CONFIRMED.
- UnitDropItemTarget: same pattern with a widget target. LIKELY.
- UnitUseItem / UseItemPoint / UseItemTarget: unit and item checks like drop, then an order (use order id not read).
  LIKELY.
- So a drop or use returns "true" when the order was queued, not when the item moved. CONFIRMED.

---

## 6. Item enumeration

### 6.1 EnumItemsInRect(rect, boolexpr, action)
- Rect resolver FUN_6F3BDB90 gives the rect. The four corner floats are at rect+0x24 (minX), +0x28 (minY), +0x2C
  (maxX), +0x30 (maxY). CONFIRMED (values read from the native body).
- Context record (5 dwords, all on the stack): [0] = boolexpr-or-action argument, [1] = a resolver value, [2] =
  a global-manager value. The dispatcher is FUN_6F4693D0 -> FUN_6F47FA90 with mode 1 and callback FUN_6F3BB9D0.
  CONFIRMED.
- FUN_6F47FA90 chooses a walk by the first two dwords of its context: both zero or -1 -> FUN_6F47DF30 (the "all"
  walk); otherwise the other walkers. Which walk is used here is LIKELY FUN_6F47DF30 (because the first two dwords
  are zero), not read.
- Per candidate, the callback FUN_6F3BB9D0:
  1. Compares the candidate's class at vf 0x1C with FUN_6F2B5AE0() (a world/layer id). If they differ, skip. LIKELY
     that this is the "same layer / same world" test.
  2. Skips a candidate whose handle key is dead (the removed/dead check, FUN_6F4786B0).
  3. Sets the enum-item slot (0xE) to the candidate (FUN_6F430DB0(0xE, item)). CONFIRMED.
  4. If the context boolexpr is null or evaluates true, and the action is non-null: sets the filter-item slot (0xD),
     runs the action (FUN_6F3BA890, FUN_6F453C50), and clears slot 0xD. CONFIRMED.
  5. Clears slot 0xE. CONFIRMED.
  6. Always returns 1 (continue walking). CONFIRMED.
- Rect test: the geometric test is NOT in the callback. It is done by the walker (FUN_6F47DF30, not read). UNSURE
  whether the rect test is inclusive or exclusive at the edges.
- Order: UNSURE. For units, the spatial walk is x-major ascending, then y ascending, head to next (unit-core and
  groups-events-2.md). Items use a different dispatcher, so this order is NOT assumed for items. Until FUN_6F47DF30
  is read, the order must be treated as an open item (see section 9.2).
- A removed or dead candidate is skipped (step 2). An item that is hidden is not skipped by this callback (LIKELY it
  is still visited, because the callback does not test bit 0). UNSURE.
- Whether the item count limit can fire: not checked. UNSURE.

### 6.2 GetEnumItem and GetFilterItem
- GetEnumItem returns the item in enum slot 0xE. GetFilterItem returns the item in filter slot 0xD. Both are set
  only inside the callback (above). CONFIRMED slots.
- Caveat (UNSURE): the callback sets slot 0xE before evaluating the boolexpr and sets slot 0xD only after the
  boolexpr passes. A boolexpr that reads GetFilterItem during evaluation would see the value from the previous
  step. This must be confirmed before reproducing filter semantics exactly.

---

## 7. Random item selection

### 7.1 Entry points (CONFIRMED)
- ChooseRandomItem(level) = FUN_6F2B9E10 with ECX = 7 (the category "any"), EDX = level, and stack arguments
  (excludeId = 0, flag4 = 1, flag5 = 0).
- ChooseRandomItemEx(itemtype, level) = FUN_6F2B9E10 with ECX = itemtype (first JASS argument), EDX = level, and
  stack arguments (0, 1, 0).

### 7.2 Candidate list (CONFIRMED structure)
- The list is the global item-type cache: a linked list stored in an array. The head is the global DAT_6FAB4BD8
  (a node offset, walked only when it is >= 1). Each node holds the next offset at node+4 and the item-type id at
  node+0x14. The record fields are read through the cache lookup FUN_6F2B88A0.
- The walk is head -> next until next <= 0.
- A node is a candidate when all of these pass:
  1. Its id is not equal to excludeId (0 in both natives, so this is "id != 0").
  2. Category: if the category argument (ECX) is 8, any type passes. Otherwise the type's class (record +0x3C,
     default 7 when the record is missing) must equal the category. For ChooseRandomItem the category is 7 and the
     class test is replaced by the constant 7, so it always passes (CONFIRMED from the branch).
  3. Level: if the category argument is 7, the level field is record +0x40 (read via FUN_6F2B8C80). Otherwise it is
     record +0x38 (FUN_6F2B8C50). Pass when the level argument is -1, or when the field equals it.
     NOTE: this is the one place where the plain form and the Ex form read different level fields. CONFIRMED from
     the branch; the meaning of +0x40 is UNSURE.
  4. flag4 (1 in both natives) makes the second flag test pass (the test is "flag4 != 0 OR record +0x44 != 0").
  5. flag5 (0 in both natives) requires record +0x48 != 0 (the test is "flag5 != 0 OR record +0x48 != 0").
- The candidate ids are stored in the order the list was walked.

### 7.3 Pick (CONFIRMED)
- Let n be the candidate count.
- If n == 0: no RNG step is taken, and the native returns 0 (CONFIRMED: the pick call is only reached when n != 0).
- Otherwise: take one step of RNG stream 35 (0x23), giving output r (32-bit). Then index k = high32(r * n), that is,
  floor(r * n / 2^32). The native returns candidates[k].
- The pick is FUN_6F285320(stream = 0x23, n = count). It reads stream 35 from the RNG manager (CONFIRMED from the
  disassembly: ECX = 0x23 at the call site in FUN_6F2B9E10). The step is the same generator step as in rng.md.
- Stream 35 is used only by these two natives and by PlaceRandomItem if it shares the same selector (UNSURE).

### 7.4 Summary of the random item algorithm for a port
1. Walk the global item-type list (order: see section 9.3).
2. Keep ids that pass the filter for the call (section 7.2).
3. If none, return 0 without drawing.
4. Draw one value r from stream 35 and return candidates[floor(r * n / 2^32)].

---

## 8. Item and pack constants (CONFIRMED values from the code)

| Name | Value | Where |
|---|---|---|
| Unit kind tag (resolver check) | 0x2B61676C | units and items resolve through it |
| Pack slot empty key | (-1, -1) = (0xFFFFFFFF, 0xFFFFFFFF) | slots |
| Pack slot stride | 12 bytes, base unit+0x70 | slots |
| Pack capacity field | unit+0x6C | slots |
| Pack inventory pointer | unit+0x1F8 | pack |
| Item flags | see section 2 table | +0x20 |
| Item charges | +0x84 | charges |
| Item owner index | +0x74 | owner |
| Item holder key | +0x8C / +0x90 | holder |
| Enum item slot | 0xE | GetEnumItem |
| Filter item slot | 0xD | GetFilterItem |
| Random stream | 35 (0x23) | ChooseRandomItem / Ex |
| Order id point drop | 0xD0021 | UnitDropItemPoint |
| Order id slot drop base | 0xD0022 (+ slot) | UnitDropItemSlot |
| Add-check error codes | 0x11 pack full, 0x12 no pack, 0x98 unit removed, 0xD4, 0xD6 no gold/lumber slot, 0x55 / 0xDD / 0xD3 pickup refusals | error table (CONFIRMED values) |
| Item ability cap | 4 | FUN_6F2B9600 |
| Float zero constant | DAT_6FAAE470 = 0.0 | runtime value (tools/emu/runtime_consts.py) |
| Float 0.1 constant | DAT_6FAAE4A0 = 0.1 | runtime value |
| Float 1.0 constant | DAT_6FAAE4F4 = 1.0 | runtime value |
| Degree-to-radian constant | DAT_6FAAE5E8 = pi/180 | runtime value |
| Handle base | 0x100000 | handles-2.md |

Note: the constants above come from runtime_consts.py output (see AGENTS.md 4.3m). The file holds zeros for them,
so do not take them from the file bytes.

---

## 9. Open items and UNSURE points

9.1 Pickup and drop event ids (UNSURE). The pickup and drop paths call the event dispatcher FUN_6F0787D0 (with
    FUN_6F0DB030(0,0,1,1) giving the event object), and a drop notification through the item's vf64 (FUN_6F2AD710)
    with arguments (0, 4). The EVENT_PLAYER_UNIT_PICKUP_ITEM / DROP_ITEM ids and the dispatch order (before or after
    the slot write) were not resolved. Next step: read FUN_6F0DB030, FUN_6F0787D0, FUN_6F2F89A0, FUN_6F3008F0 and
    FUN_6F2AD710, and match them against the event table in groups-events-2.md.

9.2 Enum walker order (UNSURE). FUN_6F47DF30 (the walker chosen for EnumItemsInRect) was not read. Items use a
    different dispatcher from units, so the unit range order (x-major, then y) is not assumed for items.

9.3 Random candidate order (LIKELY, not confirmed). The global item-type list is built by lookups (FUN_6F2BA930
    creates a node on first use and links it at the head, LIKELY) and by a startup pass (FUN_6F2B8840, which walks
    the list and calls FUN_6F3B0720 for each item type with a name). If nodes are prepended, the candidate order is
    reverse load order, and load order depends on which item types were looked up first. This is a desync risk.
    Next step: confirm the link direction in FUN_6F2BA860 and the startup load order (by emulation of the
    initializer, as in runtime_consts.py).

9.4 SetItemVisible(true) (UNSURE). vf32 (FUN_6F2B6130) calls FUN_6F26BEA0, FUN_6F2B5D00(1) (sets +0x7C = 1) and vf
    0x148. It does not clear bit 0 of +0x20 in the code read. So it is not yet clear how a hidden item becomes
    visible again, or whether bit 0 is the visibility bit at all (the resolver also reads +0x20). Next step: read
    FUN_6F26BEA0 and the vf 0x148 implementation.

9.5 PlaceRandomItem (UNSURE). The pool path (CreateItemPool, ItemPoolAddItemType, PlaceRandomItem at 0x6F3CB700)
    was not read. Whether it uses the same selector and stream 35 is not known.

9.6 Pack capacity source (UNSURE). FUN_6F0E4050 caps unit+0x6C with the value of FUN_6F6EEC20(). The source of
    the cap (type value, global constant, or the hero/unit flag) was not identified.

9.7 Pickup permission details (UNSURE). FUN_6F0F8F60 returns 0x55, 0xDD or 0xD3 for refusals and checks unit flag
    0x4 and type bit 0x400. The meanings of these flags are not named. The flag 0x4 check also appears in the full
    pack test (section 5.2), so the two uses should be cross-checked.

9.8 Item-granted abilities (LIKELY). Creation (FUN_6F2B9600) parses up to 4 abilities from the type's string, and
    removal (FUN_6F0F0F80) walks a unit ability list at +0xC0 (count at +0xBC). The exact add-on-pickup path for
    these abilities was not traced.

9.9 Charge side effect on removal (UNSURE). FUN_6F2B5CC0 runs on removal when flags 0x200 and 0x1000 are set. Its
    effect (refund, charge decrement, or nothing) was not read.

9.10 Handle of an item that is NULL (LIKELY). CreateItem with a failed builder calls FUN_6F430C80(0, 0). The
    result is assumed to be handle 0. Confirm with handles-2.md before porting.

---

## 10. Porting checklist (what must match exactly)

1. Item creation order: builder, type load, init, flags, then the handle id last (LIFO reuse of freed ids).
2. Pack: lowest free slot index first; powerups never take a slot; full pack fails with 0x11 and does not move the
   item into a slot.
3. Remove and drop put the item at the unit position; drops and uses are orders, not instant moves.
4. Charges: 0 or negative sets flag 0x1000 and charges 0; positive clears the flag.
5. Random: stream 35, one draw only when the candidate count is greater than 0, index = floor(r * n / 2^32), over
   the candidate list in its list order (9.3 must be closed before the order is final).
6. Enum: callback order and GetEnumItem / GetFilterItem slot timing as in section 6.2 (9.2 open).
