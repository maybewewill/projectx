# Abilities in Warcraft III 1.26a (Game.dll): spell cast path, cooldown, mana, Dur versus HeroDur (pass 4)

Status: **NOT CONFIRMED.** This pass did not settle the three open questions (cooldown countdown and ready check,
mana check and payment, Dur versus HeroDur choice). It narrowed the search: the cooldown is **not** read through
the generic level getter by spells, the mana cost is **not** compared in the predicate that reads it, and the
cast entry of an order is now located. `crates/sim/src/abilities.rs` keeps its `Cooldowns` and `cast_check` model
unchanged (marked as a model there). No Rust behaviour was changed in this pass.

Labels: **CONFIRMED** = read from the decompile export or the disassembly at the address; **LIKELY** = strong
inference; **UNSURE** = guess or not traced; **NOT FOUND** = searched, no result.

Sources: the Ghidra decompile export (scratchpad `decomp/`, chunks 000 to 019, index `decomp/index.tsv`), the RTTI
vtable scan (`tools/analysis/rtti_vtables.py`, output in scratchpad `vt.tsv`), capstone listings of the tiny functions
the decompiler skips, and the factory table `docs/spec/ability_factory_1.26a.tsv`. The Unicorn dynamic check
described in the pass plan was **not run** (section 6 says why and what is needed).

## 0. Corrections and new facts

| Earlier statement | Status after this pass | Basis |
|---|---|---|
| abilities-3 5.1: the only reader of the level `Cool` is `0x6F1F4A70`, "whose result is unused" | **Corrected.** `0x6F1F4A70` is `CAbilityReplenish::vf187`. It reads `Cool` into a temporary and then adds the float at `this+0x13c` (a softfloat add). The decompile loses the add's destination, so the effect on `+0x13c` is UNSURE. It is a Replenish method, not a spell cooldown. | RTTI (`0x6f91aa14` = `CAbilityReplenish`), decompile |
| abilities-3 5.2: predicate `0x6F052990` reads `Cost` for the mana check | **Corrected in part.** The predicate reads `Cost` only to test "is it usable at all" (`Cost == 0` with level count < 2 hides the ability). It does **not** compare the unit's mana. | decompile `0x6F052990` |
| abilities-3 section 2: `CAbilityAuraSpell` is the spell class | **Not established.** `0x6F074C60` (the vtable slot 3 of the spell family) is a **message switch**, not a start hook (section 2.1). The concrete class of Storm Bolt and Slow is still unknown. | decompile `0x6F074C60` |

## 1. Level-record accessor and its callers (CONFIRMED structure)

- `0x6F02E6A0(def, idx)` (ECX = definition, `+0x54` cache, `+0x50` level count, `+0x2C` loaded flag): returns the
  level record at `idx * 0x68 + def+0x54` when `idx >= 0` and `idx < count`, else 0. Lazy fetch through `0x6F265ED0`.
- Callers (12, CONFIRMED by the call scan): `0x6F03FF40`, `0x6F040900`, `0x6F040950`, `0x6F0409A0`, `0x6F0409F0`,
  `0x6F041700` (level up), `0x6F041720` (level down), `0x6F042B20`, `0x6F052A60`, `0x6F052B10` (`CAbility::vf32`),
  `0x6F14C870`, `0x6F155CA0`.
- `0x6F040900`, `0x6F040950`, `0x6F0409A0`, `0x6F0409F0` (CONFIRMED by decompile) build an **effect object** at
  `param_2`: the level record at `+0x7C`, a flag/value set at `+0x80` (`1` in `0x6F040900`) and two floats at
  `+0x84`/`+0x88` (`0.0`), then the common setup `0x6F0408B0` / `0x6F040830`. `0x6F040830` calls vtable slots
  `0x2C4`, `0x2C8`, `0x2CC`, `0x2D0` of the owning object and `0x6F02E260`. Their callers include the attack
  execute `0x6F0CF660` (attack-cycle.md), `0x6F102490`, `0x6F181E60`, `0x6F1820B0`, `0x6F1F3210`, `0x6F1F3950`,
  `0x6F1FB590`, `0x6F1DD4F0`. **LIKELY**: these are the per-cast effect objects that carry the level record, so the
  spell's timing fields (Cast, Dur, Cool) would be read from there. Not traced.

