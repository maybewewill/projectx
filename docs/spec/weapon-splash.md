# Splash, bounce and line attacks (Game.dll 1.26a)

What happens after the execute point of an attack whose `weapTp` is msplash (5), artillery (3), mbounce (6), mline (7) or
aline (8): which units are hit, with which fraction of the damage, in which order. Companion of `missiles.md` (the
single-target missile), `attack-timing.md` (the execute jump table `0x6F0D0228`) and `weapon-target.md`. Code:
`crates/sim/src/splash.rs`, `crates/sim/src/bounce.rs`, wired in `crates/sim/src/combat.rs` (`arrive`, `splash_hits`,
`bounce_*`). Harness: `tools/emu/splash_emu.py` (vectors in `crates/sim/tests/splash/`, tests `splash.rs`, `bounce.rs`).

Labels: CONFIRMED = machine code read and Unicorn 0 mismatches, LIKELY = read, not run, UNSURE = inferred.

## 1. The weapon's splash object (`0x6F0C5510`) - CONFIRMED (500 Unicorn vectors)

Each weapon of the attack ability has an aux object (`attack+0x238+4w`) of 8-byte property slots, filled from the weapon
row in the template order of `0x6F29F3E0` (`splashTargs`, `Farea`, `Harea`, `Qarea`, `Hfact`, `Qfact` at template
`+0xB0..+0xD8+4w`): `+0x20` splash targets, `+0x28` Farea, `+0x30` Harea, `+0x38` Qarea, `+0x40` Hfact, `+0x48` Qfact.
`0x6F0C5510` copies them into the launch record: `+0x7C` splash targets, `+0x80` Hfact, `+0x84` Qfact, `+0x88` Farea,
`+0x8C` Harea, `+0x90` Qarea (no aux object: all zero). The msplash, artillery and mbounce paths of the execute build the
record this way (`0x6F0CFB4A`, `0x6F0CFFB3`); the damage rolled before the weapon switch travels as the missile's
amount (`rec+0x44`, `0x6F0C54C0`).

## 2. msplash and artillery: `CMissileSplash` (vtable `0x6F89154C`)

* Launch (slot 50, `0x6F2C4680`): stores the record's rings as properties; a `Hfact` / `Qfact` of exactly zero becomes
  0.5 (`0x6FAAE4D4`) / 0.25 (`0x6FAAE4B8`) - `bounce::launch_factors`, CONFIRMED by reading.
* Arrival (slot 47, `0x6F2C2C80`): the **primary target** goes first through slot 51 (below) with no filter. The area query
  has the radius of the outermost positive ring: `Qarea`, else `Harea`, else `Farea` (positive means greater than zero by
  at least 0.001, `0x6F022270`; `splash::query_radius`, `splash::positive`). The query (group builder `0x6F2AE9E0` ->
  `0x6F476DC0`, circle around the missile's point, the target's position at arrival) calls `0x6F2C1DD0` per candidate
  unit, which runs the **candidate filter** `0x6F2BF020` and, if it passes, slot 51 on the candidate.
* Candidate filter `0x6F2BF020` - CONFIRMED (500 Unicorn vectors, the real code with the alliance rows stubbed):
  with Misc `MagicImmunesResistDamage` set, a Magic missile (attack row 4) refuses an invulnerable (vtable `0x11C`) or magic
  immune unit and a physical one (rows 1, 2, 3, 5, 6) refuses an ethereal unit; then `0x6F25D6F0(unit, splashTargs, source
  object, source owner)` must return 0 (the predicate of `weapon-target.md` with the relation stage on, the target
  object present and `unit+0x20` bit 0 clear). `splash::candidate_filter`. Not exercised in Unicorn: `unit+0x20` bit 0
  (set = refused unless `splashTargs` has `0x800`), `unit+0x60` bit 0 (owner treated as player 15), the ability test
  `0x6F0766A0`.
* Slot 51 (`0x6F2BF160`) - CONFIRMED (500 Unicorn vectors): for one unit, in order: inside the `Farea` ring -> damage
  factor 1.0 (the test runs even for `Farea` 0); else if `Harea` is positive and the unit is inside the `Harea` ring ->
  `Hfact`; else if `Qarea` is positive -> `Qfact` **without a distance test**; else nothing. The record is built by
  `0x6F2BCAC0`: the missile's amount (and its second bonus amount) times the factor, one soft-float multiply each
  (`0x6F2BB590`), then the target side `0x6F2A40D0` of that unit (armor, damage-type table, ethereal) as for a direct hit.
  `splash::ring_factor`.
