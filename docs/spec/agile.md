# The agile property engine, Game.dll 1.26a

Status: the property, modifier and listener semantics, their timer arming and the tick that pumps them are
**CONFIRMED in Unicorn** (real routines run on synthetic objects; numbers in section 8). The wiring of the game
objects that use the engine (what a listener's message does, which limits a property has) is LIKELY or UNSURE where
marked. Code: `crates/sim/src/agile.rs`. Harness: `tools/emu/agile_engine_emu.py`.

Labels: **CONFIRMED** = read from the disassembly and run in Unicorn with 0 mismatches; **LIKELY** = strong inference
from the disassembly; **UNSURE** = guess or not traced.

This engine (source path `engine\source\agile`, RTTI `NIpse::CPo*`) is what drives **every countdown that is not a
JASS timer**: construction, training, research, upgrade and revive progress, the life of a building under
construction, the time of day, the **life and mana of every unit with their regeneration** (sections 9 and 10), and (not yet
decoded as classes) unit movement. Completion of a build or a training head is a *listener firing*, not a timer expiry and not a
poll. Cooldowns are **not** part of it (section 11).

## 1. Objects (CONFIRMED, RTTI and fields read)

The game code holds thin wrappers that carry a handle (index, generation) of the real engine object:

| Wrapper (RTTI, vtable) | Real object (RTTI, vtable, constructor) | Role |
|---|---|---|
| `Float` `0x6F877BA8` | `NIpse::CPoReal` `0x6F952DEC`, ctor `0x6F48C3A0` | a float **property** |
| `FloatModifier` `0x6F878528` | `NIpse::CPoReContinuousMod` `0x6F952A4C`, ctor `0x6F48AE90` | a **modifier**: a rate for a while |
| `FloatListener` `0x6F878E40` | `NIpse::CPoReThresholdLis` `0x6F952B6C`, ctor `0x6F48B420` | a **listener**: fires at a threshold |

Wrapper calls: property value `0x6F4773A0` (getter, `ecx` = wrapper), set value `0x6F477350`, modifier creator
`0x6F4774E0(out, &rate, &horizon)` (-> `0x6F478CA0`), listener creator `0x6F477550(out, mode, &threshold, message,
owner, flag)` (-> `0x6F480FB0`). Objects are made by the world factory (a function pointer at `[G+0x10]`,
`G = [0x6FAB73D8]`, argument = a creation message with a 4-character tag: `^mod`, `^lis`), then `vf3` initialises from the
message and `vf17` is a no-op.

Fields of a property: `+0x70` / `+0x74` = time `t0` (the queue clock float and the row) at which the base was valid,
`+0x78` base `B`, `+0x7C` slope `S` (units per agile second), `+0x80` lower limit, `+0x84` upper limit, `+0x64` head of
the subscriber list (listeners, newest first). Defaults of a new property: `B = S = 0.0`, limits `-FLT_MAX` / `FLT_MAX`
(`0x6FAAE628` / `0x6FAAE624`).

Modifier: `+0x40` property, `+0x50` rate, `+0x54` horizon, `+0x1C` its timer entry. Listener: `+0x40` property, `+0x50`
threshold, `+0x54` mode (0 = falling to the threshold, 1 = rising; any other value behaves as rising in the jump
detector and never arms), `+0x30` owner, `+0x1C` its timer entry, flags `+0x4C` = `0x90400`.

## 2. Property value (CONFIRMED, 600 + 600 Unicorn vectors)

`value(now) = clamp(add(B, mul(S, w)))` with `w` the time since `t0` (`0x6F477180`, `0x6F497460` -> `0x6F4885A0` ->
`0x6F483E70`):

- `d = sub(now, t0)`; if `|d| < 1e-4` (`0x6FAAE478`) then `d = 0.0`; if the rows differ, `w = add(d, mul(300.0, I2R(row - t0row)))`
  (the horizon `300.0` is `queue+0x48`), else `w = d`.
- `clamp`: `lo > v` gives `lo`, else `hi < v` gives `hi`, else `v` (x87 compares, NaN passes).
- Operand order is `add(B, mul(S, w))`; every op is the truncating soft float.

`refresh` (`0x6F4A90B0`): `B := value(now)`, `t0 := now` (`0x6F497440` -> `0x6F483430` copies the queue clock and row).

**Slope add** (`0x6F4A9320`, CONFIRMED 600 vectors): `refresh`; `S := add(S, delta)`; if `|sub(S, 0.0)| < 8e-7` (`0x6FAAE61C`)
then `S := +0.0`; then the subscribers are told the new base (`0x6F4A91B0` -> every subscriber's `vf8`, message
`^pro`/`elr` carrying `B`). So several modifiers on one property **add their rates**.

**Value add** (`0x6F4A92B0`, the body of the wrapper's set-value `0x6F477350`, whose argument is the new value: the delta is
`sub(x, value now)`): `refresh`; `B := clamp(add(B, delta))`; subscribers get the message `^pro`/`dlr` with (new, old) from
`0x6F4A9230`. The slope is **not** touched. (Earlier notes that set-value zeroes the slope were wrong; that is the
`8e-7` snap of the slope add.)

## 3. Modifier (CONFIRMED, 600 creation vectors and the scenarios)

Creator `0x6F478CA0(wrapper, owner, &rate, &horizon)`:

1. a horizon with `|sub(horizon, 0.0)| < 2e-7` (`0x6FAAE614`) is replaced by `FLT_MAX` (forever; the default pointer is
   `[0x6FA83A88]` -> `0x6FAAE624`);
2. the object is created with rate 0.0 and horizon 0.0; the **rate setter** `0x6F4A9B00` stores the rate only when
   `|sub(0.0, rate)| >= 2e-7` (a tinier rate stays 0.0) and, if the modifier is already linked, adds the difference to
   the property's slope; the **horizon setter** `0x6F4A9ED0` only stores it while no timer exists;
3. the wrapper `vf6` (`0x6F4797F0` -> `0x6F4A6BC0`) links the modifier to the property: `+0x40 := property`, and because
   the modifier's flags have bit `0x04` it sends itself the message `^beg` (`0x5E626567`, handler `0x6F4A9F80`).
   **Begin** = `slope add(rate)` on the property, then arm the modifier's own timer for the horizon (`0x6F4A9E70`).

Timer of a modifier: horizon below `FLT_MAX`: one-shot, duration clamped to at least `1e-4`; **at expiry** the modifier
sends itself `^end` (`0x5E656E64`, via `0x6F4A68E0` -> `0x6F4A6830` -> `0x6F4A64A0`): `slope add(-rate)`. A horizon of
`FLT_MAX` or more (forever) arms a **repeating 150.0 s** timer (`0x6FAB7774`) that does nothing but occupy the queue.

## 4. Listener (CONFIRMED, 600 decision, 600 jump, 600 crossing-time vectors and the scenarios)

Creator `0x6F480FB0` order (CONFIRMED by the sequence numbers of the queue entries):

1. the object is created and a **0.1 s timer** is armed (sequence +1, `0x6F480E90`), the owner's observer table gets the
   registration of the message (`0x6F471A30`);
2. the wrapper `vf6` links it to the property: `+0x40 := property`, the 1e-4 s link timer replaces the 0.1 s one
   (`0x6F4A6AF0`, sequence +1, cancelled at once by step 4), the listener becomes the **first** subscriber (newest first);
3. mode then threshold are stored (`0x6F4AA010`, then `0x6F4A9FF0`);
4. storing the threshold evaluates the listener at once (`0x6F4A9E20`): `value(now)` goes to the **decision**.

**Decision** `0x6F4A9C70(v)`: if `|sub(v, threshold)| < 0.005` (`0x6FAB7778`, strict) the listener **fires** (cancel its timer,
send the message to the owner, `^lis`/`ltr`); otherwise **arm** (`0x6F4A9B70`):

- cancel the timer (`0x6F4A68E0`); `S := property slope`; nothing unless `|sub(S, 0)| >= 8e-7`;
- mode 1 needs `0 < S`, mode 0 needs `S < 0`, other modes nothing;
- `dt = crossing time` (`0x6F4A8FD0`): `-1.0` if the threshold lies outside `[lo, hi]`, else `div(sub(thr, value now), S)` (soft
  divide, the reciprocal table); nothing unless `0 < dt` or `|dt| < 8e-7`;
- `dt := min(dt, 4.0)` (`0x6FAAE510`: the listener looks again at least every 4 s);
- arm one timer for `dt` (clamped to at least `1e-4`): **repeating** (entry flag `0x20001`, because the listener's flag
  `0x10000` is set), period = that duration. Its callback re-evaluates (`^sig` to itself, value now) and every outcome of the
  decision cancels the entry, so in practice it is a one-shot that re-arms by the decision.

**Jump** `0x6F4A9CF0(new, old)` (the message of a value add): fire if `crossed(old, new)`, else arm.
`crossed` (`0x6F4A9940`), `near(x) = |sub(x, thr)| < 0.005`: mode 0: `(near(old) ? has_timer : thr < old) && (near(new) || new < thr)`;
any other mode: `(near(old) ? has_timer : old < thr) && (near(new) || thr < new)`.

A listener keeps firing on later changes while it is subscribed; **the owner deletes it** when it has completed
(UNSURE how: `Agile::remove_listener` cancels the timer and unsubscribes; the original's destroy path `0x6F478920` ->
`0x6F4A6970` was not run).

## 5. The queue and the tick (CONFIRMED, 560 scenarios with the 300 s restart)

All objects above arm their timers on the **world queue at `G+0x14`** (`G+0x68` for objects whose index has bit 31, not
pumped by the tick hook). The tick hook `0x6F470C70` (per 0.005 s tick, after the JASS timer queue A) pumps, in this order:
queue A (`manager+0x40`), `G+0x14` (`0x6F46E9E0`), `G+0xBC` (`0x6F46EAB0`), `G+0x164` (`0x6F46F3B0`, proximity).

- Arm (`0x6F4A6890` -> `0x6F4A66E0` -> `0x6F4A6650`): `duration := max(duration, 1e-4)`, `sequence := ++queue+0x50`,
  `expiry := add(queue now, duration)`; an old entry of the same object is **flagged cancelled (`0x10000`) and stays in the heap**.
- Tick (`0x6F46E9E0`): `tmp = add(now, 0.005)`; if `300.0 > tmp` then `now := tmp` and pump; otherwise (rollover, same as
  `lockstep.md` section 14) `excess := sub(tmp, 300.0)` (below `8e-7` becomes 0), `now := 300.0`, pump, **every pending
  expiry (cancelled ones too) := sub(expiry, 300.0)**, row + 1, `now := excess`, pump.
- Pump (`0x6F46E910`, entry callback `0x6F46E480`): while the heap top has `expiry <= target`: pop it (order: expiry, then
  sequence), set `now := expiry`, skip it if cancelled, else call slot 18 of its object (`0x4A69B0`); afterwards a **repeating**
  entry that was not cancelled meanwhile is re-armed with `expiry := add(now, period)` keeping its sequence number (`0x6F46D570`).
  At the end `now := target`.

**The handler of a firing listener runs inside the callback** (LIKELY: the message goes owner `vf8` -> observer dispatch
`0x6F62A5D0`, all synchronous calls), i.e. with the queue time equal to the expiry, not to the end of the tick. This matters:
a countdown started by the handler (the next head of a training queue, a time-of-day reset) begins at the firing time.
`Agile::begin_tick` / `next_fired` expose the pump as a resumable iteration for exactly this; the Unicorn chain vectors
(section 8) suspend the real pump at the firing and start the next countdown there (the harness stops emulation inside the
sink, runs the follow-up on a second stack, and resumes), so the *mechanics* are CONFIRMED; that the owner's handler is
synchronous is the LIKELY part.

## 6. Users

**Countdown of a timed object** (`0x6F023A30` stores the total `R = I2R(bldtm)` when `|R| >= 2e-7`; `0x6F030920` the timed
start, `construction.md` section 13): property `B = R`, a modifier with rate `-D` and horizon `add(div(R, D), 1.0)` created
**first**, then a listener (mode 0, threshold 0.0, message `0xD01C4`). `D` = 1.0 (the cheat factor `16.0` of `0x6FAAE540` is
not applied). Nothing starts when `D < 2e-7` or the object is paused (`this+0x48 > 0`). Completion = the listener firing.
`Agile::start_countdown` does exactly this. Timeline (Unicorn, `agile_countdowns.txt`): a countdown of `bldtm` seconds completes
on tick `200 * bldtm + k` after its start with `k` growing with the soft-float drift of the 0.005 s clock (bldtm 60: 12004,
bldtm 150: 30021 or 30024 depending on the start tick, bldtm 301 crosses the 300 s restart: 60311).

`production.rs`: training heads, buildings, research and in-place upgrades use `start_countdown` when the Game.dll tables
are loaded (otherwise the previous plain timers stay: tests and games without tables). A completed head starts the next one
inside the completion (the chain vectors: the second head of 5 s + 5 s ends after 1000 ticks, not 1001).

**Time of day** (`0x6F429770`): float game state 2 is a property (default limits). A forever modifier of rate
`div(mul(scale, DayHours), DayLength)` (CONFIRMED earlier in `vision.md` section 13.1), horizon 0.0; a rising listener (mode 1)
at `DayHours` with message `0x80262` whose handler (`0x6F42FC9F`) is `set value := 0.0` (value add of `-value`), run inside the
callback. `SetFloatGameState(2, x)` is the wrapper's set-value. `SuspendTimeOfDay(true)` ends the modifier (`0x6F427E70`), resuming
creates it again (`0x6F429920` -> `0x6F429770`; the native's mapping is LIKELY). `Game::start_time_of_day`, `time_of_day`,
`set_time_of_day`, `suspend_time_of_day`. The Unicorn vectors (`agile_tod.txt`: 7 cycles of 80,000 ticks incl. 240 s / 24 h and
120 s / 24 h scale 2.0) fix the wrap ticks and the values; e.g. 240 s / 24 h, start 0: first wrap on tick 48058 (queue time
240.0), the value runs 24.0 -> 0.0 -> ... exactly as the engine says, not as `scale * t`.

**Building life under construction** (`construction.md` section 13.2): a property for the life (start `A * 0.1`) with a modifier
of the ramp rate; `Production` reads the property (limits `0` and the maximum life: UNSURE, not read).

## 7. Open items

- **UNSURE** limits (`lo`, `hi`) of the real remaining-time and life properties: the wrapper `Float::create` (`0x6F477250`) takes
  them from its arguments and the creating class was not read. The model uses the unclamped defaults for countdowns and `[0, max]`
  for the life; with unclamped limits a crossing is only ever detected by the 0.005 tolerance, with limits at 0 the value would stick
  at 0.0 (the timeline above differs only if a limit is hit before the tolerance, which the ticks do not do for D = 1.0).
- **UNSURE** a listener that is created already inside the tolerance (a total below 0.005): it fires inside the creation call. The
  model queues the firing and the production reacts on the next tick (a countdown of 0 s completes one tick late).
- **UNSURE** the owner's destroy path of a finished countdown (listener and modifier): the model removes both when the firing is
  handled. The countdown modifier would otherwise end at `R / D + 1.0` and add `+D` back to the (already finished) property.
- **UNSURE** the creation order of the life modifier and the countdown of a building (only ties of equal expiry could tell).
- **UNSURE** the start value of the time of day at game start (melee scripts set 8.0; otherwise the engine keeps 0.0) and whether
  the engine starts the modifier at map load (`0x6F429770` is reached when the suspension flags clear; the model starts it at
  `engine::data::install` when a Misc day cycle exists).
- The unit life and mana properties, their regeneration and the day / night watch are decoded in sections 9 and 10; the
  cooldown is **not** an agile object (section 11); the census of the remaining `CPo*` classes with their users is section 12.
  Still undecoded: `CPoRePulseMod`, `CPoFgPulseMod`, `CPoFgLis`, `CPoLink` / `CPoLkLis`, `CPoQuery`, and the movement classes
  (`CPoPos`, `CPoPoVelocityMod`, `CPoPoInterfLis`, `CPoPosCl`, `CPoPosBh`, `CPoSeparate`) as classes (the movement *model* is in
  `movement-6.md`, which is the velocity modifier's behaviour seen from the outside, LIKELY).

## 8. Verification (all runs: Unicorn on the real Game.dll, 0 mismatches)

Harness `tools/emu/agile_engine_emu.py` (real objects with the real vtables; the only replaced pieces are the world factory
`[G+0x10]` (allocates by tag and does what the constructors do), the handle allocator `0x6F4BC310`, `operator new`
`0x6F6EB5B2`, the observer registration, and the owner of a listener, which records the message).

| What | Function(s) | Vectors | Test |
|---|---|---|---|
| property value | `0x6F4773A0`, `0x6F477180` | 600 | `agile_fn_vectors::value_matches_the_original` |
| slope add | `0x6F4A9320` | 600 | `slope_add_matches_the_original` |
| modifier creation | `0x6F4774E0` (rate, horizon, timer expiry / sequence / flags / period) | 600 | `modifier_creation_matches_the_original` |
| listener decision | `0x6F4A9C70` (+ `0x6F4A9B70`, timer arming) | 600 | `listener_value_decision_matches_the_original` |
| listener jump | `0x6F4A9CF0`, `0x6F4A9940` | 600 | `listener_jump_decision_matches_the_original` |
| crossing time | `0x6F4A8FD0` | 600 | `crossing_time_matches_the_original` |
| whole engine | create / set / tick / 300 s restart: listener firings (tick, queue time, row) and values | 560 scenarios, 230 firings | `agile_vectors::scenarios_match_the_original` |
| countdown completion tick | `start_countdown` against the real modifier + listener | 36 (12 durations x 3 start ticks, incl. the 300 s restart) | `agile_wiring::the_bare_engine_agrees_with_the_unicorn_table` |
| production wiring | training, construction, research | 36 + 3 + 1 | `agile_wiring::training_completes_on_the_original_tick`, ... |
| second head starts inside the first completion | pump suspended in the callback | 30 | `a_second_head_starts_inside_the_first_completion` |
| time of day | modifier + rising listener + wrap handler in the callback | 7 cycles x 80,000 ticks | `the_time_of_day_wraps_on_the_original_ticks` |

Regenerate: `GAME_DLL=... py -I tools/emu/agile_engine_emu.py gen OUT 560 11` (scenarios), `fnvec DIR 600 3` (per function),
`cdvec`, `chainvec`, `todvec` (their output goes to `crates/sim/tests/vectors/agile_*.txt`). No Game.dll bytes are in the vectors.

## 9. Unit life and mana, and their regeneration (pass S17, 2026-10-12)

A unit's life and mana are **agile properties**; the base regeneration is a **forever continuous modifier** on them. Nothing is
applied per tick: the property is evaluated lazily (section 2), the modifier only adds its rate to the slope. Harness
`tools/emu/regen_emu.py` runs the real routines below on a synthetic unit (fs base set up for the SEH frames, `operator delete`
replaced by a no-op); model `crates/sim/src/regen.rs` (`UnitStats`), game side `crates/sim/src/unit_stats.rs`.

### 9.1 Fields of the unit (`CUnit`, constructor `0x6F28AC90`, vtable `0x6F931934`) - CONFIRMED by the constructor and the routines

| Offset | What |
|---|---|
| `+0x98` | `Float` wrapper (life). Upper limit = the maximum life (vf76 `0x6F28B080` reads it with `0x6F4773E0`; vf81 `0x6F28B0D0` sets it with `0x6F477400`), lower limit set by vf80 `0x6F28B0C0` (`0x6F477460`) |
| `+0xA8` | smart pointer to the death listener (section 9.7) |
| `+0xAC` / `+0xB0` | cell object (class vtable `0x6F876D38`, slot 0 `0x6F021D50` stores `[this+4] = *arg`, its second argument is ignored) / **life regeneration per second** |
| `+0xB4` | smart pointer to the life regeneration modifier wrapper |
| `+0xB8` | `Float` wrapper (mana); upper limit = maximum mana |
| `+0xC8` / `+0xCC` | cell, the "other resource" read by the mana check (`abilities-5.md` section 3); set to 0.0 at creation |
| `+0xD0` / `+0xD4` | cell / **mana regeneration per second** |
| `+0xD8` | smart pointer to the mana regeneration modifier wrapper |
| `+0x20` | flags: `0x40` life regeneration on, `0x80` life regeneration disabled, `0x100` mana regeneration disabled |
| `+0x5C` | flags: `0x100` dead |

### 9.2 Creation (`vf107` `0x6F2A0E30`, CONFIRMED structure; values from the creation record LIKELY)

1. Life (`0x6F2ACDE0`, early in vf107): vf80 lower limit **0.0** (constant `0x6FAAE470`), vf81 maximum, vf73 value (`0x6F28B1F0`:
   a value `<= 0` becomes `0.0`, then `0x6F477350`). A new property is `B = S = 0.0`, limits `-FLT_MAX` / `FLT_MAX`.
2. Mana: `0x6F477400` (upper limit, record `+0x60`) then `0x6F477350` (value, record `+0x64`); the **lower limit stays the default
   `-FLT_MAX`** (the earlier "floor 0.0" for mana in `heroes.md` was an assumption).
3. The cells: `+0xC8 := 0.0`; `+0xD0 := regenMana` (type record `+0x8C`, getter `0x6F29BF20`); `+0xAC := regenHP` (record `+0x7C`,
   `0x6F29BE20`), both through slot 0 (a plain store).
4. `0x6F29DF40`: clears flag `0x40`, reads `regenType` (record `+0x78`, getter `0x6F29BDF0`) and sets it for `1` (always), for `3`
   (day) when the day flag (world `+0x80`) is non-zero, for `4` (night) when it is zero; type `2` (blight) registers the unit with the
   blight system (`0x6F06F600`) and leaves the flag to the blight ability (section 9.6).
5. `0x6F28CAA0`: the death listener (`0x6F277370`), `+0x250 / +0x254 := -1`, **`0x6F282FC0` (life), `0x6F283F60` (mana)**, then two
   virtual calls (slots 82 and 40, presentation).

The revive routine `0x6F2A64A0` ends the same way (value set through vf73 and `0x6F477350`, then `0x6F28CAA0`).

`regenType` name table at `0x6FA70F88` (CONFIRMED): `always` 1, `blight` 2, `day` 3, `night` 4; anything else 0.

### 9.3 The refreshes (CONFIRMED: 703 scenarios, 5,916 operations, 0 mismatches)

* `0x6F282FC0` (life): **no modifier** when `+0x5C & 0x100` (dead), or `+0x20 & 0x40 == 0`, or `+0x20 & 0x80 != 0`: the smart pointer
  is released (`0x6F02C960`: wrapper slot 2 = destroy, section 9.4). Otherwise: if the pointer is set and its handle valid
  (`0x6F03FA30`) the **rate setter** `0x6F4A9B00` is called with `+0xB0`; else a **new forever modifier** (`0x6F4774E0` on `+0x98`, rate
  `+0xB0`, horizon 0.0) is created and assigned to `+0xB4`.
* `0x6F283F60` (mana): no modifier when dead, or when the maximum mana is **not above 0.0** (`0.0 < hi` fails, NaN too), or when
  `+0x20 & 0x100`. Otherwise the same with `+0xD4`, `+0xB8`, `+0xD8` (creator `0x6F02DD00`).
* **Rate setter** `0x6F4A9B00` (`Agile::set_modifier_rate`): `d = |old - new|`; skipped when `2e-7 > d`; otherwise
  `slope add(new - old)` (section 2) on the property and the new rate is stored. A change smaller than 2e-7 is *ignored* (the old
  rate stays).
* Setters that end in a refresh: `0x6F28CAF0` / `0x6F28CB00` (flag `0x40` on / off), `0x6F28CB10` / `0x6F28CB20` (flag `0x80`),
  `0x6F28D570` / `0x6F28D580` (flag `0x100`, mana refresh), **`0x6F28CB30`** (`+0xB0 := add(+0xB0, delta)` through slot 0, then the
  life refresh) and `0x6F28D590` (the same for `+0xD4`). The death and removal paths (`0x6F29D320`, `0x6F2A1F20`) refresh both.
* The refreshes are **not** re-run when the maximum life or mana changes: a unit that gains mana capacity from 0 gets its mana
  modifier only at the next refresh.

### 9.4 Releasing a modifier is delayed (CONFIRMED)

The wrapper's slot 2 (`0x6F478920`) invalidates the wrapper's handle **at once** (so the next refresh creates a new modifier), but
the object's destroy `0x6F4A6970` only **arms a timer on the property queue** (duration 0.0, therefore the 1e-4 s minimum;
sequence number taken at the call) and stores it at `+0x20`. When that timer runs in a later pump (`0x6F4A69B0` with the entry
`== +0x20`, slot 4 `0x6F4A8EB0` -> `0x6F4A6920`) the object sends itself `^end` (the rate leaves the slope, `0x6F4A9F80`), cancels its
timers and is freed. Consequences, all visible in the vectors:

* after a release the **slope is unchanged until the next pump**, and the property still advances at the old rate for the extra
  1e-4 s (`Agile::destroy_modifier`, `Owner::ModifierDestroy`); a corpse therefore ends with a life of about `rate * 1e-4` rather
  than exactly 0.0;
* a modifier released and re-created in the same tick (a flag toggled off and on) coexists with its predecessor for 1e-4 s: the
  slope is the sum.

Listeners go through the same base-class path (`Agile::destroy_listener`, section 10). The countdown completion in `production.rs`
still removes its listener and modifier immediately (UNSURE, no observable difference found).

### 9.5 Day and night per unit (CONFIRMED structure; flag rule and message mapping by disassembly + the watch vectors)

The world's day flag changes send `0x80264` (day begins) / `0x80265` (night begins) to every unit (section 10). The unit's message
switch `0x6F2A7E60` maps them to `0x6F2A0330` / `0x6F2A0420`: for `regenType` 3 the former sets flag `0x40` and the latter clears it,
for type 4 the other way round (`0x6F28CAF0` / `0x6F28CB00`, each ends in the life refresh). So **night elves regenerate by night**
(type `night`) and by day nothing: at dawn their modifier is released (delayed destroy), at dusk a new one is created. The same
handlers also move the sight ramp (`0x6F27A4E0`, message `0xD0215` / `0xD0216`), which is not regeneration.
`Game::units_day_changed`.

### 9.6 Other life / mana regeneration sources (LIKELY: callers of the modifier wrappers, not run)

Each source owns **its own forever modifier** on the unit's property (created with `0x6F02DC60` for life, `0x6F02DD00` for mana, kept
in a smart pointer of the source, released when the source goes away); the slopes add. Found callers:

| Source | Life (`0x6F02DC60`) | Mana (`0x6F02DD00`) |
|---|---|---|
| Hero strength / intelligence | `0x6F209090` (rate `0x6F2080F0` = `I2R(str) * StrRegenBonus`; the old modifier is released first and a **new one is made at every change**, whatever the change; stored at hero `+0x13C`) | `0x6F2092A0` (rate `0x6F2082C0` = `IntRegenBonus` based; gated by bit `0x80` of the hero ability flags; so `IntRegenBonus` **is** read, correcting `heroes.md` section 10) |
| Aura buffs (Unholy `AUau`, Brilliance `AHab`) | `0x6F1AD810` (helper of `CBuffAuraUnholy`, slots 3 / 220 / 221; rate `DataB`, or `maxLife * DataB` with the percent flag; kept at buff `+0xF0`) | `0x6F1AC990` (helper of `CBuffAuraBrilliance`) |
| Ability classes `CAbilityRegenLife`, `CAbilityNeutralRegenLife`, `CAbilityTranquilityRegen` | `0x6F0FB0C0` (slot 201) | `CAbilityRegenMana`, `CAbilityNeutralRegenMana` `0x6F0FB240`, `CAbilityItemRegenMana` `0x6F0FB370` |
| Immolation family (`CBuffImmolation`, `CBuffPermImmolation`, `CBuffPandaImmolation`, `CBuffItemCloakOfFlames`) | | `0x6F123070`, `0x6F12C4D0`, `0x6F12C690` (the mana drain as a negative rate) |
| `CBuffManaDrain`, `CBuffMindRot` | | `0x6F0D5660` |
| Other callers (classes not identified) | `0x6F043A20`, `0x6F043FA0`, `0x6F045600`, `0x6F070AB0`, `0x6F11C4C0`, `0x6F123920`, `0x6F1B6D40`, `0x6F1E5BF0` | `0x6F043CE0`, `0x6F0D37C0`, `0x6F0D57E0`, `0x6F1175E0`, `0x6F123A00` |

The model wires the hero strength regeneration (`Game::set_hero_regen`) and the two aura buffs (`AuraBuff::regen_mod`). Blight
regeneration (type 2): the ability class that evaluates blight at the unit's position (`0x6F044FE0` and neighbours: flag `0x80` of
the *ability* tracks "on blight", then `0x6F28CAF0` / `0x6F28CB00`) is not modelled; `Game::set_unit_life_regen` is the host hook.
`0x6F05FF50` / `0x6F060020` (`CAbilityBuildInProgress` start / completion) and `0x6F158FA0` / `0x6F1708D0` (`CBuffUnsummon`) set
and clear flag `0x80`: a building under construction has no life regeneration (the model sets `0x80` and `0x100` at the start of a
construction and clears them at the completion).

