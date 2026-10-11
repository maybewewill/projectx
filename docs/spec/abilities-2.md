# Abilities and buffs in Warcraft III 1.26a (Game.dll): cooldowns, buff durations, timed life expiry (pass 2)

Scope: the three questions of pass 2. (1) Ability cooldowns: storage, countdown, ready check. (2) Buff duration
timers: how a buff gets its duration, the expiry timer, stacking. (3) Timed life expiry: does the expiry reach the
death. It builds on `docs/spec/abilities.md` (pass 1, which stays valid except where this file says otherwise).

Labels: CONFIRMED = read in the disassembly, with the code path clear, or checked in Unicorn. LIKELY = strong evidence,
not run end to end. UNSURE = a guess or an open point. NOT FOUND = searched, no answer yet.

Sources: disassembly (capstone, `tools/analysis` helpers), decompile chunks in the scratchpad, raw image bytes, and
Unicorn runs of the original code with stubbed callees (`tools/emu/abilities2_emu.py`, the 25 cases in section 4).
No function body is copied into the repo; addresses and behaviour only.

---

## 0. Summary

| Question | Answer | Confidence |
|---|---|---|
| 1. Cooldown storage | Not found. The level record (`+0x4C`, 0x68 bytes) is still the likely home, but no cooldown offset is known. | NOT FOUND |
| 1. Cooldown countdown and ready check | Not found. | NOT FOUND |
| 1. `UnitResetCooldown` | Changes no ability field. Its only other call refreshes the local player's display. | CONFIRMED (no field writes); LIKELY (display only) |
| 2. Buff duration | Timed life apply: owner set, then the duration is compared with 0.0. Duration > 0 starts a one-shot agent timer (message `0xD01C4`, queue A). Otherwise no timer. | CONFIRMED (Unicorn, 22 cases) |
| 2. Normal vs hero duration | Not found. The level field `HeroDur` is read by a wrapper, but the choice between it and `Dur` is not located. | NOT FOUND |
| 2. Stacking and refresh | No refresh path. Each application creates a new buff object with its own timer. | CONFIRMED (creator reads no existing buff); LIKELY (no stacking anywhere) |
| 3. Expiry dispatch | Timer `0xD01C4` runs the buff's handler, which removes the buff through the normal removal routine. | CONFIRMED (Unicorn, 3 cases) |
| 3. Expiry to death | Not on this path. The timed life removal notify does not call a kill directly. The death route is open (section 3.3). | CONFIRMED (no direct kill); UNSURE (the route) |

---

## 1. Cooldowns

### 1.1 Entry point (CONFIRMED)

- `UnitResetCooldown` (native at `0x6F3C86D0`) resolves the unit through `0x6F3BDCB0` (the standard unit handle check,
  pass 1 section 4.3n) and calls `0x6F078C00` with the unit.

### 1.2 What `0x6F078C00` does (CONFIRMED by disassembly)

- Walks the unit's ability list (head and tail at `+0x1DC` / `+0x1E0`, empty when their AND is all ones) with the list
  step `0x6F4786B0`, as the removal routine does.
- For each entry it calls the entry's vtable slot 7 (the type tag, at byte offset `+0x1C`), and calls `0x6F471910` with
  the tag and the constant `'AAsp'` (`0x41417370`, returned by `0x6F028F30`). `0x6F471910` answers "is this tag `'AAsp'`
  or a subclass of it" (equal tags answer yes; otherwise it consults the class registry at `0x6FAB7368`).
- The matched entry is kept in a local reference, which is moved to the entry of each iteration (or to none when the
  tag does not match) and released at the end. No field of any ability or buff is written. The only changes are
  reference counts.
- After the walk it calls `0x6F3329D0(unit)`. That routine gets a global object (`0x6F300710`, with the `0x6FAB4F80`
  path) and, when the unit belongs to the local player, calls the display path (`0x6F3009F0`, and through it the
  message path that sends `0x13C7`). The local-player test reads the player id from the global at `0x6FAB65F4`.

Conclusion (CONFIRMED for "no field writes"; LIKELY for "display only"): `UnitResetCooldown` has no simulation effect
on abilities. The implementation registers it as a no-op (`crates/sim/src/natives/ability.rs`). A later pass that finds
the real cooldowns must check whether the original's reset is only visible on screen, or whether it is hidden in a
callee of `0x6F3329D0` that the decompile did not show (UNSURE; the callee `0x6F3009F0` was not read in full).

### 1.3 Level record fields (partial)

