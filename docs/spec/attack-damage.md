# Attack damage in Warcraft III 1.26a Game.dll: spec for a clean-room port

Status: analysis from the decompile export only (chunk_*.c, natives.c, index.tsv). Game.dll is not present in this container, so no byte of the .data/.rdata tables (damage table, armor table, softfloat reciprocal table) was read, and nothing was run against the original. Every number that comes from Misc.txt is therefore UNSURE. Tags: CONFIRMED = read directly in the decompile; LIKELY = strong inference from names, call shape or consistency; UNSURE = open.

Addresses are 1.26a. "FUN_x" names are the decompiler's names.

## 0. Summary of the damage pipeline (plain English)

1. A unit's attack reads its weapon slot (0 or 1). It rolls a raw amount: a number of dice, each die with some sides, summed, plus a flat bonus. The dice draw from RNG stream 2 (combat).
2. The raw amount goes into a damage event: amount, a damage-type bitmask (1 << damage type), a damage category index (0 to 6), and the attacker.
3. The target applies its modifiers in this order: invulnerability check, then a handler pass (evasion first, where a return of 1 means the attack misses), then a second handler pass in priority categories 2 to 8 (crit and similar), then the armor factor (physical damage only), then the damage-type table lookup by category and target armor type, then the ethereal table, then a player-relation factor (UNSURE), and finally the HP change.
4. Armor is applied as a multiplier M = 1 - R, where R is computed by a software float helper with the Misc constant DefenseArmor (K). For armor >= 0, R = K*A / (1 + K*A). For armor < 0, R comes from a 21-entry table indexed by the truncated magnitude (cap 20).
5. Timing per weapon: cooldown, damage point and backswing are per-weapon float values in unit data. How they map to simulation time is UNSURE (see section 6).

## 1. Code map

| Item | Address | What it is | Tag |
|---|---|---|---|
| UnitDamageTarget native | 0x6F3C8850 | JASS native. Args: source, target, amount, attack flag, ranged flag, attack type (<8, else reject), damage type (<25, becomes bit 1<<dt), weapon type (<25). Calls FUN_6F2556E0 then the target's virtual slot 0x120 | CONFIRMED (checks), LIKELY (meaning) |
| UnitDamagePoint native | 0x6F3C8770 | Same checks; schedules the damage at the damage point via FUN_6F088050 or FUN_6F1D4440 depending on FUN_6F01FD60 | CONFIRMED (checks) |
| GetEventDamage native | 0x6F3C3C40 | Returns field +0x30 of the current event record | CONFIRMED |
| CAbilityAttack::GetDamageAmount | 0x6F0C6940 | Dice roll for the current weapon, then clamp to >= 0 | CONFIRMED |
| CAbilityAttack::Initialize | 0x6F0D1590 | Sets flag 0x2000 in the ability, reads a stream-2 real. Does no damage math | CONFIRMED (no math), LIKELY (stream 2) |
| Attack execute state machine | 0x6F0CF660 | Switch on the per-weapon phase at +0xDC + weapon*4. Calls the roll (around chunk_004 line 3168) | CONFIRMED (location), UNSURE (phase meanings) |
| Unit damage routine | 0x6F2A40D0 | Target-side pipeline (section 4). Named CUnit::Damage here | CONFIRMED (structure), LIKELY (name) |
| Armor factor | 0x6F26D270 | Computes R from DefenseArmor and the armor value | CONFIRMED (structure), LIKELY (meaning, section 5) |
| Damage-type table | 0x6F26D300 | Loads Misc DamageBonus* into a 7 x 8 table; also the ethereal table | CONFIRMED |
| Dice wrapper | 0x6F285350 | "Bogus request to roll 0 dice" message. Calls the sampler, then int-to-float and mul by 1.0 | CONFIRMED (body), UNSURE (argument binding, section 3) |
| Dice sampler | 0x6F26CAD0 | Sum of dice, or a normal approximation above 16 dice | CONFIRMED |
| Per-stream real | 0x6F2853D0 | Real in [0,1) from a chosen stream | CONFIRMED |
| RNG step | 0x6F011390 | One step on a stream state (8 bytes) | CONFIRMED |
| Evasion | 0x6F08BD20 | Returns 1 when evaded. Chance from ability field +0x50 (param word 0x14) | CONFIRMED (body) |
| Critical strike EvadeAttack | 0x6F1A34C0 | Crit chance check, stream 2 | LIKELY (body not read) |
| Unit-type loader | 0x6F263850 | Reads per-weapon keys (section 2) | CONFIRMED |
| Unit-type to runtime copy | 0x6F2A6750 | Copies weapon fields into the runtime unit | CONFIRMED |
| Damage tooltip | 0x6F353E10 | UI damage text. Not part of combat | CONFIRMED |

