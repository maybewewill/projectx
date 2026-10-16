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

### 3.1 S29 addendum (disassembly only, no new Unicorn run)

* `AOws` is `CAbilityStomp` (atlas: parent `CAbilityThunderClap`), not `CAbilityWarStomp` (that is the passive `Awar`).
* Per-unit body `0x6F1607B0` (re-read): skip when unit slot `0x11C` is true or `0x6F28D750` (immunity) rejects; damage record
  flags are `0x40` (the `0x10` bit is only set on a path that is never taken, `ebp = 0`), class from slot `0x42C`;
  the victim is then stored in the ability's hit list; the stun (slot `0x430` = `0x6F18ABF0`, Dur / HeroDur by `0x6F1D2660`)
  is applied only when the unit's slot `0x114` result has bit `0x8` clear. The area set is built by `0x6F2A3270` with the tags
  area (radius, x, y), `0x1D` (flag word = `targs & 0xFFFF0FFF | 0x10000`, caster, -1) and the mask builder `0x6F25D830`
  (`0xFF0000` family; relation bits via `0x6F25D480`). Which relation bits `ground,organic` (no `enemy` word) produce was
  NOT decoded, so the enemy-only filter of the model stays UNSURE. Status unchanged: structure CONFIRMED, filter UNSURE.

## 4. Chain Lightning (`AOcl`, `CAbilityChainLightning`, vtable `0x6F8B0E74`) - task S29

Data: `DataA` damage (100/140/180), `DataB` units hit (4/6/8), `DataC` reduction 0.15, `Area` 500, `Rng` 700.
Cast slot `0x424` = `0x6F1CB550` sets the ability flags (`0x80000` enemy, `0x100000` target `+0xEC > 0`), the factor cell
(`+0x128`, value `+0x12C`) to 1.0, count `+0x124` to 0, then hops at once.

* **Unicorn (CONFIRMED, `tools/emu/chain_emu.py`, `tests/data/orc/chain.txt`, `tests/orc_chain.rs`; the real hop body
  `0x6F1C2F50` and the real next-hop gate `0x6F1C3200` run on a fake ability with the real level getters; stubs for the
  handle helpers, the record builder, the effect constructors and the agile cell setter; 400 hop cases + 400 gate cases,
  0 mismatches):** damage = `mul(DataA, factor)`; record flags `0x400` (never miss); factor := `mul(factor, sub(1.0,
  DataC))` through the cell setter; the hit count `+0x124` grows by one; the bolt effect (`vtable + 0xA4`, `0x6F1B09C0`)
  is started with the delay pointers 0.25 (`0x6FAAE4B8`) and 1.0; the gate lets a hop through only while
  `count < F2I(DataB)`; the flag word given to the next-target slot `0x43C` is `(targs & 0xFFFE0FFF) | 0x10000 |
  (0x8000 if ability flag 0x80000)` (slot `0x42C` returns 0 for this class).
* **Timing (CONFIRMED by disassembly):** the bolt effect (`CEffectChainLightning`, vtable `0x6F8FBD74`) starts the timer
  message `0xD01D2` after 0.25 s and `0xD01D3` (destroy) after 1.0 s. Handler: slot 3 `0x6F1A2210`; `0xD01D2` -> effect slot
  `0x84` = `0x6F1C3360` -> `0x6F1C3200` (the gate, then slot `0x43C` and a new hop with the found unit). So hops are 0.25 s
  apart. The damage source of every hop is the caster (the hop arg1 is only the origin of the bolt).
* Next target (CONFIRMED by disassembly): slot `0x43C` = `0x6F1B6F30` builds the area set with the callback `0x6F1B0780`;
  slot `0x434` returns 0 and `0x438` returns 1 (distance mode), so the NEAREST candidate wins, kept only when its distance
  is strictly smaller (soft `lt`), the first of equal distances stays. Validity `0x6F1B06F0`: relation check
  `0x6F284EE0(owner, unit, 0, 4)` and slot `0x3F0`. The distance is `0x6F476860` with flag 1: the squared distance
  (`0x6F6EF3B0`) from the hit unit's point to the candidate's footprint point (`0x6F497460` + `0x6F38B0B0`), not the centre.
* UNSURE / NOT FOUND: the instant the damage lands (the effect has a repeating 0.1 s fade timer `0xD01A3` -> slot `0x98`
  `0x6F2BD210`, which only fades the lightning art; the code that applies the record was not found; the model damages at
  the hop), the footprint distance (model: centre distance), the relation filter and target-flag mask builder
  (`0x6F25D830`, class mask `0xFD0000` family, decoded but not applied), the damage class.
* Model: `orc::chain_lightning` / `chain_hop` / `chain_next` (timer kind `0x82`).

## 5. Shockwave (`AOsh`, `CAbilityShockwave`, vtable `0x6F92075C`)

Data: `DataA` damage (75/130/200), `DataB` cap, `DataC` distance 800, `DataD` end width 150, `Area` start width 150,
`Rng` 700, targs `ground,structure`. Point slots `0x418/0x41C/0x420/0x424` all forward to `0x6F1DD4F0`; the line missile
is spawned by `0x6F1F2B90` through the engine projectile system (not decoded). Model `orc::shockwave` (UNSURE): one
instant pass over the segment (width = max(Area, DataD), inclusive), hits ordered by travel distance, `min(DataA, DataB)`.

### 5.1 S29 addendum (disassembly only)

Atlas: `AOsh` `CAbilityShockwave` descends from `CAbilityCarrionSwarm` (same launcher as `AUcs`). The launcher `0x6F1DD4F0`
(all four point slots forward to it) reads the caster position, `DataC` (`0x6F02EAC0`, distance 800) and `Area`
(`0x6F02E900`), forms `d = target - start`, `len2 = dx*dx + dy*dy`; if `len2` exceeds the tiny constant `0x6FAB35F4` squared
the direction is `d` normalised (`0x6F07B870`), otherwise the caster's facing vector (`0x6F10E370`); the end point is
`start + dir * DataC` (soft add/mul). So the wave always travels `DataC` along the click direction, independent of the
click distance - the model's segment is right in that respect. The missile (`0x6F24F3E0`, class `CMissileShockwave`,
vtable `0x6F916DDC`) then flies and damages on its own path: speed, hit timing and per-unit rules were NOT decoded; the model
hits everything at the cast instant (UNSURE). Next: decode the missile's move slot and `0x6F1D5B90` (slot `0x430`).

## 6. Wind Walk (`AOwk`, `CAbilityWindWalk`, vtable `0x6F8F2534`)

Data: `Dur`/`HeroDur` 20/40/60, `DataA` fade 0.6/1/1, `DataB` speed bonus 0.1/0.4/0.7, `DataC` first strike bonus
50/75/100. Effect `0x6F18CAA0` (CONFIRMED): duration by the hero rule on the caster, buff creation through the buff
factory `0x6F189F80` (slot `0x358`) with the data, then `0x6F079CA0(caster)`. Model: buff `BOwk` as a movement factor
`1 + DataB` (strongest-slow machinery of cast.rs), the first strike (`Game::orc_crit_raw`) adds `DataC` flat and ends the
buff (UNSURE: flat versus percent, order against the crit). Invisibility, the fade, and ending on casting are not modelled.

## 7. Mirror Image (`AOmi`) and Feral Spirit (`AOsf`)

### 7.1 Mirror Image (task S29; CONFIRMED numeric cores, LIKELY structure)

