# Script damage, item use, rescue state (Game.dll 1.26a)

Plain-words behaviour of `UnitDamageTarget`, `UnitDamagePoint`, `UnitUseItem*`, `SetUnitRescuable`, plus why
`GetRescuer` and `IssueBuildOrder*` are not bound. Labels: CONFIRMED = machine code read (and Unicorn where stated),
LIKELY, UNSURE. Code: `crates/sim/src/combat.rs` (`unit_damage_target`, `apply_point_damage`), `damage.rs`
(`native_record`), `natives/unit_attr.rs`, `natives/item.rs`, `abilities.rs` (`RESCUE`).

## 1. UnitDamageTarget (0x6F3C8850)

Arguments: source unit, target widget, amount (real), attack flag, ranged flag, attack type, damage type, weapon type.

- Checks, in order (CONFIRMED, Unicorn 600 cases, 0 mismatches): the source unit resolves (0x6F3BDCB0), the target
  widget resolves (0x6F3BE5B0), damage type is not above 0x80000000 (unsigned), attack type is at most 7, weapon type
  at most 24. Any failure returns false and does nothing. Otherwise it returns true after the call.
- Record (constructor 0x6F2556E0, real code run): `+0x00` source, `+0x04` weapon type, `+0x0C` flags
  (`0x100` if the attack flag is nonzero, `|1` if the ranged flag is nonzero), `+0x10` the amount as given (no clamp),
  `+0x14` class `1 << (damage type & 31)`, `+0x18` and `+0x1C` zero, **`+0x20` the attack type argument**.
- **Correction** to attack-damage.md 11.1: the attack type argument is not dropped. The native writes it into the
  record's last dword (`mov [esp+0x38], ebx` right after the constructor, which is record `+0x20`): it is the
  damage-type table row and the ethereal slot index, exactly the field the attack execute fills from the weapon's
  atkType. Unicorn shows `rec[8] == attack type` for all accepted cases.
- Then the target's virtual slot 0x120 runs with the record (units: the target side 0x6F2A40D0, ported as
  `damage::target_side_amount`). The model uses `SpecDamage::hit_amount` (armor, table, ethereal) when the spec
  model is installed, otherwise the raw amount. The resulting amount is subtracted from the life through the life
  setter. Killer for the death events = the source unit (LIKELY, same hand-off as combat).
- Destructable and item widgets: accepted (the resolver passes) but take no damage in the model (UNSURE: their slot
  0x120 bodies are not read).
- Not modelled: the target flag/counter inputs (zero, as for attacks), the attack-handler passes.

## 2. UnitDamagePoint (0x6F3C8770)

Arguments: source, delay, radius, x, y, amount, attack, ranged, attack type, damage type, weapon type.

- Same three range checks and the source check (CONFIRMED, Unicorn 600 cases). Returns true otherwise.
- Branch (CONFIRMED, Unicorn): compare 0x6F01FD60 returns true iff `0.0 >= delay` as an ordered compare, so NaN
  counts as "delayed". True: immediate area damage (0x6F088050, forwards to 0x6F2C31D0). False: a delayed object
  (0x6F1D4440 -> 0x6F2C4D70 -> 0x6F2C00A0) is built; its timer length is the delay (UNSURE, object not read).
  The forwarded class/flags/attack type are the same as section 1 (Unicorn: class, flags, row at fixed stack slots).
- Area body 0x6F2C31D0 (structure CONFIRMED by reading): the area is a list of up to 4 (radius, amount) rings (the
  point form passes one ring `(radius, amount)`); the group builder 0x6F2A3270 enumerates candidates with the
  collector 0x6F2C2E80, which skips the candidate when its ring amount is 0, applies the immunity gate like the
  target side (classes 0x10, 0x20, 0x400000, 0x4000000 are exempt from the invulnerability test; the Misc rule
  MagicImmunesResistDamage switches to the class masks), and sorts it into one of three relation buckets by the
  owner (-1 = bucket 2). Only after the whole list is collected does the apply loop run, per candidate calling
  0x6F2BFED0, which builds the same record as section 1 (flags get `0x10` when the amount is exactly 0.0) and calls
  slot 0x120. A negative amount takes a heal branch (not modelled).
