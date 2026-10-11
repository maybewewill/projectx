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

---

# Pass 5: the cast in Unicorn (concrete class, cast path, cost value)

Labels as above. Harness: `tools/emu/abilities4_emu.py` (committed; runs the original code of the user's Game.dll
under Unicorn with the SEH and import fixes of section 5.3). Result: **11 checks, 0 failures** (section 6). Mana
payment, the cooldown and Dur versus HeroDur are **still not found**; the brief's step 2 is only partly reached.

## 8. Concrete class of AHtb, AHhb and Aslo (CONFIRMED, Unicorn)

The factory table is **not** a class-constructor table. Its `ptr_ctor` column (for example `0x6F227060` for AHtb) is a
typed lookup: it calls `0x6F21A280`, which calls the type-id getter `0x6F156C40` (`mov eax, 'AHtb'`) and the class
check `0x6F471910` (compare with an object's tag). It casts an **existing** ability object; it does not construct one.
The per-class constructor is found from the vtable instead.

Identification: a concrete class is the vtable whose **slot 7** (`+0x1C`, the code getter) returns the base code.
Checked by calling the slot in Unicorn (`abilities4_emu.py`, 3 checks):

| Base code | Getter (slot 7) | Vtable (class) | Vtable slots: `0x330` / `0x338` | Constructor (writes vtable) |
|---|---|---|---|---|
| `AHtb` | `0x6F156C70` | `0x6F8F56DC` (`CAbilityThunderBolt`, 268 slots) | `0x6F04FCA0` / `0x6F054880` | not located (vtable writers `0x6F18F5FA`, `0x6F191922`, not checked) |
| `AHhb` | `0x6F082B20` | `0x6F8B1B4C` (`CAbilityHolyBolt`) | `0x6F04FCA0` / `0x6F054880` | ctor at `0x6F0C0A50` (writes `[esi]`, then base `0x6F06F700`, member `0x6F0418E0`) |
| `Aslo` | `0x6F1D6480` | `0x6F92280C` (`CAbilitySlow`, 285 slots) | `0x6F072370` / `0x6F054880` | not traced |

Notes: `AHtb` is handled by `CAbilityThunderBolt` in the RTTI names, not by a Storm Bolt class (the names in RTTI are
the internal ones). The `0x6F0C0A8D` hit is the ctor of `CAbilityHolyBolt`; the ThunderBolt ctor writers were not
enumerated further (the search found `0x6F18F5FA` and `0x6F191922` for `0x6F8F56DC`, not checked). Slot `0x338` is the
same function for **every** spell class checked (`0x6F054880`, shared body). Spell classes without a `0x330` override
use the family forwarder `0x6F022980`.

## 9. The cast path, traced (CONFIRMED structure, Unicorn trace of the real code)

Entry: `0x6F054880` (slot `0x338`, ECX = ability). Trace of the call targets (the harness records every call and tail
jump; the stub list is in section 5.3):

1. `0x6F054880`: owner = `[ability+0x30]` (or the fallback owner `0x6F472890`); the owner flags are tested. Then slot
   `0x318` (`0x6F068870`, returns the global `[0x6F92ED9C]`) is compared with that same global. In the traced run the
   values were equal, and the body **tail-jumps to slot `0x340`**. The other branch calls slot `0x1B8`, sets flag `0x80`
   of `+0x20`, calls `0x6F021270`, and then also tail-jumps to `0x340` (CONFIRMED in the disassembly, not traced).
2. Slot `0x340` (`0x6F0290E0`): calls `0x314` (order-capable flag, `0x6F069160`, bit `0x2000` of `+0x20`); when it is
   zero it compares slot `0x348` (`0x6F068F00`) with slot `0x308` (`0x6F18F610`) and, on equality, tail-jumps to slot
   `0x378`.
3. Slot `0x378` (`0x6F074080`): the **cast start**. It reads the owner flags, makes virtual checks through slots `0x34C`
   (`0x6F068F10`, which forwards to `0x30C` = `0x6F06F7A0`), calls `0x6F155920` (a thiscall with no arguments returning
   a bool; stubbed TRUE in the harness), and then calls slot `0x370` (`0x6F029080`, the cost pre-check of section 10,
   called at `0x6F0740CC` with one pushed `0`, `ret 4`). It also has
   three event
   posters (`0x6F2963F0`, `0x6F295AC0`, `0x6F295BC0`: a key from the agent world and a post to the global agent at
   `0x6FAB7368`). The posts need the agent world; they are stubbed in the harness, so **the listeners that run the
   spell effect are not reached**.

## 10. The cost pre-check and the value it hands over (CONFIRMED, Unicorn 8/8 vectors, 0 mismatches)

Inside the cast start, the cost check `0x6F029080` (ECX = ability, 1 stack argument) does, in order:
- `slot 0x3D4` (`0x6F069170`): `return Cost(level)` = `0x6F02E840(ability, [ability+0x50])`, the integer Cost of the
  current level (level record `+0x10`).
- `I2R` (`0x6F6EF350`, EDX = integer, ECX = out pointer): the float conversion. It keeps 24 mantissa bits
  (`16777219 -> 0x4B800001`, i.e. 16777218).
- `slot 0x36C` (`0x6F069050`): writes the constant `[0x6FAAE470]` = **0.0** into an out pointer.
- the soft add (`0x6F6EF000`) of the two floats; the caller pushes a **pointer** to the result (CONFIRMED by the harness:
  the dereferenced value is the float that reaches the check).
- `0x6F283C70` (ECX = the owner unit, one argument = the result pointer): the unit-side check (section 11).

So the value the check receives is `add(I2R(Cost), 0.0)`. CONFIRMED on 8 integer costs in the harness: `0`, `1`, `75`,
`1234567`, `-5`, `16777219`, `2147483647`, `-2147483648`. The original does **not** clamp a negative cost on this path
(`-5` gives `-5.0`). This is now `crates/sim/src/abilities.rs::cast_cost_value`, with vectors in `cast_cost_vectors`.

## 11. The unit-side check `0x6F283C70` (partly decoded; the comparison is NOT decoded)

- It saves the owner's flag word and computes `ebp = arg & ((owner+0x5C >> 2) & 1)`, then reads the **agile
  value at unit+0xB8** through `0x6F4773A0` (`lea ecx, [ebx+0xB8]`). LIKELY the mana property (life is at unit+0x98,
  unit-core.md 4.3b); not confirmed by a value.
- It then walks the unit's ability list (`+0x1DC` mask and `+0x1E0`, iterator `0x6F4786B0`, ability slot `0x120`) and
  ends in an **x87 equality** (`fld [esi]`, `fld [esp+0xC]`, `fcompp`, `jne`), returning 1 (`mov eax,1; ret 8`) on
  success. The shape is therefore "some stored float equals the value read from unit+0xB8", not the plain `mana >= cost`
  that the Rust model assumed. The loop's meaning is UNSURE (it may be the per-ability check, not the mana check).
- The Rust model (`cast_check`: `mana < cost`) is kept and marked as a model; the operator and the loop are open.

## 12. Where the emulation stops (exact points)

| Attempt | Stop point | Cause | Fix made |
|---|---|---|---|
| Slot `0x338` on the real vtable with a bare unit | unmapped read at `0x6F15592D` | `mov eax, fs:[0]` (SEH) and no FS base | all 3 `fs:[0]` forms patched to absolute access (in the harness only) |
| After SEH | unmapped fetch at `0xA4D538` | IAT unresolved (file holds RVAs), `0x6F6D98E0` calls `[0x6F86D144]` = `TlsGetValue` | every import slot gets `xor eax,eax; ret N` (N from a small table); `TlsGetValue` returns a TIB block |
| After imports | unmapped read in `0x6F283C80` | per-thread context at TLS entry 13 is zero | TIB block with a context, inner flags 0 |
| Cast start | event posters need the agent world | `0x6F2963F0` etc. read `[0x6FAB7368]` | stubbed (no effect) |
| Unit check `0x6F03FA30` | unmapped read inside the agile getter | the agile object at unit+0xB8 is not built (needs the world clock) | **not fixed**: the run stops here, after the value capture |

So the **cast value** is captured before the stop, and the **comparison result** is not. The runs did not reach payment,
cooldown start, or the effect objects.

## 13. Not found in pass 5 (open)

- **Payment** (subtraction of Cost from the mana property): no write to unit+0xB8 was reached. The cast start posts
  events and returns. The subtraction is probably in a listener of those events or in the effect object (section 1 of
  the earlier pass), which the harness does not run.
- **Cooldown**: not reached. The harness has no timer arm call in the trace (`0x6F477F80` never called). Still open.
- **Dur versus HeroDur**: not run. Storm Bolt's stun buff (`CBuffStun`, vtable `0x6F8B3884`) is the candidate for the
  test; the buff apply path was not run in this pass.
- **Mana comparison operator** (`<`, `<=`, the x87 equality of section 11): open.

## 14. What changed in the code (pass 5)

- `crates/sim/src/abilities.rs`: `cast_cost_value(cost) = add(i2r(cost), +0.0)` (CONFIRMED); `cast_check` uses it for
  the mana bound instead of `cost.max(0) as f32`. The negative-cost clamp is removed (the original does not clamp).
  The comparison stays a model. Cooldown: unchanged (model).
- Tests: `cast_cost_vectors` (2 tests, 8 vectors from the original).

## 5.3 Harness notes (`tools/emu/abilities4_emu.py`)

- `fs:[0]`: each occurrence of `64 A1 00 00 00 00`, `64 A3 00 00 00 00` and `64 89 0D 00 00 00 00` in the image is
  replaced by an absolute access to one TIB cell (same instruction length). The byte search can in principle match
  inside an operand; no such match changed a traced path.
- Imports: the file IAT is unresolved. `fill_imports` maps each import name to a stub. Unknown imports return 0 with
  the stdcall cleanup from `ARITY`; the return value of an import is therefore **not** the Windows value.
- Recorders: the event posters and the fallback owner are replaced by stubs (`ret N`, arities checked against their
  `ret` instructions); the SEH helper `0x6F155920` (no arguments, bool) is stubbed TRUE.
- The value check reads the function's first stack argument at its entry (a Unicorn code hook), not a stub.

## 6. Unicorn verification (pass 5)

| Check | Count | Result |
|---|---|---|
| Class identity: slot 7 of the three vtables returns the base code | 3 | 3 ok |
| Cast cost value at `0x6F283C70` entry vs the original I2R, 8 integer costs | 8 | 8 ok, 0 mismatches |
| Rust `cast_cost_vectors` (same 8 values + negative cost) | 2 tests | ok |
| `cargo test -p sim --lib cast` | 3 | ok |

Not run: payment, cooldown, Dur versus HeroDur (section 13).