The earlier search for an RNG draw (S25) only followed direct calls from the cast effect. The draw is in the **delayed
handler**, not in the effect. Level getters (Unicorn, `tools/emu` probe with a numbered level record): `0x6F02E760` Dur,
`0x6F02E7D0` HeroDur, `0x6F02E900` Area, `0x6F02E970` **Rng**, `0x6F02E9E0` DataA, `0x6F02EA50` DataB, `0x6F02EAC0` DataC,
`0x6F02EB30` DataD, `0x6F02EBA0` DataE.

* Cast effect `0x6F135740` (slot `0x3C0`): (1) class slot `0x3E4` = `0x6F11B0C0` walks the images of the previous cast
  (12-byte handle pairs at ability `+0xF4`, count `+0xF0`) and calls the unit slot `0x124` with `0.0` on each (LIKELY
  "set life 0": a recast kills the old images); (2) buff / state calls on the caster (`0x6F0767F0`, `0x6F076700` with the
  callback `0x6F1128B0`, `0x6F0796F0`, caster flag `+0x20 |= 0x800000`, cleared again at the end of the handler); (3) the
  timer message **`0xD01C1`** (`ability+0x16C`) with `DataD` seconds (0.5). The class message slot 3 (`0x6F1432F0`) routes
  `0xD01C1` to `0x6F13D990`.
* Handler `0x6F13D990` (read in full, the numeric cores run in Unicorn):
  1. `n = F2I(DataA)`, `c = min(n, 9)`, the polygon has `c + 1` slots. The hero's slot is drawn FIRST:
     `h = R2I(roll(1 die, n + 1 sides)) - 1` with the roll wrapper `0x6F285350` on **combat stream 12** (EDX = 0xC at
     `0x6F13DA6C`), count 1, bonus 0, multiplier 1.0. One draw per cast. The sides use `n`, not the capped `c`.
  2. Slot `i` is placed at `centre + M * (r sin a_i, r cos a_i, 0)` with `a_i = (I2R(i) * 2 pi) / I2R(c + 1)`
     (`0x6F1127D0`; `r` = **Rng**, 128; `2 pi` = the constant `0x6FAAE608`), `M` = the rotation about `(0, 0, 1)` by the
     caster's facing (`0x6F4AFCD0`, flag 1 = axis not normalised), applied by `0x6F07B950`
     (`out[c] = (v0*M[c] + v1*M[3+c]) + v2*M[6+c]`), then added to the caster's position (`add(rotated, centre)`).
     The matrix is `[c, s, -0, -s, c, +0, +0, -0, c + (1 - c)]` with the soft `c + (1 - c)` = `0.99999994`.
  3. Pass one (ascending): `0x6F127E00` -> placement search `0x6F11AD50` -> **`0x6F29F990`** (the unit creator) builds a
     unit of the caster's type for the caster's owner at every slot. A failed placement search returns 0 and the slot has
     no unit. Pass two (ascending): for the hero slot (`i == h`) the created unit is the placeholder: the caster is moved
     to its position (virtual `+0x180` of the caster with the position) and the placeholder is removed (`0x6F1280D0`);
     every other unit gets the Mirror Image buff (`vcall 0xC8`) with `DataB`/`DataC` (damage dealt / taken factors), and
     the duration `Dur + ability+0x9C` (soft add `0x6F6EF000`; no HeroDur in this path).
* Unicorn (`tools/emu/mirror_emu.py`, vectors `crates/sim/tests/data/orc/mirror.txt`, `crates/sim/tests/orc_mirror.rs`):
  slot pick on stream 12 (400 cases incl. the full stream state after), polygon offset (400), rotation matrix (400),
  transform (400): **0 mismatches**. Rust: `orc::{mirror_pick, mirror_offset, mirror_rotation, mirror_transform}`.
* Model (`orc::mirror_image` / `mirror_fire`, `tests/orc_mirror_cast.rs`): the delay is a `HeroSpell` timer (kind `0x81`);
  without the soft-float or RNG tables (a host that has none) the images stand at the caster and no draw is made.
* UNSURE: which of the two branches of pass two is the real hero swap (read from the arguments of the calls, not run),
  the failure drop of a slot, the caster's state during the 0.5 s, the abilities of the images, the exact meaning of
  `vcall 0xC8`, where the taken factor (DataC = 2) applies, `unit slot 0x124` = kill.

### 7.2 Feral Spirit (`AOsf`)

Feral Spirit (vtable `0x6F927B2C`, effect `0x6F1DF330`): `UnitID` `osw1..3`, `DataB` = 2 wolves, `Dur` 60. A direct-call
search to depth 5 from the effect finds no draw of the RNG helpers. Model: units built at the caster (summoned flag),
timed life `Dur`/`HeroDur` through BTLF. UNSURE: placement offsets, life of the wolves, abilities of the wolves.

## 8. Far Sight (`AOfs`, `CAbilityFarSight`, vtable `0x6F88B25C`)

Data: `Dur` 8, `Area` 600/1800/4000, `DataA` 3, `Rng` 99999. Point slots `0x418` = `0x6F06FA50`, `0x424` = `0x6F06F840`.
Model: a visible-state fog modifier over the circle for the owner, stopped with a marker buff `BOfs` on the caster after
`Dur` (UNSURE; the original's reveal mechanism is not decoded).

## 9. Status

| Ability | Class and effect decoded | Numeric core in Unicorn | Wired |
|---|---|---|---|
| Critical Strike | yes (roll, PRD, damage) | yes, 800 cases, 0 mismatches | yes (`start_swing`, `SpecDamage::roll`) |
| War Stomp | structure CONFIRMED | no new core | yes |
| Chain Lightning | hop, gate, timing, selection rule | yes: hop body + gate run for real (800 cases, 0 mismatches) | yes (0.25 s hops; damage instant, footprint distance UNSURE) |
| Shockwave | launcher end point decoded; missile path not | no | MODEL |
| Wind Walk | effect read | no | MODEL |
| Mirror Image | cast effect, delayed handler, RNG stream 12, polygon | yes: pick, offset, rotation, transform (4 x 400, 0 mismatches) | yes (hero swap LIKELY) |
| Feral Spirit | effect entry point, no RNG found | no | MODEL |
| Far Sight | slots only | no | MODEL |

## 10. S34: Healing Wave (`AOhw`, `CAbilityHealingWave`, vtable `0x6F8B12BC`) and the shared chain collector

Order id `0xD0215` = 852501 (slot `0x308`, agrees with the `ability_orders.py` cast getter). Atlas: a child of
`CAbilityChainLightning`; the cast entry `0x6F1CB550`, hop body `0x6F1C2F50`, next-hop gate `0x6F1C3200`, next-target search
`0x6F1B6F30` and collector `0x6F1B0780` are the chain code. The class overrides only: `0x42C` = 1 ("heals", also returned
by slot `0x430`), `0x434` = 1 (score mode), `0x438` = 0 (distance mode off), `0x1FC` = 1, `0x308`.

* **Unicorn (CONFIRMED, `tools/emu/heal_emu.py`, `tests/data/orc/heal.txt`, `tests/orc_heal.rs`; real code on fake
  objects; 400 hop + 400 gate + 800 collector cases, 0 mismatches, 246 collector picks):**
  * Hop (`HH`): the record amount is `DataA * factor` with the sign bit flipped when `0x42C` is non-zero (a negative
    damage record = heal); flags `0x400`; the next-hop timer 0.25 s, the bolt destroy delay is `[0x6FAAE504]` = **2.0**
    for Healing Wave (1.0 for Chain Lightning); factor and hit count as Chain Lightning (`factor * (1 - DataC)`).
  * Gate (`HG`): the flag word for the next-target search is `(targs & 0xFFFE0FFF) | base | (0x8000 if ability flag
    0x80000)`, with base `0x7000` for Healing Wave (slot `0x42C` non-zero) and `0x10000` for Chain Lightning.
  * Collector (`CB`): validity = visible to the owner's player (`0x6F284EE0` is a fog test, `0x6F3A38F0`, not a relation
    test), then slot `0x3F0` (`0x6F1B0650`): for a healer a unit with `|life - max| < 2e-7` (`0x6F01FE40`) is refused, and
    an illusion (`unit+0x5C & 0x40000000`) is refused unless Misc `IllusionsCanRestoreLife`; then the immunity test
    `0x6F28D750(unit, slot 0x348)`, the hit-list test `0x6F1A2110`, and for a non-healer without ability flag `0x100000` a
    unit with `+0xEC > 0` is refused. Distance mode (Chain Lightning): the candidate wins if its squared distance is
    strictly smaller (soft `lt`) than `S+0x14`. Score mode (Healing Wave): score = `max - life` (soft sub, slot `0x130`
    minus slot `0x12C`), strictly greater (soft `gt`) than `S+0x18` (start 0.0) wins; ties keep the first. So Healing Wave
    always jumps to the **most wounded** valid unit in the area, not the nearest. Rust: `orc::chain_candidate_update`.
