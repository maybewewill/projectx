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
