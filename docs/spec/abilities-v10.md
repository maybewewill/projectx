# V10 verification wave (Unicorn, `tools/emu/v10_emu.py`, vectors `crates/sim/tests/data/v10/`)

Method as V9: the real routines of Game.dll run on fake objects, world-dependent callees are stubs that read per-case switches or append
to a call log (5 dwords per call: kind, ecx, three arguments), rows are dumped as hex words and re-computed by the Rust model. A row is
added to `confirmed-abilities.tsv` only after a 0-mismatch run. One check per process.

## Moon Well (`Arpb`, `Arpl`, `Arpm`) CONFIRMED (900 cases for the three codes together, 300 per code; `rep_effect`, `rep_precheck`)

The old model (a channel of `Cast / Dur` ticks on a unit target) was wrong in every part. The original:

* **No target, no channel.** `CAbilityReplenish < CAbilityAutoTargetSpell` (vtable `0x6F91AA14`; the Life / Mana classes differ only in
  slots `0x478` / `0x47C`, which return `0.0` for the missing half). The cast dispatcher `0x6F05C3A0` takes slot `0x34C` (`0x200001`),
  finds neither the target nor the point bit and calls the no-target effect slot `0x428` = `0x6F1FADC0`.
* **Cost.** The generic cost slot `0x3D4` returns 0 (`0x6F05C840` pays nothing). The readiness value is slot `0x374` = `0x6F1DBCC0` =
  `0.0 + I2R(F2I(DataE) * Cost)` (stock 5 * 6 = 30 mana, `rep_precheck`, 400 cases); slot `0x370` passes when it equals 0.0, else the
  mana check. The effect pays at its end: `max(1, min(units taken, F2I(DataE))) * Cost` through `0x6F283DB0` (even when nobody was
  taken), then starts the cooldown `Cool + ability +0x13C` through the object at ability `+0x90`.
* **The group.** `0x6F2A3270(callback 0x6F1FAAA0, record, 0xB, Area, x, y, 0x1D, targs, caster, -1, 0x1F)`: all friendly (targs
  `ground,air,friend,self,organic,vuln,invu`) units within `Area` (stock 700) around the caster, in collector order. The record holds
  `{ability, DataA, DataB, 0.0, 0.0, caster, Dur + ability+0x9C, HeroDur, ., level, count, F2I(Cast)}` (the sum is a soft add with `Dur`
  first). **`Cast` is the unit limit** (0 = none; the old model read it as a channel length and `Dur` as the tick).
* **Callback** `0x6F1FAAA0` per unit (returns 0 = stop when the limit is nonzero and reached): `life_set` = DataA nonzero, `life_need` =
  `life_set` and `0.0 < max - life` (soft sub, ordered compare); the same for mana with the agile mana properties; skip the unit unless
  `(life_set or mana_set) and (life_need or mana_need)`; skip it when the ability's target gate (slot `0x3EC` = `0x6F04F7A0`) refuses.
  Otherwise: target effect, **count the unit**, marker buff (`Brpb` both amounts set, `Brpl` life only, `Brpm` mana only; duration
  `HeroDur` for a hero else `Dur + extra`, existing marker refreshed through slot `0x350`, new one made through the class creator, slot
  `0x324`, apply `0x354(unit, &dur, caster)`, registered on the unit); then, unless an illusion is blocked (`IllusionsCanRestoreLife` /
  `...Mana` both 0 and the matching need), a unit with a positive ethereal count **multiplies the shared life amount in place** by Misc
  `EtherealHealBonus` (so later units see the scaled value, and a zero bonus switches the life half off for the rest of the pulse), then
  the life adder `0x6F277C20` gets the life amount (if needed) and the mana adder `0x6F279140` the mana amount (if needed).
* Model: `unit_abilities_m::replenish_plan` (pure, driven by the vectors) and `replenish_pulse` (collects the candidates, applies the
  events). **Bugs fixed**: channel instead of a pulse, wrong cost (`Cost` once instead of `n * Cost`), `Cast` read as a duration, no
  per-unit need test, no ethereal scaling, target order instead of an immediate order, no markers, readiness against `Cost` instead of
  `DataE * Cost`.