### 9.7 The death listener (CONFIRMED structure, effect LIKELY)

`0x6F277370` (run first by `0x6F28CAA0`, once: it does nothing when `+0xA8` is set) creates a **falling listener (mode 0) on the life
property** at threshold `0x6FAAE4C4` (runtime **0.4**, `runtime-constants-audit.md`) with message **`0xD019F`**, owner the unit, kept at
`+0xA8`. By the listener rules of section 4 it fires when the life falls below 0.4 (jump: `0.4 < old` and `new < 0.4`, or `new`
within 0.005 of the threshold) and when the value reaches it by regeneration. So a unit is dead at **life below about 0.4**, not at
0. `unit::set_life` now takes the listener's firing as the death (`UnitStats::death`, `Agile::take_fired`); a firing that comes from
the pump (negative regeneration reaching 0.4) is not turned into a death yet.

### 9.8 Verification (Unicorn, real routines, 0 mismatches)

| What | Routines | Vectors | Test |
|---|---|---|---|
| refreshes, flags, rate additions, death / revive, limits, value sets, ticks | `0x6F282FC0`, `0x6F283F60`, `0x6F28CAF0..0x6F28CB30`, `0x6F28D570..0x6F28D590`, `0x6F477350/00/60`, `0x6F4A9B00`, destroy `0x6F478920 -> 0x6F4A6970 -> 0x6F4A69B0 -> 0x6F4A6920`, the pump | 703 scenarios (700 random of 4-9 operations each, three 162 s runs through the 150 s timer), 5,916 snapshots of values, slopes, bases, rates and modifier state | `regen_vectors::regeneration_matches_the_original` |
| creation of the two properties | `0x6F477460`, `0x6F477400`, `0x6F477350` on a fresh property | 150 | `regen_vectors::creation_matches_the_original` |

