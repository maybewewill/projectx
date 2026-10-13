# Orc hero abilities in Warcraft III 1.26a (Game.dll)

Scope (task S25): Critical Strike `AOcr`, Wind Walk `AOwk`, Mirror Image `AOmi`, Shockwave `AOsh`, War Stomp `AOws`,
Chain Lightning `AOcl`, Far Sight `AOfs`, Feral Spirit `AOsf`. Code: `crates/sim/src/orc.rs`. Unicorn harness:
`tools/emu/crit_emu.py` (vectors `crates/sim/tests/data/orc/`). Labels as elsewhere: CONFIRMED = read in the
disassembly (and Unicorn where a count is given), LIKELY, UNSURE, NOT FOUND.

## 0. Classes (RTTI, slot 7 = the code getter; vtable start = COL address + 4)

| Code | Class | Vtable | Slots (class-specific, byte offsets) |
|---|---|---|---|
| `AOcr` | `CAbilityCriticalStrike` | `0x6F909D74` | init `0x80` = `0x6F1A3440`; arm `0x124` = `0x6F1B2EC0`; category `0x130` = 0x0B; handler `0x134` = `0x6F1A3590`; score `0x158` = `0x6F1A3540`; evade `0x164` = `0x6F1A34C0`; after-hit `0x154` = `0x6F1CC530` (shared with Bash) |
| `AOwk` | `CAbilityWindWalk` | `0x6F8F2534` | effect `0x3C0` = `0x6F18CAA0`; `0x308` = `0x6F188290` |
| `AOmi` | `CAbilityMirrorImage` | `0x6F8D9E5C` | effect `0x3C0` = `0x6F135740`; `0x3E4` = `0x6F11B0C0` |
| `AOsh` | `CAbilityShockwave` | `0x6F92075C` | `0x424` = `0x6F1E7D20`, `0x420`..`0x418` = `0x6F1E7D80/DE0/E40`, `0x42C` = `0x6F1F2B90` |
| `AOws` | `CAbilityStomp` | `0x6F8F6C04` | `0x424` = `0x6F1606C0`, `0x428` = `0x6F1608B0`, `0x42C` = `0x6F18F880` |
| `AOcl` | `CAbilityChainLightning` | `0x6F8B0E74` | `0x424` = `0x6F1CB550`, `0x3F0` = `0x6F1B0650`, `0x43C` = `0x6F1B6F30` |
| `AOfs` | `CAbilityFarSight` | `0x6F88B25C` | `0x418` = `0x6F06FA50`, `0x424` = `0x6F06F840` |
| `AOsf` | `CAbilitySpiritWolf` | `0x6F927B2C` | effect `0x3C0` = `0x6F1DF330`, `0x3E4` = `0x6F1FCD80` |

## 1. Critical Strike (`AOcr`)

