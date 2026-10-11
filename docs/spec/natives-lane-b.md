# Lane B Natives: Damage, Items, Build Orders (docs/spec/natives-lane-b.md)

Specification of damage, item-use, and build-order natives from Game.dll 1.26a. Addresses and signatures from `docs/spec/natives-1.26a.tsv`.

## 1. UnitDamageTarget (0x6F3C8850)

**Signature:** `(Hunit;Hwidget;RBBHattacktype;Hdamagetype;Hweapontype;)B`
- Attacker: handle (unit)
- Target: handle (widget: unit or destructable)
- Amount: real (damage amount in life points)
- Attack: boolean (UNSURE: unclear from signature)
- Ranged: boolean (UNSURE: unclear from signature, may control which flags are set)
- AttackType: handle (enum via hashtable, maps to damage class word and attack-type row)
- DamageType: handle (enum via hashtable, maps to armor-type column)
- WeaponType: handle (UNSURE: may control missile speed or other properties, not used by the original's target damage routine)
- Returns: boolean (UNSURE: likely true on success, false on null/invalid)

**Behavior (LIKELY from signature and AGENTS.md 4.3i):**
1. Resolve attacker unit handle; fail (return false) if null/invalid/removed
2. Resolve target widget handle; fail if null/invalid/removed
3. Extract damage class word from attacktype handle via enum_at (1 << damagetype per AGENTS.md 4.3s)
4. Call `damage::target_side_amount()` with amount, class, target flags, damage-type table row/col
5. Apply resulting damage via `set_life()` (same path as combat; clamps, fires events, detects death)
6. If target died from damage, set killer to attacker and dispatch death events
7. Return true

**Status:** LIKELY - signature parsed, behavior inferred from existing damage system and AGENTS.md sections 4.3i (attack/damage) and 4.3s (attack cycle)

## 2. UnitDamagePoint (0x6F3C8770)

**Signature:** `(Hunit;RRRRRBBHattacktype;Hdamagetype;Hweapontype;)B`
- Attacker: handle (unit)
- X: real (center point)
- Y: real (center point)
- Radius: real (enumeration radius)
- Amount: real (damage per hit)
- Delay: real (UNSURE: may be delay timer or boolean flag)
- Attack: boolean (UNSURE)
- Ranged: boolean (UNSURE)
- AttackType: handle (enum)
- DamageType: handle (enum)
- WeaponType: handle (enum)
- Returns: boolean

**Behavior (UNSURE - requires decoding from Game.dll):**
1. Resolve attacker unit handle
2. Enumerate units in rectangle around (X, Y, Radius) - likely using `group.rs` rect query
3. For each unit, apply damage with same target_side_amount path as UnitDamageTarget
4. If delay > 0, schedule the damage via a timer (UNSURE: message id not known)
5. Return true if any unit was hit

**Status:** UNSURE - signature parsed, behavior requires Unicorn decode of delay handling and enumeration order

## 3. UnitUseItem (0x6F3C9090)

**Signature:** `(Hunit;Hitem;)B`
- Unit: handle (unit using the item)
- Item: handle (item being used)
- Returns: boolean

**Behavior (LIKELY from spec 5.6 and order system):**
1. Resolve unit and item handles; fail if null/invalid
2. Verify item is in unit's inventory
3. Queue an ItemOrder on the game (like UnitDropItem*)
4. ItemOrder type: use (0xD0023 or similar, needs decoding)
5. Return true

**Status:** LIKELY - signature parsed, order queueing follows existing pattern from item.rs

## 4. UnitUseItemPoint (0x6F3C9100)

**Signature:** `(Hunit;Hitem;RR)B`
- Unit: handle (unit)
- Item: handle (item)
- X: real (target point)
- Y: real (target point)
- Returns: boolean

**Behavior (LIKELY):**
1. Same as UnitUseItem but queue a point-order variant
2. ItemOrder includes target coordinates
3. Return true

**Status:** LIKELY - signature parsed, order queueing follows existing pattern

## 5. UnitUseItemTarget (0x6F3C9170)

**Signature:** `(Hunit;Hitem;Hwidget;)B`
- Unit: handle (unit)
- Item: handle (item)
- Target: handle (target widget)
- Returns: boolean

**Behavior (LIKELY):**
1. Same as UnitUseItem but queue a target-order variant
2. ItemOrder includes target handle
3. Return true

**Status:** LIKELY - signature parsed, order queueing follows existing pattern

## 6. SetUnitRescuable (0x6F3C5F10)

**Signature:** `(Hunit;Hplayer;B)V`
- Unit: handle (unit)
- Player: handle (player)
- Rescuable: boolean (rescuable flag for that player)
- Returns: void

**Behavior (LIKELY from spec section and event structure):**
1. Resolve unit and player handles
2. Store rescuable flag per unit per player (likely in a per-unit array or map)
3. No immediate effect; flag is read when a rescue event fires

**Status:** LIKELY - signature parsed, behavior inferred from field layout

## Open Items

- **GetRescuer** (event response getter): marked pending because the rescue event firing site is not located in AGENTS.md. Implement only if rescue order execution is modelled.
- **UnitDamagePoint delay parameter**: needs Unicorn decode to confirm whether it is a timer delay or a flag
- **ItemOrder type ids**: the order ids for use/use-point/use-target need to be found or inferred from the original
- **WeaponType parameter**: appears in all damage natives but is not used by target_side_amount; may be placeholder or affect display

## Implementation Notes

- All damage natives use `damage::target_side_amount()` for the core calculation (CONFIRMED bit-exact in Unicorn)
- AttackType/DamageType/WeaponType are enums stored as handles; resolve via `enum_at()` from `natives/hashtable.rs`
- Killer tracking for death events already exists in the death path
- Item order queueing follows the pattern of existing order systems in `orders.rs` and `item.rs`
- Rescuable state is read-only in combat/targeting (no gates found in AGENTS.md); just storage needed