The scenarios mix rates (zero, tiny below 2e-7, negative, large), start clocks (also near the 300 s restart), maxima (also 0 and
negative), flags and operation orders. `Agile::set_limits` (`0x6F4A9390`) follows the disassembly: `lo > B` gives `lo`, else
`hi < B` gives `hi` (this differs from `heroes::clamp_pair` for an inverted pair, which the vectors do not reach: `lo` is 0 or
negative there). `heroes::life_sink` / `mana_sink` now run on the property through `Game::life_cells` / `put_life_cells` (the cells are
the property refreshed to now; the slope is kept).

## 10. The day / night watch (`0x6F429460`, CONFIRMED: 60 scenarios, 178 flag changes, 252 evaluations, 90 wraps, 0 mismatches)

`T` is the game-state object (`CGameState`, whose message handler is `0x6F42FC80`; not the world `G` of section 1: `G+0x68..` holds
the second queue; the float game states are `Float` wrappers at `T + 8 + 16 * i`, so `+0x28` is
float game state 2, `+0x78` / `+0x7C` the two listeners, `+0x80` the day flag as an `Integer` wrapper). The routine runs at start-up
(`0x6F42C210` first stores the flag from the day rule `Dawn <= t < Dusk`, then calls it) and **whenever message `0x80263` arrives**
(`0x6F42FC80` case `0x80263`). It
1. releases both listeners (`+0x78`, `+0x7C`; the delayed destroy of section 9.4 applies, a released listener can still fire before
   its timer runs);