* Ring membership (`0x6F476C40`, flag 1): the squared distance in cells between the unit's position and the point against
  `(ring/32)^2`, accepted when the ring value is greater or the two differ by less than 0.001 (the operand roles are
  UNSURE). The model uses the stored cell position (`natives::group::in_ring`); the original predicts the position at
  the current time (`0x6F38B0B0`), UNSURE.
* Order: primary target first, then the candidates in the circle query's order (cell walk x-major, then y; LIKELY, the
  same query as the group natives); the damages are applied after the whole list is collected, in that order (LIKELY).
* Artillery (3) builds the same ring spec (`0x6F0CFFB3`) but its missile class is chosen by the missile record
  (`0x6F0CE0C0`); that it applies the rings like `CMissileSplash` is UNSURE (the sim treats 3 like 5).

## 3. mbounce: `CMissileBounce` (vtable `0x6F8B5BC4`)

* Launch (slot 50, `0x6F2C5E30`) from the record: hit counter `+0xB4` <- `rec+0x80` (`attack+0x100+4w`, the weapon's
  `targCount`, LIKELY), search radius `+0xB8` <- `rec+0x84` (the aux `Farea`, LIKELY), loss `+0xC0` <- `rec+0x88`
  (`attack+0xBC+16w`, the weapon's `damageLoss`, LIKELY), target flags `+0xB0` <- `(splash flags & 0xFFFF0FFF) | 0x10000`
  (relation bits cleared, **enemy** set; a weapon without an aux object uses `1`, which matches nothing) -
  `bounce::bounce_targs`.
* Arrival (slot 47, `0x6F2C5ED0`) - structure CONFIRMED (read, not run): hit the target with the current amount (factor
  1.0); decrement the counter, **end when it reaches zero** (so `targCount` hits in all, including the first); if the loss
  differs from zero by at least 0.001, `amount = amount * (1.0 - loss)` (soft float) and end when the new amount is within
  0.001 of zero; otherwise the amount is unchanged; then pick the next target and fly to it. `bounce::after_hit`.
* Next target `0x6F2BD090` -> group builder `0x6F2A3270` with callback `0x6F2BBAF0` - callback CONFIRMED (500 Unicorn
  vectors): around the **current target's** position within the search radius, among the candidates the missile accepts
  (`0x6F2BF2C0`: visible to the owner `0x6F3A38F0`, `unit+0xEC` not positive, the same immunity gates as the splash
  filter, not already carrying the missile's "hit" marker - the marker is set on every target hit) the **nearest** wins; a
  candidate replaces the best only when its distance is strictly smaller, so ties keep the earlier one in the enumeration
  order; the initial best is the largest finite float. `bounce::pick_nearest`. The relation test is the flag word above
  (LIKELY: it is an argument of the builder, not decoded).
* Flight between targets: the path request over the missile speed (as `missiles.md`). The sim hits the next target when the
  leg's flight time has elapsed (`AttackMsg::Bounce`), skipping targets that died meanwhile (the chain then ends, UNSURE).
* Not modelled: the visibility test and `unit+0xEC`; evasion and `ChanceToMiss` for the later legs.

## 4. mline (7) and aline (8): not decoded

mline uses `CMissilePath` / `CBulletPath` (vtables `0x6F916A8C`, `0x6F9169BC`; launch `0x6F2766A0`.. setup `0x6F0CE040`,
record fields from `attack+0x110+16w` and `attack+0x134+16w`) and aline `CArtilleryLine` (`0x6F9325F4`; slot 47
`0x6F2C1360`, slots 50..54 `0x6F2C50C0`..). `spillDist` / `spillRadius` are read by gamedata (`Weapon::spill`) but the line
selection and damage are not decoded; those weapons keep the single-target hit.

## 5. Open

* The enumeration order of `0x6F476DC0` for the area query and `0x6F2A3270` for the bounce builder (the sim uses the group
  query order); whether the primary target is excluded from the area query by the builder (the sim excludes it).
* The second ("bonus") amount `rec+0x48`: the sim has one amount.
* `attack+0xBC+16w` / `attack+0x100+4w` as `damageLoss` / `targCount` (the template copy chain was not traced to the end).
* Artillery's missile class, mline, aline (section 4); `MissDamageReduction` for the splash types.
