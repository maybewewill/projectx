# Missiles (projectiles) of basic attacks in Game.dll 1.26a

Scope: what happens to a ranged attack after the execute point (`0x6F0CF660`): creation of the missile, its flight,
its arrival (slot 47), the impact check (slot 45) and the damage that lands. Companion of `attack-cycle.md` (section 5)
and `attack-cycle-2.md` (section 2), which this pass extends. Implementation: `crates/sim/src/missiles.rs`, hooked into
`crates/sim/src/combat.rs` (`impact_delay`, `missile_arrive`).

Confidence labels: **CONFIRMED** (read in the decompile or the disassembly, or checked in Unicorn), **LIKELY** (the
structure is read but the meaning is inferred), **UNSURE** (not decoded, or a model).

Summary in plain words:

- A missile is an object (CMissile, vtable `0x6F89147C`) that holds the source and target as mask pairs, a flags word
  and a damage record. It is not a unit.
- The **arrival** (slot 47, `0x6F2BECB0`) asks whether the target still exists. If so, it builds the damage record and
  calls the target's damage routine (vtable slot `0x120`), except in one "ability hits" branch that returns without
  damage. Then it calls the missile's slot 42 (`0x6F2BD010`).
- The **impact check** (slot 45, `0x6F2C42D0`) runs the same gate. When the gate and the ability checks hold, it sets the
  "hit done" flag `0x20000000` and launches (`0x6F2C2AF0`). Otherwise it hands over to slot 47.
- The **flight time** is not decoded. The model is distance over speed (UNSURE).
- The damage amount is rolled at the execute point, before the missile type switch (CONFIRMED, `attack-cycle.md` 4.4),
  so a missile carries the amount it was given. Nothing is rolled at arrival.

## 1. Code map

| What | Address | Status |
|---|---|---|
| CMissile vtable | `0x6F89147C` (50 slots) | CONFIRMED (attack-cycle.md 5) |
| Slot 47 (per-missile arrival) | `0x6F2BECB0`, vtable `+0xBC` | CONFIRMED (vtable offset read in `0x6F2BB570`: `jmp [eax+0xBC]`) |
| Slot 45 (impact check) | `0x6F2C42D0`, vtable `+0xB4` | CONFIRMED structure (attack-cycle-2 2.2; the offset is from the spec) |
| Slot 42 (end of arrival) | `0x6F2BD010`, vtable `+0xA8` | CONFIRMED (tail jump `jmp [eax+0xA8]` in slot 47; the target per spec) |
| Arrival routine (gate, damage call) | `0x6F2BE8B0` (thiscall, 3 stack args, `ret 0xC`) | CONFIRMED (Unicorn) |
| Point at a fraction, source of the record | `0x6F2BCAC0` (thiscall, `ret 0xC`) | CONFIRMED structure |
| Point at a fraction, two floats | `0x6F2BB590` (CBulletBase slot 43) | LIKELY (two softfloat multiplies, Unicorn not run) |
| Target from the missile mask | `0x6F2BBA80` (returns 0 when the mask is `-1`, else `0x6F4786B0`) | CONFIRMED (Unicorn) |
| Launch (binds target, sets the start) | `0x6F2C2AF0` (thiscall, `ret 8`) | CONFIRMED structure |
| Schedules a move/arrival message `0xD019C` | `0x6F2BCC50` (through `0x6F47A650`) | CONFIRMED call; see 3.2 |
| Death timer message `0xD019F` | `0x6F2BCBF0` (reads Misc key "BulletDeathTime") | CONFIRMED |
| Projectile list insert (float key at `+0x18`) | `0x6F4E85F0` / `0x6F4E84B0` | CONFIRMED structure, meaning UNSURE |
| Projectile registration callback (position via slot `0xA0`) | `0x6F2BB4A0` | CONFIRMED |
| Missile record loader (speed, arc, homing) | `0x6F3222C0` | CONFIRMED (key strings and field offsets) |
| Getters: speed / arc / homing / art | `0x6F0407B0` / `0x6F0407E0` / `0x6F040810` / `0x6F040760` | CONFIRMED (fields `+0x68`, `+0x6C`, `+0x70`) |
| Missile type enum reader | `0x6F262D30` (values 1 to 8) | CONFIRMED (attack-damage 12.1) |
| Launch and impact helpers of the weapon fields | `0x6F2608C0` (`launchX`), `0x6F260940` (`impactZ`), `0x6F29C830` | CONFIRMED (unit-types.md) |

