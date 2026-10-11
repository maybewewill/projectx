# Unit classification: IsUnitType, UnitAddType, UnitRemoveType, IsUnitIdType (Game.dll 1.26a)

Labels: CONFIRMED = machine code read and/or Unicorn-checked, LIKELY = strong inference, UNSURE = guessed.
Harness: `tools/emu/unit_class_emu.py` (runs the real native code on synthetic memory; vectors embedded in
`crates/sim/tests/unit_class_vectors.rs` and `crates/gamedata/tests/unit_class_vectors.rs`). Code:
`crates/sim/src/unitclass.rs`, `crates/gamedata/src/unitclass.rs`, natives in `crates/sim/src/natives/unit_attr.rs`.

## 1. The unittype values

`common.j` `ConvertUnitType(n)`, n = 0 to 26: HERO 0, DEAD 1, STRUCTURE 2, FLYING 3, GROUND 4, ATTACKS_FLYING 5,
ATTACKS_GROUND 6, MELEE_ATTACKER 7, RANGED_ATTACKER 8, GIANT 9, SUMMONED 10, STUNNED 11, PLAGUED 12, SNARED 13,
UNDEAD 14, MECHANICAL 15, PEON 16, SAPPER 17, TOWNHALL 18, ANCIENT 19, TAUREN 20, POISONED 21, POLYMORPHED 22,
SLEEPING 23, RESISTANT 24, ETHEREAL 25, MAGIC_IMMUNE 26. The native resolves the unit handle (`0x6F3BDCB0`); an invalid
unit gives 0. A value above 26 gives 0 (the jump table is guarded by a compare with 0x1A). CONFIRMED.

## 2. IsUnitType (`0x6F3C7E90`) per value (all CONFIRMED, 1,500 random units x 27 values, 0 mismatches)

| value | test |
|---|---|
| HERO | the first character of the type id (`unit+0x33`) is `A` to `Z`, and flag `unit+0x5C` bit 30 (illusion) is clear (`0x6F03FDC0`) |
| DEAD | `unit+0x5C` bit 8, read through unit vtable slot 79 (`0x6F28B2F0`) |
| STRUCTURE | `unit+0x5C` bit 16 (`0x6F26DE50` with argument 0). The bit is set at creation from the type's `IsStructure` value (`0x6F32CC80`) |
| FLYING | movement word `unit+0x1FC` bit 1 (value 2) |
| GROUND | `unit+0x1FC & 0xD` (foot, horse, hover) |
| ATTACKS_FLYING / ATTACKS_GROUND | the attack ability (`unit+0x1E8`) has an enabled weapon whose target word has bit 2 (air) / bit 1 (ground) |
| MELEE_ATTACKER | an enabled weapon with `weapTp` kind 1 (`normal`) whose target word is not 1, 0x40, 0x80 or 0x100 |
| RANGED_ATTACKER | same with kind 2 to 8 |
| GIANT, SUMMONED, UNDEAD, MECHANICAL, PEON, SAPPER, TOWNHALL, ANCIENT, TAUREN | `unit+0x248 & bit`, bit from the table at `0x6F942888` (pairs mask, value): GIANT 1, UNDEAD 2, SUMMONED 4, MECHANICAL 8, PEON 0x10, SAPPER 0x20, TOWNHALL 0x40, ANCIENT 0x80, TAUREN 0x800 |
| STUNNED, PLAGUED, SNARED, POISONED, POLYMORPHED, SLEEPING, RESISTANT | walk of the unit's ability list (`unit+0x1DC`, buffs are members): true when a member answers non-zero to vtable slot 98 (`+0x188`) / 101 / 99 / 100 / 102 / 103 / 104. See section 4 |
| ETHEREAL / MAGIC_IMMUNE | signed counter `unit+0x158` / `unit+0x15C` is greater than 0 |

A weapon is enabled (`0x6F0C6F90`) when flag `0x80000 << w` of the attack ability (`+0x20`) is set and none of three
disable counters fires: counter `+0x224` (signed, > 0) disables a weapon passing the melee test, `+0x228` a weapon
passing the ranged test, `+0x22C` a weapon whose target word is in the excluded set above. Per weapon w the ability
holds the kind at `+0xDC + 4w` and the target word at `+0x218 + 4w`; they are copies of the type record
(`+0xF8 + 4w`, `+0xA0 + 4w`), LIKELY.

Not modelled by the simulation (no writer exists): the three disable counters, ethereal and magic immunity counters
(fields exist on `UnitObj`, default 0), the Rust model of "weapon enabled" is the `weapsOn` bit (LIKELY).

## 3. UnitAddType / UnitRemoveType (`0x6F3C7BC0`, `0x6F3C7C20`) (CONFIRMED, 3,600 Unicorn checks)

Resolve the unit; the value minus 9 must be 0 to 11 and a byte map admits only 9, 10 and 14 to 20; then OR (or AND NOT)
the bit of section 2 into `unit+0x248` and return true. Every other value returns false and changes nothing. The word
`unit+0x248` is initialised from the type record `+0x1C8` at creation (`0x6F2A1F20` stores template field 92, filled by
`0x6F29F3E0` from `0x6F29C210`, which reads record `+0x1C8`). The same word also carries bits the unittype values do not
expose (0x100 neutral, 0x200 ward, 0x400 standon, and runtime bits such as 4 = summoned set by summon effects).

