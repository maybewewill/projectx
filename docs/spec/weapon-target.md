# Weapon target acceptance and the weapon chooser (Game.dll 1.26a)

Which target a weapon may attack, which of a unit's two weapons attacks a given target, and what the acquisition scan
accepts. Companion of `attack-timing.md` (section "Weapon choice", now superseded by this file), `attack-cycle.md` and
`unit-classification.md`. Code: `crates/sim/src/target_accept.rs`, wired in `crates/sim/src/combat.rs`
(`choose_weapon`, `target_flag_word`, `attackable`, `scan_target`). Harness: `tools/emu/target_accept_emu.py`
(vectors in `crates/sim/tests/target_accept/`, test `crates/sim/tests/target_accept.rs`).

Labels: CONFIRMED = machine code read and Unicorn 0 mismatches, LIKELY = read, not run, UNSURE = inferred.

## 1. The target flag word of a unit (`0x6F25D280`) - CONFIRMED (600 Unicorn vectors)

Built from the unit object. Bits (all named by the `targs` token table `0x6FA6F478`, see `unit-classification.md` 6):

| bits | source |
|---|---|
| `0x1000000` alive / `0x2000000` dead | `unit+0x5C` bit `0x100` |
| `0x100000` vulnerable / `0x200000` invulnerable | `unit+0x20` bit 3 |
| `0x2` ground, `0x4` air, `0x8` structure, `0x10` ward | the type's `targType` (`unit+0x24C`, record `+0x1B4`, masked to `0x1E` at load, `0x6F2A7084`) |
| `0x400000` hero / `0x800000` nonhero | first character of the type id `A`..`Z` (`unit+0x33`) |
| `0x20000000` sapper / `0x10000000` nonsapper | classification word `unit+0x248` bit `0x20` |
| `0x8000000` mechanical / `0x4000000` organic | `unit+0x248` bit `0x8` |
| `0x80000000` ancient / `0x40000000` nonancient | `unit+0x248` bit `0x80` |

The relation bits (`0x1000` self, `0x2000` player, `0x4000` ally, `0x8000` neutral, `0x10000` enemy) are not in the
word: the relation is tested separately (section 3). A non-unit target (item, destructable) uses another branch
(`0x6F25D341`, not ported). `targType` comes from `UnitData.slk` (`gamedata::unittype::UnitTypeData::targ_type`); when
the host leaves it zero the sim derives structure / flying / ground from the building flag and the movement type.

## 2. The predicate `0x6F25D560` - CONFIRMED (600 Unicorn vectors, codes included)

`ECX` = target flag word `T`, `EDX` = the weapon's `targs` word `W`, 0 = accepted. The weapon choosers push the arguments
`(-1, 0, 0, -1)`, which switches the relation check and the ability test off; the splash filter (see `weapon-splash.md`)
passes real ones.

* `W` is first **expanded** (`0x6F2552F0`): every group `W` does not name becomes "any": relation `0x1F000`, ancient
  `0xC0000000`, organic/mechanical `0xC000000`, sapper `0x30000000`, hero `0xC00000`; the kind group `0x7FE` becomes
  `0xE` (ground, air, structure only), alive `0x3000000` becomes alive only (`0x1000000`), vulnerable `0x300000`
  becomes vulnerable only (`0x100000`). So a weapon without an `invulnerable` / `dead` token never hits those.
* Stages, the first failing one gives the code (`0x6F2553C0` for the kind stage): `(T|W)&1` (`none`) or `T&0x7FE == 0`
  -> `0xDD`; no kind bit in `T & E` -> kind code (`0x5E`..`0x93`); alive -> `0x74/0x98`; vulnerable -> `0x96/0x5C`;
  then **`T & 0x1E == 0` accepts at once** (a target without ground/air/structure/ward bits skips the rest); organic /
  mechanical -> `0x9B/0x9C`; (relation stage, section 3); hero -> `0x97/0x72`; (ability test `0x6F0766A0`, not ported:
  code `0xCD`); sapper -> `0x9D/0x5C`; ancient -> `0x9E/0x5C`.