* Distance for Chain Lightning: `0x6F476860` is a method of the candidate's position object; its stack arguments are
  (out, x, y, flag) pushed BEFORE the position getter call (slot `0xB8`, no arguments). It subtracts the grid origin
  (`[0x6FAB7368] + 0x6C/0x70`), scales by 2^-5 (exponent subtract with underflow flush) and takes the fused squared
  distance `0x6F6EF3B0` against the unit's cell position (`+0x78`), or against the footprint point via `0x6F497460` /
  `0x6F38B0B0` with flag 1. Not run (needs the world); the model keeps the centre distance (UNSURE, unchanged).
* Hop ethereal bonus (NOT modelled): when `0x42C != 0` and the target's `+0x158 > 0`, Misc `EtherealHealBonus` modifies the
  amount (`0x6F1C3045..0x6F1C307D`).
* Model: `orc::chain_lightning(.., heals = true)`: the heal adds life clamped to the maximum (as Holy Light; the
  negative-record path of the target side is UNSURE); next target per the collector (allies, alive, not hit, wounded,
  not structure / mechanical, visible assumed).
* Corrections made while here: the order ids of Mirror Image (852123) and Feral Spirit (852126) were wrong (fixed upstream
  by the coordinator); other ids from slot `0x308`: Earthquake 852121, Whirlwind (Bladestorm) 852128, Reincarnation
  852124, Hex 852502, Voodoo 852503, Serpent Ward (`AOwd`, not `AOsw`) 852504.

## 11. S34: Wind Walk (`AOwk` / buff `BOwk`, `CBuffWindWalk`, vtable `0x6F8ED58C`)

Order id `0xD00A1` = 852129 (slot `0x308`). Atlas: `BOwk` descends from `CBuffInvisibility` (`Binv`).

* **Buff constructor (slot `0x358` = `0x6F171630`), CONFIRMED by two Unicorn runs (`tools/emu/ww_ctor_emu.py`: the real
  cast effect `0x6F18CAA0` with marker level values, then the real constructor with marker arguments):** the effect hands
  `(owner, &duration, &DataA, &DataB, &DataC, &Area, ability, owner id, DataD != 0)`; the duration is `HeroDur` for a hero
  (`0x6F1D2660`) else `Dur + ability+0x9C`. The constructor stores DataB into the agile cell `+0xDC` (value `+0xE0`: the
  movement bonus), DataC into the cell `+0xE4` (value `+0xE8`: the first strike bonus), Area into `+0xF4`, the ability in
  `+0xFC`, the owner id in `+0x108`; DataD != 0 sets the buff flag `0x400` (record flag "never miss"). DataA (fade time) is
  not stored by the constructor (presentation / the invisibility base).
* **Movement bonus:** the buff adds `DataB` to the unit's movement multiplier (`0x6F1717F0` / `0x6F171840` copy `+0xE0` into
  the stat block; the removal slot `0x33C` = `0x6F171710` calls `0x6F202FF0(-DataB)` on the mover `unit+0x1EC`; the property
  is an additive agile float snapped to 1.0 within `[0x6FAAE47C]`). So bonuses ADD (1 + sum), they do not take the maximum.
  The model keeps its slow machinery (UNSURE for stacking with other speed bonuses).
* **First strike, Unicorn (CONFIRMED, `tools/emu/ww_emu.py`, `tests/data/orc/windwalk.txt`, `tests/orc_windwalk.rs`, 400
  handler + 400 arming cases, 0 mismatches):** the buff has category `0x130` = 0x0B (the SAME category as Critical Strike) and
  handler `0x134` = `0x6F1637B0`, after-hit `0x154` = `0x6F163820` (presentation: a text tag). With the buff flag `0x200`
  set the handler posts event 0xE, writes `DataC` through the shared record writer `0x6F255720` (`orc::record_bonus`:
  refused when the record's bonus `+0x18` is positive, else `damage := (damage - bonus) + v`, `bonus := v`), ORs `0x400` into
  the record flags when the buff flag `0x400` is set, and calls slot `0x34C(0.01 s, 0)`: the buff ENDS 0.01 s later through
  the timed-life message `0xD01C4` (not in the same call). The bonus is FLAT (CONFIRMED: an add, not a percentage).
* **Arming:** slot `0x124` = `0x6F171870` is called by the `0xD000F` broadcast at the swing start (same as Critical Strike):
  clears `0x200`; with a target and buff flag `0x100` clear it calls the target filter `0x6F26D590(owner, target, buff+0xB8)`
  (= `0x6F25D6F0(.., -1)`); answer 0 sets `0x200` and posts event 0xE, a non-zero answer REMOVES the buff (`0x6F079CC0`).
  Which targets the filter refuses (buff word `+0xB8`) is not decoded (UNSURE; the model accepts every target).
* **Order against Critical Strike (CONFIRMED rule):** `0x6F2763A0` walks the unit's ability list head to tail for each
  category; `0x6F079990` (add ability) links a new ability at the HEAD, so the newest ability is visited first. The buff is
  added at cast time, after the hero learned Critical Strike, so the Wind Walk handler runs first, fills the bonus field with
  `DataC` and the Critical Strike writer is then refused: **a Wind Walk strike does not crit** (unless `DataC` is not
  positive). The exception (Critical Strike learned while the buff is up) is not modelled. Both handlers return 0, so the
  walk continues through all abilities. Rust: `Game::orc_crit_raw`.
* **Cancel on cast:** the buff's slot `0x178` (`0x6F15A270`, six arguments) forwards to the removal `0x6F079CC0(owner, buff)`:
  LIKELY the "ability cast by the owner" notification (not run; the model does not end the buff on casting).
* Not modelled: the invisibility itself (`Binv` base, fade `DataA`), which belongs to the vision / detection system.

## 12. S34: Bladestorm (`AOww`, `CAbilityWhirlwind`, vtable `0x6F8F2144`)

Order id `0xD00A0` = 852128. Atlas: a direct child of `CAbilitySpell`; class slots: message `0xC` = `0x6F191350`, level
initialiser `0x3E8` = `0x6F163570` (also called from the init slot `0x80`), cast effect `0x3C0` = `0x6F1712D0`, end
`0x6F171450`, damage handler `0x134` = `0x6F163650` (category `0x130` = 0x0B), timer message `0xD01C1`.

* **Set-up, Unicorn (CONFIRMED, `tools/emu/bs_emu.py`, `tests/data/orc/bladestorm.txt`, `tests/orc_bladestorm.rs`; the real
  initialiser on a fake ability with a real level record, 400 cases, 0 mismatches):** `total = Dur + ability+0x9C`
  (soft add; `HeroDur` is NOT used, there is no hero branch), `ticks = f2i(total * [0x6FAB1F4C])` with
  `[0x6FAB1F4C]` = 5.0, at least 1, stored at `+0xF8` (and copied to the countdown `+0xFC` by the cast effect); the damage
  per tick (agile cell `+0x100`, read at `+0x104`) = `div(mul(DataA, total), I2R(ticks))` (so the total damage is
  `DataA * total`, spread evenly over the tick count; DataA is a damage PER SECOND); the radius cell `+0x108` = `Area`.
* **Cast effect (CONFIRMED structure):** sets ability flag `0x10000`, starts a periodic timer (message `0xD01C1`,
  period = soft reciprocal of 5.0 = 0.2 s), disables the attack ability (`0x6F0CCB00(.., 1, 0)`), posts the owner event
  0x12, applies the movement state through unit slot `0x8C` (4, 0, 1), and toggles the Mirror Image / Detect abilities
  (`Amim`, `Adet`); flag `0x8000` marks the spin. The end (`0x6F171450`) undoes all of it.
* **Tick `0x6F1912A0` (CONFIRMED by reading):** `+0xFC -= 1`; when it reaches 0 the end routine runs; then, when the
  pause counter `+0x3C` is not positive, the area damage body `0x6F2C31D0` is called with the caster, the ring
  (`+0x10C` radius, `+0x104` amount), the ability's target word `+0x4C`, damage class **`0x20`** (= `1 << 5`, damage type
  ENHANCED) and the eye-candy callback `0x6F18C930`. The final tick therefore still deals its damage: exactly `ticks` damage
  ticks at 0.2 s, 0.4 s, ... (see `natives-damage-items.md` section 2 for the area body).
