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

### 3.2 The +1 per die: RESOLVED (CONFIRMED, 2026-10-11)
- The sampler `0x6F26CAD0` gives `0..sides-1` per die (CONFIRMED, Unicorn). The **+1 per die is added by the roll wrapper** `0x6F285350`: its code is `sum = sampler(...); eax = sum + count; eax += bonus; i2r; mul by 1.0`. So the roll is `count + sum(floor(r*sides/2^32)) + bonus` = one die is `1..sides`, plus the bonus, plus nothing else (CONFIRMED by disassembly).
- Verified in Unicorn: `tools/emu/test_damage_roll.py`, 400/400 wrapper cases (count -1..16, sides 1..100, bonus -30..60; value and stream state) and 400/400 `GetDamageAmount` cases (stream 2, clamp at zero). Random seeds, all 45 streams set.
- rng.md section 3.4 (+1 per die, then +count, then +bonus) is therefore **right in value**, but the "+count" and the "+1" are the same term: the wrapper adds `count` once, which is one per die.
- Count <= 0 path (CONFIRMED): the wrapper stores `0.0` and draws nothing; the flat bonus is dropped too. The caller `GetDamageAmount` then clamps at zero.
- OpenRealm's `rand() % S + 1` (1..S per die) is the same range (its RNG differs; only the range matters).

### 3.3 Roll wrapper `0x6F285350` (CONFIRMED by disassembly and Unicorn)
- Signature: ECX = result pointer, EDX = RNG stream index, stack = `count`, `sides`, `bonus`, pointer to the real multiplier (`ret 0x10`). Returns EAX = result pointer.
- Body: stream table lookup (`0x6F4C34D0(13)` -> stream base, entry `base + 8*stream + 4`); `count <= 0` gives `0.0` (plus a log call on a non-draw path); otherwise `sampler(ECX=sides, EDX=count, stack=stream)`, then `total = sum + count + bonus` (int32), `I2R(total)`, then `MUL(multiplier, I2R)`.
- The multiplier is `1.0` (`DAT_0x6FAAE4F4`) in the two callers checked (GetDamageAmount and the others below). Its value is 1.0 only after start-up (zero in the file; written by the start-up code, see `tools/emu/runtime_consts.py`).
- Callers: 18 call sites of the wrapper (`0x6F0814B4`, `0x6F088524`, `0x6F0C698F` [GetDamageAmount], `0x6F12DC18`, `0x6F13DA70`, `0x6F15482E`, `0x6F19F235`, `0x6F19F2B6`, `0x6F1ED4EA`, `0x6F1ED569`, `0x6F29DDA8`, `0x6F29DE4B`, `0x6F2C4A89`, `0x6F2C4ADA`, `0x6F2DEA08`, `0x6F2DEAA0`, `0x6F2E5C05`, `0x6F2F00A2`), with streams 0x17, 0x18, 0x0C, 0x11, 0x13, 0x1F, 0x2C, 0x19, 0x1A, 0x0D, 0x00 and 0x02 (the combat stream). The wrapper is the general "roll N dice d(S) + B" of the engine. Which of these are the other gameplay rolls is not mapped (UNSURE beyond GetDamageAmount).

### 3.4 Attack roll for a unit (GetDamageAmount, 0x6F0C6940)
- The weapon index is this+0x2B8 (CONFIRMED).
- Dice count is this+0x88[w], sides this+0x94[w] (CONFIRMED by argument order in the call: the first stack argument is the count, the second the sides). Bonus is this+0xA0[w] + this+0xAC[w] (CONFIRMED read, the sum is passed as the third argument). The meanings of the two summed fields are UNSURE (likely dmgplus and a per-level/upgrade term).
- Stream: 2 (CONFIRMED: `EDX = 2` in the call to the roll wrapper). Multiplier: pointer to 1.0 (CONFIRMED).
- The result of the wrapper is then clamped: positive or zero kept, negative becomes `0.0` (CONFIRMED by the `fcompp`/`fnstsw` sequence).
- Verified in Unicorn (`tools/emu/test_damage_roll.py`): 400/400 cases with random counts (-1..16, so the count-0 path is included), sides 1..100 and bonuses; the result and the stream state are compared with the model.
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
- A < 0 (CONFIRMED branch): n = trunc(-A) (float-to-int truncation), clamped so that n = 20 for any n >= 20. R = Table[n] from a 21-entry float table at 0x6FAB4300 (values in section 5.1: R_n = -(1 - 0.94^n), so M = 1 - R = 2 - 0.94^n, the OpenRealm form with K = 0.06; see 5.1). The table is in BSS and written by a start-up initialiser, not stored in the file.
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