- The ability data field names are a table of string pointers at `0x6FA6F270` to `0x6FA6F2C0` (CONFIRMED): `priority`,
  `targs`, `Area`, `Cast`, `Cost`, `Dur`, `HeroDur`, `Cool`, `Rng`, `DataA` to `DataI`, `UnitID`, `BuffID`, `EfctID`,
  `version`. The level suffix (`1`, `2`, ...) is added to these names when the SLK row is read.
- Values are read with the Storm SLK cell call (`Ordinal_578`, table `0x6F876A74`, field name pointer, row index).
  Each field has a small getter that passes the value on to `0x6F262760`. The getters found in this pass are `0x6F01F7D0`
  (Area), `0x6F01F850` (Cast), `0x6F01F8D0` (Dur) and `0x6F01F950` (HeroDur); the readers for `Rng` and `DataA` to `DataC`
  are the next getters in the same chunk (CONFIRMED read, names from the table).
- The binder that writes these values into the 0x68-byte record at `+0x4C` was **not located**. The callers found for the
  getters (`0x6F01F7D0(&local, 1)` and similar) read row 1 into a local, so they belong to another reader. The record
  offsets of `Cool`, `Cast`, `Dur`, `HeroDur`, `Cost`, `Area`, `Rng` and `DataA` are therefore NOT FOUND.
- The field `Cool` has no direct getter in the search so far (its name pointer `0x6FA6F28C` has no code reference).

### 1.4 Countdown and the ready check (NOT FOUND)

- No per-tick or per-timer decrement of a cooldown was found. Candidates not yet checked: a queue A timer that the cast
  path starts with the cooldown value, or a field of the unit's own update path.
- The ready check used when an order starts was not found. The order start path of pass 1 (`0x6F29DFF0`, ability lookup
  `0x6F279A90`) was not followed to a cooldown test.

---

## 2. Buff duration and the expiry timer

### 2.1 Application of a timed life buff (CONFIRMED structure)

1. `UnitApplyTimedLife` (`0x6F3C85D0`) calls the creator `0x6F07B580` with the buff code and a duration pointer (pass 1,
   section 3.4). The seven special codes select seven creators; any other code, including `'BTLF'`, selects the default
   creator `0x6F07B240`.
2. The default creator builds a `CBuffTimedLife` object. Its constructor `0x6F0D8570` builds a `CBuffProgressBar` base
   (vtable `0x6F893A04`) and then the class vtable (`0x6F893D64`). It embeds a `FloatMini` at `+0xCC` (vtable `0x6F876D38`),
   embedded sub-objects at `+0xA0` and `+0xA8`, and a float `+0xD0` set to `0.0` (the constant at `0x6FAAE470`).
   The creator does not read the unit's list: it looks up the code in the registry (`0x6F001EC0`), takes a record from
   the registry (`0x6F472C20`) and a new id from `0x6F48AB80`, and calls `0x6F0757C0` (CONFIRMED read of the body; the
   last helper was not read).
3. The creator then calls slot 199 (`+0x31C`) of the new buff with the unit and the duration pointer, and releases the
   buff. Slot 199 of the class is `0x6F0D67C0`. It calls `0x6F0D28F0`, which:
   a. calls slot 200 (`+0x320`) of the buff with the unit, the duration pointer and the constant 1 (this is `0x6F0D4CB0`,
      section 2.2);
   b. calls the `FloatMini` value setter (slot 0 of `+0xCC`, `0x6F021D50`) with the duration pointer and 1. That setter
      copies the float into the object's own `+4` (CONFIRMED read);
   c. calls `0x6F332810` with the unit.
   Afterwards `0x6F0D67C0` sets flag `0x4` in the unit's `+0x248` (the first argument, the unit), registers the unit's
   event with `0x6F2AB1E0` (flag 1), calls a virtual at `+0x360` of the object in ECX, and when that returns zero it
   changes the unit's flags word `+0x5C` (clears `0x200`, sets `0x1000`). The meaning of the virtual call is not read.

### 2.2 The duration branch, slot 200 (`0x6F0D4CB0`) (CONFIRMED, Unicorn: 22 apply cases, 0 mismatches)

Arguments as the code reads them: arg1 = unit, arg2 = pointer to the duration (f32), arg3 = flag.

- The owner helper `0x6F0D1C40` is called with the unit and a zero. Its result is stored as the owner at `+0x34` when it
  is not zero.
