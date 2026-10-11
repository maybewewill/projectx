# Lane F natives (food use, corpses, build orders, hotkeys, sound wait, rescue and sell getters)

Readings of the eight natives a real melee map referenced that were unbound (map "(2)Circumvention": 55 -> 8 after lanes
A-E, 8 -> 0 here: `w3boot` reports "natives referenced but not implemented (0)"). Method: IDA decompile and disassembly
of Game.dll 1.26a, plus Unicorn runs of the real instructions where a piece could be isolated. CONFIRMED = machine code
read (or run), LIKELY = inference from the code, UNSURE = guessed or not read. Code: `crates/sim/src/natives/lane_f.rs`
(and `event.rs` for the two getters). No decompiled code is kept; this file describes behaviour.

## 1. SetUnitUseFood `0x6F3C54A0` (CONFIRMED)

* The unit handle resolves (`0x6F3BDCB0`), the owner is the unit's player object (`0x6F2834E0`).
* The unit's registration word (`unit+0x64`) holds the food bits: `0x100`/`0x200` = the type's food used is applied
  (`0x300` together), `0x400`/`0x800` = the food made is applied (`0xC00`).
* `true`: if bit `0x200` is clear, add the type's food used (`0x6F29BAF0`, type record `+0x5C`) to the owner's food-used
  counter (`0x6F40F5E0`: the counter at player `+0x90`, then the peak word `+0x24C` is raised when the new value is above it,
  unsigned compare); then `word |= 0x300`.
* `false`: if bit `0x200` is set, subtract the same value; then `word &= ~0x300`.
* Model: `UnitObj::food_reg`, `Game::lane_f.food` (used/made per player), the player state 5 (food used) follows, the
  peaks go to `lane.stats` (`FOOD_MAX_USED` / `FOOD_MAX_PRODUCED`). Because the baseline is needed, the model now also does
  the confirmed creation tail (`0x6F2A64A0`, flags `0x502`: food used then food made, words `0x300` and `0xC00`), the unit
  teardown release (`0x6F29D320`: made first, then used, tests bits `0x800` / `0x200`; reached from `RemoveUnit`), and the
  owner-change move (`0x6F2A4BE0`, order inside it not read: LIKELY).
* Gaps: food is not released when a unit merely dies (the call site of the release for a death is not located), the food
  made is not folded into the cap that `GetPlayerState(FOOD_CAP)` returns (min with `+0xA0` and the ceiling), and the
  upkeep refresh (`0x6F40F620`) is not triggered.

## 2. CreateCorpse `0x6F3C5350` and UnitSuspendDecay `0x6F3C81F0`

CreateCorpse (CONFIRMED sequence): player must resolve (else null); angle * pi/180 (soft-float multiply by `0x6FAAE5E8`);
the same builder `CreateUnit` uses (`0x6F24F1F0`; null on failure) with no handle yet; then the teardown
`0x6F2A63B0(unit, 0, 0)`; then notice `0xD015A`, `0xD0159` when type flag `0x200` (unit word `+0x5C`) is set and `0xD0158`
when `0x1000` is clear; drain; the handle id is allocated last.