Software float helpers (all float32 bit patterns, truncation, not IEEE): add 0x6F6EF000, subtract 0x6F6EEEF0 (negate the second operand, then add), multiply 0x6F6EEE20, reciprocal 0x6F6EF100 (table-driven, 2 entries at 0x6F9774C8, linear interpolation on the top mantissa bits), integer to float 0x6F6EF350, float to int with truncation toward zero 0x6F6EEC20, and divide 0x6F6EFE10 (returns 1.0 when both operands are equal, else x times reciprocal(y)). The add, subtract and multiply helpers match the repo model in crates/sim/src/softfloat.rs. CONFIRMED for add, subtract, multiply, truncation. LIKELY for divide. The reciprocal needs its table bytes from Game.dll (UNSURE).

Float constants (read as the runtime values): DAT_0x6FAAE470 = 0.0 (LIKELY), DAT_0x6FAAE4F4 = 1.0 (LIKELY; matches rng.md section 3.3 and the literal 0x3F800000 in the divide helper), DAT_0x6FAAE464 = -1.0 (LIKELY, AGENTS 4.3f). The bit pattern 1.4e-45 (denormal, one mantissa bit) is used as a status flag (section 4).

## 2. Unit-type data per weapon (2 weapon slots)

The loader (0x6F263850) loops over slots 1 and 2 and reads these keys. Each key is formatted as name plus slot index. Stride per slot is 0xC bytes inside the unit-type record. Types are from the getter used (FUN_6F25F7A0 = real, FUN_6F25F640 = integer).

| Key (as read) | Type | Use | Tag |
|---|---|---|---|
| weapType | enum (type not read) | weapon type | CONFIRMED key |
| weapTp | string/enum | attack type (JASS attack type) | CONFIRMED key, LIKELY meaning |
| targs | flags | legal targets (decoded by S_AttackCanTarget per OpenRealm) | CONFIRMED key |
| splashTargs | flags | splash target filter | CONFIRMED key |
| atkType | enum | attack type used by the damage event | CONFIRMED key, LIKELY meaning |
| rangeN | real | attack range | CONFIRMED key |
| "cool" (unnamed literal) | real | attack cooldown in seconds | LIKELY (float type; name inferred from WC3 data convention) |
| dmgpt | real | damage point (seconds from start of attack) | CONFIRMED key, LIKELY units |
| backSw | real | backswing | CONFIRMED key, LIKELY units |
| RngBuff | real | range buffer | CONFIRMED key |
| unnamed literal (integer) | int | dice count (dmgnumb-like) | LIKELY (integer getter; name UNSURE) |
| sides | int | sides per die | CONFIRMED key |
| dmgplus | int | flat base damage | CONFIRMED key |
| dmgUp | int | per-upgrade damage bonus | CONFIRMED key |
| Farea, Harea, Qarea | real | splash radii (full, half, quarter) | CONFIRMED key |
| Hfact, Qfact | real | splash damage factors (half, quarter) | CONFIRMED key |
| targCount | int | max targets | CONFIRMED key |

Unit-level armor keys (type record): an unnamed literal at +0x2D8 (probably the base armor value "def", name UNSURE), "defUp" (armor per upgrade, +0x2E4, CONFIRMED key), "defType" (armor type, +0x2F0, CONFIRMED key). In the runtime unit, the armor value is read as float bits at +0xE0 and the armor type index at +0xE4 (CONFIRMED reads in 0x6F2A40D0 and 0x6F353E10; LIKELY that +0xE0 is the armor value).

Runtime copy (0x6F2A6750): per slot, dmgUp goes to slot word 0, dice count to word 2, sides to word 4, dmgplus to word 6, cooldown to word 0x1A, damage point to 0x1C, backswing to 0x1E, range buffer to 0x18, area and factor keys to 0x22 to 0x2A. The runtime slot base is not resolved (UNSURE).