- If arg3 is zero, flag `0x01000000` is set in `+0x20`.
- Slot 208 (`+0x340`, `0x6F0D1B00`) is called with no argument (an init hook; not read).
- The attach helper `0x6F0214A0` is called with the unit.
- **The duration test.** The duration is compared with `0.0` by an ordered compare (x87 `fcompp` and `fnstsw`):
  - duration strictly greater than 0.0 (including `+inf`, `3.4e38` and the smallest positive denormal): flag `0x80`
    is set in `+0x20`, and slot 211 (`+0x34C`, `0x6F0D30D0`) is called with the duration pointer and 0 (section 2.3);
  - otherwise (zero, negative zero, negative values, `-inf`, NaN): both `FloatMini` sub-objects (`+0xA8`, `+0xA0`)
    are set to `0.0` through their slot 0, and slot 59 (`+0xEC`, `0x6F0D31D0`) is called with no argument. No timer
    starts and `0x80` stays clear.

### 2.3 The expiry timer, slot 211 (`0x6F0D30D0`) (CONFIRMED structure; queue A LIKELY)

- Cancels the timer object at `+0x84` (`0x6F477D30`), sets `+0xA0` to `0.0` through its slot 0.
- Starts the timer with `0x6F477A80(timer object, timeout pointer, message 0xD01C4, payload = buff, 0, 0)`. That helper
  calls the timer holder's slot 2 with the buff's agent (`0x6F02F780`), the timeout, the message, repeat 0 and queue
  select 0. `lockstep.md` section 11.2 (from the `CAgentTimer::vf2` reading) gives select 0 as queue A (`mgr+0x40`). So the
  expiry is a one-shot timer on queue A (LIKELY: the select argument is passed as an integer zero here, not as a pointer
  to 0.0 as that reading describes).
- If the second argument is non-zero, slot 193 (`+0x304`) is called. In the apply path the second argument is 0, so it is
  not called.

Consequences (for the model): the expiry time is `clock + duration` in softfloat, as for every queue timer. A duration
of zero or less never expires (section 4 cases and `crates/sim/src/abilities.rs`).

### 2.4 Stacking and refresh (CONFIRMED absence, LIKELY no rule)

- Applying the same timed life again runs the creator again: a new object, a new serial, a new timer. No code in the
  creator, in the apply path or in slot 200 looks for an existing buff of the same code. So two applications give two
  entries, each removed by its own expiry. The implementation does this.