* **Damage handler `0x134`:** while the spin is on (`+0xFC > 0`) and the target filter `0x6F26D590(owner, target, +0x4C)`
  is zero, the unit's own attack record is zeroed (`damage = damage * 0.0`, `bonus = bonus * 0.0`, record flag `0x10`).
* Model: `orc::bladestorm` / `blade_tick` with the three pure functions above; the target filter is UNSURE (living enemy
  ground non-structure units), the re-armed one-shot timer replaces the repeating timer, a marker buff `BOww` lasts
  `total`.

## 13. S39: War Stomp (`AOws`, `CAbilityStomp`) - the area filter and the per-unit body (Unicorn)

All numbers CONFIRMED by reading and by the Unicorn run `tools/emu/stomp_emu.py` (the real per-unit callback `0x6F1607B0`
on a fake ability with real level getters and a fake victim; 400 cases, 0 mismatches; `tests/data/orc/stomp.txt`,
`tests/orc_stomp.rs`).

* **Area filter (corrects section 3 / 3.1, which left it UNSURE).** The instant slot `0x428` (`0x6F1608B0`) calls the
  shared area slot `0x434` = `0x6F176CD0` with the third argument 1. That slot forms the flag word
  `W = (targs & 0xFFFF0FFF) | 0x10000`, plus `0x8000` because the argument is non-zero, and calls the builder `0x6F2A3270`
  with the tags `0xB` (circle: area, x, y of the CASTER), `0x1D` (`W`, caster, -1), `0xF` and `0x1F`. Tag `0x1D` sets
  builder bit `0x20000000`, which appends the per-candidate predicate `0x6F26E510` (it is
  `0x6F25D6F0(unit, W, caster, -1) == 0`) and ORs the coarse category mask of `0x6F25D830` into the iterator's mask. So
  the filter is exactly the splash candidate filter of `weapon-splash.md` (relation, alive, vulnerable, kind, organic /
  mechanical, hero, sapper, ancient) with the word `W`: the relation bits are **enemy and neutral only** (self, player,
  ally cleared). A unit that is passively allied but does not share spells counts as "neutral" and is hit; the caster
  itself, own units and shared-control allies are not. With the shipped `targs` `ground,organic` (word `0x4000002`) the
  word is `0x4018002`, so air units, structures that are not "ground" and mechanical units are refused; a dead unit and
  an invulnerable (vulnerable-only default) unit are refused by the expanded word. Rust: `orc::stomp_word`,
  `orc::area_predicate`, `Game::set_orc_targs` (host-supplied words, default `STOMP_TARGS_DEFAULT`).
* **Per-unit body `0x6F1607B0`.** The unit's slot `0x11C` (`CUnit` returns 0) and the spell-immunity test `0x6F28D750`
  (walks the unit's ability lists and asks each immunity ability, slot `0x26C`) select the ZERO path. Normal path: damage
  record amount `DataA`, class from slot `0x42C` (**`0x8000`**, CONFIRMED in the run), flags **`0x40`**; zero path: amount
  `0.0`, flags **`0x50`** (the `0x10` bit is set on exactly this path; this corrects section 3.1, which said it was never
  set). The record goes to the victim's damage slot `0x120` in both cases (a zero amount does nothing in the model).
  Afterwards `0x6F282F60` resolves the victim (valid handle, not removed, dead bit `0x100` of `unit+0x5C` clear); a resolved
  victim is stored in the ability's hit array at `[owner index + 1]` (`vf 0xEC` = `unit+0x58`; last one per owner wins),
  and the stun slot `0x430` (`0x6F18ABF0`, `HeroDur` for a hero by `0x6F1D2660`, else `Dur + ability+0x9C`; nothing for a
  zero duration; an existing `BPSE` buff is refreshed through its slot `0x360`) runs only when the unit slot `0x114`
  (`unit+0x24C`, the `targType` word) has **bit 8 (structure) clear**, the unit is resolved and the zero path was not
  taken. Rust: `orc::stomp_unit`, `orc::stomp_record_flags`.
* Enumeration order: the area query's cell walk around the caster (x-major cells), as before; the damage of each unit is
  applied before the next unit is visited (the callback runs per candidate), deaths are dispatched after the loop
  (model).
* Not found: the "building factor" of the task text. Nothing in the stomp body scales damage for structures; the only
  structure-dependent branch is the stun skip above.

## 14. S39: Feral Spirit (`AOsf`, `CAbilitySpiritWolf`, vtable `0x6F927B2C`) - count, duration, flag, placement (Unicorn)

Unicorn run `tools/emu/feral_emu.py` (two separate processes, `E` and `P`, 400 cases each, 0 mismatches against the Rust
functions; vectors `tests/data/orc/feral.txt`, tests `tests/orc_feral.rs`). Order id 852126 (section 10 correction).

* **Effect `0x6F1DF330` (the real code on a fake ability; stubbed: event registration `0x6F1D7990`/`0x6F46D1B0`, info
  builder `0x6F042B20`, creator wrapper, the class slot `0x3E4`).** `n = F2I(DataB)` (`0x6F02EA50`, `0x6F6EEC20`);
  duration `T = Dur + ability+0x9C` (soft add `0x6F6EF000`). **There is no `HeroDur` branch** (corrects section 7.2, whose
  model used `HeroDur` for a hero caster). The loop runs `n` times; each iteration calls the summon wrapper `0x6F1D27A0`
  (ECX = the unit type from `UnitID` `0x6F02EDD0`, EDX = the caster; stack: the caster's owner index from slot `0xEC`,
  a pointer to `Area`, the constant 1, the flag word) and then `0x6F053040` (ability bookkeeping), the class slot `0x3E4`
  (`0x6F1FCD80`: builds the buff `BOsf` = `CBuffSpiritWolf`, vtable `0x6F9277BC`, through `0x6F1FA830`, init slot `0x364`
  `0x6F1E9900` = `CBuffTimedLife` init `0x6F0D67C0` + register `0x6F26EF10`; the duration argument is a pointer to `T`)
  and the release `0x6F299390`. The flag word is `0x502` when `T == 0` (x87 equality, NaN = non-zero) and `2` otherwise.
  The timed-life init sets class flag 4 (`unit+0x248 |= 4`, the model's summoned bit). No RNG draw anywhere (CONFIRMED by
  the run: the stream state is untouched). Rust: `orc::feral_plan`.
* **Placement (`0x6F1D27A0` -> `0x6F1D26B0`, real code).** The point given to the unit creator `0x6F29F990` (ECX = owner,
  EDX = type, stack = pointers to x, y, facing, then `flags | 2`, 1, a cell id from `0x6F2ABF20`) is
  `(cx, cy) + M * (Area, 0, 0)` with `M` the rotation about (0,0,1) by the caster's facing (the matrix and transform of
  Mirror Image, `mirror_rotation` / `mirror_transform`); the wrapper adds the CASTER's coordinate first (operand order
  matters for the sign of a zero), and the facing handed on is the caster's own (third argument 1 selects the caster's
  facing; 0 would use a Misc-derived random angle via `0x6F020D30`, not used by Feral Spirit). So the wolves appear
  `Area` (200) in FRONT of the caster, all at the same requested point; separation is the creator's business (its
  placement search is not decoded: UNSURE). Rust: `orc::feral_place` (needs the soft-float tables; without them the model
  places the wolves at the caster).
