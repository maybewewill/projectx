# Shop stock natives (lane C)

Natives: `AddItemToStock 0x6F3C9350`, `AddItemToAllStock 0x6F3B3270`, `RemoveItemFromStock 0x6F3C93F0`,
`RemoveItemFromAllStock 0x6F3B32B0`, `SetItemTypeSlots 0x6F3C9470`, `SetAllItemTypeSlots 0x6F3B32D0`, and the unit
twins `AddUnitToStock 0x6F3C93A0`, `AddUnitToAllStock 0x6F3B3290`, `RemoveUnitFromStock 0x6F3C9430`,
`RemoveUnitFromAllStock 0x6F3B32C0`, `SetUnitTypeSlots 0x6F3C94B0`, `SetAllUnitTypeSlots 0x6F3B32E0`.
Code: `crates/sim/src/shop.rs`, `crates/sim/src/natives/shop.rs`. Check: `tools/emu/shop_stock_emu.py`.

## 1. Where the stock lives (CONFIRMED, machine code)

- A shop's item list is the state of its ability with base code `Asid` (class `CAbilitySellItemDynamic`, vtable
  `0x6F87DD5C`); the unit list is `Asud` (`CAbilitySellUnitDynamic`, vtable `0x6F87D6C4`). The per-shop natives
  resolve the unit handle, look the ability up by code (`0x6F0787D0`, the generic list search) and call the ability
  method; **no ability: nothing happens**. The per-shop natives do **not** validate the type id.
- Ability object layout (offsets in the ability): slot count `+0x47C`; 12 type ids at `+0xCC`; 12 slot flag words at
  `+0x98` (bit 0 = depleted); 12 ages at `+0x480`; 12 stock property objects of 16 bytes at `+0x100` (current real at
  `+4`, maximum real at `+0xC`); 12 restock timer sub-objects of 0x1C bytes at `+0x1C4`.
- Unit twins have identical instruction streams (diffed), same offsets.

## 2. Rules (CONFIRMED, Unicorn)

- **Add** `0x6F036D60(id, current, max)`: scan slots `0 .. cap-1` in order; for each, first `if age[i] > age[best]
  then best = i` (signed compare, `best` starts at 0), then stop if `ids[i] == id`. Found slot is updated in place;
  otherwise the slot is `best` (**the oldest, first of equals; slot 0 when cap is 0**, so a zero-capacity list still
  writes slot 0) and gets the id. Current = `I2R(current)`; maximum = `I2R(max)`, or `I2R(type stockMax)` when `max`
  is 0 or -1. The old restock timer is cancelled and the depleted bit cleared. If `|current - maximum|` (soft-float
  sub) is **at least 2.0e-7** a restock timer is started with the type's `stockRegen` seconds, for that slot. If
  `current == 0` the depleted bit is set. Then every age is incremented and the used slot's age becomes 0.
- **Remove** `0x6F036F50(id)`: the first of **all 12** records holding `id` gets id 0, its timer cancelled, depleted
  cleared, refresh run. Value, maximum and age stay. No match: nothing.
- **Set slots** `0x6F036FB0(n)`: cap = `n` if `n < 11` unsigned, else 11 (so negative and large become 11). Records
  `cap .. 11` get depleted cleared, timer cancelled, id 0 and age = (largest of all 12 ages) + 1. The kept records are
  not touched.
- **All variants**: the item/unit id is checked first (add/remove: id non-zero and the type exists in the item / unit
  type database `0x6FAB4BCC` / `0x6FAB445C`; the slot natives check nothing), then the ability category walk
  `0x6F46D1B0` over the ability code calls the same method for every shop, the callback always returning 1. The
  arguments are passed as (id, current, max) and a context flag chooses add or remove. Visiting order is the category
  list order: **LIKELY = creation order** (the insertion into the list was not located, same open point as
  heroes.md 8.9). The sim visits live units with the ability in creation order.
- Type data: item `stockMax` = type record `+0x28` (missing type: 0), `stockRegen` = `+0x2C` (missing: 1.0),
  `stockStart` = `+0x30` (missing: 0.0); unit types read `+0x48` / `+0x4C` / `+0x50` the same way (regen missing: 0.0).
  `stockStart` is not read by these natives.

## 3. Not found / UNSURE

- The constructor state of a fresh list (capacity, initial ages, initial stock from the ability definition); the sim
  uses capacity 11, ages 0, empty (UNSURE).
- The property setter (stock value clamp against the maximum) and the restock timer expiry (`0x6F477A80` start,
  handler not read): the Unicorn harness stands in a plain copy for the setter; the sim records the started interval in
  `Slot::restock` and never fires it. Buying (the sell event, `GetSellingUnit`) is not modelled, so `GetSellingUnit`
  stays pending.
- The registration helpers `0x6F32DC70`, `0x6F32DCB0`, `0x6F332A10` (called when a new id enters a slot) are
  presentation/listener bookkeeping; stood in by hooks.
- No RNG, no handle allocation in any of these natives.

## 4. Verification

`tools/emu/shop_stock_emu.py` runs the real add, remove and set-slots routines of both twins on synthetic ability
objects (3,600 cases, 600 per operation and kind) and writes `crates/sim/tests/shop_stock_vectors.txt`;
`tests/shop_stock_vectors.rs` replays all of them through `StockList`: 0 mismatches (state, restock start and
interval). `tests/shop_natives.rs` drives the natives through the VM.