## 5.1 Negative-armor table: start-up initialiser (resolved 2026-10-11)

- **Where**: the 21 entries at `0x6FAB4300..0x6FAB4350` (4 bytes each) and the sentinel `-99999.0` at `0x6FAB4354` are in .bss. The writer is the function `0x6F85A4C0` (a static initialiser, called by the CRT start-up ranges). Its 21 blocks each store one entry. The sentinel is written by a separate stub at `0x6F85A820` (`mov edx, -99999; mov ecx, 0x6FAB4354; jmp I2R`). Found by scanning all sections for dwords in `0x6FAB4300..0x6FAB4358`: the only references are in `.text` (CONFIRMED). The references around `0x6F26D2xx` (inside the armor-factor code, `0x6F26D270`) are the readers (LIKELY); the 7 x 8 damage table starts at `0x6FAB4358`, right after the sentinel.
- **Arithmetic** (CONFIRMED by disassembly; each block is the same): the initialiser calls I2R on the integer `100000` (the scale), I2R on an integer constant `k_n` (an immediate in the code, `k_0 = 0`, negative for n >= 1), and then the softfloat DIV of `I2R(k_n)` by `I2R(100000)` (both operands are pointers, the result is stored at the entry). The 21 integers are all immediates in the code (CONFIRMED by extracting them; the values are not recorded here, see `tools/emu/neg_armor_vectors.py`).
- **Meaning of k_n** (CONFIRMED for the 21 values): `k_n = -trunc(100000 * (1 - 0.94^n))`. The stored entry is therefore `DIV(I2R(k_n), I2R(100000))`, which is about `-(1 - 0.94^n)` with the DIV's own rounding (not correctly rounded: the stored bits differ from the correctly rounded quotient in the last place for several entries; `tools/emu/neg_armor_vectors.py` gives the bits).
- **Float ops**: all softfloat (I2R, DIV); no x87 instruction is used in the initialiser. The DIV is the table-driven RECIP plus MUL described in section 3.3 of `docs/spec/softfloat.md`.
- **Store points**: each entry is written as the DIV result (a 32-bit store into the entry). The sentinel is stored by I2R directly.
- **Port**: `sim::damage::NegativeArmorTable::computed(&Tables)`. `k_n` is computed as `-trunc(100000 * (1 - p))`, `p` being `0.94^n` by repeated `f64` multiplication (the trunc margins are at least 0.0166, so `f64` rounding cannot change an integer; the 21 integers were checked against the original). The DIV uses the reciprocal table of the DLL (`Tables::from_game_dll`).
- **Verification**: `tools/emu/neg_armor_vectors.py` runs the initialisers in Unicorn and reads the 22 dwords back. `crates/sim/tests/damage.rs::computed_table_matches_the_original_start_up_values` compares all 21 entries and the sentinel bit for bit (22/22 with `GAME_DLL` and `NEG_ARMOR_VECTORS`). The values are not committed.
- **Confidence**: CONFIRMED for the 22 stored values, the initialiser's structure and the DIV arithmetic; LIKELY that the source expression is `-(1 - 0.94^n)` (the integers match it exactly, which is the only evidence of the 0.94 rate).
- Open: whether the initialiser in the original runs once per process only (it does, as a CRT initialiser; not checked for re-entry); the sentinel is not used by the model (the lookup clamps to n = 20).

## 9.1 Status update (2026-10-11): sections 3.2 and 5.1 resolved

The dice question of section 3.2 and the negative-armor table of section 5.1 are closed. The "Implementation status" table below is partly stale: the `+1 per die` line and the `from_game_dll` load row are superseded by sections 3.2 and 5.1 (`NegativeArmorTable::computed`, `roll_total`).

## 10. Misc tables, name readers, physical and ethereal (2026-10-11, CONFIRMED unless marked)

This section supersedes the UNSURE items of sections 2, 4, 5 and 7 that it covers. Evidence: decompile export
(`chunk_012.c`, `chunk_013.c`, `chunk_020.c`, `chunk_027.c`) checked against the disassembly of the same functions.
Plain words; no code from the binary is reproduced.