* Wired: `feral_spirit` uses both. The wolf abilities, the `BOsf` buff polarity and the placement search stay UNSURE.

## 15. S39: Serpent Ward (`AOsw`, base `AOwd`, `CAbilityWard`, vtable `0x6F8F91E4`) - point cast (Unicorn)

The ability row is `AOsw` (code `AOwd`); order id 852504 (section 10). Unicorn run `tools/emu/ward_emu.py` (the real slot
`0x418` = `0x6F162DC0` on a fake ability; 400 cases, 0 mismatches; `tests/data/orc/ward.txt`, `tests/orc_ward.rs`).

* The ward cast has the same shape as the Feral Spirit loop: `n = F2I(DataA)` (`0x6F02E9E0`; 1 in the data) wards of the
  level's `UnitID` (slot `0x434` = `0x6F190980`) for the caster's owner (slot `0x42C` = `0x6F190960` -> caster `vf 0xEC`),
  duration `Dur + ability+0x9C` (**no `HeroDur`**), creator flag `0x502` when that duration is zero else `2`.
* Every ward is created AT THE TARGET POINT: the creator `0x6F29F990` receives the cast's own x and y pointers (the values
  equal the ordered point bit for bit, no offset, no range pull) and a pointer to the facing, which is the Misc
  `BuildingAngle` (read through `0x6F009EE0`, key `BuildingAngle` of section `Misc`) rounded down to a multiple of 90
  degrees: `v = F2I(a) % 360; if v != 0 && v % 90 != 0 { v -= v % 90 }; if v < 0 { v += 360 }; facing = I2R(v) * (pi/180)`.
  With the shipped Misc value 270 the facing is `4.712389`. `ai_site::building_angle` (AI code) has no rounding; the ward
  uses `orc::building_angle_radians`. The sixth creator argument is -1 (no cell id; Feral Spirit passes a cell id).
* Afterwards slot `0x430` (`0x6F18C650`) builds the ward buff `BOwd` (factory `0x6F189900`, init slot `0x324`, apply slot
  `0x31C` with the ward and a pointer to the duration; `0x6F079CA0` adds it) and `0x6F299390` releases the unit.
* Model: `orc::serpent_ward` (wired in `run_effect`, order 852504, `takes_point`): builds the wards, marks them summoned
  (class bit 4), timed life = the duration. The ward's own attack (a `Aatk` ability of `osp1..3`) and the ward target
  class come from the unit data. UNSURE: the buff's polarity and the unit creator's placement search (the target point
  may be occupied).

## 16. S39: Hex (`AOhx`, `CAbilityHex`, vtable `0x6F8B1704`, a `CAbilityPolymorph`) - the creature pick (Unicorn)

Order id 852502 (section 10). Unit-target slot `0x424` = `0x6F12F560` (shared with Polymorph). Unicorn run
`tools/emu/hex_emu.py` (the real selector `0x6F12DAA0`, real class vtable, real token routine, vector helpers and roll
wrapper; the profile lookup, Storm's tokenizer / allocator and the type database are Python hooks; 400 cases, 0
mismatches including the stream state; `tests/data/orc/hex.txt`, `tests/orc_hex.rs`).

* **Cast `0x6F12F560`.** A target whose `unit+0x5C` has bit `0x40000000` (an ILLUSION) is killed outright: the unit's slot
  `0x124` (set life) is called with 0.0 and nothing else happens. Any other target: duration = `HeroDur` for a hero (the
  hero test `0x6F1D2660`: hero-letter type id, a type flag, or the ability test `0x6F0766A0`) and `Dur + ability+0x9C`
  (`0x6F02F880`) otherwise; the creature comes from `0x6F12DAA0(level, class)` with `class = unit+0x1FC` (the target's
  `movetp` word); slot `0x43C` = `0x6F0BAD50` then builds the morph buff `BOhx` (helper `0x6F0BACD0`; the flag word of the
  buff has bit 1 / bit 2 cleared by a switch on the class).
* **The pick.** The list text is the level's `DataB` (default), `DataC` (class 2 = fly), `DataD` (class 32 = amphibious) or
  `DataE` (class 16 = float), looked up by `0x6F113DE0` with the key text `Data?` + level; a missing or empty text falls back
  to the class default of the class vtable: `nshe` (slot `0x42C`, default class), `nshf` (`0x430`, class 2), `nsha`
  (`0x434`, class 32), `nshw` (`0x438`, class 16). The text is split by Storm's `SStrTokenize` with the delimiters blank,
  comma, hyphen, underscore, tab; each token becomes its first four bytes as a big-endian word (`0x6F4C3F50`); a type
  the unit database does not know (`0x6F29D2F0` -> `0x6F29A8C0`) is dropped. With NO candidate nothing is drawn and the
  result is type 0; otherwise ONE draw of combat **stream 24** (`edx = 0x18` at `0x6F12DC14`): the roll wrapper `0x6F285350`
  with count 1, `n` sides, bonus -1, multiplier 1.0 -> `R2I` -> index `0..n-1` into the candidate list in list order. Rust:
  `orc::{hex_pick, polymorph_tokens, polymorph_code, polymorph_default_list, polymorph_data_column}`. The shipped data:
  `DataB` = `npig,nsea,ncrb,nhmc,nrat,nfro,nech,necr,nrac`, `DataC` = `nalb,nvul,nsno`, `DataD` = `nsha,npng`, `DataE` =
  `nshw,npnw`, `DataA` = 99 (not read by this path), `Dur` 15/30/45, `HeroDur` 4/5/6.
* Model: `orc::hex_cast` (illusion dies; otherwise buff `BOhx` with the duration and the recorded form,
  `Game::hex_form`; `Game::set_orc_lists` takes the level texts, the shipped lists are the default). NOT modelled: the
  stat / ability swap of the morph (the hexed unit cannot attack or cast) and what the buff does when no candidate exists.

## 17. S39: Big Bad Voodoo (`AOvd`, `CAbilityVoodoo`, vtable `0x6F8F8124`) - structure (reading only, no Unicorn run)

Order id 852503 (section 10). Data: `Dur` = `HeroDur` 30, `Area` 800, `Cool` 180, `Cost` 200, `targs` `air,ground,friend,vuln,invu`,
buffs `BOvd,BOvc`.