Unicorn harness: `tools/emu/missile_gate_emu.py`. Vectors: `crates/sim/tests/missile/gate_data.rs`. Test:
`crates/sim/tests/missile_gate.rs`.

## 2. Creation: weapon to missile

### 2.1 Which phase creates a missile (LIKELY)

The execute function switches on the missile type stored per weapon at `unit+0xDC+4w` (attack-cycle 4.4; attack-damage
12.1). The enum reader `0x6F262D30` gives the numbering 1 to 8 (CONFIRMED): 1 normal, 2 missile, 3 artillery, 4 instant,
5 msplash, 6 mbounce, 7 mline, 8 aline.

**Conflict to note.** `attack-cycle.md` 4.4 names phase 1 "instant hit" and phase 4 "no hit (status float 1.4e-45)". The
Unicorn run of the execute code (`attack-damage.md` 12.1, `tools/emu/execute_row_emu.py`) shows that the direct paths
(the calls to the target routine at once) are type 1 (normal) and type 4 (instant). The model follows the Unicorn run
and the enum. The status value `1.4e-45` is a target-side flag (attack-damage 11.4), so phase 4 may be the instant case
after all. UNSURE which numbering the attack-cycle note used.

| weapTp | Name | Path in the execute | Status here |
|---|---|---|---|
| 1 | normal | direct: target routine at once (CONFIRMED, Unicorn) | implemented (`MissileType::Normal`, no flight) |
| 2 | missile | attack-cycle 4.4 phase 2 helpers (`0x6F0408B0`, `0x6F040830`), launch check, cleanup | implemented: flight, arrival gate |
| 3 | artillery | attack-cycle 4.4 phase 3 helpers (`0x6F08D280`, `0x6F0C71C0`, `0x6F476670`, `0x6F022190`) | flown to the target, damages only that target (UNSURE for the ground point and splash) |
| 4 | instant | direct: target routine at once (CONFIRMED, Unicorn) | implemented (no flight) |
| 5 | msplash | attack-cycle 4.4 phase 5 helpers (`0x6F0408B0` through a thunk, `0x6F09A730`) | flown to the target only (UNSURE splash) |
| 6 | mbounce | attack-cycle 4.4 phase 6 helpers (`0x6F040900`, per-weapon fields) | flown to the target only (UNSURE bounce) |
| 7 | mline | attack-cycle 4.4 phase 7 helper (`0x6F040950`) | flown to the target only (UNSURE line) |
| 8 | aline | attack-cycle 4.4 phase 8 (`0x6F040A60` / `0x6F040AA0`) | flown to the target only (UNSURE) |

The helper names in the last column come from the phase list of `attack-cycle.md` 4.4, which uses the other numbering;
they are shown only to locate the code, not as a proof of the mapping.

### 2.2 Missile record fields (CONFIRMED values, LIKELY owner)

The loader `0x6F3222C0` writes these fields of a missile record. Its defaults include the model
`abilities\weapons\spear\spearmissile.mdl` and a button texture, so the record belongs to the missile data of an ability
or a weapon:

| Record field | Read as | Used by |
|---|---|---|
| `+0x68` | `Missilespeed` (real) | `0x6F0407B0` (GetMissileSpeed) |
| `+0x6C` | `Missilearc` (real) | `0x6F0407E0` (GetMissileArc) |
| `+0x70` | homing flag from `0x6F314370` (bool) | `0x6F040810` (GetMissileHoming) |
| `+0x54`, `+0x58`, `+0x5C`, `+0x74`, `+0x50` | flags and reals read through `0x6F314370` | not decoded |
| art list `+0xEC` / `+0xF0` | `Missileart` (indexed) | `0x6F040760` |

