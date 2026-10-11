# Warcraft III 1.26a game.dll: game-start seeding (spec)

Scope: the value `S` the original gives to the gameplay generator and the 45 streams when a game
starts, where it comes from, and how a replay's game-start record (block `0x19`) relates to it.
Companion to `docs/spec/rng.md` (generator, streams, SetRandomSeed). Clean-room: addresses and
plain-words behaviour only; no decompiled text is reproduced here.

Confidence: CONFIRMED = read from the machine code and, where noted, checked under Unicorn;
LIKELY = strong structural evidence, not run; UNSURE = open or a guess.

## 1. Summary

1. At game start the original seeds the global generator `G` with one value `S`, then initialises
   the 45 streams from the same `S` (no step in between). CONFIRMED. This is `sim::Streams::start_game`.
2. `S` comes from the game-setup function `0x6F5C37A0` (CONFIRMED structure). It is one of:
   - **the seed field of the game-start container**, when the container parsed from the player-0
     blob is accepted (`SetupPath::Record`); this container is the same layout as the replay's
     `0x19` record body (section 4). LIKELY the replay path.
   - **a rotate-xor fold** over per-slot sub-records, when the record is refused and the setup mode
     argument is 5 (`SetupPath::Fold`, section 3). The per-slot input `X` is confirmed under Unicorn;
     its source in a replay is not.
   - a second site, `0x6F5C0EE0`, seeds `G` with the container seed directly, with no fold and no
     `0x8000` rule (section 5).
3. The settings flag `0x8000` (settings object `+0x38`) replaces `S` with `0x77617233` (CONFIRMED
   in the tail `0x6F5C3F72`). Whether the replay's `flags` word carries this bit: UNSURE.

## 2. The seeding tail (CONFIRMED)

Tail `0x6F5C3F72..0x6F5C3FB5` in `0x6F5C37A0`. The game object is the global `0x6FAB65F4`; its `+0x30`
is the settings object, whose flag word is `+0x38` (`test dword [ecx+0x38], 0x8000`).

- Bit `0x8000` set: push the constant `0x77617233` and call the generator seeder `0x6F4AB660` with
  global generator state `0x6FAB73D8`; then the stream initialiser `0x6F24F1E0` with `ecx = 0x77617233`.
- Bit clear: push `S` (`[esp+0x44]`) and do the same with `S`.

Checked under Unicorn (`tools/emu/game_start_emu.py`, 300 random `(flags, S)` pairs, 0 mismatches).

`[esp+0x44]` is the seed slot of the container described in section 4 (container at `[esp+0x38]`,
its `+0x0C` field). It is written in two places only: the container parse (through the reader) and
the fold (`S := 0` at `0x6F5C38F4`, step at `0x6F5C3A4C..0x6F5C3A57`).

## 3. The per-slot fold (CONFIRMED structure; input X CONFIRMED under Unicorn; source UNSURE)

Used only on the `SetupPath::Fold` path: record refused and setup mode `== 5`
(`cmp [esp+0x3cb0], 5` at `0x6F5C382D`, the first stack argument of `0x6F5C37A0`).

The loop runs over player ids `1..16` (byte counter at `[esp+0x17]`, `0x6F5C3958..0x6F5C3AA8`). For each
id that maps to a slot (`0x6F54F3F0` returns not `0xFF`), it:

1. Writes a 9-byte slot record at `[esp+0x40] + 9*k` (player id, status 2, computer 0, team from the
   player object `+0x278`, race `0x20`, AI strength 1, handicap `0x64`). Defaults only.
2. Calls `0x6F545A00(cl = slot, edx = blob, push 0)`. The function copies from the per-slot
   sub-record of **player object 0** (`0x6F53E050` lookup in the table at `obj0+0x290`, `+0x30`
   array indexed by slot):

   | sub-record offset | size | copied to blob |
   |---|---|---|
   | `+0x19` | dword | blob `+0x00` |
   | `+0x1D` | dword | blob `+0x04` |
   | `+0x21` | dword | blob `+0x08` |
   | `+0x25` | dword | blob `+0x0C` |
   | `+0x29` | byte (count) | blob `+0x10` |
   | `+0x2A` | `count` bytes (memcpy `0x6F7E15EC`) | blob `+0x11..` |