* **Effect `0x6F18C340` (slot `0x3C0`), CONFIRMED by reading.** It first removes the caster's earlier buffs of three types
  (`0x6F10F990`, `0x6F07D0B0`, `0x6F159120`) through `0x6F079CC0`, starts the ability's timer (message `0xD01C1`, period
  the float at `0x6FAAE4D0` = 0.5 s) and creates an area watcher (`0x6F474720`) of radius `Area` (`0x6F02E900`) around the
  caster with the message `0xD01C7`, stored at ability `+0x138`. The message handler `0x6F191220` routes `0xD01C1` ->
  `0x6F1592E0` (restart the watcher), `0xD01C2` -> `0x6F1592F0`, `0xD01C3` -> `0x6F14BE50`, `0xD01C7` -> `0x6F18C630`:
  resolve the unit of the message (`0x6F282F60`) and run `0x6F18C580` on it.
* **Per unit `0x6F18C580`.** The class gate slot `0x3EC` returns non-zero -> nothing. Otherwise the buff duration is
  `add(0.5, 0.5)` = **1.0 s** (soft add, `0x6FAAE4D0` twice); a unit that already has the buff (lookup `0x6F0787D0` by the id of
  `0x6F159120`) gets it refreshed through the buff's slot `0x350`, else a new buff (`0x6F189830`) is initialised (slot
  `0x324`), applied (slot `0x31C` with the unit and the duration) and added (`0x6F079CA0`). So the effect is a pulsing aura
  of 1-second buffs, not one 30-second buff per unit.
* **`targs`.** The area predicate of section 13 with the unmodified ability word (`friend` = `0x6000`: player and ally;
  there is NO `self` bit, so the area never selects the caster; `vuln` and `invu` both named: any vulnerability state).
* Model: `orc::voodoo` (pulse every 0.5 s over the circle for `Dur`, buff `BOvd` 1.0 s on every friend that passes
  `area_predicate`, each live buff holds one invulnerability count). UNSURE: the slot `0x3EC` gate, the watcher's own enter
  events (the model pulses), the channel interruption (shared channel lifecycle, other agent), `BOvc`, the healing
  (the 1.26a buff `BOvd` only grants invulnerability, LIKELY from the tooltip).

## 18. S39: Command Aura (`AOac`) base damage - resolved (Unicorn); Endurance Aura (`AOae`) status

`buffs.md` left the base damage of the Command Aura as "the weapon's minimum roll (UNSURE)". The apply routine `0x6F1ACC40`
computes per weapon slot `w` (`unit+0x1E8` is the attack ability): `X = 0x6F26D5F0(unit, &out, w)`, then
`F2I(add(mul(DataA, X), 0.5))` (or `F2I(DataA)` when flag D, `buff+0x20` bit 8, is set), clamped below by the negative of
`0x6F0C6C70(w)`. `0x6F26D5F0` gives 0.0 without an attack ability, else `0x6F0C6D00(attack, &out, w)`:

`X = I2R(bonus) + I2R(count) * (I2R(sides) * 0.5 + 0.5)` (soft float) with the attack ability's `+0x88 + 4w` (dice count),
`+0x94 + 4w` (sides) and `+0xA0 + 4w` (base bonus; `GetDamageAmount` `0x6F0C6940` adds the extra `+0xAC + 4w` for the roll, this
getter does not). So the aura scales the AVERAGE damage `bonus + count (sides + 1) / 2`, not the minimum roll. CONFIRMED by
Unicorn (`tools/emu/cmdaura_emu.py`: 200 `0x6F0C6D00` + 200 `0x6F26D5F0` cases incl. negative and huge counts, 0
mismatches; `tests/data/orc/cmdaura.txt`, `tests/orc_cmdaura.rs`). Rust: `orc::command_base_damage`; `auras.rs`
(`aura_damage_amount`) now calls it. Worked example (made-up weapon, 1 die of 6 sides, bonus 20): average 23.5; `DataA` 0.1 -> `F2I(2.35 + 0.5)` = 2. Also read: `0x6F1ACC00` (illusion caster: bonus only when the Misc flag at `0x6F8B4ABC` is
set), the weapon-type filters (`+0xDC + 4w` == 1 for ranged or `2..8` for the other kinds, `+0x218 + 4w` targs masks `0x80`, 1,
`0x40`, `0x100` allowed) which stay as in `buffs.md`.

**V1: the whole weapon-slot bonus of the Command / Trueshot buff `0x6F1ACC40` is CONFIRMED by Unicorn, 400 cases, 0
mismatches** (`tools/emu/spell_buff_emu.py` check `command_aura`, the real routine on a fake buff with random fraction, flag
bits 8..10, one or two weapon slots, random dice, bonus, weapon kind `+0xDC + 4w` and `targs` `+0x218 + 4w`;
`crates/sim/tests/data/unit_spells/command_aura.txt`, `spell_buff_vectors.rs`; Rust `orc::command_aura_weapon_amount`, used by
`auras.rs`). Beyond the average-damage getter above: a weapon slot is skipped when its `targs` is not one of `1`, `0x40`,
`0x80`, `0x100` and (kind 1 with bit 8 clear, or kind 2..=8 with bit 9 clear); bit 10 makes the amount `F2I(DataA)`
(flat); **a zero result takes the sign of `DataA`** (+1 for a positive fraction, so the aura always adds at least one
point; -1 for a negative or NaN one; 0 for exactly zero), which the model lacked; a negative result is clamped from below
by `-(bonus + count)`; the amount goes into the weapon's damage counter and the buff keeps it for the undo. The mapping of
`DataB` / `DataC` / `DataD` to bits 8 / 9 / 10 of the buff's flag word is from `buffs.md` (not part of this run).

Endurance Aura (`AOae`): the movement bonus `DataA` and the attack speed bonus `DataB` are applied as in `buffs.md` (additive
agile multiplier cell snapped to 1.0 within 0.001; attack ability re-arm in proportion); no new number was decoded in S39.

## 19. S39: Bladestorm tick and targets (`AOww`) - Unicorn for the tick, reading for the filter

* **Tick `0x6F1912A0`, Unicorn (`tools/emu/bladetick_emu.py`, 400 cases, 0 mismatches; `tests/data/orc/bladetick.txt`,
  `tests/orc_bladetick.rs`; the real tick on a fake ability, the end routine and the area body recorded).** The counter at
  `+0xFC` is decremented first (wrapping); when it is then exactly 0 the end routine `0x6F171450` runs; afterwards, when the
  pause counter `+0x3C` is NOT positive (signed `jg` skip), the area body `0x6F2C31D0` is called once with the 12 stack words
  (ring pointer, ring count 1, damage class `0x20`, 0, 0, the ability word `+0x4C` UNCHANGED, the caster, owner -1, the
  eye-candy callback `0x6F18C930`, the ability, 1, 0); the ring is `(radius = cell +0x10C, amount = cell +0x104)`. Rust:
  `orc::blade_tick_step`.
* **Target filter (corrects section 12 and the old model, which excluded structures).** The area body dispatches on the
  word: with any of the bits `0x7E0` (item, tree, wall, debris, decoration, bridge) set it uses the destructable-aware group
  builder `0x6F2AE9E0` (tags `0x25`, `0x2A`, `0x2B`), otherwise `0x6F2A3270` (tag `0x1D`). The shipped word of Bladestorm
  (`ground,structure,debris,enemy,neutral` = `0x1810A`) contains debris, so the destructable-aware builder runs. Both builders
  install the SAME per-unit predicate for the word: `0x6F2AB890` (builder `0x6F2AE9E0`, flag `0x400`) and `0x6F26E510`
  (builder `0x6F2A3270`) are both `0x6F25D6F0(unit, word, source, -1) == 0` (section 13). Therefore the units hit are the
  ground units AND structures (kind bits ground/structure; air is refused), enemy or neutral relation, alive and
  vulnerable (the default for an unnamed group), any hero / organic / mechanical state. The per-candidate immunity gate and the
  ring membership test (`0x6F476C40`, collision-adjusted) are inside the collector `0x6F2C2E80` (not reproduced; the model keeps the
  plain circle). Damage class `0x20` = damage type ENHANCED (type 5). Rust: `blade_tick` uses `area_predicate` with
  `BLADESTORM_TARGS_DEFAULT` / `Game::set_orc_targs`.