Which record a basic attack's weapon uses (the unit's weapon row or an ability) is UNSURE. The getters fall back to a
second source, `0x6F32C8E0`, when the first lookup `0x6F02E640` is empty (CONFIRMED). The `UnitWeapons.slk` column for
the speed is not mapped in `gamedata` (attack-damage 12.6, unit-types 8). **The engine therefore sets
`AttackType::missile_speed` to `None` (every attack is direct) until this is decoded.**

### 2.3 What the missile receives at creation (LIKELY from the structure)

- Source handle: stored with the missile (`0x6F0408B0` stores the source handle, CONFIRMED; the field is the
  CMissile's own, not the record).
- Launch point: the weapon's launch offsets (`launchX`, `launchY`, `launchZ`, unit-types.md; they reach unit
  `+0x7C`/`+0x80`/`+0x84` at creation) plus the source's position (`0x6F2C2AF0` reads the source's point through slot
  `0xA0`, or `0x6F29E6A0` for the other case). LIKELY.
- Impact height: `impactZ` (type record `+0x210`, unit `+0x88`, unit-types.md). The impact Z appears only in 3D points of
  the flight, not in the 2D gate. UNSURE whether it changes the time.
- Damage: the amount rolled at execute is kept for the arrival (the damage record built in 2.4). CONFIRMED that the
  roll happens before the switch.
- Target: the launch (`0x6F2C2AF0`) calls `0x6F2C1B50` with the target, which clears the target mask (`0x6F2BEC80`,
  CONFIRMED) and, for a non-null target, sends message `0xD01A4` (`0x6F2AB2D0`) and converts the target to an id pair
  (`0x6F0419C0`). Where that pair is stored is UNSURE; the arrival reads it back through `0x6F2BBA80`.

### 2.4 The damage record built at arrival (CONFIRMED structure)

`0x6F2BCAC0` builds the record that the target routine receives (`0x6F2556E0` is the same constructor the execute uses,
`execute_row_emu.py`):

- record `+0x20` is copied from the missile's `+0x5C` (`*(int*)(record+0x20) = missile[0x17]`), the atkType carried with
  the missile (attack-damage 12.1 says the basic-attack row is the weapon's atkType; LIKELY the same value);
- the source is the missile's source mask resolved by `0x6F4786B0` (the missile's `+0x2C`/`+0x30` pair);
- the target position is obtained through the target's slot `0x11C`.

## 3. Flight

### 3.1 What is known

- The CMissile slot 47 is called with the fraction **1.0** (`FUN_6f2be8b0(&local_4, target, &1.0)`, decompile of
  `0x6F2BECB0`). So the arrival computes the point at the end of the path, not a per-tick position. CONFIRMED.
- The point at a fraction goes through the missile's slot 43 (`0x6F2BB590`, two softfloat multiplies of the fraction
  with the stored coordinates). LIKELY that it is `point * t` (attack-cycle-2 2.3).
- The projectile registration `0x6F4E85F0` keeps each projectile in a list sorted by a float key at `+0x18`
  (`0x6F4E84B0` inserts by `key <=` comparison, so equal keys are inserted after the existing ones). The callback
  `0x6F2BB4A0` copies the missile's position (slot `0xA0`, 12 bytes). CONFIRMED structure. The key's meaning (arrival
  time or an expiry on the agile clock) is UNSURE.

### 3.2 The arrival message and its path (CONFIRMED call, meaning UNSURE)

`0x6F2BCC50` calls `0x6F47A650` with the message `0xD019C`, the 3D target point and the missile. Through that call:

- `0x6F47A650` takes the world origin (`DAT_6fab7368+0x6C/+0x70`) and the point, through soft-float operations (LIKELY a
  cell conversion), then calls
  `0x6F497080` (the unit path/mover setup: path search `0x6F496D10`, heading `0x6F4B2DE0`, mover list `0x6F483290`) or
  `0x6F497260`, and finally `0x6F471AB0` sends message `0x40190065` to the missile.