3. Builds a stream reader over the blob (vtable `0x6F932C08`): data begins at blob `+0x0D`, the limit
   is the byte at blob `+0x0C` (the low byte of the `+0x25` dword), position starts at 0. Reads one
   8-byte element with two `ReadU32` calls (`0x6F5BD190` → `0x6F4C2D30`); each read succeeds only when
   `pos + 4 <= limit`, and a failed read sets `pos = limit + 1`.
4. If `pos == limit` after the two reads (`0x6F5C3A35`), two things happen:
   - if the second dword is `0` or a power of two (`x & (x-1) == 0`), the low byte of the second dword is
     stored in the slot record's byte `+6` (race position; CONFIRMED by reading, not run);
   - the fold step `S := rotl(S xor X, 3)` runs with `X` = the first dword (`0x6F5C3A4C..0x6F5C3A57`).

**Unicorn result** (`tools/emu/game_start_x_emu.py`, 300 random sub-records, 130 with the fold running,
0 mismatches): `X` = bytes `+0x26, +0x27, +0x28, +0x29` as a little-endian dword, that is
`((dword(+0x25) >> 8) & 0xFFFFFF) | (count << 24)`, and the fold runs exactly when the byte at `+0x25`
is 8. Fixed vectors (in `crates/engine/src/game_start.rs` tests):

| sub-record | tag `+0x25` | count | name | X |
|---|---|---|---|---|
| tag 8, `+0x26` = 0x16 | 8 | 2 | `Ab` | `0x02000016` |
| tag 8, `+0x26` = 0x00 | 8 | 4 | `Xyz1` | `0x04000000` |
| tag 4 | 4 | 1 | `Q` | (no fold; reader pos 5, limit 4) |

Observations (CONFIRMED by reading, not a claim about intent):
- The fold element is not two aligned dwords of the record. It spans the top three bytes of the tag
  dword, the count byte and the first four name bytes (second dword). With a short name the second dword
  contains stale stack bytes (the caller does not zero the blob beyond the copied name).
- The tag byte 8 and the byte-1 slot index of the other caller (section 6) suggest the `+0x25` dword is a
  packed header. Nothing in this pass identifies the meaning of the tag values.

**Source of the sub-records (open).** The per-slot object (`0x6F54E7E0`: `[obj+0x36]` = slot index) holds
`+0x18` (a byte read by `0x6F53E080`), the 16-byte block `+0x19..+0x28`, the count `+0x29` and the name at
`+0x2A`. No code in `Game.dll` stores directly to `+0x19`, `+0x1D`, `+0x21`, `+0x25`, `+0x26` or `+0x29`
(searched all `mov` forms with a base and an index register), and no reader call writes to these offsets.
So they are filled by a bulk copy from a packet or a stream structure, not found in this pass.
In a replay the name is LIKELY the replay player's name (`0x16` record), but the 16 bytes and the tag have no
match yet in the `w3g` records. UNSURE.

## 4. The game-start container and the replay record (CONFIRMED layout; replay link LIKELY)

The fold is the fallback. The normal path parses the **player-0 blob**:

- Player object 0's blob at `+0x1F4` is a `u16` length followed by the bytes. It is read by `0x6F545A80`
  (`ecx` = output buffer, `edx = 0`) and written by `0x6F545AE0`.
- `0x6F5C37A0` reads it into `[esp+0x180]` (`0x6F5C37FB`), then parses the container at `[esp+0x38]`
  with `0x6F5BE740` (`0x6F5C381E`). `0x6F5BE740` reads the stream with `0x6F655C40` → `0x6F6559A0`:

| container offset | read | meaning (from the w3g `0x19` body) |
|---|---|---|
| `+0x00` | `u8 n`; refused when `n >= 16` (`0x6F6559BC`) | slot count |
| `+0x08` | `n` records of 9 bytes (`0x6F6559D4`, `0x6F653E40`) | slot records |
| `+0x0C` | `u32` (`0x6F6559F0`) | **random seed** (the `S` of section 2) |
| `+0x10` | `u8` (`0x6F6559F9`) | select mode |
| `+0x11` | `u8` (`0x6F655A00`) | start spot count |

This is exactly the layout of the w3g `0x19` body (`1 + 9n + 4 + 1 + 1` bytes; `crates/w3g/src/game.rs`
reads it that way). CONFIRMED structure. The seed field of the replay's game-start record is therefore the
seed field of this container.

**Acceptance** (`0x6F5BE740`, after the parse): refused when the count is 0 (`0x6F5BE74F`) or the start
spot count is greater than the count (unsigned compare, `0x6F5BE75A`); then a duplicate check over the 9-byte
records (not modelled, UNSURE). Returns 1 when accepted (`0x6F5C3823`: `cmp eax, 0` → accepted path
`0x6F5C3B7A`), 0 otherwise.

**Setup decision** (`0x6F5C37A0`):
- accepted → no fold; `S = [esp+0x44]` = the record seed; the tail `0x6F5C3F72` applies the `0x8000` rule.
  The fold code (`0x6F5C38F0`) is not reached. CONFIRMED by control flow; the tail reading `[esp+0x44]`
  (`0x6F5C3FA2`) is CONFIRMED.
- refused and mode 5 → fold (section 3), then the same tail. CONFIRMED.
- refused and mode other → the path `0x6F5C3842..` is not traced (UNSURE).

Side effects on the fold path only (CONFIRMED by control flow; the accepted path jumps over them to `0x6F5C3B7A`):
the fold result is serialised into the blob (`0x6F5BD1F0`, `0x6F5C3B3B`) and written back to player 0
(`0x6F545AE0`, `0x6F5C3B54`), and `S` seeds a **local** generator at `[esp+0x2c]` (`0x6F4AB660` at `0x6F5C3B03`),
which then drives `0x6F4ACF60` (`0x6F5C3B15`, LIKELY the random start order). Whether the accepted path does
the same draws: UNSURE.

**Where the blob is filled from a replay (open).** The record body is exchanged by messages `0x40090079`
(handler table `0x6F5BD230`, `0x6F5C40A8`), `0x4009007E` (`0x6F5C4030`, calls `0x6F5C37A0` when
`[esi+0x10] == 0`) and `0x40090093/94` (`0x6F3011B0/0x6F3011F0`). The replay loader that would store the
`0x19` body into player 0's `+0x1F4` blob was not found. Until found, "the replay seed is `S`" is LIKELY,
not CONFIRMED.

## 5. The second seeding site `0x6F5C0EE0` (CONFIRMED path; behaviour CONFIRMED)

- The only caller is `0x6F5C141B` (`call 0x6F5C0EE0` inside the function at `0x6F5C1xxx`; the address
  `0x6F5C131A` in the previous version of this spec was wrong, found by cross-reference search).
- That caller runs `0x6F5C0EE0` when `0x6F3AFE90(flags & 0x80000)` returns non-zero (`0x6F5C13D8..0x6F5C13DF`,
  `0x6F5C1419`). The stack argument is the caller's `[esp+0x60] & 0x80000`. Which game word this is: UNSURE.
- `0x6F5C0EE0` reads the same player-0 blob (`0x6F5C0F68`, `0x6F545A80`), parses the container with
  `0x6F5BE740` (`0x6F5C0F8B`) and **does not test the result**. The seed field (`[esp+0x2c]`, container `+0x0C`)
  is then used whether or not the parse succeeded (CONFIRMED: no test between the call and the use).
  If the parse fails early, `[esp+0x2c]` is not written by the parser and holds stale stack data (UNSURE, a
  likely source of run-to-run differences in a broken record).
