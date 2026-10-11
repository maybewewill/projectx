# Player actions in replay time slots (Game.dll 1.26a)

Status: pass 1, 2026-10-11. Scope: decoding the per-player command sub-streams of a 0x1E/0x1F time slot
into typed actions (`crates/w3g/src/actions.rs`), converting them to simulation commands
(`crates/sim/src/commands.rs`), and applying a turn's commands before its ticks (`crates/engine/src/replay.rs`,
`Engine::run_turn`). Sources: the decompile export (chunk_014, chunk_015, chunk_016, chunk_018, chunk_022), the
disassembly of the small handlers, and Unicorn runs of the readers (`tools/emu/`-style harness, scratch copy,
not committed). Game.dll SHA1 `c5d8624b...`.

Labels: **CONFIRMED** = read in the disassembly or traced under Unicorn at the address given.
**LIKELY** = strong inference (public replay format, code shape). **UNSURE** = not resolved.

---

## 1. Where the commands are read

- Time slot (0x1E/0x1F) = `u16 length`, `u16 increment (ms)`, then per player a block
  `u8 player, u16 length, length bytes of actions` (CONFIRMED: `0x6F550730` reads the byte and the length
  from the 4-byte word `uStack_414`; the action bytes follow). Our parser: `w3g::blocks::parse_commands`.
- `0x6F5516E0(this, reader, player, flag)` applies one slot's stream (lockstep.md 1.3, CONFIRMED).
- `0x6F550730` is the applier. Loop: read one action byte (`0x6F4C2C10`), then a `switch` on it. Each case
  calls a handler (table in section 3). The `default` case goes to the loop end without reading a payload
  (CONFIRMED). So an unknown id does not skip its payload: the original would read the next bytes as
  actions. Our decoder stops with `w3g::Error::UnknownAction` and the absolute offset instead (a deliberate
  divergence; a replay with an unknown id cannot be followed past it).
- Special case in `0x6F550730`: ids `0x81`, `0x82`, `0x83` (replay data markers, `0x6F554680/0x6F5546C0/0x6F554570`)
  are not player actions in our replays; the decoder reports them as `UnsupportedAction`.

## 2. Handler shape (CONFIRMED for all handlers in the table)

Every handler (except 0x06, which is inline in the applier, and the 0x81..0x83 markers) has the same three steps
(checked for every handler in section 3 by its call targets):

1. **Reader**: `ECX` = the stream, `EDX` = the payload object. The reader reads the payload fields
   (layouts in section 4). The reader is called before the skip test, so it always runs.
2. **Bound check** `0x6F5378A0` on failure, when the stream position is below its end (UNSURE what the
   exact comparison means; the handlers differ only in this check).
3. **Apply** `0x6F53Bxxx` when the skip flag is 0. Every apply has the same body (CONFIRMED for all
   apply functions in the table): if the skip flag is set, it prints `War3Net: (INFO): turn %u: %s from %u`
   (verbose only); then, if the player byte of the payload (`+0x15`) is not `0xFF`, it dispatches the payload
   as an event with `0x6F5378E0` (CONFIRMED).

The payload object is the command object of the sender, whose vtable and fields are set by the sender
(`0x6F2CBAD0`, see 5.2): the event id is `0x000A0000 | action` (CONFIRMED: the sender writes
`0xA0012` for the move-target command, `0xA0003` for the speed command, and the others in
`chunk_014/016/018`). The **semantic effect of each action is in the event listeners**, which are not
located (UNSURE, see section 7).

## 3. Action table

Reader and apply addresses are the handler's calls (CONFIRMED). Field layouts are in section 4.

