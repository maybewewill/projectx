# The agile property engine, Game.dll 1.26a

Status: the property, modifier and listener semantics, their timer arming and the tick that pumps them are
**CONFIRMED in Unicorn** (real routines run on synthetic objects; numbers in section 8). The wiring of the game
objects that use the engine (what a listener's message does, which limits a property has) is LIKELY or UNSURE where
marked. Code: `crates/sim/src/agile.rs`. Harness: `tools/emu/agile_engine_emu.py`.

Labels: **CONFIRMED** = read from the disassembly and run in Unicorn with 0 mismatches; **LIKELY** = strong inference
from the disassembly; **UNSURE** = guess or not traced.

This engine (source path `engine\source\agile`, RTTI `NIpse::CPo*`) is what drives **every countdown that is not a
JASS timer**: construction, training, research, upgrade and revive progress, the life of a building under
construction, the time of day, and (not yet decoded) cooldowns, regeneration and unit movement. Completion of a
build or a training head is a *listener firing*, not a timer expiry and not a poll.

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
- Not decoded: the other `CPo*` classes (`CPoRePulseMod` periodic modifiers, `CPoFgPulseMod`, `CPoPoVelocityMod`,
  `CPoPoInterfLis`, `CPoFlag`, `CPoLink`) and the integer / range / toggle / step modifiers (`IntegerModifier`, `StepModifier`,
  `ToggleModifier`, `RangeListener`). Cooldowns, mana / life regeneration and unit movement are probably built from them.

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
