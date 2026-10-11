# Warcraft III 1.26a game.dll: game-start seeding (spec)

Scope: the value `S` the original gives to the gameplay generator and the 45 streams when a game
starts, what it is computed from, and how a replay's game-start record relates to it.
Companion to `docs/spec/rng.md` (generator, streams, SetRandomSeed). Clean-room: addresses and
plain-words behaviour only; no decompiled text is reproduced here.

Confidence: CONFIRMED = read from the machine code and checked under Unicorn where noted;
LIKELY = strong structural evidence; UNSURE = open or a guess.

## 1. Summary

1. At game start the original seeds the global generator `G` with one value `S` and then
   initialises the 45 streams from the same `S` (no step in between). CONFIRMED.
   This is `sim::Streams::start_game`.
2. `S` is either the constant `0x77617233` or a rotate-xor fold `S := rotl(S xor X, 3)` over
   per-player values `X`, starting from 0. The choice is made by bit `0x8000` of the game
   settings flag word. CONFIRMED (structure and branch); the origin of `X` is UNSURE (section 3).
3. A replay's game-start record has a random seed field. Whether the original uses that field as
   `S` is UNSURE: the record is not read by any code path found so far (section 4).
4. Random races, random start positions and the start-location order are not resolved here (section 5).

## 2. The game-setup seeding block (CONFIRMED)

Function `0x6F5C37A0` (game setup, very large). The seeding tail is at `0x6F5C3F72..0x6F5C3FB5`.

- The game object is the global at `0x6FAB65F4`; its field `+0x30` is the settings object; the
  flag word is the settings object's `+0x38` (`test dword [ecx+0x38], 0x8000`).
- Flag bit `0x8000` set: push the constant `0x77617233` and call the generator seeder `0x6F4AB660`
  with the global generator state (global pointer `0x6FAB73D8`) as `this`. Then `ecx = 0x77617233`
  and the stream initialiser `0x6F24F1E0` runs.
- Flag bit clear: the accumulator `S` (frame slot `[esp+0x44]`) is pushed and the seeder runs; then
  `ecx = S` and the stream initialiser runs.

Checked under Unicorn (`tools/emu/game_start_emu.py`): the seeder and the stream initialiser are
hooked and their arguments recorded. 300 random `(flags, S)` pairs, 0 mismatches. Vectors:
flags `0x0` with S `0x11111111` gives `S` in both calls; flags `0x8000` gives `0x77617233` in both.

## 3. The fold (CONFIRMED step, UNSURE inputs)

Inside the same function, the loop over player ids `1..16` (the loop counter is a byte from 1 to
16 inclusive) does, for each id that maps to a slot (the id lookup `0x6F54F3F0` returns `0xFF` for
none), a fold step at `0x6F5C3A4C..0x6F5C3A5B`:

    S := rotl32(S xor X, 3)

`X` is the first dword of an 8-byte element read by `0x6F5BD190` from a small reader object. The
reader is a binary-stream reader (buffer pointer, base offset, limit, position); its buffer is a
local byte blob that `0x6F545A00` fills from the player record of the slot. Checked under Unicorn:
the step itself, 300 random `(S, X)` pairs, 0 mismatches.

What is NOT established:
- What the blob contains. `0x6F545A00` copies, from a sub-record of player 0 selected by the slot
  index, four dwords, one count byte and a string. The reader is then given the count byte and
  the bytes after it, and this does not line up cleanly with the copy's own layout. So the fold
  input is not yet identified. Candidates (none confirmed): the per-slot 16-byte player key, the
  player name bytes, or values from the 9-byte slot records.
- Whether the loop also folds the second dword (it is used only as a test on the reader position).
- The pre-loop count `N` (from `0x6F5C0780`) is an allocation size for 9-byte slot records
  (`0x6F443810`), not the XOR operand. The earlier note in `rng.md` 4.1 (XOR with the count) is
  wrong on this point.

Other setup-side facts (CONFIRMED by reading):
- The 9-byte records built in the loop have the same layout as the replay's slot records: byte 0 =
  player id, byte 2 = status (2 written), byte 3 = computer flag, byte 4 = team (from the player
  object `+0x278`), byte 5 = colour, byte 6 = race (0x20 = random by default), byte 7 = AI
  strength (1), byte 8 = handicap (0x64 = 100 by default). These are only defaults for the
  in-memory slot table; they do not feed the fold as far as found.

## 4. The second seeding site `0x6F5C0EE0` and the replay (UNSURE)

Function `0x6F5C0EE0` has the same seeding tail: it seeds `G` with the value at its frame slot
`[esp+0x2c]` and then initialises the streams from that same value (`0x6F5C11E6..0x6F5C11FA`).
Within this function that slot is only read, never written, in the code read so far (it is
written through an aliased structure from `0x6F5BE740`'s caller). This is the most likely
replay / join path because the caller `0x6F5C131A` takes a record argument and passes its field
`+0x10` (the candidate random seed) on the stack. But the callee does not read a stack argument
and the caller does not clean up, so the mapping of that argument is inconsistent and unresolved.

The replay's game-start record (`0x19`) is parsed by the `w3g` crate: `GameInfo::random_seed`,
the dword after the slots. Whether the original reads that field into `S`: UNSURE. No
replay file was available to check it end to end.

## 5. Other draws before the first tick (open)

- Random race and random start-location resolution happen in setup code that this pass did not
  reach; the order of those draws relative to `S` is unknown. Open.
- The settings flag word in memory (`+0x38`) and the replay's `GameSettings::flags` are not
  proven to be the same bits. The `0x8000` rule is therefore not yet applied to replays. Open.

## 6. What the code does

- `sim::rng::slot_fold_step`, `sim::rng::slot_fold`: the fold step and the fold from zero.
- `sim::rng::game_start_seed(settings_flags, fold)`: the `0x8000` rule (CONFIRMED branch).
- `sim::Streams::start_game(table, S)`: seeds `G` with `S` and the streams from `S`.
- `engine::game_start::resolve(explicit, replay)`: `--seed` if given, else the replay's record
  seed (UNSURE link), else 0. `w3boot --replay FILE.w3g` uses it.

## 7. Verification

- `tools/emu/game_start_emu.py` (Unicorn, `GAME_DLL` required): fold step 300/300, seed
  selection 300/300 (0 mismatches).
- `crates/sim/tests/game_start.rs`: the emulator vectors for the fold step, fold order, the flag
  rule and the start-game seeding.
- `crates/engine/src/game_start.rs`: resolution rules on a synthetic replay.

## 8. Open items

1. Identify the per-player value `X` of the fold (the blob of `0x6F545A00` and the reader's layout).
2. Decide whether the replay record seed is `S`, or the fold result; check with a real 1.26a replay
   (none available in this session).
3. The 0x8000 flag: which bits of the replay's game flags correspond to the settings object's `+0x38`.
4. Random race / start location draws and their order.