So the missile's flight is set up through the same mover and path helpers as a unit's move. **Whether the flight is a
mover walk (speed in the mover's units, per 0.005 s tick) or a separate timer is UNSURE.** This is the open point that
decides the flight time. The decompile is ambiguous here (the softfloat operands are passed in registers), so it was not
settled in this pass.

### 3.3 Death of the missile (CONFIRMED)

`0x6F2BCBF0` sets a flag on the missile (`vtable+0x8C`, argument 1), reads the Misc key **"BulletDeathTime"** (`0x6F009EE0`),
and schedules message `0xD019F` after it. A missile therefore stays for `BulletDeathTime` after its end and then is
removed. The Misc value is not in the container. Not modelled (no effect on damage).

### 3.4 Model used (UNSURE)

`missiles::flight_seconds(distance, speed) = distance / speed`, distance = `sqrt(dx*dx + dy*dy)` in soft float from the
source to the target at the execute point, speed = the unit's missile speed in world units per second. The impact fires
at the first agile-clock tick whose clock reaches the launch clock plus this time (the timer queue, lockstep.md 10). The
earlier placeholder rounded to whole 0.1 s steps; that rounding is removed (it is not in the spec).

Not modelled: the arc (visual in this model), the launch offset and impact height in the distance, homing turns, and
the flight's own speed curve.

## 4. Arrival: slot 47 (CONFIRMED by Unicorn)

Code path (`0x6F2BECB0`): `ECX = missile`. It calls `0x6F2BBA80` (the target from the mask). If there is a target it
calls the arrival routine `0x6F2BE8B0(out, target, &1.0)`. Then it tail-jumps to slot 42.

The arrival routine:

1. Reads the missile flags `F`.
2. If `F & 0x30000000 == 0`, `F & 2 != 0`, and the target's kind value (slot `0x1C`) equals the constant of
   `0x6F26C1C0` (`0x2B773375`, real code), then:
   - source = `0x6F021A00`; if the source is non-zero, `canTarget(source, 0, 4)` (`0x6F284EE0`) holds, and the predicate
     `0x6F0774E0` holds on the arrival point record, then the result is 0.0 and the routine returns **without damage**
     (the "ability hits" branch; meaning LIKELY: a spell effect that replaces the damage).
3. Otherwise it calls the target's damage routine (slot `0x120`, `ret 0x10`) with the record of 2.4 and the class flag
   `(F >> 1) & 1`.

Gate summary (the model in `missiles::arrival`, verified):

| Input | Effect |
|---|---|
| no target (mask `-1`, or the handle does not resolve) | nothing (slot 42 still runs) |
| target present, flags bit 2 clear | damage with class flag = bit 1 |
| bit 28 or bit 29 set | damage with class flag = bit 1 (the ability branch is skipped) |
| kind mismatch | damage with class flag = bit 1 |
| target present, bit 2 set, bits 28/29 clear, kind ok, source, canTarget and predicate all true | no damage |

**Unicorn:** `tools/emu/missile_gate_emu.py`, real code of slot 47 and slot 45 (plus the arrival routine), stubs for the
callees named in the script (source, canTarget, predicate, the point builder, the launch routine, the cookie routine, the
resolver and the vtable slots), 1,500 random cases per slot: **0 mismatches** against the Python model. Branch coverage
of the vectors: damage with class flag 0 and 1, damage with bit 29 set, blocked arrival, no-target arrival, slot-45 launch
and fall-through (both with and without bit 29). The runs leave the image's constant `0x6FAAE4F4` unchanged (the code
decrements a count in it on the blocked branch; the harness restores it).

Emulator notes: the `fs:[0]` exception-frame instructions of the three functions were replaced by NOPs in the emulator
copy (no effect on the decisions). The harness passes the image's own `&1.0` constant as the original does.

## 5. Impact check: slot 45 (CONFIRMED by Unicorn)

`0x6F2C42D0`: when `F & 0x30000000 == 0`, `F & 2 != 0`, a target is present, the kind matches, and then source,
`canTarget` and the predicate hold, it sets `F |= 0x20000000` ("hit done", name LIKELY) and calls the launch routine
`0x6F2C2AF0` (`ret 8`). Otherwise it tail-jumps through `0x6F2BB570` (`jmp [ecx]+0xBC`), which is slot 47.

Consequence (CONFIRMED by the flag test): a predicate that holds at the impact check launches (set bit 29), and the later
arrival then skips the ability branch (bit 29 set) and **does** damage. So the ability branch takes effect only at the
arrival of a missile whose impact check did not launch.