### 10.1 The Misc reader `0x6F009EE0` (section, key, element)

- Calling form (CONFIRMED): ECX = output dword address, EDX = section name (`"Misc"` at `0x6F875DE0`), stack = key
  pointer, then element index. The result is written as one dword (an f32 bit pattern), `ret 8`.
- Layers (CONFIRMED structure): the reader reaches a per-process object (global, then `+0x10`, then `+0x4C`, created by
  `0x6F002790`). That object holds two profile layers at `+4` and `+8` (class `CMiscCustom`). The lookup tries `+4`,
  then `+8`; the first layer that has the key supplies the value (CONFIRMED order). Which file is `+4` (the map's
  `war3mapMisc` or the game's `MiscData`) is LIKELY, not checked; `gamedata::MiscData` gives the later layer priority,
  which agrees only if the map layer is `+4` (UNSURE).
- Element lookup (CONFIRMED): `0x6F4D1EB0` walks `index` links of a value list from its head (link at `+0x1C`, value
  at `+0x18`). So element `i` is the i-th comma item. An index past the end gives no value.
- Parse (CONFIRMED call): the value text goes through the S2R real parser `0x6F6F0520` (`0x6F4D2040` calls it), the same
  routine as `softfloat::text::s2r`.
- Default (CONFIRMED): every "no value" path returns `0.0` from `DAT_6FAAE470` (= 0.0, see
  `tools/emu/runtime_consts.py`). So an absent key, a short list, and a missing layer all read as `0.0`. Consequences:
  - `DefenseArmor` absent gives K = 0.0 (not 0.06). The armor factor reads `DefenseArmor` (element 0) on every damage
    call (`0x6F26D270` calls the reader each time; LIKELY no cache).
  - A missing `DamageBonus*` cell is 0.0 (not 1.0). `gamedata::misc` uses this now.
- Malformed text (e.g. `abc`): not traced (S2R on a non-number). Our code gives `0.0` with a note (UNSURE).

### 10.2 The damage-type table builder `0x6F26D300` (CONFIRMED, run in Unicorn)

- One-time build (CONFIRMED): a flag at `0x6FA70E90` gates the whole build. When it is non-zero the function reads all
  Misc values and then clears the flag. Later calls only look up. Where the first call happens in the game's start-up
  is UNSURE; a map's Misc layer loaded later than that call would not be seen.
- Rows (CONFIRMED): row = attack type, in the order Spells (`DamageBonusSpells`, row 0), Normal (1), Pierce (2),
  Siege (3), Magic (4), Chaos (5), Hero (6). Columns = element `0..7` of the same key, in list order.
- Storage (CONFIRMED): row-major, 8 dwords per row, base `0x6FAB4358` (cell = base + 0x20·row + 4·col).
- Ethereal (CONFIRMED, store addresses): element 0 goes to `0x6FAB42E8`, 1 to `0x6FAB42EC`, 2 to `0x6FAB42F0`,
  3 to `0x6FAB42F4`, 4 to `0x6FAB42F8`, 5 to `0x6FAB42E4`, 6 to `0x6FAB42FC`. As a function of the attack row `a`
  (slot `0x6FAB42E4 + 4a`): row 0 = element 5, row 1 = element 0, row 2 = element 1, row 3 = element 2, row 4 =
  element 3, row 5 = element 4, row 6 = element 6.
- Lookup (CONFIRMED): the function is also the lookup. ECX = output, EDX = row, stack = column; it returns
  `table[row·8 + column]` (CONFIRMED by the Unicorn lookup check). In the target path the column is the target's
  armor type and the row is the attack type (LIKELY: the decompile hides the EDX setup of that call).
- Check: `tools/emu/damage_table_emu.py` hooks the reader with synthetic Misc values and runs the builder in Unicorn:
  63 reader calls (7·8 + 7), 56 table cells, 7 ethereal slots, the flag, a second call that reads nothing, and 56 lookups:
  **122 checks, 0 mismatches**. The reader itself is replaced by the hook (its layer and list code is section 10.1).

### 10.3 Attack-type names (`0x6F262B00`, CONFIRMED by decompile; Unicorn not run)

Compare is case-insensitive (`_strnicmp`-style ordinal with a length of 0x7FFFFFFF, LIKELY; the data's capitalised
names only match this way). Result:

| Text (any case) | Attack type | Index |
|---|---|---|
| empty string | Spells | 0 |
| `unknown` | Spells | 0 |
| `spells` | Spells | 0 |
| `normal` | Normal | 1 |
| `pierce` | Pierce | 2 |
| `siege` | Siege | 3 |
| `magic` | Magic | 4 |
| `chaos` | Chaos | 5 |
| `hero` | Hero | 6 |
| anything else (for example `melee`) | Spells | 0 |

The lowercase name strings are at `0x6F92E104`..`0x6F92E12C` (`chaos`, `magic`, `siege`, `pierce`, `normal`, `spells`),
`hero` is at `0x6F92CA94`. Column names are `atkType` plus the weapon number (CONFIRMED string `atkType`; the digit is
added by the loader, LIKELY `atkType1` for weapon 1).

### 10.4 Armor-type names (`0x6F262C10`, CONFIRMED by decompile)

| Text (any case) | Armor type | Column |
|---|---|---|
| `small` | Small | 0 |
| `medium` | Medium | 1 |
| `large` | Large | 2 |
| `fort` | Fort | 3 |
| `normal` | Normal | 4 |
| `hero` | Hero | 5 |
| `divine` | Divine | 6 |
| `none` | Unarmored | 7 |
| `unknown`, empty, anything else | Normal | 4 |

Strings: `small` / `medium` / `large` / `fort` / `divine` / `unknown` at `0x6F92E15C`..`0x6F92E13C`, `none` at `0x6F92CAE0`.

### 10.5 How numbers in unit data are read (PARTLY UNSURE)

- Misc reals: S2R (section 10.1, CONFIRMED).
- Unit SLK reals (`cool1`, `dmgpt1`, `backSw1`, `rangeN1`, `def`) go through the generic value getter `0x6F25F6F0`,
  then `0x6F25B2F0` and `0x6F70C4F0`, with a fallback `0x6F7084F0`. That chain was not traced to the string-to-float
  routine. The gamedata reader uses S2R for them (the same text rules as Misc). UNSURE until traced.
- Missing unit cell (real or int): not traced; gamedata uses 0.

### 10.6 Physical armor (CONFIRMED compare, UNSURE mapping)

- The target-side path (`0x6F2A40D0`) applies the armor factor only when the damage class word (attack record
  `+0x14`) equals `0x10` exactly, and only when the status value from `0x6F26DE50` (bit 57) is zero. This is an
  equality compare, not a bit test (CONFIRMED). The damage-type table step (10.2) runs regardless of the class.
- How a weapon's `atkType1` sets that class word: not traced (writers of `+0x14` not found). The gamedata
  `DamageType::is_physical` set (Normal, Pierce, Siege, Hero) is therefore a heuristic (UNSURE).

### 10.7 Ethereal (structure CONFIRMED, rule UNSURE)

- Slot = the attack record's type field `+0x20` (same value as the table row, CONFIRMED in the decompile: `local_8`
  feeds both the row and the slot index).