Level data (AbilityData.slk, all three heroes' levels): `DataA` = chance in **percent units** (15 means 15 %),
`DataB` = damage multiplier (2, 3, 4), `DataC` = 0, `DataD` = 0 (chance to evade), `DataE` empty.

### 1.1 The roll is a pseudo-random distribution, taken at the swing start (CONFIRMED)

* Where: the attack ability broadcasts the order `0xD000F` to all abilities of the attacker (`0x6F281580`, called from
  `0x6F0CD722` and `0x6F0D0CE5`); the broadcast calls ability slot `0x124`. For Critical Strike that is `0x6F1B2EC0`.
  It runs at the swing START, one draw per swing, not at the damage point.
* `0x6F1B2EC0`: clear flag `0x200`; stop when the target is null, the attacker carries the blocking buff (looked up
  through `0x6F0787D0`), the count gate `+0x3C > 0`, the enabled slot `0x268`, the target filter `0x6F02E000`, or
  `0x6F26DE50(target, 1)`. Otherwise: `eff = 0x6F1A3470()`; **one draw of combat stream 3** (`0x6F2853D0`, EDX = 3);
  `armed = draw <= eff` (soft `le` `0x6F01FD60`). Armed: set flag `0x200`, counter `+0x84 = 1`, event `0xE` to the
  owner; not armed: counter += 1. The init slot sets the counter to 1.
* `0x6F1A3470`: `c` = `DataA` (`0x6F02E9E0`), `n` = counter; `eff = mul(I2R(n), PRD(c))`.
* `PRD(c)` = `0x6F1D21F0`: `table[clamp(f2i(add(mul(c, 0.2), 0.5)), 0, 20)]`; table (21 entries, built on first use as
  `I2R(x) / I2R(10000)`): x = 0, 38, 147, 320, 547, 819, 1126, 1461, 1815, 2184, 2562, 2953, 3333, 3781, 4203, 4607,
  5000, 5799, 6666, 7706, 10000. The stored percentages are therefore 0, 5, 10 ... 100 in steps of 5.
* Unicorn (`tools/emu/crit_emu.py`, `crates/sim/tests/orc_crit.rs`): 400 lookup cases and 400 effective-chance cases
  (random float bit patterns incl. NaN/inf/negative, counters incl. 0, -1, 2^20), plus the 21 entries: **0 mismatches**.
  Rust: `orc::prd_value`, `orc::effective_chance`, `orc::crit_armed`.

### 1.2 The damage (CONFIRMED order, Unicorn for the roll only)

Execute `0x6F0CF660`: dice (`0x6F0C6940`), record constructor, then `0x6F2763A0` walks the attacker's abilities for the
categories `0x0B..0x19` (slot `0x130` equals the category -> slot `0x134`). Critical Strike has category `0x0B` and handler
`0x6F1A3590`: when flag `0x200` is set and `DataB != 0` (exact x87 equality; NaN goes on), `extra = mul(D, DataB) - D` is
added to the record damage through `0x6F255720`, which does nothing if the record's bonus field (`+0x18`) is already
positive and otherwise stores `extra` there. The **raw** damage is multiplied: armor and the damage type table come
after (`orc::crit_damage`, wired in `SpecDamage::roll`). `DataE != 0` would set the record flag `0x400` (never miss:
skips the miss gate); not modelled (data empty). `DataC` is the flat bonus of the after-hit slot `0x154`
(`0x6F1CC530`, shared with Bash, also uses Dur/HeroDur for a buff); not modelled (data 0, NOT FOUND in detail).

### 1.3 Evade share (`DataD`)

Slots `0x158`/`0x164` are the evasion walk slots (attack-damage.md 12.7): score = `DataD` (`0x6F02EB30`), evade = one
stream-2 draw, `DataD > draw`. With the shipped data `DataD = 0`, the score is 0 and the entry is never selected, so
no draw happens. Not wired (as before).

### 1.4 Model notes

`Game::orc_swing_start` (from `start_swing`) draws the roll, `Game::orc_crit_raw` (from `SpecDamage::roll`) applies it.
The placeholder damage model does not apply a crit. UNSURE: the blocking buff, the count gate and the target filters of
the arm function are modelled as open.

## 2. Shared plumbing (CONFIRMED structure)

* The cast dispatcher `0x6F05C3A0` (slot `0x3C0`) branches on the target kind (slot `0x34C`): unit target -> slot
  `0x424`; point/destination (`kind & 0x0A`) -> slot `0x418`; no target -> slot `0x428`.
* The area spells build their unit set with the group builder `0x6F2A3270` (tag list: area `0xB`, filter flags `0x1D`,
  owner, mode `0xF`, end `0x1F`; flags = the ability's target word `+0x4C & 0xFFFF0FFF | 0x10000`). The per-unit callback
  is called in the enumeration order of the area query (x-major cells, groups-events-2.md).
* The duration rule of every effect is the Slow/Storm Bolt rule: `HeroDur` for a hero (`0x6F1D2660`), else `Dur` plus the
  float at ability `+0x9C` (0.0 in the model).
* Model plumbing: `CastTarget::{None, Point}` casts (point range check against `Rng`), `Issue{Immediate,Point}Order*`
  reach `begin_cast`, order ids from the public list (UNSURE): stomp 852127, shockwave 852125, chain lightning 852119,
  wind walk 852129, mirror image 852093, far sight 852122, spiritwolf 852120.

## 3. War Stomp (`AOws`, `CAbilityStomp`, vtable `0x6F8F6C04`)

Data: `DataA` damage (25/50/75), `Dur`/`HeroDur` stun, `Area` 250/300/350, targs `ground,organic`.
Instant slot `0x428` = `0x6F1608B0` (eye candy, then the area slot `0x434` = `0x6F176CD0` with the caster position) ->
per-unit `0x6F1607B0` (CONFIRMED): skipped when the unit's slot `0x11C` is true or the immunity test `0x6F28D750`
rejects it (then a zero-damage record with flag `0x10` is sent); otherwise a damage record of `DataA`, class
`0x8000` (slot `0x42C` = `0x6F18F880`), flags `0x40`, goes to the unit's damage slot `0x120`; afterwards, if the unit
is not flagged `0x8` in slot `0x114` and was not skipped, slot `0x430` = `0x6F18ABF0` applies the stun with the
Dur/HeroDur rule. Model: `orc::war_stomp`. UNSURE: the relation filter (enemies), the meaning of the `0x114` bit, the
exact ground/organic tests. Numeric cores: none new (the arithmetic is the verified soft-float add and the Storm Bolt
duration rule).

## 4. Chain Lightning (`AOcl`, `CAbilityChainLightning`, vtable `0x6F8B0E74`)

Data: `DataA` damage (100/140/180), `DataB` units hit (4/6/8), `DataC` reduction 0.15, `Area` 500 bounce radius, `Rng` 700.
Slot `0x424` = `0x6F1CB550` sets the relation flags (`0x80000` when the target is an enemy, `0x100000` when
`target+0xEC > 0`), resets the factor cell (`+0x128`, value at `+0x12C`) to 1.0 and starts the hop body `0x6F1C2F50`
(CONFIRMED): hop damage = `mul(DataA, factor)`, then `factor := mul(factor, sub(1.0, DataC))`; the unit is appended to the hit
list (`+0x134`, 12-byte entries, count `+0x124`); damage record flags `0x400` (never miss) to the source's damage slot.
Next target: `0x6F1B6F30` = group builder around the unit just hit with `Area`, callback `0x6F1B0780`: in the distance mode it
keeps a candidate only when `lt(distance, best)` (strict, `0x6F00C110`), so the NEAREST valid unit wins and the first of equal
distances stays; validity `0x6F1B06F0`: attackable by the owner (`0x6F284EE0(owner, unit, 0, 4)`), not rejected by slot
`0x3F0`, not immune (`0x6F28D750`), not already hit (`0x6F1A2110`), and the `+0xEC` count rule. Model: `orc::chain_lightning`
(no hop delay: UNSURE; the damage class and the exact relation filter UNSURE). The arithmetic composes already
Unicorn-checked soft-float helpers (mul `0x6F6EEE20`, sub `0x6F6EEEF0`); the hop function itself was not run (it needs the
agile cell objects).

## 5. Shockwave (`AOsh`, `CAbilityShockwave`, vtable `0x6F92075C`)

Data: `DataA` damage (75/130/200), `DataB` cap, `DataC` distance 800, `DataD` end width 150, `Area` start width 150,
`Rng` 700, targs `ground,structure`. Point slots `0x418/0x41C/0x420/0x424` all forward to `0x6F1DD4F0`; the line missile
is spawned by `0x6F1F2B90` through the engine projectile system (not decoded). Model `orc::shockwave` (UNSURE): one
instant pass over the segment (width = max(Area, DataD), inclusive), hits ordered by travel distance, `min(DataA, DataB)`.

## 6. Wind Walk (`AOwk`, `CAbilityWindWalk`, vtable `0x6F8F2534`)

Data: `Dur`/`HeroDur` 20/40/60, `DataA` fade 0.6/1/1, `DataB` speed bonus 0.1/0.4/0.7, `DataC` first strike bonus
50/75/100. Effect `0x6F18CAA0` (CONFIRMED): duration by the hero rule on the caster, buff creation through the buff
factory `0x6F189F80` (slot `0x358`) with the data, then `0x6F079CA0(caster)`. Model: buff `BOwk` as a movement factor
`1 + DataB` (strongest-slow machinery of cast.rs), the first strike (`Game::orc_crit_raw`) adds `DataC` flat and ends the
buff (UNSURE: flat versus percent, order against the crit). Invisibility, the fade, and ending on casting are not modelled.

## 7. Mirror Image (`AOmi`) and Feral Spirit (`AOsf`)

Mirror Image (vtable `0x6F8D9E5C`, effect `0x6F135740`): `DataA` images (1/2/3), `DataB` damage dealt 0, `DataC` damage taken
2.0, `DataD` 0.5 (animation delay), `Dur` 60, `Area` 1000. Feral Spirit (vtable `0x6F927B2C`, effect `0x6F1DF330`): `UnitID`
`osw1..3`, `DataB` = 2 wolves, `Dur` 60. A direct-call search to depth 5 from both effects (and from the summon helpers
`0x6F1D27A0`, `0x6F299390`, `0x6F1DD4F0`) finds NO draw of the RNG helpers `0x6F2853D0/0x6F285320/0x6F285350`: no stream
draw is known for the placement (virtual calls not followed: UNSURE). Model: units built at the caster (illusion flag with
the damage factors; summoned flag), timed life `Dur`/`HeroDur` through BTLF. UNSURE: placement offsets, life of the images,
abilities of the images, where the taken factor applies.

## 8. Far Sight (`AOfs`, `CAbilityFarSight`, vtable `0x6F88B25C`)

Data: `Dur` 8, `Area` 600/1800/4000, `DataA` 3, `Rng` 99999. Point slots `0x418` = `0x6F06FA50`, `0x424` = `0x6F06F840`.
Model: a visible-state fog modifier over the circle for the owner, stopped with a marker buff `BOfs` on the caster after
`Dur` (UNSURE; the original's reveal mechanism is not decoded).

## 9. Status

| Ability | Class and effect decoded | Numeric core in Unicorn | Wired |
|---|---|---|---|
| Critical Strike | yes (roll, PRD, damage) | yes, 800 cases, 0 mismatches | yes (`start_swing`, `SpecDamage::roll`) |
| War Stomp | structure CONFIRMED | no new core | yes |
| Chain Lightning | hop arithmetic and selection rule read | components only | yes (no hop delay) |
| Shockwave | data + launcher only | no | MODEL |
| Wind Walk | effect read | no | MODEL |
| Mirror Image / Feral Spirit | effect entry points, no RNG found | no | MODEL |
| Far Sight | slots only | no | MODEL |