2. reads the time of day `v` (float state 2), `Dawn`, `Dusk` and `c = 0.01` (`0x6FAAE480`);
3. - `Dawn > v` (before dawn): **A** = listener mode 1 at `Dawn + c`, message `0x80263`; flag 0;
   - else `Dusk <= v` (after dusk): **B** = listener mode 0 at `Dusk - c`; flag 0 (the wrap of the cycle to 0.0 jumps across it and wakes
     the watch at once, same instant as the wrap);
   - else (day): **A** = mode 0 at `Dawn - c`, then **B** = mode 1 at `Dusk + c`; flag 1;
4. when the flag differs from the stored one: store it and send `0x80264` (flag 1) or `0x80265` (flag 0) to the world (slot 4) and
   to every unit through the group builder `0x6F2A3270` (callback `0x6F4269B0`).

So the day starts when the time of day reaches `Dawn + 0.01` hours (240 s / 24 h: tick 12,023 for Dawn 6) and ends at `Dusk + 0.01`.
Code: `daynight::DayWatch` (`recompute`, `owns`), `Game::day_watch_run`, `Game::day_flag`; the pump handler in
`Production::agile_tick` calls it for any listener of the watch. Harness `tools/emu/daynight_emu.py` (replaced: the Misc reader
`0x6F009EE0`, the integer cell `0x6F473170` / `0x6F473140`, the world's self dispatch, the broadcast `0x6F2A3270`, `operator delete`);
test `daynight_vectors::day_night_watch_matches_the_original`.

## 11. Cooldowns are not agile (CONFIRMED by disassembly)

The spell cooldown (`abilities-5.md` section 2) is flag `0x200` plus a **`CAgentTimer`** of message `0xD01BE`: `0x6F050B70` calls the
timer primitive `0x6F4778F0`, which ends in `CAgentTimer::vf2` (`0x6F477FF0`) with queue selector 0, i.e. the **JASS timer queue**
(`[0x6FAB7368] + 0x40`), through the minimum clamp `0x6F477F80` (1e-4 s) - the same path as `TimerStart`. It is not a property, a
modifier or a listener; the remaining cooldown is `expiry - now` of that timer. `cast::start_cooldown` already uses the `TimerQueue`
with the clamp, so nothing changes. (The UI refresh `0x6F021270` is presentation.)

## 12. Census of the other `CPo*` classes (RTTI `NIpse::`)

Objects are made by the world factory `0x6F48E280` from a tag pair (the second dword picks the class); `InstanceGenerator` helpers
(`0x6F48D440` ... `0x6F48DE20`) and the `vf3` initialisers do the rest. Status: **decoded** = run in Unicorn; **role** = from the
class name, the factory tag and the owners of the creating code; no behaviour claimed beyond that.

| Class | Tag pair | Constructor | Status | Role and known users |
|---|---|---|---|---|
| `CPoReal` | `^pro` `ler` | `0x6F48C3A0` | decoded (sections 2, 9) | float property: countdowns, time of day, unit life and mana |
| `CPoReContinuousMod` | `^mod` `rcm` | `0x6F48AE90` | decoded (3) | forever / timed rate: countdowns, regeneration, time of day |
| `CPoReThresholdLis` | `^lis` `rtl` | `0x6F48B420` | decoded (4, 10) | completion, wrap, day / night watch, unit death |
| `CPoRePulseMod` | `^mod` `rpm` | `0x6F48B100` | **not decoded** | periodic rate (a pulse of a given length every period). Creators `0x6F478AD0`, `0x6F478DE0`; the `Float` wrapper (vtable `0x6F877BA8`) and `FloatModifier` expose it. Users not traced (periodic buffs are the candidates: `CBuffAuraRegenLife` pulses at 2 s, `buffs.md`) |
| `CPoFlag` | `^pro` `flg` | `0x6F48D8F0` | partly (the day flag) | integer / boolean property behind the `Integer` wrapper (value at object `+0x78`, change notice `0x6F4A8980`). Users: `CGameState` (`0x6F427CC0`: the day flag, the time-of-day suspension), `CAlliances` (`0x6F3DF050`), `CPlayerWar3` (`0x6F40F320`), the hero object (`0x6F207490`, `0x6F207530`) |
| `CPoFgPulseMod`, `CPoFgLis` | `^mod` `fpm`, `^lis` `flc` | `0x6F48B970`, `0x6F48BCC0` | not decoded | flag pulse modifier (a flag on for a while) and flag listener; creators `0x6F480800`, `0x6F480890`, `0x6F480910`, `0x6F480B60..0x6F480C40`; wrappers `ToggleModifier` / `StepModifier` / `IntegerModifier` / `IntegerListener` (vtables `0x6F951B9C`, `0x6F951BD0`, `0x6F951B78`, `0x6F8785F4`) |
| `CPoLink`, `CPoLkLis` | `^pro` `nlk`, `^lis` `lkl` | `0x6F48D950`, `0x6F48C250` | not decoded | a relation between two objects and its listener; creators `0x6F478420`, `0x6F4784D0`, `0x6F481050..0x6F481100`, `0x6F4AA0C0`, `0x6F4AA250` |
| `CPoPos` | `^pro` `pos` | `0x6F4875F0` | not decoded as a class | the position property (x, y in cells) behind `Position` / `SmartPosition` (`0x6F473A00`). Movement (`movement-6.md`): the stored position is a snapshot, the value is `cell + V * (t - snapshot)` evaluated on read, which is exactly a property plus a velocity modifier |
| `CPoPoVelocityMod` | `^mod` `pvm` | `0x6F48B680` | not decoded as a class | the velocity of a moving unit; creators `0x6F47C5B0`, `0x6F47C6A0` (callers `0x6F47C610`, `0x6F47C7F0`) |
| `CPoPoInterfLis` | `^lis` `pil` | `0x6F48C6E0` | not decoded | the interference listener of two positions (fires when two moving objects come within a distance); creators `0x6F478FE0`, `0x6F4793D0`, `0x6F4A7E10`, `0x6F4A7E70`; the `RangeListener` wrapper (vtable `0x6F878618`) |
| `CPoPosCl` | `^pro` `puc` | `0x6F487A80` | not decoded | a cluster of positions in the proximity map (the world's fourth queue `G+0x164`, section 5): users `SmartPosition` (`0x6F473A00`), the mover code `0x6F493B60`, the projectile behaviour `CBhPoProjectile` (`0x6F497340`, the missile flight), `0x6F495140` (class method, push-apart gather) |
| `CPoSeparate` | (internal, `0x6F484360`, `0x6F486540`, `0x6F489750`, `0x6F4958D0`) | | not decoded | the push-apart of `movement-7.md` (gather `0x6F495D60` -> `0x6F495CA0` -> `0x6F495B00`) |
| `CPoPosBh` | `^pro` `pbh` | `0x6F48C9D0` | not decoded | position behaviour; creators `0x6F479AC0`, `0x6F4AACC0` |
| `CPoQuery` | (inside `0x6F489750`) | | not decoded | the spatial query object of the separation scan |

Regeneration, cooldowns and the day cycle need none of the undecoded classes. Movement (`CPoPos`, `CPoPoVelocityMod`, `CPoPosCl`,
`CPoPoInterfLis`) and the missile flight (`CBhPoProjectile`) are the next users; vision needs none of them (its grid is plain arrays,
`vision.md`).

## 13. The model

* `regen::UnitStats` (refreshes, flags, rates, death, `start_death_watch`), `agile::{set_modifier_rate, destroy_modifier,
  destroy_listener, set_limits, limits, refreshed_cells, put_cells, take_fired}`, `daynight::DayWatch`.
* `Game` (`unit_stats.rs`): `UnitObj::stats` exists when the softfloat tables are loaded (created at `unit::build`); `life`, `mana`,
  `max_life`, `max_mana` of the unit are then a **cache** of the properties, refreshed after every write path and after every agile
  tick (`sync_unit_stats`, end of `Production::agile_tick`). Write paths: `write_life` (`unit::set_life`, building growth, upgrades),
  `write_mana` (`SetUnitState`, spell payment), `write_max_life`, the hero sinks. Hosts and tests that used to assign
  `unit_mut(k).life` call `write_life` / `write_mana`. Without tables nothing regenerates (as before) and the fields are plain.
* `UnitType::{regen_hp, regen_mana, regen_type}` come from `UnitBalance` (`engine::data`); `Game::tick_regen` (the old stand-in) now
  skips units with stats.
* Cost: 400 regenerating units tick 4,000 times in 0.2 s (debug build, `regen_perf.rs`).

### 13.1 Open items

- **UNSURE** `regenType` name matching (case) and the creation record's life / maximum values (the model uses the type's maximum
  for both, like before); the hero regeneration is not applied at hero creation (only when an attribute changes, as before); the
  hero's regeneration modifier is released at death (the original ties it to nothing that was read); the adjustment `0x6F2D4510` /
  `0x6F2D4560` of the hero rates for flagged owners is not modelled; hero mana regeneration (`0x6F2092A0`, `IntRegenBonus`) not wired.
- **UNSURE** the death of a unit whose life reaches 0.4 by *regeneration being negative* (the listener fires from the pump).
- Blight regeneration, item / ability regeneration classes (section 9.6), `CBuffManaDrain` / Immolation as modifiers: not wired.
- The mana regeneration refresh is not re-run when the maximum mana changes (as in the original), so a model that lets a unit gain
  mana capacity relies on the host calling `add_unit_mana_regen_rate(0)`.
- `Agile::next_due` scans every pending timer per pop; fine for hundreds of units, a heap would be needed for thousands.