## 3. The relation stage `0x6F25D390` - structure CONFIRMED (Unicorn through the splash filter, 500 vectors)

Only when `W` names a relation (`E & 0x1F000 != 0x1F000`): the source owner is the source object's owner (or the passed
index); the candidate itself needs `self` (`0x1000`, else `0x85`); the same owner needs `player` (`0x2000`, else `0x84`);
otherwise the source owner's **passive row** (`0x6F3A3400`, alliance type 0) decides: no bit for the target owner ->
`enemy` (`0x10000`, else `0x88`); a bit and the **fifth row** (`0x6F3A36D0`, alliance type 4 = shared spells, LIKELY)
-> `ally` (`0x4000`, else `0x86`); a bit without it -> `neutral` (`0x8000`, else `0x87`). So "ally" in `targs` means
"passively allied and sharing spells".

## 4. The weapon choosers - CONFIRMED (600 Unicorn vectors each)

`0x6F0C95D0(target_info, out)` (target-info object, flag word at `+4`) and `0x6F0C93E0(unit, out)` (a target unit; the
validator `0x6F0CBD70`, the swing gate, the execute and the **acquisition callback `0x6F0CCFA0`** all reach it).
For weapon `w = 0, 1`: skip when disabled (`0x6F0C6F90`: bit `0x80000 << w` of the attack ability flags; the three disable
counters have no writer), skip when its `targs` is exactly `1`; count it; OR its expansion; run the predicate with the
target; the first accepted weapon wins (`*out = w`, result 0). Otherwise: one weapon counted -> the lowest error code;
none -> `0xDD`; two -> the predicate over the OR of both expansions (its code if non-zero, else the lowest error).
`0x6F0C93E0` additionally refuses an accepted weapon when the target is magic immune (`unit+0x15C > 0`) and the Misc flag
`MagicImmunesResistDamage` is set and the weapon's attack type is Magic (row 4; code `0x36`), or when the target is
ethereal (`unit+0x158 > 0`), the flag is set and the attack type is physical (rows 1, 2, 3, 5, 6; code `0xBC`). The
"restriction list" at `attack+0x2BC` (weapons of kind 1 or an excluded `targs` skipped while it is non-empty) has no
writer in the model and is not applied.

## 5. In the simulation

* `gamedata::unittype` reads `targType`; `gamedata::combat::Weapon` carries `targs`, the kind, the splash fields and
  `UnitCombat::weapon2` (present when `weapsOn` bit 1 is set); `engine::data` fills `sim` weapon 1, weapon 2
  (`Game::set_attack_type2`) and the per-weapon `WeaponExtra`; `engine::objdata` gives weapons of kinds 2, 3, 5 and 6
  a missile speed from `Missilespeed` (list entry per weapon).
* `Game::choose_weapon(attacker, target)`: a type without `WeaponExtra` keeps weapon 0 and no check (hosts and tests that
  never filled `targs`). `attackable` and `scan_target` require an accepted weapon; `drive_target` stores the chosen
  weapon in `AttackObj::weapon` between swings, and the swing timing, range, damage and missile speed come from it.
* Units with ground and air weapons (gryphon-type, archer-type) therefore pick the right weapon, and a target no weapon
  accepts ends the attack order without damage.

## 6. Open

* The validator `0x6F0CBD70` post-processes some codes (`0x8F`, `0x90`, `0x92` pass through; `0x96` consults the target's
  class bit `0x400` "standon"): item / tree / wall targets are not modelled (the sim only attacks units).
* Targets that are not units (destructables, items) use the branch at `0x6F25D341`.
* `0x6F0766A0` (ability test on the target, code `0xCD`) and the `unit+0x20` / `unit+0x60` bit tests in `0x6F25D6F0` /
  `0x6F25D560` are not ported (they read state the model does not hold).
* The Misc flag `MagicImmunesResistDamage` is now read into `CombatMisc::magic_immunes_resist` (the target-side
  `DamageRules` still default it to false, as before).