- Applied only when three conditions hold (CONFIRMED structure): (a) the attacker is null, or the flag from
  `0x6F009F90(0)` is set, or `0x6F285B00(target)` is non-zero; (b) the target's `+0x158` counter is positive; (c) the
  slot is not `1.0` (`0x6FAAE4F4`). Then the amount is multiplied by the slot value through `0x6F255800`. The meanings
  of (a) and (b) are UNSURE.
- `sim::damage::SpecDamage::ethereal_factor` returns the slot for a row; the roll does not apply it yet.

### 10.8 Summary of changes made in code (2026-10-11)

- `gamedata::misc`: reader default 0.0 for every missing value (was 1.0 and 0.06); lists read by element index (short
  list = zeros, long list cut at 8); ethereal elements mapped to attack rows as in 10.2.
- `gamedata::combat`: attack-type names per 10.3 (empty or unknown = Spells); armor names per 10.4 (`unknown` = Normal).
- `sim::damage`: `DamageMisc.ethereal_by_attack_type`, `SpecDamage::ethereal_factor`.
- Tests: `crates/gamedata/tests/{misc,combat}.rs`, `crates/sim/tests/damage.rs`, `tools/emu/damage_table_emu.py`.

## 11. Target side of one damage event: class word, ethereal, status, order (2026-10-11)

