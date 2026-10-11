# Unit movement, pass 4: the per-walk step length and the speed chain (Game.dll 1.26a)

Status: reverse engineering for a clean-room port (interoperability), 2026-10-11. Written from the decompiled listing
(scratchpad, not committed) and from disassembly with capstone (`0x6F26DE00`, `0x6F202F30`, `0x6F201D50`, `0x6F29F3E0`
and the call sites). Image base 0x6F000000. Labels: CONFIRMED = read directly from code or constants; LIKELY = consistent
with the code but the role is inferred; UNSURE = not resolved.

Question (from AGENTS.md section 6, item 1): how is the per-walk step length computed from the unit's movement speed,
where is `unit+0x88` (the step cap) written, and is the walk's distance a multiply by 0.03, by an elapsed time, or by a
cell scale? Earlier notes (orders.rs, lockstep.md 11.3) model it as `speed * 0.03`, capped at the remaining distance.

Result in one line: the step cap `unit+0x88` is written once, at unit creation, from the unit type record; the
speed-setter chain does NOT write it; no multiply by the 0.03 period exists in the mover chain. The per-call formula
(speed to step length) is therefore NOT the `speed * 0.03` model, and it is not determined here.

---

## 1. The per-record step reads the step length from the owner's vector (CONFIRMED, decompile of 0x6F4907F0)

`FUN_6F4907F0(group, record R)` with the owner U = `R+0x14`:

1. `U+0x80` and `U+0x84` are loaded as a 2D vector (x, y). Its length is computed with the softfloat MUL, MUL, ADD and
   SQRT (`0x6F6EEE20`, `0x6F6EF000`, `0x6F6EF750`). CONFIRMED.
2. `R+0x20` (the step length) := that length if its magnitude is at least `4e-7` (the constant at `0x6FAAE618`), else
   0.0. CONFIRMED.
3. Facing: `R+0x24` := the heading of the vector (`0x6F4B2DE0`, see movement.md 4.1), or `U+0x8C` when the length is 0.
   CONFIRMED.
4. The path step `0x6F495320` runs with the record's target and writes the record's step fields (movement.md 3.1). CONFIRMED
   call; its effect on `U+0x80/+0x84` is not read.
5. The cap: `if (U+0x88 < R+0x20) R+0x20 := U+0x88`. CONFIRMED (decompile, the `*(iVar1+0x88) < *pfVar4` branch).

So the per-walk distance is the length of the owner's vector `U+0x80/+0x84` as it stands at the start of the walk,
capped at `U+0x88`. Nothing in this function multiplies by a time, a period or a cell scale. (This confirms
movement.md 11.3 and the withdrawal of `speed * 0.1` there.)

What writes `U+0x80/+0x84` during the walk is NOT found (UNSURE). The likely writer is the path step (4 above), since
the next walk reads what it wrote. The only other writer found is the unit creation copy (section 3).

## 2. The period constant and the walk timer (CONFIRMED readers, no multiply)

The 0.03 f32 at `0x6FAB7370` (`0x3CF5C290`, set by the static initialiser `0x6F85F650` from the text "0.03") has five
absolute-address references in `.text`:

| Site | Use (disassembly) | Meaning |
|---|---|---|
| `0x6F01E0EB` | `push 0x6fab7370` before a timer call | a timer with period 0.03 (not the walk) |
| `0x6F426C02` | the post of message 0x8026A (`0x6F426BF0`) | the movement walk timer (lockstep.md 11.2) |
| `0x6F442083` | `push 0x6fab7370`, `mov edx, 0x6fab71f8` | another timer with the same period |
| `0x6F443197` | `push 0x6fab7370`, `mov edx, 0x6fab71fc` | another timer with the same period |
| `0x6F85F656` | the initialiser | the value |

None of the five is a multiply (`fmul`/softfloat MUL with this operand). So the walk does not compute `speed * 0.03`
from this constant. CONFIRMED (scan of all `.text` absolute references; capstone). The constant `0x3CF5C290` also
occurs as an immediate once, at `0x6F84DCC6` (in the initialiser area, not read: UNSURE what it stores).

The walk period is therefore a timer period only. The per-walk distance comes from the vector (section 1), not from
the period.

## 3. The step cap `unit+0x88`: written at unit creation from the type record (CONFIRMED write, LIKELY owner)

`FUN_6F29F3E0` (unit creation; the function that copies the type fields into the new object, `param_3` = the object,
`param_2` = the unit type id):

- `0x6F29F4F0-0x6F29F51B`: `FUN_6F29C790(type)` gives a 3-float vector stored at object `+0x7C`, `+0x80`, `+0x84`.
  CONFIRMED (disassembly).
- `0x6F29F521-0x6F29F52C`: `EDX = type id; ECX = result; call 0x6F29C830`; the result is stored at `object+0x88`.
  CONFIRMED (disassembly and decompile line `*(param_3+0x88) = *puVar2`).
- `FUN_6F29C830(result, key)`: looks up the type record for `key` (`FUN_6F4C8520`, then `FUN_6F29A8C0`, not read), and
  returns the float at `record+0x210`; if no record is found it returns 0.0 (`DAT_6FAAE470`). CONFIRMED (decompile).
- So `unit+0x88` = the type record's `+0x210` float, copied once at creation. No other store of a float to `+0x88`
  was found on the unit path (the other stores to `+0x88` in the decompile are flag words of other classes, see 5).