| Id | Handler | Reader | Apply | Payload bytes | Public name |
|---|---|---|---|---|---|
| 0x01 | 0x6F5402B0 | 0x6F5546E0 (empty) | 0x6F53B790 | 0 | pause game |
| 0x02 | 0x6F540380 | 0x6F554700 (empty) | 0x6F53B7D0 | 0 | resume game |
| 0x03 | 0x6F540450 | 0x6F554770 | 0x6F53B810 | 1 | set game speed |
| 0x04 | 0x6F540520 | 0x6F554720 (empty) | 0x6F53B850 | 0 | increase speed |
| 0x05 | 0x6F5405F0 | 0x6F554740 (empty) | 0x6F53B890 | 0 | decrease speed |
| 0x06 | inline in 0x6F550730 | 0x6F5547C0 | inline (sets +0x1B44 = 1, +0x1B5D = action; no event) | cstring | save game |
| 0x07 | 0x6F5406C0 | 0x6F554790 | 0x6F53B8D0 | 4 | save game finished |
| 0x10 | 0x6F540790 | 0x6F553EF0 | 0x6F53B910 | 14 | ability, no target |
| 0x11 | 0x6F540870 | 0x6F553F60 | 0x6F53B950 | 22 | ability, point |
| 0x12 | 0x6F540960 | 0x6F553FC0 | 0x6F53B990 | 30 | ability, point + object |
| 0x13 | 0x6F540A50 | 0x6F554020 | 0x6F53B9D0 | 38 | ability, point + two objects |
| 0x14 | 0x6F540B50 | 0x6F5540B0 | 0x6F53BA10 | 43 | ability, point + object + point (item give/drop shape, UNSURE) |
| 0x15 | 0x6F540C50 | 0x6F554130 | 0x6F53BA50 | 51 | as 0x14 + 8 bytes |
| 0x16 | 0x6F546370 | 0x6F554430 | 0x6F53BA90 | 3 + 8n | change selection (add / remove) |
| 0x17 | 0x6F546450 | 0x6F5544A0 | 0x6F53BAD0 | 3 + 8n | assign group hotkey |
| 0x18 | 0x6F540D60 | 0x6F554250 | 0x6F53BB10 | 2 | select group hotkey |
| 0x19 | 0x6F540E30 | 0x6F5542A0 | 0x6F53BB50 | 12 | select subgroup |
| 0x1A | 0x6F540F00 | 0x6F5542E0 (empty) | 0x6F53BB90 | 0 | pre-subselection |
| 0x1B | 0x6F540FD0 | 0x6F554320 | 0x6F53BBD0 | 9 | (UNSURE) |
| 0x1C | 0x6F5410A0 | 0x6F554380 | 0x6F53BC10 | 9 | (UNSURE) |
| 0x1D | 0x6F541170 | 0x6F5543D0 | 0x6F53BC50 | 8 | (UNSURE) |
| 0x1E | 0x6F541250 | 0x6F554410 | 0x6F53BC90 | 5 | remove unit from queue (LIKELY) |
| 0x20 | 0x6F5413F0 | 0x6F554AC0 (empty) | 0x6F53BD10 | 0 | (UNSURE) |
| 0x21 | 0x6F5414C0 | 0x6F554C70 | 0x6F53BD50 | 8 | (UNSURE) |
| 0x22 | 0x6F541320 | 0x6F5549C0 (empty) | 0x6F53BCD0 | 0 | (UNSURE) |
| 0x23 | 0x6F5415A0 | 0x6F5549E0 (empty) | 0x6F53BD90 | 0 | (UNSURE) |
| 0x24 | 0x6F541670 | 0x6F554A00 (empty) | 0x6F53BDD0 | 0 | (UNSURE) |
| 0x25 | 0x6F541740 | 0x6F554A20 (empty) | 0x6F53BE10 | 0 | (UNSURE) |
| 0x26 | 0x6F541810 | 0x6F554A40 (empty) | 0x6F53BE50 | 0 | (UNSURE) |
| 0x27 | 0x6F5418E0 | 0x6F554BF0 | 0x6F53BE90 | 5 | (UNSURE) |
| 0x28 | 0x6F5419B0 | 0x6F554C30 | 0x6F53BED0 | 5 | (UNSURE) |
| 0x29 | 0x6F541A80 | 0x6F554A60 (empty) | 0x6F53BF10 | 0 | (UNSURE) |
| 0x2A | 0x6F541B50 | 0x6F554A80 (empty) | 0x6F53BF50 | 0 | (UNSURE) |
| 0x2B | 0x6F541C20 | 0x6F554AA0 (empty) | 0x6F53BF90 | 0 | (UNSURE) |
| 0x2C | 0x6F541CF0 | 0x6F554AE0 (empty) | 0x6F53BFD0 | 0 | (UNSURE) |
| 0x2D | 0x6F541DC0 | 0x6F554CB0 | 0x6F53C010 | 5 | (UNSURE) |
| 0x2E | 0x6F541E90 | 0x6F554B00 | 0x6F53C050 | 4 (float) | (UNSURE) |
| 0x2F | 0x6F541F70 | 0x6F554B20 (empty) | 0x6F53C090 | 0 | (UNSURE) |
| 0x30 | 0x6F542040 | 0x6F554B40 (empty) | 0x6F53C0D0 | 0 | (UNSURE) |
| 0x31 | 0x6F542110 | 0x6F554B60 (empty) | 0x6F53C110 | 0 | (UNSURE) |
| 0x32 | 0x6F5421E0 | 0x6F554B80 (empty) | 0x6F53C150 | 0 | (UNSURE) |
| 0x50 | 0x6F5422B0 | 0x6F554910 | 0x6F53C190 | 5 | change ally options |
| 0x51 | 0x6F542380 | 0x6F554960 | 0x6F53C1D0 | 9 | transfer resources |
| 0x60 | 0x6F542450 | 0x6F555010 | 0x6F53C210 | 8 + cstring | chat trigger (UNSURE) |
| 0x61 | 0x6F542550 | 0x6F554CE0 (empty) | 0x6F53C250 | 0 | ESC pressed (LIKELY) |
| 0x62 | 0x6F542620 | 0x6F554D20 | 0x6F53C290 | 12 | (UNSURE) |
| 0x63 | 0x6F542700 | 0x6F554D70 | 0x6F53C2D0 | 8 | (UNSURE) |
| 0x64 | 0x6F5427E0 | 0x6F554DB0 | 0x6F53C310 | 8 | (UNSURE) |
| 0x65 | 0x6F5428C0 | 0x6F554DF0 | 0x6F53C350 | 8 | (UNSURE) |
| 0x66 | 0x6F542F60 | 0x6F554F20 (empty) | 0x6F53C610 | 0 | (UNSURE) |
| 0x67 | 0x6F543030 | 0x6F554F40 (empty) | 0x6F53C650 | 0 | (UNSURE) |
| 0x68 | 0x6F543100 | 0x6F554F80 | 0x6F53C690 | 12 (3 floats) | minimap signal / ping (LIKELY) |
| 0x69 | 0x6F5429A0 | 0x6F554E50 | 0x6F53C390 | 16 | continue game, block A (LIKELY) |
| 0x6A | 0x6F542A80 | 0x6F554ED0 | 0x6F53C3D0 | 16 | continue game, block B (LIKELY) |
| 0x6B | 0x6F54BE30 | 0x6F555110 | 0x6F53C410 | 3 cstring + 4 | sync integer (LIKELY from the task's list) |
| 0x6C | 0x6F54BF60 | 0x6F555150 | 0x6F53C450 | 3 cstring + 4 (float) | cheat-style record (UNSURE) |
| 0x6D | 0x6F54C090 | 0x6F555190 | 0x6F53C490 | 3 cstring + 4 | cheat-style record (UNSURE) |
| 0x6E | 0x6F54C1C0 | 0x6F5552E0 | 0x6F53C4D0 | not modelled | (20 reads, loops; UNSURE) |
| 0x70 | 0x6F542B60 | 0x6F555250 | 0x6F53C510 | 3 cstring | (UNSURE) |
| 0x71 | 0x6F542C60 | 0x6F555270 | 0x6F53C550 | 3 cstring | (UNSURE) |
| 0x72 | 0x6F542D60 | 0x6F555290 | 0x6F53C590 | 3 cstring | (UNSURE) |
| 0x73 | 0x6F542E60 | 0x6F5552B0 | 0x6F53C5D0 | 3 cstring | (UNSURE) |
| 0x75 | 0x6F5431E0 | 0x6F554FD0 | 0x6F53C6D0 | 1 | (public: unknown, 1 byte) |
| 0x84 | 0x6F5432B0 | 0x6F554590 (empty) | 0x6F53C710 | 0 | (UNSURE) |
| 0x85 | 0x6F543380 | 0x6F5545B0 (empty) | 0x6F53C750 | 0 | (UNSURE) |

"(empty)" = the reader makes no stream read (traced). "cstring" = NUL-terminated (max 0x104 for the save name,
0x100 per string elsewhere in the traces).

## 4. Field layouts (CONFIRMED by Unicorn trace, widths and order)

Offsets are the payload object's fields as the reader writes them (`+0x15` = player, `+0x14` = action byte).
Multi-byte values are little endian; `f32` is the IEEE bit pattern of a single.

- **0x10** (14 bytes): `u16 flags @0x18`, `u32 ability @0x1C`, `u32 @0x20`, `u32 @0x24`.
  Reader `0x6F553EF0`.
- **0x11** (22): as 0x10, then `f32 x @0x28`, `f32 y @0x2C`. Reader `0x6F553F60` (two calls `0x6F6EED50`, which read a float).
- **0x12** (30): as 0x11, then `u32 @0x30`, `u32 @0x34` (object pair). Reader `0x6F553FC0`.
- **0x13** (38): as 0x12, then `u32 @0x38`, `u32 @0x3C` (second object pair). Reader `0x6F554020`.
- **0x14** (43): `u16 @0x18`, `u32 @0x1C`, `u32 @0x20`, `u32 @0x24`, `f32 @0x28`, `f32 @0x2C`, `u32 @0x30`,
  `u32 @0x34`, `u32 @0x38`, `u8 @0x3C`, `f32 @0x40`, `f32 @0x44`. Reader `0x6F5540B0`.
- **0x15** (51): as 0x14, then `u32 @0x48`, `u32 @0x4C`. Reader `0x6F554130`.
- **0x16** (3 + 8n): `u8 mode @0x18`, `u16 count` (local), then `count` × (`u32`, `u32`) object pairs. Reader
  `0x6F554430` (allocates through `0x6F2CEC00`). Mode 1 = add, 2 = remove (LIKELY, public).
- **0x17** (3 + 8n): `u8 group @0x18`, `u16 count`, pairs. Reader `0x6F5544A0`.
- **0x18** (2): `u8 @0x19` (first byte on the wire), then `u8 @0x18`. Public: group, then an unknown byte (LIKELY).
- **0x19** (12): `u32 item @0x18`, `u32 @0x1C`, `u32 @0x20` (object pair). Reader `0x6F5542A0`.
- **0x1B / 0x1C** (9): `u8 @0x18`, `u32 @0x1C`, `u32 @0x20`. Readers `0x6F554320`, `0x6F554380`.
- **0x1D** (8): `u32 @0x18`, `u32 @0x1C`. Reader `0x6F5543D0`.
- **0x1E** (5): `u8 @0x18`, `u32 @0x1C`. Reader `0x6F554410`.
- **0x03** (1): `u8 setting`. Reader `0x6F554770`. Apply clamps the setting (`0x6F53E890`: values above 2 are 2).
- **0x06** (cstring): `0x6F5547C0` reads `0x6F4C2E10(dst, 0x104)` (cstring with a maximum).
- **0x07** (4): `u32`. Reader `0x6F554790`.
- **0x50** (5): `u8 slot @0x18`, `u32 flags @0x1C`. Reader `0x6F554910`.
- **0x51** (9): `u8 slot @0x18`, `u32 @0x1C`, `u32 @0x20`. Reader `0x6F554960` (gold, lumber: LIKELY).
- **0x60** (8 + cstring): `u32 @0x118`, `u32 @0x11C`, then the cstring at `@0x18`. Reader `0x6F555010`. The two
  words are read first.
- **0x68** (12): `f32 @0x18`, `f32 @0x1C`, `f32 @0x20`. Reader `0x6F554F80`.
- **0x69 / 0x6A** (16): `u32 @0x18, @0x1C, @0x20, @0x24`. Readers `0x6F554E50`, `0x6F554ED0`.
- **0x6B** (3 cstring + 4): three cstrings at `@0x18`, `@0x118`, `@0x218`, then `u32 @0x330`. Reader `0x6F555110`.
- **0x6C** (3 cstring + 4): as 0x6B with `f32`. Reader `0x6F555150`.
- **0x6D** (3 cstring + 4): as 0x6B. Reader `0x6F555190`.
- **0x70..0x73** (3 cstring): `0x6F555250/270/290/2B0`.
- **0x75** (1): `u8`. Reader `0x6F554FD0`.
- **0x2E** (4): `f32`. Reader `0x6F554B00`.
- **0x21, 0x63, 0x64, 0x65** (8): `u32 @0x18`, `u32 @0x1C` (readers `0x6F554C70`, `0x6F554D70`, `0x6F554DB0`, `0x6F554DF0`).
- **0x27, 0x28, 0x2D** (5): `u8 @0x18`, `u32 @0x1C` (readers `0x6F554BF0`, `0x6F554C30`, `0x6F554CB0`).
- **0x62** (12): `u32 @0x18, @0x1C, @0x20` (reader `0x6F554D20`).
- **0x6E** (not modelled): 20 reads with a loop; the trace gives the three leading cstrings, one `u32`, then a
  loop-shaped run. Not traced further.

Note on the 0x6B..0x73 readers: they read through the 0x104/0x100 bounded cstring reader, so a missing NUL is an
error (`UnterminatedString` in the decoder).

## 5. Object references and order flags

### 5.1 Object pairs (CONFIRMED layout, resolver UNSURE)

- An object reference is two `u32` words. `(0xFFFFFFFF, 0xFFFFFFFF)` = no object. CONFIRMED: the sender
  `0x6F2CBAD0` writes `-1, -1` when it has no target (`param_2 == 0`, the branch that sets the pair to
  `0xFFFFFFFF`), and copies the pair of the target otherwise (`param_2 + 0x0C`, `+0x10`).
- Resolving a pair to an agent: **the original's resolver is not located.** The sender takes the pair from
  the target object's fields at `+0x0C/+0x10`, so the pair is an attribute the game assigns to a unit and that
  replays carry (LIKELY). What assigns it, and which function maps a received pair back to the agent, is
  UNSURE. Our sim therefore does not guess: the pairs are registered explicitly
  (`CommandState::register_object`), and a pair with no entry is reported as unresolved and has no effect.

### 5.2 Command sender (CONFIRMED structure)

- `0x6F2CBAD0` builds the move-target command: vtable `CNetCommandUnitOrderTargetImage`, id `0xA0012`,
  action byte `0x12`, the flags word as `u16` at the wire position 0x18, the pair from `param_2 + 0x0C/0x10`
  (or `-1, -1`), and the point from the target (`vtable + 0xB8`, `0x6F4743A0`).
- `0x6F339CC0`, `0x6F339D50`, `0x6F339DD0`, `0x6F339C60` build the other order commands with the same flags
  formula (5.3). Callers: `0x6F37xxxx` region (chunk_016), not all traced.

### 5.3 Flags word (CONFIRMED formula; meaning of the inputs UNSURE)

The `u16` flags are computed as (`0x6F339CC0`, `0x6F339D50`, `0x6F339DD0`, same formula):

```
flags = ((B>>2)&1)<<6 | ((B>>4)&1)<<4 | ((B>>3)&1)<<3 | ((B>>1)&1)<<1 | (B&1)
      | (A & 0x000800) >> 3       ; A bit 11 -> 0x0100
      | (A & 0x100000) >> 18      ; A bit 20 -> 0x0004
      | (A & 0x200000) >> 16      ; A bit 21 -> 0x0020
```
where `A` is the sender's first flag word (`param_5` / `param_4`, for example `param_3` of `0x6F37AC..`) and `B` is
the second, built from input predicates in `0x6F37xxxx` (chunk_016):

- `B` bit 0 = `bVar6`: `FUN_6F62B170() != 0` (a modifier state; probably the queue key, UNSURE).
- `B` bit 2 (`0x04`): `FUN_6F339C20()` (option `0x3B` set and the modifier state, UNSURE).
- `B` bit 3 (`0x08`): `FUN_6F26DE50(1) == 0` and `FUN_6F41F660() >= 2` (UNSURE).
- `B` bit 4 (`0x10`): `FUN_6F339BC0()` (options `0x2C`/`0x2D`, UNSURE).
- `B` bit 1 (`0x02`) from `A & 0x2000000` in the immediate path (CONFIRMED mask).

Decoder and sim: the raw `u16` is kept. The queue decision in `sim::commands` uses **bit 0x0001** (`FLAG_QUEUED`),
which is the modifier bit above. **UNSURE**: whether the original's listener reads this bit as "queue"
(`replace = false`). The listener is not located (section 7).

## 6. Sim mapping (implemented)

| Replay action | Sim command | Effect | Confidence |
|---|---|---|---|
| 0x16 | `Command::Select{add}` | add (mode 1) or remove (mode 2) units of the player's selection, no duplicates | LIKELY |
| 0x17 | `Command::AssignGroup` | the group (0..9) becomes these units | LIKELY |
| 0x18 | `Command::SelectGroup` | the selection becomes the group | LIKELY |
| 0x19 | `Command::SelectSubgroup` | the selection becomes the one unit | LIKELY |
| 0x1A | `Command::PreSubselection` | nothing | LIKELY |
| 0x10, 0x11, 0x12, 0x13 | `Command::Order` | the order id (or ability rawcode) to every unit of the selection; `Point`, `Object` (resolved pair) or `None` target | LIKELY (public model) |
| 0x03 | `Command::SetSpeed` | the setting (clamped to 2) is recorded; the caller's scheduler reads it | CONFIRMED value table (lockstep.md 7.1) |
| all others | `Command::Recorded` | nothing | n/a |

Order commands use `Game::issue_order` (the native path, `orders.rs`) with `replace = !FLAG_QUEUED`. The order
id must be one the order system knows (for example `0xD0012` move, `0xD0003` smart, `0xD000F` attack,
`0xD0019` hold, `0xD0004` stop); other ids (spell rawcodes) are refused by the validator. The issued-order events
are not fired for replay orders yet (the event path is in `natives/order.rs`).

Turn order (lockstep.md 3, LIKELY): `push_time_slot` decodes the slot's blocks in stream order (player blocks as
they appear), `TurnScheduler::push_turn` converts the slot's increment into game ticks, and
`Engine::run_turn` applies the commands in stream order, then runs the ticks.