* **Debris.** The word names `debris`, so destructable debris (trees are not named) in the ring is damaged by the same body
  (the destructable-aware builder collects them). The model has no destructable damage; NOT modelled.

## 20. S39: Earthquake (`AOeq`, `CAbilityEarthquake`, vtable `0x6F8A8154`; effect `CEffectEarthquake` vtable `0x6F88F86C`)

Order id 852121 (section 10). Data (shipped): `Dur` 25 (level 1) / 20, `HeroDur` 20, `Cool` 90, `Cost` 150, `Area` 250 / 400, `Rng` 1000,
`DataA` 0.5, `DataB` 50 / 40, `DataC` 0.75, `DataD` 250 / 400, buffs `BOeq,BOea`, effect `XOeq`, `targs`
`ground,structure,debris,tree`.

* **Cast (point slot `0x418` = `0x6F08AE40`, CONFIRMED by reading).** Stores the target x and y into agile cells of the
  ability (`+0x150`, `+0x158`), reads `DataA` (`0x6F02E9E0`) and starts the ability timer (message `0xD01C1`, cell `+0x13C`) for
  `DataA` seconds, and clears the caster's earlier buffs of three channel-bookkeeping types (as Big Bad Voodoo does). The duration
  cell `+0x90` of the ability is `Dur + ability+0x9C` (slot `0x42C` = `0x6F08ADE0`; no hero branch).
* **Effect start (`0x6F0C4420`, the handler of `0xD01C1`; Unicorn, `tools/emu/quake_emu.py`, 400 cases, 0 mismatches; real
  handler + initialiser `0x6F0C4230` on a fake ability with real level getters and recording cell setters;
  `tests/data/orc/quake.txt`, `tests/orc_quake.rs`).** The effect object takes: centre = the stored point (cells `+0x24`,
  `+0x2C`), **cell `+0x38` = `DataB`** (damage per tick), **cell `+0x40` = `Area`** (radius), **cell `+0x64` = `DataC`**,
  **tick counter `+0x34` = `F2I(Dur + ability+0x9C)`**; `DataD` is not read; cell `+0x48` is a presentation value derived from the
  centre. The initialiser ends by running the first tick at once. Rust: `orc::quake_plan`.
* **Tick `0x6F0C3780` (CONFIRMED by reading).** Counter -= 1; the units and destructables in the circle (destructable-aware
  builder `0x6F2AE9E0`, predicate `0x6F25D6F0` with the ability's `targs` word UNMODIFIED: it names no relation, so ALL
  relations are accepted and the caster's own buildings and allies are affected) run the callback `0x6F0B72C0`; then, while the
  counter is positive, the next tick is armed with 1.0 s (`0x6FAAE4F4`), else the effect destroys itself. So `ticks` ticks at
  `t0, t0 + 1, ...`.
* **Per candidate `0x6F0B72C0`.** The zero path as in War Stomp (unit slot `0x11C` or spell immunity `0x6F28D750`). A target whose
  `targType` (slot `0x114`) has bit 8 (structure): make sure it carries the marker buff `BOea` (`0x424F6561`, factory `0x6F0B4AC0`)
  and send a damage record of `DataB` (cell value `+0x3C` of the effect), class **`0x4000000`**, flags `0x40` (`0x50` on the zero
  path) to its damage slot. Any other unit (not on the zero path): the slow buff `BOeq` (`0x424F6571`) with the NEGATED `DataC`
  (`xor 0x80000000` of the cell value `+0x68`), duration 2.0 s (`[0x6FAAE504]`), created through `0x6F0B4B90` or refreshed through
  the buff's slot `0x358` when the unit already carries one.
* Model: `orc::earthquake` / `quake_tick` (timer kind `0x86`): structures take `DataB` per tick (damage type 26), other units get
  `Slow { move_delta: -DataC }` for 2.0 s each tick. UNSURE: how the slow's `-DataC` enters the unit's speed in the original (the
  buff's stat apply was not decoded; the model's slow machinery treats it as a delta, `0.75` then means 75 % slower), the debris and
  tree damage (destructables are not modelled), the channel end (interruption ends the effect in the original: shared channel
  code, other agent), the marker `BOea`.

## 21. S39: Far Sight (`AOfs`, `CAbilityFarSight`, vtable `0x6F88B25C`, a `CAbilityDetectAoe`) - cast arguments (Unicorn)

Order id 852122. Unicorn run `tools/emu/farsight_emu.py` (the real point slot `0x418` = `0x6F06FA50` on a fake ability; the
effect creator `0x6F06F8D0` recorded; 400 cases, 0 mismatches; `tests/data/orc/farsight.txt`, `tests/orc_farsight.rs`). The
unit-target slot `0x424` is an empty `ret 4`: Far Sight is a point ability only.

* The creator receives, as eight stack words: the effect id (the level's `EfctID` list entry through `0x6F02EE80`, `Xbdt`),
  pointers to the point's x and y, a pointer to the terrain height at the point (`0x6F24F3E0`, flag 1), the caster's owner
  index (`vf 0xEC`), a pointer to the **duration `Dur + ability+0x9C`** (class slot `0x430` = `0x6F072550`; **no `HeroDur`**: the
  old model used `HeroDur` for a hero caster, equal in the data), a pointer to `Area` (slot `0x42C`) and the integer
  `F2I(DataA)` (slot `0x434` = `0x6F031C60`; 3 in the data; its meaning inside the effect was not decoded).
* The effect object (`0x6F069210`, read) stores the owner, a timed-life style expiry message `0xD01C4` of `duration + 0.1` seconds
  (`0x6FAAE4A0` = 0.1), and creates an area watcher (`0x6F474720`, message `0xD01B4`) at the point; the visibility itself is applied
  by that watcher / effect through the vision system. Not decoded: the watcher's radius use and the per-cell visibility writes.
* Model: `orc::far_sight_plan`, and `far_sight` keeps a visible-state fog modifier for the plan's lifetime (marker buff on the
  caster; the original has no such buff). UNSURE: the cadence at which the original refreshes the revealed cells.

## 22. S39: Reincarnation (`AOre`, `CAbilityReincarnation`, vtable `0x6F9199C4`; effect `CEffectReincarnation` vtable `0x6F916934`) - reading only

Data: `Cast` 3, `Dur`/`HeroDur` 0, `Cool` 240, `Cost` empty, `DataA` 7, effect `XOre`. A passive: no order id (the cast getters at
`0x308`/`0x30C` exist for the order tables). Class slots: `0xCC` = `0x6F1F43E0` (0), `0x280` = `0x6F1F43F0` (2), `0x284` = `0x6F1E5E30`
(the death handler), `0x3E4` = `0x6F1D2F20` forwarder, `0x3E8` = `0x6F1F1C10` (the revival start).

* **Death handler `0x6F1E5E30` (CONFIRMED by reading).** Runs for the dying unit when ALL hold: the ability pause counter
  `+0x3C` is not positive; slot `0x268` (enabled) answers non-zero; the unit is not an illusion (`unit+0x5C & 0x40000000`) and has
  `unit+0x20` bit 0 clear; `0x6F1DB6E0(unit)` is zero; slot `0x370(0)` (ready: mana, cooldown) is non-zero and the ability's flag `0x200`
  is clear; and the death point passes the pathing probe `0x6F468F90(point, 2)` (non-zero blocks it). Then a positive `Cool` (slot
  `0x2EC`) starts the cooldown (slot `0x3A4`) and slot `0x3E8` starts the revival.
* **Revival start `0x6F1F1C10`.** Creates the effect object (factory `0x6F1D89B0`, class `CEffectReincarnation`), reads `DataA`
  (`0x6F02E9E0`) and stores it into the effect's agile cell `+0xB8` (`0x6F4773E0`), registers the effect with the unit (virtual
  `+0x78` of the effect with the unit, the ability owner id and `unit slot 0x130`), and sets the unit flag `unit+0x5C |= 0x1000`
  (`0x6F2964E0(unit)`).
