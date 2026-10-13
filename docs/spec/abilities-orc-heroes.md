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