## 7. Open items (what would confirm the table)

1. **Event listeners for 0xA0000 | action** (the effects of pause, speed, selection, orders). Dispatch is
   `0x6F5378E0` on `CNetData+8`, the same path as the tick events (lockstep.md 9.1). The registration site for
   the listeners of the 0xA00xx events is not located. Known: the id constants appear only in the senders
   (chunk_014/016/018) and in the `0xA0000000` base loops (`0x6F4561C0`, chunk_019). Next step: find the
   registration calls that take an event id in `0x000A0000..0x000AFFFF`, e.g. with `0x6F549620` (CONFIRMED
   register function) and a Unicorn run of the observer walker.
2. **Resolver** from an object pair to an agent (section 5.1). Next step: the function that reads
   `cmd+0x30`/`+0x34` (the first pair of 0x12) in a listener.
3. **Queue bit** (section 5.3): the listener of 0xA0012 and the callers of `0x6F37xxxx` (the modifier
   predicate `0x6F62B170`).
4. **0x6E** layout (loops), the meaning of 0x14/0x15 (item give/drop), 0x1B..0x1D, 0x20..0x32, 0x60..0x73.
5. **Default case** of `0x6F550730`: the original does not skip the unknown id's payload. Confirm by a replay
   containing an unknown id, if one exists.

## 8. Verification

- `crates/w3g/src/actions.rs` unit tests: layouts (every payload size of section 3 against the traced
  readers), NUL-terminated strings, offsets in errors, unknown and unmodelled ids.
- `crates/sim/tests/commands.rs` (9 tests): selection add/remove, hotkeys, subgroup, point and object orders
  to the selection, the queued flag, unresolved pairs, speed clamp, recorded actions. JASS-created footmen.
- `crates/engine/tests/replay_commands.rs` (5 tests): a slot with two player blocks through `push_time_slot`
  and `Engine::run_turn` (stream order, ticks 16 for 100 ms at normal speed), an order before its selection,
  speed and recorded actions, unknown id with its offset, selection mode error.
- `crates/engine/src/replay.rs` (unit tests): conversion of point, object, none targets, mode and group errors.

## 9. Known gaps outside this change (found while doing it)

- `Engine::boot` never calls `Game::set_type_orders`: a booted map's units have no move or attack abilities, so
  every order is refused (validator). The tests set the footman's abilities explicitly. This blocks replay
  orders in a real boot and needs the unit records' ability lists (gamedata agents).
- Units take their abilities at creation (`natives/unit.rs::create`), so type orders must be set before the
  units are created.