## 3. Damage roll

### 3.1 Sampler (CONFIRMED structure, CONFIRMED arithmetic)
For count N <= 16: for each die, take one RNG step r of the chosen stream (32-bit output), add floor(r * sides / 2^32). Sides is the sampler's first argument; N is its second. The result is the sum over N dice of values in [0, sides-1]. One step per die, always.

For N > 16: no loop. It computes a mean from (sides-1)*N, takes a square root, makes a Gaussian draw on the same stream, rounds, and clamps to [0, (sides-1)*N]. Exact constants UNSURE.

### 3.2 Discrepancy to test (IMPORTANT)
- The repo spec rng.md section 3.4 says each die gives floor(...) + 1, and then "+ count", then "+ bonus". In the decompile of 0x6F26CAD0 I see only the floor values summed, with no +1 per die and no +count. The +1 may be applied elsewhere, or rng.md may be wrong. UNSURE which.
- OpenRealm says raw = base + sum(rand() % S + 1) per die, i.e. 1 to S. That is the same as rng.md's claim, but OpenRealm uses the wrong RNG, so only the value range matters.
- Decision needed by a differential test: run 0x6F26CAD0 under the Unicorn harness (tools/emu) for a known stream state and compare the sum with floor-only and floor+1 models.

### 3.3 Dice wrapper argument binding (UNSURE)
Call sites show four arguments in this order: count, sides, bonus, and a pointer to the float 1.0 (DAT_0x6FAAE4F4). Examples: 2d10 with bonus 0 (stream 23, FUN_6F081420, consistent with rng.md), 1 die of (hi-lo) plus offset lo (FUN_6F19F1D0), and count 1 with a bonus of -1 (chunk_006.c line 9819, enclosing function not identified). The callee's own declaration does not match these four arguments (decompiler artifact). Conclusion: the wrapper takes (count, sides, bonus) and multiplies the result by 1.0 at the end. Where the bonus is added is not visible in 0x6F285350 or 0x6F26CAD0. LIKELY it is added to the sum, but the position (before or after the float conversion) is UNSURE.