- Model: candidates = live units in the circle by the existing range query (`group.rs`, inclusive `D <= V`, cell
  order), collected first, then each goes through the section 1 path in that order. **UNSURE**: the original's
  membership test is the collision-adjusted compare 0x6F476C40, not the plain circle; the unit class filter of the
  group builder (flags 0x1D/0x1F) and the bucket use are not decoded; the source and allies are damaged.

## 3. UnitUseItem, UnitUseItemPoint, UnitUseItemTarget (0x6F3C9090 / 9100 / 9170)

CONFIRMED structure: the unit resolves and can use items (0x6F282C60: flag test on +0x5C, inventory ability at
+0x1F8 present and its slot 0x268 answers nonzero), the item resolves (0x6F3BEB50) and is in the unit's pack
(0x6F26EC40), the target form also needs a resolving widget (0x6F3BE5B0). The order id is read from the item
(0x6F2B6CC0: the first usable item ability, id = `0xD0028` + that ability's order index; data driven, falls back to a
constant when the item has no usable ability) and issued like the immediate / point / target Issue natives
(0x6F3B2EF0 / 0x6F3B2F30 / 0x6F3B3090) with replace = 1 and drain = 1, issuer = the unit's own owner. The natives
return the helper's result (false only when the validator rejects).
Model: the checks above (capacity > 0 stands in for the inventory test) and a recorded `ItemOrderKind::Use*` order,
returning true; the ability order id is not modelled (item ability tables), nor the validator. LIKELY.

## 4. SetUnitRescuable (0x6F3C5F10 -> 0x6F07B3E0)

CONFIRMED: unit and player resolve, else nothing. The state is a 32-bit mask at `+0x6C` of the unit's `Arsc`
ability (bit = the player's slot index, `shl` by the byte at player `+0x30`). Ability present: a false flag clears the
bit, a true flag does nothing (the true branch returns before writing; so a second player cannot be added to an
existing ability, an original quirk). Ability absent: a false flag does nothing; a true flag creates the ability from
a built-in definition (not the registry), sets the bit and adds it to the unit. Model: `Ability::mask`,
`abilities::RESCUE`.

## 5. GetRescuer (0x6F3C2390) - not bound

The getter answers for the player-unit rescue event (key 0x80213) and the unit rescue event (key 0x8023E, the getter
test is `id + 0x80200` equal to 0x80213 or 0x8023F-class value). The only poster is 0x6F297EC0 (CONFIRMED by xrefs):
it calls 0x6F41C800 (player-unit event 0x80212-class key 524818) and then posts key 524862 (0x8023E) with the
rescuer. Its single caller is the attack swing gate 0x6F0CD350 (the rescue happens when a swing against a rescuable
unit completes; the rescuer is the attacker; the ownership transfer is 0x6F297DA0). The swing branch conditions
(0x6F03FDC0, 0x6F27A320) and the transfer are not decoded and `combat.rs` has no rescue branch, so nothing fires:
bind it after those are ported.

## 6. IssueBuildOrder / IssueBuildOrderById - not bound

They validate with 0x6F2A50C0 (placement) and build a build order record handled by the construction system; the
build path (placement check, payment 0x6F0EDA70 is decoded, but progress driver and the order link are not found,
construction.md section 12) is not reachable cheaply. Unchanged.

## 7. Tests

`crates/sim/tests/unit_damage_vectors.rs` (+ `unit_damage_vectors.txt`, 600 cases per native from
`tools/emu/unit_damage_target_emu.py`), `crates/sim/tests/damage_natives.rs`, `item_natives.rs`
(`use_natives_queue_orders_and_check_the_pack`).
