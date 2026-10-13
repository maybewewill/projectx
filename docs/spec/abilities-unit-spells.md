# Unit (non-hero) spells of the melee races in Warcraft III 1.26a (Game.dll) (S31)

Scope: the spells of ordinary units that the four melee computer players and the autocast rules use. Model:
`crates/sim/src/unit_spells.rs` (hooks in `cast.rs`, `auras.rs`, `natives/order.rs`). Tests:
`crates/sim/tests/unit_spells.rs`. Unicorn harness: `tools/emu/unit_spells_emu.py`. Addresses are 1.26a; no code or
tables are copied. Method: capstone reading of the vtable slots (class identity and slot lists from the RTTI
vtable map and the ability atlas in `docs/research/ability-atlas/`), Unicorn runs of the numeric/selection cores.

Labels: **CONFIRMED** = read in the machine code (and Unicorn-run where a count is given); **LIKELY** = strong
inference; **UNSURE** = a stand-in; **NOT DONE** = not modelled.

## 0. Order ids (CONFIRMED)

The registration code of the DLL pushes the order string and its id as immediates; the ids are read from there
(`heal` = 0xD005F = 852063 and so on, ids are `0xD0000 + index`). Heal 852063 / healon 852064 / healoff 852065;
innerfire 852066/67/68; dispel 852057; purge 852111; polymorph 852074; bloodlust 852101/02/03; lightningshield
852110; ensnare 852106; healingward 852109; raisedead 852197/98/99; unholyfrenzy 852209; cripple 852189; curse
852190/91/92; web 852211/12/13; rejuvination 852160; roar 852164; faeriefire 852149/50/51; autodispel 852132/33/34;
sentinel 852182; scout 852181.

## 1. The autocast lifecycle of `CAbilityAutoTargetSpell` (17 classes share it)

Static reading (CONFIRMED structure; the values marked below are UNSURE):

* A one-shot agent timer (message `0xD01C2`) lives on the ability. Activation (slot `0x438` = `0x6F044340`) arms it
  with 0.02 s (constant `0x6FAAE484`, runtime value `0x3CA3D70A`); deactivation (slot `0x43C`) sets the disabled flag
  `0x100000` and cancels it.
* The handler (`0x6F0443A0`) re-reads the period from slot `0x468` (`0x6F0231E0`: the constant `0x6FAAE50C` =
  **3.0 s**). It scans only when the autocast flag `0x80000` is on and slot `0x3D8` (`0x6F0379E0`) accepts the
  caster: its order list is empty, or the current order is **attack `0xD000F` or attack-move `0xD0014`**. Further
  gates on the caster (`+0x5C & 0x1000000`, `+0x20 & 0x4000000`, `+0x198 > 0`) are not modelled.
* The target search is slot `0x448` (`0x6F030680`): a group around the caster (radius from slot `0x440`, which
  combines the level's `Rng` with the caster's collision size and a scale), the per-unit callback `0x6F0232E0`
  calls slot `0x44C` (`0x6F044570`) with the best-so-far.
* Slot `0x44C` per candidate, in this order: the owner's visibility test `0x6F284EE0`; the target validity slot
  `0x3EC` (`0x6F04F7A0`: the level's targs flags, then the class slot `0x3F0`; any nonzero code rejects); a hostile
  candidate is rejected unless its flag `+0x5C & 0x100`; the class filter slot `0x450`; then the class comparator
  slot `0x458` with (candidate, its distance value `V`, best, best's `V`, threshold `T`).
* After a successful cast order the next delay is `max(total cast time (slot 0x3E0), 1.0)`; without a target it is
  the 3.0 s period.
* UNSURE: the radius (the model uses `Rng`), `V`/`T` units (squared world units here; the original compares cell
  units), the default autocast flag (the model has Heal on; the source of the default was not found), walking into
  range before the cast (the model casts only when in range).

## 2. Heal (`Ahea`, `CAbilityHeal`, vtable `0x6F8C9674`)

Data of the first level (read from the game's `AbilityData.slk`, not committed): cost 5, cooldown 1, range 250,
`DataA` 25 (the amount), `BuffID` `Bhea`, targets: air, ground, friend, vulnerable, invulnerable, self, organic,
non-ancient, neutral.

* Effect slot `0x424` = `0x6F108280` (CONFIRMED): amount = `DataA`; an ethereal target (`+0x158 > 0`) multiplies it
  by Misc `EtherealHealBonus`; the life adder `0x6F277C20` (model: add and clamp to the maximum life); then the
  marker buff `Bhea` (code getter `0x6F0DA880`) with the duration `1.0 + (Cool + 0.01)` (`0x6FAAE480` = 0.01, the
  ability's float holder at `+0x90/+0x94`; LIKELY), set by the buff's set-duration slot (never shorten).
* Extra target rule, slot `0x3F0` = `0x6F0F0290` (CONFIRMED): an illusion target is refused unless Misc
  `IllusionsCanRestoreLife`; a target with `maxlife - life < 1.0` returns error `0x1C` (refused). So full-life units
  cannot be healed by hand either.
* Autocast filter, slot `0x450` = `0x6F0F0370` (CONFIRMED): the base filter `0x6F030730` (caster flag
  `+0x20 & 0x40000000`), illusion candidates refused unless Misc `IllusionsGetAutocast`, the candidate must be an ally
  (`0x6F285B80`), and a `Bhea` buff on it made by another healer refuses it.
* Comparator, slot `0x458` = `0x6F0DADD0` (CONFIRMED, **Unicorn 600 cases, 0 mismatches**, life-fraction helper
  stubbed): `a` = candidate in range (`V <= T`, ordered x87 compare), `b` = best in range. `a && !b` takes the
  candidate, `!a && b` keeps the best, both in range take the strictly lower life fraction (`life/maxlife`, helper
  `0x6F020640`, LIKELY meaning), neither in range take the strictly smaller `V`. NaN keeps the best.
* Model: `unit_spells.rs` (`heal`, `heal_prefers`, `pick_target`, the autocast timer), the `healon`/`healoff`
  immediate orders, behaviour tests (amount, cap, mana, cooldown, marker lifetime 2.01 s, refusals, autocast target
  choice, foreign marker, toggling).