Plain words; addresses are 1.26a. Tags as before. Verified by running the original routine `0x6F2A40D0` in Unicorn
(`tools/emu/target_damage_emu.py`) against `sim::damage::target_side_amount`: **15,000 random cases (3,000 with seed
20261011, 12,000 with seed 7; 7,579 of them with damage), 0 mismatches**. The routine runs with its real armor factor
(`0x6F26D270`), damage-type lookup (`0x6F26D300`), softfloat helpers, status test (`0x6F26DE50`) and record constructor
(`0x6F2556E0`). Stubbed: the target's ability lists (not walked, see 11.8), the HP change (`vf 0x128`), the invulnerability
answer (`vf 0x11C`), and the Misc keys and player objects the routine reads (set per case).

### 11.1 Damage class word (`+0x14`) of a basic attack: literal 0x10 (CONFIRMED)

- `CAbilityAttack` execute `0x6F0CF660` builds the attack record at `0x6F0CF803`..`0x6F0CF834`. It calls `GetDamageAmount`
  (`0x6F0C6940`, the amount goes to a buffer), then the record constructor `0x6F2556E0` (`ret 0x14`) with class argument
  **the literal `0x10`** (`push 0x10` at `0x6F0CF81E`). The weapon's attack type is not an input of the class.
- Record layout written by the constructor (CONFIRMED by disassembly): `+0x00` source unit, `+0x04` weapon field
  `unit+0xE8+4w` (name UNSURE; the target never reads it), `+0x0C` flags (`0x100`, or `0x180` when the execute's flag
  argument is set), `+0x10` amount, `+0x14` class, `+0x18` `0.0`, `+0x1C` 0, `+0x20` 0.
- Native `UnitDamageTarget` (`0x6F3C8850`): class = `1 << damage type` (`shl eax, cl` at `0x6F3C88B5`, damage type below
  25); flags `0x100` (attack argument) and `1` (ranged argument). The attack-type argument (arg 6) is stored only in a
  local (`uStack_4`, `[esp+0x38]` at `0x6F3C88CF`) and is **not** passed to the target (CONFIRMED by disassembly).
- So every basic attack has class `0x10` (the armor test passes) whatever its attack type. For natives the armor test
  `== 0x10` is damage type 4 (LIKELY that this is DAMAGE_TYPE_NORMAL in common.j; not checked against common.j).
- Code: `AttackType::damage_class` = `sim::damage::ATTACK_DAMAGE_CLASS` (0x10). The gamedata heuristic
  `DamageType::is_physical` (Normal, Pierce, Siege, Hero) is removed; it had no basis in the binary.

### 11.2 Damage-type row and ethereal slot (`+0x20`): NOT RESOLVED (UNSURE, the most important open item)

- The target reads `+0x20` as the table row (`0x6F26D300`, EDX) and as the ethereal slot index (`0x6FAB42E4 + 4k`).
- The constructor writes 0 there. The execute body writes nothing to the record's `+0x20` (the record is the stack slot
  `apiStack_118`; its direct writes are the constructor's). Callbacks run between the constructor and the target call:
  `0x6F2763A0` (slots `0x130`/`0x134` for categories `0x0B..0x19`) and `0x6F2764C0` (slot `0x154`). Their writes were not
  found. The native drops its attack type (11.1).
- Consequence: for a basic attack the row written by this path is 0 (Spells) unless a callback writes it. The model keeps
  the weapon's attack type as the row (`weapon.damage_type`). That is UNVERIFIED and may be wrong. Needs a runtime
  breakpoint on `0x6F2A40D0` (the value of `[R+0x20]` for a normal attack) or the writer.

### 11.3 Ethereal rule (structure CONFIRMED, meanings LIKELY)

Block `0x6F2A44F9`..`0x6F2A4548`, after the armor and table steps. The amount is multiplied by the ethereal slot when all
three hold:

- (a) **source condition**: the record's source is null, OR the Misc flag `EtherealDamageBonusAlly` (key string at
  `0x6F88E420`, read by the boolean reader `0x6F009F90`, element 0) is nonzero, OR `0x6F285B00(source, target)` is nonzero.
  That function is `not (the source owner's alliance mask has the target's player id)` (`0x6F3A3400`, mask `0x6F473170`).
  So (a) reads "source is an enemy of the target (not allied)" (LIKELY).
