# Buffs and auras in Warcraft III 1.26a (Game.dll): lifecycle, refresh, expiry, timed life death, aura emitters

Scope: the `CBuff` lifecycle (apply, refresh, expiry, removal), the death route of timed life, the aura emitter
(`CAbilityAura*`) and the stat effects of the melee aura and buff classes. Labels: CONFIRMED = read in the disassembly
(and Unicorn-run where stated), LIKELY = strong inference, UNSURE = guess or open. Model: `crates/sim/src/auras.rs`
(`abilities.rs` holds the buff list). Unicorn tool: `tools/emu/buffs_emu.py`; vector test: `crates/sim/tests/buff_vectors.rs`
(set `BUFF_VECTORS` to the tool's output directory). Addresses are 1.26a; tables are not copied.

## 1. Result summary

| Question | Answer | Confidence |
|---|---|---|
| What kills a unit when its timed life ends | The removal notify (vtable slot 106, `0x6F0D5F10`) of `CBuffTimedLife`: when the buff has flag `0x80` (a duration is running or paused) it calls the owner's slot 73 with `0.0`. Slot 73 is `SetLife`, the same call `KillUnit` makes (native `0x6F3C8040`); it fires event `0xD01E6` through `0x6F2AB460`. So expiry and any other removal of a running timed life buff (Dispel Magic) kill the unit. The earlier reading "slot 214 returns 0, so the branch with the kill is skipped" was a misreading: slot 214 gates the other branch (flag/state checks and the special effect), not the SetLife call | CONFIRMED |
| Apply of a buff | Creator makes the object, `vf201(info)` copies the level's data (`0x6F042B20`), `vf213(source, target)` = `0x6F0D5010` runs the duration branch `0x6F0D4CB0` with duration `I2R(4)` for aura buffs; the strength is stored by `vf216` | CONFIRMED |
| Duration set rule `CBuff::vf212` `0x6F0D4EF0` | duration `<= 0` or NaN: cancel the timer, clear `0x80`. Positive: set `0x80`, restart the timer only if the duration exceeds the time left (never shorten). 1,200 Unicorn cases, 0 mismatches | CONFIRMED |
| Refresh `CBuffAura::vf212` `0x6F0D5070` | stored strength greater than new (tolerance 0.001): nothing at all. Else set duration 4.0; store the new strength if it is greater, or if equal and `vf215` says yes (Command/Trueshot: target is a hero; every other class: no). 2,400 Unicorn cases, 0 mismatches | CONFIRMED |
| Strength compares `0x6F07C450`, `0x6F0D2290`, `0x6F0D2380` | `stored < new` and `0.001 <= abs(sub(new, stored))`; `stored > new` and the same; `0.001 > abs(sub)`. 3,600 Unicorn cases | CONFIRMED |
| Stacking | Aura buffs: one buff per buff code per unit, found by `0x6F0787D0(code,...)` in `0x6F043770`; strongest wins. Timed life: no lookup, each application adds a buff. Slow: refresh replaces the factors. Stun: see 6 | CONFIRMED (aura, timed life, slow); UNSURE (stun lookup) |
| Aura emitter | repeating 2.0 s timer (msg `0xD01C1`: apply to owner) + proximity listener, radius `Area`, period `DIV(2.0, 4.0)` = exactly 0.5 s (Unicorn `0x3F000000`), message `0xD01C7` per unit in range | CONFIRMED structure; LIKELY that the proximity listener repeats each period |
| Aura buff duration | 4.0 s, topped up by every refresh | CONFIRMED |
| Aura lifetime after the source goes | up to 4.0 s (buffs expire, nothing removes them earlier) | CONFIRMED structure |
| `0xD01C1` on a buff | 0.1 s one-shot (`0x6FAAE4A0`) started on (re)attach (`vf197`) to retry the effect after it was removed; the effect functions are idempotent through flags | CONFIRMED |

## 2. Buff object (CONFIRMED layout bits)

`CBuff` (vtable 213 entries) is a `CAbility` subclass. Fields used: `+0x20` flags (`0x08` removed, `0x80` duration running,
`0x01000000` not from the owner, class flags `0x100/0x200/0x400` = "this component applied"), `+0x30` owner unit,
`+0x84` expiry timer object, `+0xA0/+0xA8` saved elapsed/remaining (pause), `+0xCC` strength cell (value at `+0xD0`).
Slots: 3 message handler; 41 destroy (calls 207 = remove effect, 209, cancels the timer); 106 removal notify; 197 re-attach;
200 duration branch (`0x6F0D4CB0`, shared with timed life); 201 copy info; 207 remove effect; 211 start expiry timer
(`0x6F0D30D0`, message `0xD01C4` on queue A, one shot); 212 set duration / refresh; 213 apply(source, target); 216 store
strength; 218 replace strength; 219 class init.

Expiry: message `0xD01C4` -> handler `0x6F0D3130` -> removal routine `0x6F079CC0` (marks `0x08`, calls slot 106 on every list
member, unlinks, releases; the release runs `vf41` which calls `vf207`, undoing the stat effect).

## 3. Timed life death (the open item of abilities-2.md 3.3)

`0x6F0D5F10` (slot 106, acts only for the removed buff itself): unregisters the buff event, and if flag `0x80`:
`0x6F3331D0(owner)` (UI bookkeeping) then `owner.vf73(&0.0)`. `vf73` = `0x6F28B1F0`: clamp the argument at 0 (NaN to 0),
write the life cell (`0x6F477350`), then `0x6F2AB460` which sends event `0xD01E6`. `KillUnit` is exactly `vf73(&0.0)`. The
branch of slot 214 (`0x6F0ADFF0`, returns 0) selects the eye-candy path only. Consequences, all implemented:
expiry kills; removing a running timed life buff by `UnitRemoveBuffs(Ex)` kills (the sim's `remove_buffs` returns `kill`
and the natives set life 0 and dispatch the death); a timed life with duration `<= 0` (flag `0x80` clear) is removed
without killing; pausing keeps the flag, so removal while paused also kills.

## 4. Aura emitter (CAbilityAura, vtable `0x6F87F3AC`)

Init (slot 32 `0x6F05F940`): level data into two cells: `+0x84` <- DataA, `+0x8C` <- Area (level getters
`0x6F02E9E0` DataA, `0x6F02E900` Area, `0x6F02EA50` DataB, `0x6F02EAC0` DataC, `0x6F02EB30` DataD); registers unit events
`0xD01A0`, `0xD01A1`, `0xD01A2`, `0xD01A7`; applies to the owner at once (`0x6F053C80`). Start (slot 57 `0x6F053B30`): timer
`I2R(2)` repeating, message `0xD01C1`; proximity listener around the owner (`0x6F474720` -> position system) with radius =
Area cell and period `DIV(2.0, 4.0)`; then the owner is applied if the unit is alive. Handler (slot 3 `0x6F05FA30`):
`0xD01C1` -> `0x6F053C80` (owner), `0xD01C7` -> `0x6F053CB0` (resolve the unit in the message, apply), `0x80261`
(state change) -> `0x6F0436A0` (toggle `0x80` and the art), `0xD01A2` registers the pair.

Apply `0x6F043770(ability, target)`: `vf208` target filter must return 0 (`0x6F02FD60`: the level's `targs` match via
`0x6F26D590`, and the enabled test); source must not be loaded/dead/removed; for a non-self target the source must not be an
illusion; then lookup of an existing buff with the level's `BuffID`: found -> `vf211` (`0x6F0438D0`, buff `vf212`), else
`vf212` creator (class specific, packs the level data: `0x6F042B20`) and `vf210` (store strength, `0x6F022CB0`). Brilliance
adds a filter (`vf208` `0x6F195B10`): the target must have mana; Command/Trueshot (`0x6F19F970`, `0x6F19FA70`) add a
melee/ranged restriction through DataB (`0x6F195EF0`, not decoded).

Order of targets of a scan: the position system's range query (the cell walk of groups-events-2.md).

## 5. Stat effects (CONFIRMED sinks, class meanings of the data fields LIKELY from the object data columns)

| Class (ability / buff) | Data | Effect and sink |
|---|---|---|
| Devotion `AHad` (`CBuffAuraDevotion`) | DataA armor, DataB!=0 percent | `0x6F2769C0` adds to the unit armor cell `+0xE0` (the same cell the damage routine reads); percent = DataA times the type record's armor (`0x6F29BF60`, `+0x90`). Removal adds the negative of what was added (`0x6F1AD0B0`) |
| Endurance `AOae` | DataA movement, DataB attack speed | `0x6F202FF0` adds to the movement multiplier cell (`Amov +0x78`, starts 1.0, snaps to 1.0 within 0.001); speed = clamp(base times multiplier) (`0x6F2019F0`). `0x6F0C9090` does the same on the attack ability (`+0x1AC`) and re-arms the swing timers in proportion |
| Unholy `AUau` | DataA movement, DataB life regen, DataC!=0 percent | movement as above; life regen rate modifier (`0x6F02DC60`), percent multiplies by a unit stat (slot 76 of the unit; max life vs max mana UNSURE) |
| Brilliance `AHab` | DataA mana regen, DataB!=0 percent | mana regen rate modifier (`0x6F02DD00`), percent times max mana (`0x6F4773E0`); targets without mana are filtered (error 221) |
| Command `AOac`, Trueshot `AEar` | DataA damage, DataB/DataC/DataD flags | `0x6F1ACC40`: per weapon slot, flat `trunc(DataA)` when DataD!=0, else `f2i(DataA times base damage + 0.5)`, added through `0x6F0C6C10`; weapon-type filters from DataB/DataC not decoded |
| Slow `Bslo` (`CBuffSlow`) | move factor, attack factor (negative) | `0x6F202FF0` and `0x6F0C9090` with the factors; refresh (`vf214` `0x6F0D6500`) removes the old factors and applies the new ones, with the never-shorten duration rule |
| Stun `BSTN` (`CBuffStun`) | none | counts a disable on the unit (`0x6F296680`, undone by `0x6F284A50`); `IsUnitType(STUNNED)` is true while a `BSTN` is in the list (unitclass state table) |

All stat effects are idempotent through per-component flags and are suspended for an illusion owner (`0x6F1ACF90`, unit
`+0x5C` bit `0x40000000`).

## 6. What the model does and what is a stand-in

Implemented (`auras.rs`, hooks in `unit.rs` creation, `UnitAddAbility`/`UnitRemoveAbility`, `combat.rs`, `damage.rs`,
`movement.rs`, `orders.rs`, `script.rs`): the emitter timers (2.0 s and 0.5 s), apply/refresh/expire with the strength
rules, stat modifiers for armor (`unit_defense`), movement (`effective_move_speed`, used by the mover), attack speed
(cooldown of new swings divided by the multiplier), damage (added to the dice bonus), regeneration (stored per second;
`Game::tick_regen` applies it on request for units without stats; with the tables loaded the aura regeneration is a forever modifier of its own on the property (`AuraBuff::regen_mod`, agile.md 9.6)), stun (attacks and movement stop; `apply_stun`) and slow (`apply_slow`), removal
of timed life with the kill. Engine: `engine::objdata` reads the six aura abilities from `AbilityData.slk` into
`sim::auras::AuraTypes`.

UNSURE stand-ins: the aura proximity listener firing every period for units already in range (the original may send only
on entering; the 4.0 s duration and 0.5 s period suggest it repeats); the `targs` decoding is reduced to tokens; the
weapon filter of the damage auras is not modelled; attack speed does not rescale a swing in progress; the percent
variants use type armor / max life / max mana; the flat speed term (`+0x70`) of `0x6F2019F0` is not modelled; the stock
buff ids (`BHad`, `BOae`, ...) are the defaults when the level has no `BuffID`; the stun/slow spells (Storm Bolt, Slow) are
not wired to the ability casts (`apply_stun`/`apply_slow` are the entry points); aura regen classes
(`CAbilityAuraRegenLife/Mana`, `AuraBlightRegen`, `AuraThorns`, `AuraVampiric`, `AuraPlague`, `AuraSlow`, `AuraKotoBeast`)
are not modelled.

## 7. Verification

Unicorn (`tools/emu/buffs_emu.py`, three modes because the refresh mode patches the set-duration routine):
compare 3,600 cases (3 routines x 1,200, edge floats and random), refresh 2,400, set-duration 1,200, the scan period
(`0x3F000000`). Rust: `crates/sim/tests/buff_vectors.rs` (gated; 0 mismatches), `crates/sim/tests/auras.rs` (5 behaviour
tests), `crates/sim/tests/ability_natives.rs` (timed life removal kills; unlimited does not).

## 8. V5 verification of the aura stat effects (Unicorn, 0 mismatches)

`tools/emu/v5_aura_emu.py` runs the real apply routines on a fake buff and owner, 600 cases (100 per routine), vectors
`tests/data/v5/aura.txt`, test `tests/v5_aura_vectors.rs` (`auras::aura_stat_amount`, `auras::aura_gate_suspends`):
Devotion armor `0x6F1ACFD0`, Endurance attack speed `0x6F1AD150` and movement `0x6F1AD240`, Unholy movement `0x6F1AD700` and
life regeneration `0x6F1AD810`, Brilliance mana regeneration `0x6F1AC990`. CONFIRMED:

* The amount the sink gets is the strength as it is, or with the percent flag `soft_mul(strength, stat)` (strength first): the
  type record's armor (`0x6F29BF60`, Devotion), **unit slot `0x130` = the max life** (Unholy, resolves the old "max life vs
  max mana" doubt) and the max of the agile mana property (`0x6F4773E0` on `unit+0xB8`, Brilliance). Percent flag: Devotion and
  Unholy life regeneration bit `0x100` of the buff flags, Brilliance bit `0x200` (its bit `0x100` is the applied flag).
* Each component applies once (applied bit: Devotion `0x200`, Endurance attack `0x100` / movement `0x200`, Unholy movement
  `0x200`, Unholy regeneration `0x400`, Brilliance `0x100`); the bit is set **before** the illusion gate, so a suspended
  application is not retried until the undo clears it.
* The illusion gates read Misc keys, default 0 (illusions are suspended): `IllusionsGetDefenseBonus` (Devotion),
  `IllusionsGetMoveSpeedBonus` (Unholy movement, Endurance movement in its callers `0x6F1AD200`), `IllusionsCanRestoreLife`
  (Unholy regeneration), `IllusionsCanRestoreMana` (Brilliance), `IllusionsGetAttackSpeedBonus` (Endurance attack speed, caller
  gate `0x6F1AD110`), `IllusionsGetAttackBonus` (Command, `0x6F1ACC17`). The model's single "illusion = suspended" matches the
  default data; per-key overrides are not wired. The Endurance applies `0x6F1AD150` / `0x6F1AD240` themselves have no gate.
* Not run: the aura emitter, the target filter (Brilliance's "has mana" filter), the regeneration modifier objects.
