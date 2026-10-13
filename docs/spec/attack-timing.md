# Attack timing (cooldown, damage point, backswing, attack speed)

Status: timing routines CONFIRMED by Unicorn (`tools/emu/attack_timing_emu.py`, 600 random cases each for the five
routines, 3,000 total, 0 mismatches; vectors in `crates/sim/tests/attack_timing/`, test `crates/sim/tests/attack_timing.rs`).
Rust port: `crates/sim/src/attack_timing.rs`, wired into `combat.rs` (`start_swing`, `fire_execute`,
`combat_add_attack_speed`, called from `auras.rs::add_attack_speed`).

## Timers of the attack object
Three timer sub-objects: `A+0x1D8` message `0xD01B0` (cooldown end, handler `0x6F0D0C70`), `A+0x1EC` message `0xD01B1`
(damage point, handler `0x6F0D0D30` -> execute `0x6F0CF660`), `A+0x200` message `0xD01B2` (backswing end, handler
`0x6F0CD7B0`: clears busy flags and drains the order list). Correction: the old combat model called `0xD01B0` the swing end;
it is the cooldown end, the swing end is `0xD01B2`.

## Routines
* Multiplier `0x6F0C6EB0`: `base` (A+0x1B0, starts 1.0), plus the negative `slow` value (A+0x1C0) in the cooldown form when
  it is <= -0.001; clamped to [0.2, 5.0]; weapon modes 1/0x40/0x80/0x100 give 1.0.
* Cycle delay `0x6F0C8D70` = cooldown / M(cooldown form). Zero multiplier gives 1.0 / +-FLT_MAX (`0x6F0223C0`).
* Swing start (`0x6F0CD74D` -> `0x6F0CB9E0`): the cooldown timer is (re)started with the delay only if its remaining time is
  below the delay (stopped for a negative delay); the damage point timer = dmgpt / M(plain), at most `delay - 0.01`.
* Backswing `0x6F0C8DA0`, armed first thing in execute: backSw / M, at least 0.01; if less than 0.02 of cooldown is left
  the cooldown is restarted with 0.02; the backswing ends at least 0.01 before the cooldown.
* Speed bonus `0x6F0C9090`: stored value snapped to 1.0 (0.0 for slow) within 0.001; pending timers restarted with
  remaining / (new M / old M) when the ratio differs from 1 by >= 2e-7.
Constants (runtime values): 0.01 `0x6FAAE480`, 0.02 `0x6FAAE484`, 0.001 `0x6FAAE47C`, 2e-7 `0x6FAAE614`, 0.2 / 5.0 `0x6FAB0078/74`.

## Weapon choice (decoded, NOT ported)
`0x6F0C95D0(target_info, out_index)` (and the variant `0x6F0C93E0` with extra target checks) choose the weapon: for each
weapon w in 0..2 enabled by bit `0x80000 << w` of A+0x20, skipping modes the same way as the multiplier, the weapon target
word `A+0x218+4w` is tested against the target's flag word (`[t+4]`) with `0x6F25D560`; the first weapon that returns 0
is stored in `*out`; if none matches the lowest error code is returned (0xDD = cannot). With one usable weapon it is
used without the test. `0x6F25D560` -> `0x6F2552F0` (expands targs groups) -> `0x6F2553C0` (error code) and calls
`0x6F25D390`, `0x6F0766A0`: not ported, so the sim still has one attack record per type.

## weapTp launch paths (decoded structure, damage application NOT decoded)
Execute `0x6F0CF660` jump table `0x6F0D0228`: 1 normal `0x6F0CF889`, 2 missile `0x6F0CF9FE`, 3 artillery `0x6F0CFEF2`,
4 instant `0x6F0CF881`, 5 msplash `0x6F0CFB14` (launch `0x6F276600`, missile class setup `0x6F0CDF40`), 6 mbounce
`0x6F0CFC22` (helper `0x6F040900`, launch `0x6F2766A0`, `0x6F0CDFC0`), 7 mline `0x6F0CFDAC` (`0x6F040950`, launch
`0x6F276740`, `0x6F0CE040`), 8 aline `0x6F0D003B` (launch `0x6F2767E0`, `0x6F0CE0xx`). The launch record carries
`[0xE4]` the weapon value at A+0x100+4w, `[0xE8]/[0xEC]` area fields: msplash from the aux object (+0x28) and A+0xBC+16w,
mbounce/mline from A+0x110+16w and A+0x134+16w. The single-target arrival `0x6F2BE8B0` only calls the target routine
(vtable 0x120) once, so splash/bounce/line damage is applied by the per-type missile class, not found yet.