- (b) **target `+0x158` counter > 0** (signed compare, `cmp [ebp+0x158], 0; jle`).
- (c) **slot is not 1.0**: slot `k` = `0x6FAB42E4 + 4k`, with `k` = the record's `+0x20` row (see 11.2).

The multiply is softfloat `mul(amount, slot)`. `0x6F255800` writes the product to `+0x10` and the second copy to `+0x18`
(`+0x18` times the slot, which stays 0 for basic attacks; CONFIRMED structure).

### 11.4 Status flag (`1.4e-45`) (CONFIRMED)

- `0x6F26DE50(T, 1)` returns 1 when `T+0x5C` has bit 16 and (bit 7 of the **low byte** is clear OR bit 27 is set). The sign
  test is `test al, al; jns` (`0x6F26DE61`): it reads bit 7, not bit 31 (the earlier note about bit 57 was wrong). The caller
  ANDs the result with bit 25 (`0x2000000`, `test edx, 0x2000000` at `0x6F2A40FE`). The int 1 is stored and read as the
  float `1.4e-45`.
- Effects: the armor step is skipped, and the damage-type column is 4 (not the target's armor type).
- Code: `sim::damage::status_flag`.

### 11.5 Order of the target side (CONFIRMED by disassembly and Unicorn)

1. `T+0x5C & 0x100` (dead or untargetable): output 0.
2. **Immunity gate.** Misc flag `MagicImmunesResistDamage` (key `0x6F88E2CC`, element 0) selects the test:
   - flag off: gate = class is not one of `0x10, 0x20, 0x4000000, 0x400000`; the second test is off;
   - flag on: gate = (row == 4) or `class & 0x033EE700`; second = (row in 1, 2, 3, 5, 6) or `class & 0x00C11830`. The two
     masks are the start-up values of `0x6FA6F2CC` and `0x6FA6F2D0` (meaning UNSURE).
   - If gate: output 0 when `T.vf 0x11C()` (invulnerable) is nonzero, or `T+0x15C > 0`.
   - Else if second: output 0 when `T+0x158 > 0`.
3. Record flags: `+0x0C & 0x4` or `& 0x10`: output 0. (The other flags only drive side effects in the target.)
4. **Minimum-one value** `m = min(c, 1.0)`, where `c = raw`, except `c = 0` when `raw < 0` and the row is not 0. Taken
   **before** the armor step (`0x6F2A43F5`..`0x6F2A4446`). This is the minimum-damage rule that section 7 says is absent:
   it is in the target side, not in `GetDamageAmount` (which clamps only at 0).
5. **Armor**, only when not status and class == `0x10`: `amount *= M`, `M = 1 - R`. For `A >= 0`: `R = K·A / (1 + K·A)` with
   `K` = Misc `DefenseArmor` (element 0; `0x6F26D270` reads it through `0x6F009EE0`). For `A < 0`: `R = table[min(trunc(-A), 20)]`
   (negative table). If `R >= 1`, `M = 0`.
6. **Damage-type factor**: `amount *= T[row][column]` unless the factor is exactly 1.0; column = armor type (`T+0xE4`), or 4
   with the status flag.
7. **Ethereal** (11.3): `amount *= slot` when (a), (b), (c) hold.
8. **Sent value** `s = (m < amount) ? amount : m` (ties keep `m`). CONFIRMED; the comparison is the x87 `fcom` at `0x6F2A4554`.
9. **Zero test** on `amount` (record `+0x10`, after steps 5 to 7): zero gives output 0. The test is on `amount`, not `s`.
10. **Relation case** (11.6): if the source is present and the relation flag is set:
    - source flag clear, target flag set: **`s = raw × 100.0`**. The multipliers of steps 5 to 7 are lost. CONFIRMED by
      Unicorn: the operand is the raw amount, not the product;
    - source flag set, target flag clear: `s = 0`.
11. Target `+0x20 & 0x8` with record flags lacking `0x20`: output 0.
12. HP change: the routine calls `vf 0x128` of the target with the negated value and returns `-(result)`, so the output is
    `s` (CONFIRMED pattern; the stub returns its argument).

Code: `sim::damage::target_side_amount` (one function, the same order), used by `SpecDamage::roll` with the target's armor
and type, the record flags `0x100`, the weapon's class, and zero counters, flags and alliance (see 11.8).

### 11.6 Relation case (CONFIRMED structure, meanings UNSURE)

- `0x6F4C34D0(13)` returns an object. The routine reads `object+0x10`, then its first dword, then that block's flag word
  at `+4`; bit 1 (`0x2`) enables the relation case, and the source must be present. The roll wrapper also calls
  `0x6F4C34D0(13)` (section 3.3), so this is a global table entry; its meaning is UNSURE.
- Player flags: `0x6F2834E0(unit)` returns the unit's player object (through `vf 0xEC` and `0x6F3A1650`); its `+0x268` is the
  flag tested for the source and the target.
- Constant `0x6FAAE574` = 100.0 (start-up value; CONFIRMED value, UNSURE meaning).

### 11.7 Evasion `CAbilityEvasion::EvadeAttack` (`0x6F08BD20`, slot `0x168` of the evasion class, vtable `0x6F89D628`)

CONFIRMED body (disassembly):
- returns 0 when the ability's `+0x3C` is > 0 (signed);
- returns 0 when the ability's `vf 0x268(0)` is nonzero;
- chance `c` = `0x6F02E9E0(&c, level)` with level = ability `+0x50` (the chance table is not decoded);
- `r` = `0x6F2853D0(&r, stream 2)`: one step of **RNG stream 2** (combat), a real in `[0, 1)`;
- evades **iff `c > r`** (x87 `fcom`, strict). An unordered compare (NaN) also evades, because `test ah,0x41` sees even parity;
- on a hit it calls `0x6F052FB0(ability)` (effect not decoded) and returns 1, otherwise 0.

NOT RESOLVED: which handler calls slot `0x168` (the offset occurs in several hundred instructions of `.text`), and where
the call sits relative to the armor step (the handler passes 1 and 2 of the target routine are not in 11.5). The chance
table and what the caller does with a 1 (probably zeroing the amount) are UNSURE. Not implemented in the model: there is no
chance table and no consultation point. Only the comparison is confirmed.

### 11.8 Corrections to earlier sections, and what is not covered

- Section 4, steps 7 and 10.6: the class is `0x10` for basic attacks (11.1). The armor test is not an attack-type heuristic.
- Section 7, "Minimum damage: No min-1 seen": true for `GetDamageAmount`, wrong for the target side (11.5 step 4).
- Section 10.7: the ethereal conditions are (a) source null, the ally flag, or not allied; (b) `+0x158 > 0`; (c) slot != 1.0
  (11.3). The status word of section 4 step 8 is 11.4.
- Not covered by the Unicorn check: the handler passes 1 and 2 (abilities, evasion, critical strike, bash; the list walk is
  stubbed, the masks at `+0x1DC`/`+0x1E0` and `+0x250`/`+0x254` are set to -1), the HP change itself, the record `+0x20`
  origin (11.2), the meanings of the two masks and the relation object, and the evasion roll.

### 11.9 Code and tests

- `crates/sim/src/damage.rs`: `ATTACK_DAMAGE_CLASS`, `ATTACK_RECORD_FLAGS`, `CLASS_MASK_A/B`, `RELATION_CONSTANT_BITS`,
  `DamageRules`, `TargetSide`, `status_flag`, `target_side_amount`. `SpecDamage` holds the Misc values and calls
  `target_side_amount`; its counters and flags are zero, and `with_rules` sets the Misc flags.
- `crates/sim/src/combat.rs`: `AttackType::physical` is replaced by `damage_class` (default 0, set it).
- `crates/engine/src/data.rs` sets `damage_class` to `ATTACK_DAMAGE_CLASS`; `crates/gamedata/src/combat.rs` no longer has
  `is_physical`.
- `crates/sim/tests/target_damage.rs`: 9 synthetic tests (always run) and one vector test (`GAME_DLL` +
  `TARGET_DAMAGE_VECTORS`, the output of `tools/emu/target_damage_emu.py`, kept outside the repository).
- `tools/emu/target_damage_emu.py`: the Unicorn harness (per line: 21 inputs, 56 table cells, 7 ethereal slots, 21
  negative-armor entries, the output and the value sent to the HP change).