LIKELY: the object of `FUN_6F29F3E0` is the unit, so `U` in section 1 is the unit itself, and `U+0x80/84/88` are unit
fields. NOT CONFIRMED: `FUN_6F29F3E0` also stores `unit+0x164` from `FUN_6F29BC40()` (a type-derived pointer); whether
the mover record's `U` is the unit or this sub-object was not settled (movement.md 1.4 says "the mover sub-object"
for the older notes; UNSURE).

What the `+0x210` value is: a stat of the type record. A writer of `+0x210` exists (`FUN_6F2835E0`, a raise-only
maximum: when the flag argument is set and the old value is below the new one, it stores the new value and notifies a
holder through its vtable slot 0). Its class is not identified (UNSURE that it is the unit-type class). The name of
the stat (maximum move speed per second, or something else) is UNSURE.

## 4. The speed setter does not write the cap (CONFIRMED, disassembly of the chain)

`SetUnitMoveSpeed` calls `0x6F26DE00` with `ECX = unit` and the speed on the stack:

| Address | What it does (disassembly) |
|---|---|
| `0x6F26DE00` | `ECX := [unit+0x1EC]` (the Amov ability); if non-zero, `jmp 0x6F202F30` with that ECX. Otherwise return. |
| `0x6F202F30` | `this = ECX`; calls vtable slot 0 of `*(this+0x6C)` with `(speed, 1)`, `ECX = this+0x6C`; then `call 0x6F201D50` with `ECX = this`. |
| `0x6F201D50` | `local = this`; `owner = [this+0x30]` or `FUN_6F472890(this)` if zero; `obj = owner.vtable[0xB8](owner)`; `obj.vtable[0x18](&local)`; then `0x6F26FE20` (a jump to `0x6F332810`, which calls `0x6F300710` and `0x6F2F89A0`, not read). |

So the speed value is stored in the holder object at `Amov+0x6C` (the ability's stat holder), and the owner is told
that the ability changed. None of the three functions stores to `+0x88` of any object. CONFIRMED. The notification
chain `0x6F332810` and the holder's slot 0 are not read: whether they update `unit+0x88` (or the vector `+0x80/+0x84`)
is UNSURE. The class of the holder is not identified, and the ECX-based vtable calls cannot be run in isolation without
that object graph, so Unicorn was not used for this chain.

Consequence: the value of `speed` (per second, from the SLK/native) does not reach the step cap `+0x88` in this
chain. Either the cap is the type value only (and the speed reaches the walk through the vector `+0x80/+0x84`, through
a writer not yet found), or the notification path updates `+0x88` (UNSURE). Both are open.

## 5. Other stores to `+0x88` (class-scoped, not the cap)

The decompile contains many stores to `+0x88` in other classes. Two examples show why the writer scan must be
class-scoped:

- `FUN_6F4924D0` (cluster add, decompile chunk_019 line 50284): `*(param_1+0x3C)+0x88 |= 0x400000`. The object at
  `+0x3C` is a flag word here, not a float. CONFIRMED (decompile).
- `FUN_6F4B3F60` (a curve object with `+0x50..+0x88`): writes `+0x88` as a float (`sqrt` of a difference). Not a unit
  object (it has `+0x68` state and `_CIsqrt`). CONFIRMED (decompile).

So a float write to `+0x88` is not, by itself, a unit cap writer.

## 6. What this means for the model (orders.rs)

- The per-walk step `min(speed * 0.03, remaining)` in `crates/sim/src/orders.rs` (`step_unit`) has no support in the
  code read: no multiply by 0.03 exists in the mover chain (section 2), and the cap is a type value (section 3), not
  a per-walk time product. It is kept as the model until the writer of `U+0x80/+0x84` is found; the code comment now
  points here.
- The per-walk step of the original is `min(|U.(80,84)|, U.(88))` (section 1). That is CONFIRMED. The velocity writer
  (and the speed unit conversion) is the open part.
- The movement period (0.03 s of agile clock, one walk every 6 ticks) is CONFIRMED as a timer; it is not the speed unit.

## 7. Open items, in order

1. The writer of `U+0x80` and `U+0x84` during the walk: read the path step `0x6F495320` and its callees
   (`0x6F49E020`, `0x6F49DAC0`), and `0x6F4940C0`, and find the stores to `+0x80/+0x84`. A dynamic check is cheaper:
   break on writes to the unit's `+0x80` while a unit with a move order walks, under Wine with the user's game files.
2. The notification path `0x6F332810` → `0x6F2F89A0`: does it refresh `+0x88` or `+0x80/+0x84` from the stat holder?
3. The class of the holder at `Amov+0x6C` and of the object at `FUN_6F2835E0`'s `this` (for the meaning of type `+0x210`).
4. Whether `U` is the unit or `unit+0x164` (the mover sub-object).
5. The value of the immediate `0x3CF5C290` at `0x6F84DCC6`.

## 8. Verification

- CONFIRMED items were read from the decompile (chunks 010, 012, 013, 019, 020) and from capstone disassembly of the
  listed addresses. The scans for absolute references (`0x6FAB7370`, `0x3CF5C290`) cover the whole `.text`.
- Unicorn: not run for this pass (the chain goes through holder vtables whose classes are unknown). The arithmetic
  helpers it would use are already checked (softfloat, docs/spec/softfloat.md).
- No change to the movement golden values in tests came from this pass; the only test changes are the start-timing ones
  in lockstep.md section 12.5.