## 2. The order entry of a spell (CONFIRMED structure, LIKELY for the concrete class)

### 2.1 Message switch `0x6F074C60` (vtable slot 3 of the spell family; CONFIRMED decompile)
The function takes a message object (`[msg+8]` = message id) and dispatches:

1. Fixed ids handled inline: `0xD0144` (calls slot `0x3A0`), `0xD015F` (`0x6F050A10`), `0xD0160` (`0x6F0741E0`),
   `0xD01B6` (`0x6F050850`), `0xD01B7` (`0x6F050820`), `0xD01BB` (`0x6F06C180(1)`), `0xD01BC` (`0x6F063760(1)`),
   `0xD01BD` (slot `0x39C(1)`), `0xD01BE` (slot `0x3A8`).
2. Otherwise, if slot `0x314` returns non-zero **and** slot `0x368` (the order id) equals the message id, call slot
   `0x330` with the message and return 1.
3. Otherwise compare the message with slot `0x348`; on a match, check slots `0x308` and `0x318` and call slot `0x378`
   when neither equals it.
4. Otherwise forward to `0x6F024210(msg)` (the base handler).

- Slot `0x314` = `0x6F069160` (CONFIRMED): returns bit `0x2000` of `+0x20` (order-capable flag, LIKELY the "has an
  order" bit).
- Slot `0x368` = `0x6F037860` (CONFIRMED): returns **`0xD0295` if flag `0x20000000` is set in `+0x20`, else
  `0xD0028 + field(+0xAC)`**. So the order id of a spell is the ability's order index on top of base `0xD0028`
  (`+0xAC` is that index; LIKELY the ability's order-string number).
- Slot `0x330` = `0x6F022980` (CONFIRMED): a forwarder, `call [vtable+0x338]` with the message, `ret 4`. So the cast
  body is slot `0x338` of the **concrete** class. **NOT FOUND** for Storm Bolt and Slow.
- Slot `0x348`/`0x308`/`0x318`/`0x378` forwarders: `0x6F068F00` jumps to `[+0x308]`, `0x6F068F10` jumps to
  `[+0x30C]`; not traced further.

So the start of a targeted spell is: order issued (the order id of section 2.1 step 2) → message posted to the
ability → `0x330` → `0x338` of the concrete class. The check and the payment of mana and the cooldown start are
**inside** that concrete `0x338` body or in the functions it calls. Not yet read.

### 2.2 Storm Bolt and Slow constructors (partly CONFIRMED)
- Factory wrapper of `AHtb` (`FUN_6f247150`) calls the smart-pointer constructor `0x6F227060`; of `Aslo`
  (`FUN_6f240af0`) calls `0x6F224300`. Both build a pointer, then call `0x6F21A280` / `0x6F216600`. These two are
  **smart-pointer copy-assign** helpers (refcount swap on `+4`, release through the vtable), **not** the class
  constructors. The concrete class is created inside the callees of the wrapper (`0x6F21A280` first calls
  `0x6F156C40`, then vtable slot `0x1C`, then `0x6F471910`). Not followed to the `new` in this pass.

## 3. Mana (CONFIRMED structure; payment NOT FOUND)

- Unit mana is an **agile float property** (`0x6F477350` setter, `0x6F4773E0` agile value copy; `0x6F4773A0` agile
  time getter). `0x6F477350` has more than 40 callers, so it is the common property setter, not a cost path.
- `0x6F278F50(float *value, mode)` is the **mana sink** of heroes.md 10.6. Modes 1, 2 and 3 take different branches
  (mode 1 ends in the property setter `0x6F477350`; modes 2 and 3 use the agile-value copy `0x6F4773E0` and the
  setter `0x6F477400`); the exact meaning of each mode for mana is taken from heroes.md 10.6 and UNSURE here. It is a
  setter, not a cost check.
- `0x6F2783F0` (CONFIRMED decompile): sets a property value toward a target through a FloatModifier
  (`0x6F4774E0`, rate from `(target - current)` and the agile time `0x6F32D090`), with a per-object modifier handle
  stored at `+0x224`. This is the **timed regeneration / approach** path (LIKELY life or mana regen), not a cost.
- The cost readers found by the `Cost` getter scan (`0x6F02E840`): `0x6F0B0880` (returns `Cost` unless bit
  `0x80000000` of `+0x20`), `0x6F1CFA90`, `0x6F1DBCC0`, `0x6F1FADC0` (a targeted effect with group builder
  `0x6F2A3270`, callback `0x6F1FAAA0`, reads Dur/HeroDur). **None compares the unit's mana or subtracts it.**
- **Payment and the check: NOT FOUND.** Remaining candidates: the concrete `0x338` body of section 2.1, and the
  callee of `0x6F0B0880` (its caller is not in the export).

## 4. Cooldown (NOT FOUND for spells; the model stands)

- The only direct reader of `Cool` (getter `0x6F02E890`) is `CAbilityReplenish::vf187` (section 0). Spells do not read
  `Cool` through the getter in the export.
- Countdown and ready check: NOT FOUND. The level-record consumers of section 1 (the effect objects) are the next
  place to look: their `+0x80..+0x88` floats might hold the cooldown or cast timing (UNSURE).
- The agile property mechanism (`0x6F4774E0` FloatModifier, `0x6F477350` setter) is the natural shape for a cooldown
  counting down at 1 s per agile second (LIKELY, not confirmed). It would make the ready check "the modifier value
  reached 0", and it would make the countdown depend on the agile clock, which the model's one-shot timer does not.

## 5. Dur versus HeroDur (NOT FOUND)

- Dur getter `0x6F02E760` has about 190 callers in the export, HeroDur getter `0x6F02E7D0` fewer. Among the readers
  seen in this pass, `0x6F1FADC0` reads both (`0x6F02E760(auStack_48, iVar1)` and `0x6F02E7D0(auStack_48, iVar1)`), and
  stores them into its own record (`uStack_20 = uStack_50`, `uStack_1c = *puVar3`). The choice by target type is
  not visible there; it is UNSURE whether that function is a hero-aware path at all.
- Not run in Unicorn.

## 6. Dynamic check: not run, and what it needs

The pass plan asked for a Unicorn run of a spell cast with stubs and call tracing. This was not done, for these
reasons:
1. The concrete class of Storm Bolt and Slow is not known (section 2.2), so the `0x338` target cannot be picked.
2. The cast body reaches the mover, the target search (`0x6F2A3270`) and the timer and property layers. Stubbing them
   needs the same recorder approach as `tools/emu/abilities2_emu.py` for each layer, plus the agile world object
   (construction.md 12, vision.md 13.4 say the world query must be stubbed).
3. Within this pass the static searches were the cheaper route, and they narrowed the question but did not close it.

Next steps, in priority order:
1. Find the `new` of the concrete spell class: follow the constructor chain below `0x6F21A280` (`0x6F156C40`, the
   `[vtable+0x1C]` call, `0x6F471910`) to the allocation that writes the class vtable. Read that class's slot `0x338`.
2. In that body, look for the mana check (a compare of the unit mana property with `Cost`) and the subtraction
   (the property setter `0x6F477350` with a negative delta, or the sink `0x6F278F50` mode 1 with `-Cost`).
3. Trace the effect objects of section 1 (`0x6F040900` family) for the `+0x84`/`+0x88` float use: a countdown there
   would be the cooldown.
4. Only then run the Unicorn check with the timer hook (`0x6F477F80` arm path) and the setter hook.

## 7. Verification of this pass

| Check | Count | Result |
|---|---|---|
| Decompile export: callers of `0x6F02E890` (Cool), `0x6F02E840` (Cost), `0x6F02E6A0` (level accessor) | 1, 5, 12 | CONFIRMED lists; readers in section 0, 3, 1 |
| Decompile: bodies of `0x6F1F4A70`, `0x6F052990`, `0x6F0B0880`, `0x6F1CFA90`, `0x6F1DBCC0`, `0x6F1FADC0`, `0x6F278F50`, `0x6F2783F0`, `0x6F074C60` | 9 | read; facts in sections 0, 2, 3, 5 |
| Capstone: tiny forwarders and helpers `0x6F022980`, `0x6F068F00`, `0x6F069160`, `0x6F037860`, `0x6F227060`, `0x6F224300`, `0x6F21A280` | 7 | read; facts in sections 2 and 3 |
| Unicorn | 0 | not run (section 6) |
| Rust | 0 | no change to `crates/sim/src/abilities.rs` |

Files (not committed, analysis only): scratchpad `vt.tsv`, `vtdiff.py`, `getfn.py`, `dis.py`.