### 3.4 Attack roll for a unit (GetDamageAmount, 0x6F0C6940)
- The weapon index is this+0x2B8 (CONFIRMED).
- Dice count and sides come from the per-attack fields at this+0x88 and this+0x94, indexed by weapon (CONFIRMED read; LIKELY count=0x88 and sides=0x94, from argument order). Bonus is the sum of this+0xAC and this+0xA0 (CONFIRMED read, indexed by weapon; the two summed fields' meanings are UNSURE: likely dmgplus and dmgUp-based).
- Stream: 2 (stream number taken from rng.md section 6; the call that selects it is not visible in this decompile, so LIKELY).
- Result is clamped to >= 0 (CONFIRMED): if raw < 0 (0.0), the result is 0.0.
- There is no min-1 clamp in this function. OpenRealm's min 1 is NOT seen on this path (see section 7).

Writers of the fields at +0x88 to +0xB0 include 0x6F09D5F0, 0x6F09E9E0, 0x6F0A5C60 and 0x6F0A76F0. They are probably setters on the attack object, but this is UNSURE: several of these writes go through virtual objects, and the weapon-to-field mapping is not resolved.

## 4. Target-side pipeline (0x6F2A40D0)

Order as it appears in the code (CONFIRMED order; each step's meaning as noted):

1. If the target has flag 0x100 (dead or untargetable), the amount is set to 0 and the function returns. CONFIRMED.
2. A check on the damage type (bits 0x10, 0x20, 0x400000, 0x4000000 of the damage class) decides whether the attacker-side path runs. CONFIRMED (flags), UNSURE (meaning).
3. Virtual checks on the target (slot 0x11C, then counters at +0x15C and +0x158). CONFIRMED structure, UNSURE meaning (probably invulnerability and magic immunity).
4. Pass 1 over the target's handler list (vtable slot 0xF8 returns a category, slot 0xFC applies). A returned 1 applies immediately; any other nonzero value is recorded as a bit for pass 2. Evasion (0x6F08BD20) returns 1 on a miss. CONFIRMED structure, LIKELY meaning.
5. Pass 2 over categories 2 to 8, in order, on the same list (category 1 is never recorded, so it is only applied in pass 1). Each handler whose recorded category matches applies. The loop stops early if the amount becomes 0. CONFIRMED structure. Which abilities are in which category (e.g., crit, curse, bash) is UNSURE, though OpenRealm lists "crit, bash, orbs" as a gap.
6. The amount is clamped: if it is negative and the damage category (piVar3[8]) is nonzero, it becomes 0. A second clamp at 1.0 computes a "max with min" that only matters for negatives. CONFIRMED read, LIKELY purpose.
7. Armor factor, only when the armor-flag is clear (local_1c == 0.0) and the damage class is 0x10 (bit 4, LIKELY DAMAGE_TYPE_NORMAL, i.e., physical). The factor is M = 1 - R and the amount is multiplied by it (LIKELY, section 5). If the armor-flag is set, the armor column is 4 instead (see 8).
8. Damage-type table: the amount is multiplied by Table[category][column], where category = damage event +0x20 and column = target +0xE4 (armor type), or 4 if the status flag (1.4e-45) is set (CONFIRMED read; the flag's meaning is LIKELY "ethereal or magic-immune").
9. Ethereal: if target field +0x56 > 0 and the ethereal-table entry is not 1.0, the amount is multiplied by EtherealDamageBonus[k] (table slot k from category, with a non-obvious order: see section 7). CONFIRMED read.
10. A relation factor: if the attacker and target have a flag at +0x268 (player or team state), the amount is multiplied by the float at 0x6FAAE574 (UNSURE, probably an ally or self factor).
11. Final stage: the amount goes through FUN_6F29B730 and then into the target's virtual slot 0x128, with a negative sign (the value is stored with the sign bit flipped). Probably the HP reduction (LIKELY).

Multiplications use the softfloat multiply through FUN_6F255800, which writes the result to the event's amount (+0x10) and a second copy (+0x18). CONFIRMED structure.

Rounding: no float-to-int truncation is seen on the main path. The amount stays float32 through the softfloat helpers until the HP update. Whether the HP update truncates is UNSURE.

## 5. Armor factor (0x6F26D270)

Inputs: armor value A from target +0xE0 (float bits), K = Misc key "DefenseArmor" at index 0 (CONFIRMED key; value not in container, OpenRealm says 0.06, UNSURE).

Output R (stored in the status slot): R is the reduction fraction. The damage factor is M = 1 - R (from step 7 of section 4, which subtracts R from 1.0).

- A >= 0 (CONFIRMED branch, from the sign test): X = K * A (softfloat multiply), Y = 1 + X (softfloat add), R = X / Y (divide helper). So M = 1 - X/(1+X) = 1/(1+K*A). This matches OpenRealm's 1/(1+K*A). LIKELY.
- A < 0 (CONFIRMED branch): n = trunc(-A) (float-to-int truncation), clamped so that n = 20 for any n >= 20. R = Table[n] from a 21-entry float table at 0x6FAB4300. Tabled values are not in the container. If the table holds R = (1-K)^n - 1 (so M = 2 - (1-K)^n), this matches OpenRealm's 2 - (1-K)^(-A). LIKELY for the form, UNSURE for the stored numbers (the table has no writer in the decompile, so it is probably static data in Game.dll).
- If R > 1.0, the step-7 branch uses factor 0 (the code takes an else-path). For A >= 0 and the table form above, R < 1 always, so this does not trigger for normal armor.

Armor applies only to damage class bit 4 (0x10), and only when the status flag is clear. LIKELY: physical damage only.

## 6. Timing (PARTIAL)

- Per-weapon cooldown, damage point and backswing are float values (section 2), in seconds in the unit data (LIKELY).
- Attack execute (0x6F0CF660) is a state machine on the phase at +0xDC + weapon*4. Case 1 and case 2 (missile launch via FUN_6F0408B0) and case 3 (FUN_6F08D280, FUN_6F0C71C0, FUN_6F022190) are seen. The damage roll is inside this function (around line 3168 of chunk_004.c). CONFIRMED location.
- The damage point is the moment the target is hit (from the state machine, LIKELY). The backswing is the delay after that. The cooldown is the total swing time.
- Mapping to simulation ticks: UNSURE. The game-loop spec (specs/game-loop.md) says the unit of one step is not known ("1 ms or one turn: UNSURE"), and nothing in the decompile converts a JASS real (seconds) to steps. So any seconds-to-ticks rule must be found before the timing can be ported exactly.

## 7. Comparison with OpenRealm (docs/research/openrealm/04-gameplay.md)

Verified against 1.26a unless noted.

| Topic | OpenRealm says | 1.26a (this analysis) | Verdict |
|---|---|---|---|
| Dice | base + sum(rand() % S + 1) | sum over dice of floor(r*S/2^32) (0-based), one stream step per die, stream 2. A +1 per die is in rng.md and not seen in the sampler | DIFFERENT (RNG differs for sure; the 0-based or 1-based question is open, section 3.2) |
| Attack-type table rows | normal, pierce, siege, spells, chaos, magic, hero (their order) | Misc keys DamageBonus{Spells, Normal, Pierce, Siege, Magic, Chaos, Hero}, 7 rows, binary row order Spells(0), Normal(1), Pierce(2), Siege(3), Magic(4), Chaos(5), Hero(6) | ORDER DIFFERS, use the binary order. Values UNVERIFIED |
| Defense columns | small, medium, large, fort, normal, hero, divine, none | 8 columns (index 0 to 7); names not in container | COUNT AGREES, names UNVERIFIED |
| Armor, A >= 0 | 1/(1+K*A) | 1 - K*A/(1+K*A) = 1/(1+K*A), via softfloat | AGREES (LIKELY) |
| Armor, A < 0 | 2 - (1-K)^(-A) | Table of 21 values, index min(trunc(-A), 20); form LIKELY (1-K)^n based | AGREES in form; table bytes UNSURE |
| Armor constant K | 0.06 (Misc.DefenseArmor) | Key "DefenseArmor", index 0 | Key AGREES; value UNVERIFIED |
| Armor scope | applied to all damage | applied only to damage class bit 4 (0x10) when status flag is clear | DIFFERENT (LIKELY) |
| Minimum damage | min 1 when base > 0 | No min-1 seen. Clamp to 0 only (and a 1.0-related max on negatives) | DIFFERENT (LIKELY, check) |
| Rounding | (int) cast | No float-to-int on the main path; float32 through softfloat. HP update rounding UNSURE | DIFFERENT (UNSURE) |
| Ethereal | physical returns 0; magic x1.66 | Table EtherealDamageBonus[k] from Misc, not hardcoded. Index map is irregular: key 0 goes to slot 1, key 1 to slot 2, key 2 to slot 3, key 3 to slot 4, key 4 to slot 5, key 5 to slot 0, key 6 to slot 6 (CONFIRMED writes). Lookup key UNSURE | DIFFERENT (mechanism) |
| Hit order | Evasion, then Curse miss, damage bonuses, then Bash | Evasion (pass 1), then pass 2 categories 1 to 8, then armor, then type table, then ethereal, then relation factor, then HP | PARTLY AGREES (evasion first). Full order UNSURE |
| Recovery | max(cooldown - damagePoint, backswing) | Not verified (section 6) | UNVERIFIED |
| Hero STR and AGI | attack bonus 1.0 per point; agility armor 0.3 | Not in this path (hero code not read) | UNVERIFIED |

OpenRealm notes that they used process rand(). That is confirmed wrong for combat (rng.md section 5; stream 2).

## 8. Open items (ordered by value)

1. Differential test of the sampler (0x6F26CAD0) against the Unicorn harness: floor-only versus floor+1 (section 3.2). This decides the roll range.
2. Extract the Game.dll data: the damage-type table (7 x 8 floats), the ethereal table (7 floats), the negative armor table (21 floats), the reciprocal table at 0x6F9774C8, and the float constants at 0x6FAAE470, 0x6FAAE4F4, 0x6FAAE464, 0x6FAAE574. Needs the file, not in this container.
3. Confirm the Misc values by reading a Misc.txt from the user's MPQ (DefenseArmor, DamageBonus*, EtherealDamageBonus, StrAttackBonus, AgiDefenseBonus). Do not trust the OpenRealm stock values.
4. Confirm the wrapper's argument binding (count, sides, bonus) and where the bonus is added (section 3.3).
5. Identify the handler categories (which abilities return which category) and the order in pass 2 (section 4 step 5). Read CAbilityCriticalStrike::EvadeAttack (0x6F1A34C0) and the curse and bash bodies.
6. Resolve the damage class bits (0x10, 0x20, 0x400000, 0x4000000) and the status flag (1.4e-45) to damage type names (JASS constants). Needs the JASS constant table from common.j (not in container).
7. Timing: find the seconds-to-steps conversion (section 6). Read the attack execute state machine (0x6F0CF660) fully.
8. Decide the armor-table storage form (R or M) by reading the bytes at 0x6FAB4300 once the DLL is available.

## 9. What is NOT in this report

- No decompiled code is pasted here; the spec uses the addresses above.
- No claim about hero attributes, splash math, projectiles (CMissile* DealDamage), or the miss chance (CMissile::ChanceToMiss, stream 2 per rng.md) beyond the names. Those are separate specs.

## Addendum (lead, 2026-10-10): dice sampler resolved by emulation
`tools/emu/test_sampler.py` calls the original 0x6F26CAD0 (ecx = sides, edx = count, stack = stream
state) for 300 random cases (count 1..16, sides 1..100) and compares with the model
`sum over dice of floor(step() * sides / 2^32)`: **0 mismatches**. So each die is 0..sides-1 inside
the sampler; if the game's damage uses 1..sides per die, the +count must be added by the caller
(still to locate in 0x6F0C6940). The >16-dice Gaussian path is not covered by this test.

## Implementation status (Haiku, 2026-10-11): `crates/sim/src/damage.rs`

Implemented as `sim::damage::SpecDamage` (a `DamageModel`; install with `Game::set_damage_model`). Not the default:
the default stays `PlaceholderDamage` because the model needs Misc values (`DefenseArmor`, the 7 x 8 table) that the
engine does not load yet.

| Step | Implementation | Verified |
|---|---|---|
| Dice | `dice_sum`: one stream-2 step per die, `floor(r * sides / 2^32)`, plus the flat bonus (added to the integer sum), clamped at 0, `i2r` | Sampler per die: original, by the addendum above. Synthetic tests for step count and range only |
| Dice > 16 | Not modelled: the model returns the weapon's placeholder value (`AttackType::damage`) | No |
| Armor A >= 0 | `M = 1 - div(K A, 1 + K A)` (softfloat) | Formula from the spec. Division needs `Tables`; only a GAME_DLL-gated check (the stock K = 0.06 is a test input) |
| Armor A < 0 | `R = table[min(trunc(-A), 20)]`, `M = 1 - R` | Synthetic table. Table contents not verified (see below) |
| Attack x armor type | `DamageTypeTable[attack_type][armor_type]`, clamped indices | Structure only |
| `R > 1` | factor 0 | Spec wording only |
| Physical only | `AttackType::physical` gates the armor factor | LIKELY (spec 4 step 7) |
| Negative table load | `NegativeArmorTable::from_game_dll`, VA `0x6FAB4300`, refused if all zero or non-finite | Reader tested on a synthetic PE. Not tested on a real file |
| Defense per unit type | `Game::set_unit_defense`, `Game::unit_defense` | Not tested beyond compilation |

Deviations from the task brief, with reasons:
- The 7 x 8 damage-type table is **not** read from `Game.dll`. The spec (0x6F26D300) builds it from the Misc
  `DamageBonus*` keys at run time, so the host passes it in as `DamageMisc`.
- Each die is `0..sides-1`, as the sampler gives. The +1 per die is **not** added: the addendum says it must come from
  the caller, and the caller (0x6F0C6940) is not located. This is the biggest open question for the damage values.
- The bonus position (integer sum versus after conversion) is a choice; the spec leaves it open.
- The ethereal table is not implemented: its lookup key is UNSURE (section 7). Evasion, the other handler categories,
  the relation factor and the miss/critical rolls are not implemented either.

Not verified against the original: no `Game.dll` was available in this environment, so no Unicorn vectors were
generated. The GAME_DLL-gated test in `crates/sim/tests/damage.rs` only checks the positive-armor formula on the real
tables and skips without the variable. The 0x6FAB4300 table may be all zeros in the file (the AGENTS notes say it is
filled at run time); in that case `from_game_dll` refuses it, and the model cannot be built from the DLL alone.
Next step: dump the bytes at 0x6FAB4300 and 0x6FAB4358 with `tools/emu/runtime_consts.py` on a real `Game.dll`, and
run the dice (+1 question) and armor factor through the Unicorn harness.