## 4. State values: which classes answer

All 3,675 vtables of Game.dll were scanned (`tools/analysis/rtti_vtables.py`) for slots 98 to 104. The base class
answers 0. The overriding classes (class, base code, answer):

* stunned: CBuffStun BSTN, CBuffMagicLeashTarget Bmlt, CBuffFreezingBreath Bfrz, CBuffFreeze Bfre, CBuffSleepStun BUst: 1.
* snared: CBuffEnsnare Bens, EnsnareAir Bena, EnsnareGround Beng, Web Bweb, WebAir Bwea, EntanglingRoots BEer: 1;
  CBuffLavaMonster BNlm, CBuffPurge Bprg, CBuffCargoHoldDeath Bchd: conditional on a field of the buff's target object
  (not modelled, answer false).
* poisoned: Bpoi, Bpsd, BIpb, BIpd, Bspo, Bssd, BNpa, Bliq, BNab, BHca, Bcsd, BEsh: 1.
* plagued: CBuffAuraPlague Bapl: 1.
* polymorphed: CBuffPolymorph Bply, CBuffHex BOhx: 1.
* sleeping: CBuffSleep BUsl, CBuffDarkConversion BNdc: 1; CAbilitySleepAlways Asla, CAbilityCreepSleep ACsp: ability
  flag `+0x20` bit 7 (not modelled, false).
* resistant: CAbilityResistantSkin Arsk: `+0x3C > 0` gives 0, otherwise vtable slot `0x268` of the ability (not
  modelled, false).

The sim answers the unconditional classes from `Unit::buffs` / the ability list by base code (`unitclass::state_codes`).
The vectors only cover the empty ability list (true state needs the list walker `0x6F4786B0`, not run). UNSURE for the
conditional classes.

## 5. IsUnitIdType (`0x6F3B7F60`) (CONFIRMED, 1,200 records x 27 values, 0 mismatches)

Same values, from the type record: HERO = first character `A` to `Z`; STRUCTURE = `IsStructure` value; FLYING =
movement word == 2; **GROUND = movement word == 0xD (exact compare: no real movement type has that value, so GROUND is
always false for real data)**; 5/6 OR the target words of the weapons selected by `weapsOn` bits 1 and 2 and test bit 2 /
bit 1; 7/8 the melee / ranged weapon tests over the same weapons; the nine classification values test the record
classification word; DEAD, the state values, ETHEREAL and MAGIC_IMMUNE are always false.

## 6. Loading the record fields (CONFIRMED, 500 strings per parser compared with the Rust port)

* `type` (UnitBalance, record `+0x1C8`), `0x6F26DA70`: empty text or first character `_` or `-` gives 0. Otherwise the
  text is split at any of space, comma, minus, tab (Storm ordinal 504: one delimiter ends a token, tokens are cut at
  15 characters) and each token equal to a name (Storm ordinal 509 = `_strnicmp`, case-insensitive, whole token) ORs its
  bit. The 12 names: giant 1, undead 2, summoned 4, mechanical 8, peon 0x10, sapper 0x20, townhall 0x40, ancient 0x80,
  neutral 0x100, ward 0x200, standon 0x400, tauren 0x800. Unknown tokens are ignored (a message is logged when nothing
  matched).
* `movetp` (UnitData, record `+0x1A8`), `0x6F26DB40`: the whole text must equal (case-insensitive) a name: foot 1,
  fly 2, horse 4, hover 8, float 0x10, amph 0x20, unbuild 0x40; `_`, `-`, `none`, empty and unknown give 0.
* `targs1/2` (record `+0xA0`, `+0xA4`), `0x6F255250`: the same tokenizer, 51 names (air 4, ground 2, structure 8,
  none 1, tree 0x40, wall 0x80, debris 0x100, ward/totem 0x10, item 0x20, ... ). **The 51st entry `_` ORs the odd word
  0xFDD1F00E** (so a `_` target list sets ground 2, air 4 and structure 8 among others).
* `weapTp1/2` (record `+0xF8`, `+0xFC`), `0x6F262D30`, case-sensitive (Storm ordinal 508 = `strncmp`): `_`/empty 0,
  normal 1, missile 2, artillery 3, instant 4, msplash 5, mbounce 6, mline 7, aline 8, other 0.

`gamedata::unittype::UnitTypeData` carries `classification`, `movement`, `weapon_kind`, `weapon_targs` (parsed in
`load_unit_type`, SLK row order untouched); `engine::data` copies them into `sim::natives::game::UnitType` and now also
marks `isbldg` types as buildings (`UnitTypes::set_building`, which the engine never called before).

## 7. Other natives of the lane

* `UnitSetUsesAltIcon` (`0x6F3CB860`): sets or clears bit 0x200 of `unit+0x280` (CONFIRMED). Stored as `UnitObj::alt_icon`.
* `IsUnitInTransport` (`0x6F3C7B70`): returns the cargo relation `0x6F079F80(unit, transport)` (not read). The model has
  no transport system, so it answers false (LIKELY).
* `IsUnitLoaded`: already implemented (`+0x5C` bit 4).
* Not done: `SetUnitUseFood` (changes the food accounting through `0x6F40F5E0`, no food model), `UnitSuspendDecay`
  (corpse decay flags and events `0xD015A` / `0xD0159`), `CreateCorpse`.