The teardown `0x6F2A63B0` is the same core `RemoveUnit` ends in, with its first two flags clear: heroes (type id first byte
'A'..'Z', not an illusion) leave their two category lists (`0x6F26EBD0`), the stop step `0x6F2964E0` (notice `0xD0144`,
all order lists emptied, `0x6F2831A0`), the pathing-disabled counter `unit+0x198` is incremented (`0x6F2A3CC0` ->
`0x6F2A0830`, which re-registers the unit's footprint), notices `0xD0164`, `0xD0178`, `0xD0144`, drain. It does NOT set
the removed flag.

Model: `unit::build` (no handle), `Game::stop_all_orders`, `LaneF::corpses[unit].pathing_off += 1`, the notices in that
order. LIKELY/UNSURE: the unit is made dead (life `+0.0`, `dead = true`, no death event). No code path in the sequence above
writes the life; the death state may come from the animation state `4` that slot 35 stores in the builder, or from the
`0xD0164` handlers. The sim marks the unit dead because that is what scripts observe in the real game (community-known),
not because the code was traced. The hero category removal and the decay flags `0x200`/`0x1000` (type bits, not in the
registry) are not modelled, so only `0xD015A` is posted after the teardown.

UnitSuspendDecay: the unit resolves; gate (CONFIRMED) `word(+0x5C) & 0xC00 != 0` and `& 0x100 != 0` (0xC00 = a corpse
decay phase, 0x100 = dead/locked; the sim treats "dead and not removed" as that). Then the stop step `0x6F2964E0` runs; for
`suspend = false` it also posts `0xD015A`, `0xD0159` and drains. `suspend = true` stops after the stop step. LIKELY: the
decay timers are cancelled through that stop step (the decay itself is not simulated, so only the notices and the
`LaneFEvent::DecayStop` record are visible).

## 3. IssueBuildOrderById `0x6F3C8BE0` / IssueBuildOrder `0x6F3C8E80`

* `IssueBuildOrder` resolves the unit name to a type id (`0x6F3BB650`, the `UnitId` lookup) and returns false when it is 0,
  then continues as `IssueBuildOrderById(unit, id, x, y)` (CONFIRMED).
* ById (CONFIRMED structure): the unit handle resolves; the placement validator `0x6F2A50C0(point, ..., unit type id,
  owner, ...)` returns a code, and codes `0` and `0x45` ('E') let the order go on, anything else returns false; then the
  ability acceptance `0x6F279560(unit, order id = type id, ...)` (the loop over the unit's abilities whose class selector is
  0, returns 0 when one accepts; `221` when none does); then the point order is built (`0x6F294B30`) and handed to the
  dispatcher `0x6F2A4AB0(unit, order, replace = 1, drain = 1)`; returns true.
* The validator `0x6F2A50C0` is a 3 KB general "can this be placed here" routine (footprint against the pathing cells,
  other units in the way, which it orders to move away: that is the `0x45` result, tech and blight tests). It is not
  reproduced. `Game::set_placement_validator` takes a function; with none, a registered unit type passes (code 0) and an
  unknown one fails (code 68).
* Ability acceptance: the six build abilities (`ANbu AHbu AEbu AGbu AObu AUbu`, fixed slot `+0x1F4`) are now
  `BaseKind::Build`; they accept any order id that is a raw code (top byte non-zero; UNSURE: the original checks the unit's
  buildable list). A unit with no build ability gets false. The order is class Point (LIKELY) with the type id as order id,
  goes through the existing `issue_order` (limit 500, replace, issued-order events), and completes at once because no
  construction driver exists in the sim (`construction.rs` has the registration transition and percent setters only; the
  timer that drives progress is not found, see construction.md).

## 4. GetLocalizedHotkey `0x6F3B2D80` (CONFIRMED structure, LIKELY data source)

The earlier note (a virtual-key table) was wrong. The native reads the string argument (`0x6F4C4630`); null or empty gives
0. Otherwise `0x6F009F90(section "HotKeys", key, index 0)` looks the key up in the string tables, layered: a map-level
override pair first, else the profile tables (`0x6F4D21B0` selects the table that holds the section). The first value of
the key (`0x6F4D1EB0`, index 0 of the comma-separated list) is converted by `0x6F4CE050`: text starting with `'` is a raw
code of at most four characters (stops at the closing quote or the end, big-endian fold), anything else is the C runtime
`strtol(text, 0, 0)` (decimal, `0x` hex, leading-zero octal, saturating). A missing key gives 0.
Model: `LaneF::hotkeys` (lower-cased key to value list, filled by the host from the text files; the key case rule of the
table hash is not read: LIKELY case-insensitive). Unicorn: the quoted branch of `0x6F4CE050` is run on 300 texts
(`tools/emu/hotkey_text_emu.py`, `tests/hotkey_text_vectors.txt`, 300/300). `strtol` is imported from MSVCR80, so
`c_strtol` follows the C rules and is unit-tested only.

## 5. TriggerWaitForSound `0x6F3C3F20` (CONFIRMED structure)

The native resolves the sound handle (`0x6F3BE910`; 0 for a bad handle) and calls `0x6F44B2F0(offset, sound)`, which marks
the thread suspended (state 1) and stores the sound at thread `+0x38` and the offset real at `+0x40`. `TriggerSleepAction`
is the same call with sound 0. The earlier note ("woken by the sound system") was wrong: the trigger-execution state
machine `0x6F447340` (case 3, thread suspended) reads `+0x38`; a non-zero value that is not the thread itself is the
sound wait, and it starts the resume timer itself:

* `T = I2R(length_ms of the sound, 0x6F3DC110) / 1000.0` (`0x6FAAE5B4` = 1000.0 at run time).
* `elapsed` = the playing instance's position in seconds (`0x6F3E8BF0`; the same `T` when the sound has no playing
  instance, which is what a sound that is not playing reports).