* Stubs and what is NOT covered: the group builder (collector order and the targs filter are the model's, LIKELY), slot `0x3EC` (the
  model refuses only removed / targs-mismatching units), the two adders (their listener walks `68..82` / `84..98` and the clamp of the
  setters), the marker buff internals after the apply slot (the model uses the set-duration rule, UNSURE for `0x350` / `0x354` of
  `CBuffReplenish`), the autocast scan of the class (no autocast in the model), the target effect and caster effect (presentation).

## Repair (`Arep`), Restoration (`Arst`), Renew (`Aren`): start and gate CONFIRMED (400 cases each; `rep_start`, `rep_gate`)

The old model (a 0.5 s tick that healed `max / (bldtm * DataB)` and paid `DataA * cost * fraction` as it went) was a stand-in. The original, for a
finished building (no `ABnP` construction ability on the target), unit-target effect slot `0x424` = `0x6F070AB0`:

* The numbers come from the **UnitBalance repair columns**: `goldRep` (type record `+0x28`), `lumberRep` (`+0x2C`) and **`reptm`** (`+0x1C`; the old spec
  said there was no repair-time column; `bldtm` is `+0x18`). `T = max(5.0, DataB * reptm)` (constant `0x6FAAF09C` = 5.0); totals `DataA * goldRep`,
  `DataA * lumberRep` (soft multiplies).
* A nonzero total needs one unit payable (`0x6F02D730` / `0x6F02D760`, `stored/10 >= 1`), else **nothing starts**. Then each nonzero total **pays one unit
  immediately** (`0x6F416A00(-1,...)`, scaled by 10) and starts a **periodic timer of `T / total` seconds** (messages `0xD01D0` gold, `0xD01D1` lumber): the
  repair is paid in single units at a steady rate. When both periods are at least 1.0 (a missing period counts as 1.0) a one-second tick (`0xD01C1`,
  constant `0x6FAAF0A0` = 1.0) validates the job. The target gets a **life regeneration modifier of `max life / T` per second** (`0x6F02DC60`, kept in the
  ability, `0x6F029580`); presentation callbacks `0x6F2AB3E0`/`0x6F2AB1E0`/`0x6F021500` follow.
* Every timer message runs `0x6F070FE0`: the validation `0x6F035070` (target present, targs, **the class gate slot `0x3F0` = `0x6F04BF70`**, ...) stops the
  ability (`0x6F06BBB0`: timers and modifier go) when it fails; a payment message then takes one more unit (`0x6F416A00`/`0x6F416A40(-1,...)`).
* Gate `0x6F04BF70` (finished building): `152` locked unit (`+0x5C & 0x100`), `30` when `max - life` is ordered below 1.0 (**the repair ends when less than 1 life
  is missing**, and a target that is not hurt by 1.0 cannot be ordered), `161` with an `Buns` buff, `162` illusion, `8` / `9` when `goldRep` / `lumberRep`
  is nonzero and one unit cannot be paid (so a repair also **stops when the owner runs out**), else `0`.
* Model: `unit_abilities_m::{repair_start_plan, repair_gate_code}` (pure, driven by the vectors), `RepairJob` (modifier + payment timers),
  `repair_event` (the timer handler). `UnitType` gained `gold_rep`, `lumber_rep`, `repair_time`. **Bugs fixed**: wrong time base (`bldtm` instead of `reptm`),
  wrong cost base (the type's gold/lumber cost instead of `goldRep` / `lumberRep`), payment proportional to the life restored instead of single units on a
  timer, a 0.5 s healing tick instead of a regeneration modifier, an order accepted with an empty purse, no refusal of a nearly whole target.
* NOT covered (stubs / open): the `ABnP` branches (taking over or joining a construction, "power build" `DataC` / `DataD`), the validation details of
  `0x6F035070` beyond the gate (range re-check and re-order; the model stops when the builder moves), Renew's own gate (slot `0x3F0` = `0x6F04C5D0`; its
  start effect is the same function), the stop of the agile property at max life, the `Buns` / locked flags in the sim (locked is not modelled).