- `0x6F5C0EE0` seeds `G` with that value (`0x6F4AB660` at `0x6F5C11F1`), then the streams (`0x6F24F1E0` at
  `0x6F5C11FA`). No fold and no `0x8000` rule here (CONFIRMED).
- Before the seeding it loops over players `0..11` (`0x6F5C0FC0..0x6F5C0FEC`) and then over the slots
  (`0x6F5C1000..0x6F5C1099`); those loops do other setup and were not analysed further.

## 6. The 0x1A..0x1C blocks, the replay header and the flags (UNSURE)

- `w3g::GameInfo::random_seed` is the `u32` after the slot records of the `0x19` body (CONFIRMED against
  the layout in section 4; `crates/w3g/src/game.rs`). The new `w3g::start::parse_start_body` reads the same
  field with the original's limits (count below 16, count not 0, start spots not above count).
- `0x6F5C1B20` (another caller of the blob filler, `0x6F5C1BA5`) interprets the same sub-record differently:
  it requires the tag byte `+0x25` to be `1`, and then uses byte `+0x26` as a slot index into the 9-byte slot
  records. So the tag byte has at least two meanings (1 and 8). UNSURE which one a replay produces.
- The flag word: the seed tail tests `+0x38` of the settings object (game `+0x30`). The `w3g` stat-string
  flags are a different dword (`GameSettings::flags`). No code path linking the two was found. The `0x80000`
  bit that selects site 2 (section 5) and the `0x8000` bit that selects the fixed seed are both in
  `flags`-sized words. No evidence that they are the same word or that the replay bits match. UNSURE.

## 7. What the code does

- `sim::rng::slot_fold_step`, `sim::rng::slot_fold`: the fold step and the fold from zero (unchanged).
- `sim::rng::game_start_seed(settings_flags, fold)`: the `0x8000` rule (unchanged).
- `sim::Streams::start_game(table, S)`: seeds `G` with `S` and the streams from `S` (unchanged).
- `w3g::start::parse_start_body(body)`: the `0x19` body with the original's limits and acceptance rules
  (new, additive). `StartRecord::seed` is the container seed.
- `engine::game_start::fold_input(sub)`: the per-slot `X` (new, Unicorn-checked vectors).
- `engine::game_start::fold_seed(subs)`: the fold over sub-records in player-id order (new).
- `engine::game_start::setup_seed(body, mode, subs)`: the record / fold choice of section 4 (new, the
  refused-and-other-mode case returns `None`).
- `engine::game_start::resolve(explicit, replay)`: unchanged. `--seed` if given, else the replay's record seed.

## 8. Open items

1. **Source of the per-slot sub-records (section 3)**: which packet or stream structure fills the `+0x19..+0x2A`
   records of player object 0's slot table. Needed to know whether the fold ever runs for a replay.
2. **Replay loader into the player-0 blob (section 4)**: find the code that writes the `0x19` body into player
   object 0's `+0x1F4` blob, or the message that carries it. Until then the replay-seed link is LIKELY, not
   CONFIRMED. A dynamic check on the original with a real replay would settle it.
3. **Settings flag `+0x38` vs replay flags (section 6)**: which replay flag bit (if any) sets `+0x38 & 0x8000`,
   and what `0x80000` is in the flags argument of the `0x6F5C1xxx` caller.
4. **Mode argument (section 3/4)**: the meaning of the value 5 passed to `0x6F5C37A0` (from the message handler
   `0x6F5C4030`), and the mode-other path `0x6F5C3842..`.
5. **Duplicate check in acceptance (section 4)**: the rule in `0x6F5BE740` after the count and start-spot checks.
6. **Race store and the `+0x25` tag meaning (section 3, 6)**: the tag values 1 and 8 and the second-dword race store.
7. **Random race and start-location draws** (not resolved here): the local generator at `[esp+0x2c]` and
   `0x6F4ACF60` (section 4), relative order against `S`.