## 6. Target changes during the flight (CONFIRMED structure, LIKELY meaning)

- **The arrival point is the target's live position** (slot `0x11C` of the target inside `0x6F2BCAC0`). A bound missile
  therefore lands on the target where it is at arrival, which is homing-like for every bound missile. The homing getter
  (`0x6F040810`) is not read by the arrival path. Whether the flight time is re-evaluated while the target moves is
  UNSURE; the model fixes the time at launch.
- **Target dead, not removed:** the handle still resolves, so the damage routine is called; the target side rejects a
  dead unit first (attack-damage 11.5). No damage (CONFIRMED structure of the target routine, Unicorn-checked there).
- **Target removed:** the mask should resolve to nothing (`0x6F4786B0` through the handle lookup `0x6F03FA30`, which
  returns 0 for an id whose entry is gone). The arrival then applies nothing, as the gate test's "no target" case
  (CONFIRMED for the gate; LIKELY that a removed unit's id resolves to nothing; the lookup was not run for this case).
  The model's test `missile_into_a_target_removed_in_flight_deals_nothing` checks the outcome, not the lookup.
- **Attacker dead or removed:** the missile keeps its own target mask and still lands (CONFIRMED structure: the arrival
  reads only the missile's fields).

## 7. Implementation (this pass)

- `crates/sim/src/missiles.rs`:
  - `MissileType` (weapTp 1 to 8, `flies()`), `GateInputs`, `arrival` (slot 47), `impact_check` (slot 45),
    `distance`, `flight_seconds`, the flag constants.
  - Tests: enum mapping, arrival and impact branches, flight time (needs `GAME_DLL` for the tables).
- `crates/sim/src/combat.rs` (small hooks):
  - `impact_delay` = `missiles::flight_seconds(missiles::distance(dx, dy))`, no 0.1 s rounding;
  - `missile_arrive` (the `AttackMsg::Impact` handler): `missiles::arrival(GateInputs::plain(FLAG_TARGET_BOUND,
    target_valid))`, damage through `apply_damage` only for `Damage`.
  - The `STEP` constant of combat.rs was removed (no other use).
- `crates/sim/tests/missile_gate.rs` + `tests/missile/gate_data.rs`: the Unicorn vectors, 3,000 rows (1,500 per slot),
  embedded; no `GAME_DLL` needed.
- `crates/sim/tests/combat_natives.rs`: `missile_into_a_target_removed_in_flight_deals_nothing` (end to end, with
  `GAME_DLL`); the existing `missile_damage_lands_after_the_flight_time` still holds (1.0 s flight, the impact 10 steps
  after the execute point).

## 8. What is still open (ordered by value)

1. **Flight arithmetic**: is the flight a mover walk (0.005 s ticks, mover speed) or a timer with distance over speed?
   The call chain of 3.2 reaches the mover setup `0x6F497080`. Test: a dynamic run of the original under Wine with a
   known distance and speed, measuring the arrival tick.
2. **Which record feeds a weapon's `Missilespeed`/`Missilearc`/homing** (the loader `0x6F3222C0` is per record kind). Map
   the unit weapon column (`UnitWeapons.slk`) and the attack path that calls `0x6F0408B0`/`0x6F040830`.
3. **Projectile list key** (`+0x18`, `0x6F4E84B0`): arrival time, expiry or position sort? Its consumer is not read.
4. **Phase numbering** (1 and 4, section 2.1): settle with a Unicorn run of the execute switch with the phase value as an
   input (the target-side status `1.4e-45`).
5. **Splash, bounce, line** (types 3, 5 to 8): the target list and the per-type loops are not decoded.
6. **Ability branch** (`0x6F0774E0`, `0x6F284EE0`): which abilities the predicate reads (the "spell absorbed" family?) and
   what the target's kind constant `0x2B773375` identifies. Not needed for basic attacks.
7. **Arc and impact Z**: whether they change the arrival time (2D versus 3D distance). Test with two launches on a
   slope in the original.
8. **`BulletDeathTime`** value and when the missile object is released (no effect on damage, but on handle ids if a
   missile holds one).