* `d = T - offset`; if `elapsed > d` the wait is `0.0`, else `d - elapsed`; if the result is below `0.0` it is `0.0`.
* The timer (`0x6F477A80`) takes this wait (the timer start clamps to 1e-4 s as for every timer).
* One earlier branch uses a fixed wait (`0x6FAB7224` = 0.075 s at run time) when `0x6FA76030 & 8` and `0x6FAB50D4 & 1`
  (the sound subsystem's debug/logging flag, 0 in a normal game); the model takes the normal branch.

Headless behaviour: a sound has the length stored by `SetSoundDuration` (or the file length when known) and `playing` is
false unless the host started it, so `TriggerWaitForSound(s, 0.0)` waits `0` (resumes after the minimum timer), a negative
offset waits `-offset`, a positive offset waits `0`. A sound with length 0 behaves the same way. A bad handle sleeps the
offset (`clamp_sleep`, like `TriggerSleepAction`). Without the soft-float tables the wait is the minimum (UNSURE: the
division needs them). Unicorn: the real instructions `0x6F4475F8..0x6F4476A0` run with the length and position getters
stubbed, 400 cases (143 with a non-zero wait), all matched by `wait_from_timing` (`tools/emu/sound_wait_emu.py`,
`tests/sound_wait_vectors.rs`, 400/400). The offset slot of the frame (`esp+0x18`, written by `0x6F44B3C0`) was matched by
layout, not by running that function.

## 6. GetRescuer `0x6F3C2390` and the rescue event (CONFIRMED getter, host fires)

`GetRescuer` answers for event ids `0x13` (player-unit rescued; payload slot `+0x38`) and `0x3F` (unit rescued; payload slot
`+0x2C`), and returns null otherwise. The pending note that blamed the attack swing gate was wrong: `0x6F297DA0` and
`0x6F297EC0`, called from the gate `0x6F0CD350`, post `0x8023D` (acquired/in-range, id `0x3D`) and `0x8023E` (unit
attacked, id `0x3E`). The rescue events are posted by `0x6F298000(rescued, rescuer)`: first the player-unit event `0x13` on
the rescued unit's owner (`0x6F41C920`), then the unit event `0x3F`, trigger unit = the rescued unit, rescuer in the payload.
`0x6F298000` has one caller, the message handler `0x6F1EC030` of the rescue ability class (vtable data reference at
`0x6F9174F0`): it requires the message `0xD0257` carrying a live unit (not removed; if flag `0x20000000` it needs
life > 0), that the unit's owner bit is in the ability's player mask (`this+0x6C`), and that the ability's unit passes
`0x6F0418A0`; after posting it calls `0x6F079CC0` and `0x6F021250` on the ability (UNSURE: the ownership transfer). The sender of
`0xD0257` is not located (its id is only returned by a getter, `0x6F1388C0`, stored in a vtable at `0x6F8D8814`, so the post
is generic). The sim has no rescue ability or proximity trigger, so `event::fire_rescued(game, rt, rescued, rescuer)` is
the host entry point (CONFIRMED order and payload). The swing gate does have a branch that issues an order instead of
swinging (`this+0x220 > 0`, unit flag bit 2, `0x6F03FDC0`, `0x6F27A320`); it is not a rescue and `combat.rs` is unchanged.

## 7. GetSellingUnit `0x6F3C2560` (CONFIRMED getter, host fires)

Event ids `0x10D` (sell unit), `0x10F` (sell item), `0x115` (pawn item): the shop is payload slot `+0x2C` (the first of
the three unit slots; sold is `+0x38`, buyer `+0x44`). Ids `0x11E`, `0x120`, `0x126` (the unit-listener twins): the
trigger unit (slot 0, with the usual live-unit check), because the shop is the listener's own unit. Any other event gives
null. Added `Field::Seller`. The sell events are posted by `0x6F41E370` (and siblings at `0x6F41E4xx`/`0x6F298xxx`), called
from the sell-unit completion `0x6F298FA0`, itself called from the sell ability's order handler `0x6F04E3B0`. The buy order,
payment and spawn of a bought unit are not modelled (`shop.rs` has the stock lists only), so the firing site stays with the
host (`event::fire_event` with the payload), like `GetSoldUnit` and `GetBuyingUnit`.

## Tests and vectors

* `crates/sim/tests/lane_f_natives.rs` (13): strtol rules, hotkey text, hotkey native, food toggling, owner move of food,
  corpse notices, suspend-decay gate, build order acceptance and the placement codes, sound wait, 300 Unicorn hotkey cases.
* `crates/sim/tests/lane_f_events.rs` (3): rescue order and payload, rescuer null for other events, selling unit for the
  payload and trigger-unit forms.
* `crates/sim/tests/sound_wait_vectors.rs`: 400 Unicorn cases.
* Support tables: the rows of `CreateCorpse`, `IssueBuildOrder`, `IssueBuildOrderById`, `GetRescuer`, `GetSellingUnit` moved
  from `pending.rs` to `lane_f::NATIVES`; `SetUnitUseFood`, `UnitSuspendDecay` (unit_attr.rs), `GetLocalizedHotkey`,
  `TriggerWaitForSound` (lane_e.rs) changed support in place. `tests/support_tables.rs` includes the new table;
  `unit_attr_natives.rs` counts 85 implemented.

## Open

* Death-time food release; food made in the cap state; upkeep refresh.
* Corpse life/dead state and the decay phase bits; hero category removal.
* The placement validator `0x6F2A50C0` and the build ability's buildable-list check; the construction progress driver.
* The sender of the rescue message `0xD0257`; the sell/buy order handler.