- No refresh rule (for example "the new duration replaces the old one") was found. UNSURE whether the engine has one
  elsewhere (for example in the JASS natives' callers).

### 2.5 Normal and hero duration (NOT FOUND)

- The level field `HeroDur` (name table entry `0x6FA6F288`, wrapper `0x6F01F950`) exists. `UnitApplyTimedLife` takes a
  single duration argument. The step that chooses `Dur` or `HeroDur` for a hero was not located. Whether `HeroDur` applies
  to timed life at all is UNSURE.

---

## 3. Expiry and the death of the unit

### 3.1 Expiry dispatch (CONFIRMED, Unicorn: 3 handler cases, 0 mismatches)

- When the timer for message `0xD01C4` fires, the pump calls the agent's handler, which is the buff class's message
  handler `0x6F0D3130` (the vtable slot that holds it is 221, `+0x374`; the event path that reaches it is LIKELY).
- Handler behaviour: if the message id is `0xD01C4`, it calls the removal routine `0x6F079CC0` with the buff (the owner
  of the buff is passed in ECX; if the owner field is zero it uses the fallback `0x6F472890`), and returns 1.
- Any other message goes to `0x6F0D1D20`, which dispatches only `0xD01A1` (slot `+0x314`) and `0xD01A7` (slot `+0x318`),
  and returns 0 for anything else.

### 3.2 Removal (CONFIRMED path, pass 1 section 1.6)

- `0x6F079CC0` sets flag `0x08`, calls slot 106 (`+0x1A8`) on the buff and on every other entry, and unlinks the buff.
- For the timed life class, slot 106 is `0x6F0D5F10` (CONFIRMED vtable entry).

### 3.3 The notify of the timed life class and the death (CONFIRMED read; death route UNSURE)

Slot 106 of the timed life class (`0x6F0D5F10`) acts only when the removed object is the buff itself:
- it gets the owner unit (`+0x30`, with the fallback `0x6F472890`) and calls `0x6F2AB1E0` on it with the buff and flag 0
  (LIKELY: unregister the buff's event, the counterpart of the flag-1 call in section 2.1);
- when flag `0x80` is set, it calls the owner's slot 73 (`+0x124`) with `0.0`, then calls `0x6F3331D0` on the owner;
- it calls slot 214 (`+0x358`) of the buff. For the timed life class that slot is `0x6F0ADFF0`, which returns **zero**;
- zero selects the lower branch: it calls `0x6F0405D0` (an event fire through the definition of the ability: it reads
  the definition from `0x6F02E640`, or the global default from `0x6F32C8E0`, and calls `0x6F2AAE40` on the observer list
  stored there), then calls the owner's slot 33 (`+0x84`);
- the non-zero branch (which does the unit checks with flags `+0x5C` bit `0x100` and `+0x20` bit 1, and calls
  `0x6F2835A0`, then slot 215) is **not taken for the timed life class**.

So the removal of a timed life buff does not call a kill itself (CONFIRMED). The unit dies (or not) through what the
lower branch does: an event fired on the definition's observer list, or the owner's slot 33. Neither was run. The
candidate death path is therefore UNSURE, and it is the single most important open item of this pass.

The model keeps `KillUnit` at expiry (`crates/sim/src/natives/ability.rs`, `expire_timed_life`), because the unit
dies in the game in practice. It is marked UNSURE there and in the tests (`timed_life_expires_at_the_expected_step_and_kills_the_unit`
keeps it, the multi-buff test does not assert it).

### 3.4 Pause (partial)

- `UnitPauseTimedLife` calls slot 58 (or slot 59 for resume) on each timed life buff. Slot 59 of the class is
  `0x6F0D31D0` (not read). The model pauses by cancelling the timer and keeping the time left, which matches the
  spec's description; the exact original behaviour is UNSURE.

---

## 4. Verification

Harness: `tools/emu/abilities2_emu.py` (run with `GAME_DLL=<Game.dll>`). It maps the original image with `emu.py`,
replaces three callees with recorders (the owner helper, the attach helper, the removal routine, the other-message
handler and the fallback owner), uses fake vtables whose slots 208, 211 and 59 are recorders, and calls the original
code.

| Check | Cases | Result |
|---|---|---|
| Apply (`0x6F0D4CB0`): 11 durations × flag 0 and 1 | 22 | 0 mismatches. Durations: 1.0, 0.5, 1e-38, smallest denormal, 0.0, -0.0, -1.0, -inf, +inf, NaN, 3.4e38 |
| Handler (`0x6F0D3130`): message `0xD01C4`, `0xD01A1`, `0xD01A7` | 3 | 0 mismatches |
| Total | 25 | 0 failures |

Rust tests that encode the same vectors: `timed_life_timer_follows_the_original_duration_branch` (unit, abilities.rs,
11 bit patterns) and the integration tests in `crates/sim/tests/ability_natives.rs` (`zero_duration_timed_life_starts_no_timer_and_stays`,
`negative_duration_timed_life_starts_no_timer`, `second_timed_life_adds_an_entry_with_its_own_expiry`,
`reset_cooldown_leaves_timed_life_and_the_unit_alone`). The earlier test `zero_duration_timed_life_expires_on_the_next_step`
asserted the old model (expiry on the next step) and was replaced: the Unicorn result says a zero duration starts no timer.

Not run in Unicorn: slot 106 of the timed life class (section 3.3), the timer start helper `0x6F477A80`, the cooldown walker
(section 1.2 is by reading the disassembly), the level getters (section 1.3).

---

## 5. Open items (priority order)

1. **The death route (section 3.3).** Find what the lower branch of slot 106 does with its event (`0x6F2AAE40` on the
   observer list of the definition) and what slot 33 of the owner does. A Unicorn run of slot 106 with a recorded event
   list would settle it.
2. **The level record binder.** Find the code that writes each `Area`/`Cast`/`Dur`/`HeroDur`/`Cool`/`Cost`/`Rng`/`DataA`
   value into the 0x68-byte record at `+0x4C`, and so the offsets. Only then can the cooldown field be named.
3. **Cooldown countdown and the ready check.** Follow the cast path from the order start (`0x6F29DFF0`) and the
   attack-ability slot 3 of pass 1 to the first test of a cooldown. Check whether a queue A timer carries the countdown.
4. **Hero duration.** Find the choice between `Dur` and `HeroDur` for a hero unit, and whether `UnitApplyTimedLife` uses it.
5. **Queue A confirmation.** Run `0x6F477A80` in Unicorn with a holder object to read the queue select (section 2.3).

## 6. Files

- `tools/emu/abilities2_emu.py`: the Unicorn checks of section 4 (25 cases).
- `crates/sim/src/abilities.rs`: `timed_life_starts_timer`, `TIMED_LIFE_EXPIRY`, the `Unlimited` state, the one-shot timer
  on apply.
- `crates/sim/src/natives/ability.rs`: `UnitResetCooldown` as a no-op; the expiry comments.
- `crates/sim/src/natives/unit_attr.rs`: `UnitResetCooldown` entry changed from NotImplemented to Likely.
- `crates/sim/tests/ability_natives.rs`, `crates/sim/tests/unit_attr_natives.rs`: tests above.