* **Revival `0x6F1EBE30`** (run by the effect's handler `0x6F1EBF80` on message `0xD01C4`, the expiry of the effect): the unit
  must still resolve and have the dead bit `0x100`; it is moved to its own point (virtual `0x180`), revived by `0x6F2A64A0` with the
  flag word `0x500`, and its food use is corrected (`0x6F29BAF0` / `0x6F2834E0`). The life and mana the revive leaves were not decoded
  (the shared revive path `0x6F2A64A0`).
* **Model (UNSURE).** `Game::orc_on_unit_died` (called from `dispatch_unit_death_by`) starts the cooldown and a timer of `DataA`
  seconds (LIKELY the effect delay: DataA 7 matches the known 7-second delay of the hero ability), then `resurrect_unit` (full life at the
  death point). The pathing probe, the pause counter and the readiness slot are not modelled; the death events fire as for any death.

## 23. S39: leftovers - Mirror Image pass two, Wind Walk cancel, Chain Lightning damage timing, corrections

### 23.1 Mirror Image, pass two of the handler `0x6F13D990` (CONFIRMED by reading; corrects 7.1)

Pass one (per slot, ascending): `0x6F127E00` creates the unit (placement search + creator); a created unit's handle pair is copied
into the 12-byte array entry at `+0x14/+0x18` of the unit's agent record and the **buff** (`0x6F13A5A0` factory, init slot `0x324`, apply
slot `0x364` with the unit and the ability, add `0x6F079CA0`, `0x6F052BC0(buff, 1, 1)`) is put on EVERY created unit, the hero's
placeholder included. A failed creation leaves a 0 in the array.

Pass two (ascending, `h` = the hero slot):
* **Creation failed** (entry 0): if `i == h` the caster's state is restored by `0x6F1280D0(flag 1)` with no unit (so the hero stays where
  it stood) and the ability's bookkeeping cells `+0xE4/+0xE8` are set to -1; any other failed slot is simply skipped (fewer images).
* **Created and standing EXACTLY on the requested point** (soft compare of x, y, z against the slot's point): `0x6F1280D0(flag = i == h)`:
  for an image (`flag 0`) the unit's slot `0x80` runs (activate); for the hero slot (`flag 1`) the placeholder is removed (`0x6F2A63B0(unit, 1,
  0)`), the CASTER is moved to its point (virtual `0x180`) and the caster's state is undone: `0x6F2A3DA0`, unit flag `+0x20 &= ~0x800000`,
  `0x6F26FE20`, `0x6F0767F0(unit, 1, 0, 0, 0)` (releases the ability pause counters), `0x6F079730(unit, 1)` (decrements the invulnerability
  counter `+0xE8`; at 0 the unit flag 8 is cleared) and `0x6F2A4B80(unit, 1)` (a stop order, order id `0xD0004`).
* **Created but NOT on the requested point** (the placement search moved it): a separate path builds a record and calls the virtual `0xC8` of
  a factory object (`[0x6F48AB80 result + 0x54]`) with the unit, the flag `i == h` and the ability's value at `+0x180`; not decoded.
* **The caster during the delay** (the cast effect `0x6F135740`, read): invulnerable (counter `+0xE8`, unit flag 8), its abilities paused
  (pause counters by `0x6F0767F0`) and `+0x20 |= 0x800000`; all undone in the hero-slot branch above. Model: `mirror_image` / `mirror_fire` now
  give the caster one invulnerability count for the delay (`tests/orc_spells.rs`); the pause counters, the flag and the stop order are not modelled.

### 23.2 Wind Walk: the "cancel on cast" (S34 LIKELY) is NOT supported

The buff vtable slot `0x178` (`0x6F15A270`, six arguments, removal) is never called through a virtual call anywhere in the image (the
scans found only field loads at `+0x178` of other classes). The `0xD000F` broadcast `0x6F281580` (callers `0x6F0CD722`, `0x6F0D0CE5` only: the
attack) calls the arming slot `0x124` of every ability and buff of the attacker with the target; it does not run for spell casts. The
mechanism that ends Wind Walk when a spell is cast was not found; the first strike (section 11) is the only confirmed end. The Wind Walk
arming slot `0x6F171870` clears flag `0x200`, does nothing without a target or with buff flag `0x100`, and otherwise asks the filter
`0x6F26D590(owner, target, buff+0xB8)`: ZERO arms the strike (flag `0x200`, event 0xE), NON-ZERO removes the buff. The buff word `+0xB8` is
LIKELY the ability's `targs` (`air,ground,enemy,neutral`; the buff base constructor was not read), in which case an attack on an ally or other non-enemy removes the invisibility instead of arming it.

### 23.3 Chain Lightning: when the damage lands

The hop (section 4) copies the record into the bolt effect (`0x6F2BF7F0`: source and target smart pointers, amount cell `+0x44`, class and
flags words) and the effect start `0x6F1B09C0` creates the lightning object (`0x6F040160`), then registers the callback `0x6F2BF8B0` with the
lightning table (`0x6F00E0E0` stores the callback and the effect at `+0x84/+0x88` of the lightning entry). The callback `0x6F2BF8B0` is the
only place that reads the effect's source, target and record and calls the effect's virtual `+0x7C` with them (the damage delivery); WHEN the
lightning table invokes it was not decoded (LIKELY at the creation of the bolt, i.e. in the same tick as the hop; the 0.1 s repeating timer
`0xD01A3` only fades the art). The model keeps the instant damage.

### 23.4 Corrections to sections 17 and 20

The three buffs that the Voodoo effect `0x6F18C340` and the Earthquake cast `0x6F08AE40` remove from the caster (ids from `0x6F10F990`,
`0x6F07D0B0`, `0x6F159120`) are `Bvul` (invulnerable), `BHds` (divine shield) and `BOvd` (the voodoo buff itself); the call `0x6F077630(unit)`
then sets `unit+0x20 |= 0x40000000`.

### 23.5 Wind Walk: the invisibility transition (read)

`CBuffWindWalk` (`0x6F171630`) hands the effect's pointers to the invisibility base initialiser `0x6F123FC0(unit, duration, fade)`: it sets the
agile cell `+0xCC` of the buff to the FADE value (`DataA`, 0.6 s / 1.0 s: the time the unit needs to become invisible, a property that runs
over that time), applies the buff (slot `0x320`), registers it with the unit (`0x6F26F930(unit, buff, 1)`), sets the buff flag `0x100` and calls
`0x6F28D7D0(unit, fade)` (the unit's invisibility fade update). So the unit is not invisible during the first `DataA` seconds; the movement bonus
(section 11) applies from the start. The sim has no invisibility or detection state, so the transition is NOT modelled; the cell value is the
only decoded number.
