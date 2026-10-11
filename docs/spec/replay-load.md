# Replay load path, header, block framing and game setup (Game.dll 1.26a)

Status: pass 1, 2026-10-11. Scope: how a `.w3g` file is opened, how its header and blocks are read,
how the game-info records reach the setup, the stat-string encoding, and `Engine::boot_from_replay`.
Companion to `docs/spec/game-start.md` (seed `S`), `docs/spec/replay-actions.md` (commands) and
`docs/spec/game-loop.md` (replay pump). Clean-room: addresses and plain-words behaviour only; no
decompiled text is reproduced here. Game.dll SHA1 `c5d8624b...`.

Labels: **CONFIRMED** = read in the machine-code decompile at the address given, or checked by
arithmetic. **LIKELY** = strong structural evidence, not run. **UNSURE** = open or a guess.

Result in one paragraph: the header and the block framing are confirmed and one rule (the header size per
version) is now enforced. The stat-string encoder is confirmed and its decoder is its exact inverse. The
game-info records (host, name, settings, player records) are read by a path that is **not yet located**, so
the link "replay record -> `+0x1F4` start blob -> setup" is still open (LIKELY, unchanged from game-start.md).
The engine now boots from a replay's setup (seed, slots, names, teams, colors, races) and the synthetic tests
check that path. Speed, handicap and the random race draw are not applied.

---

## 1. Open: the replay header reader (CONFIRMED)

- Magic: the 28-byte string `Warcraft III recorded game\x1A\0` at `0x6F957B08` (.rdata). Two code references
  (dword `imm32`): `0x6F534803` and `0x6F5359ED`.
- `0x6F534750` (replay header reader, CONFIRMED). It reads 40 bytes first (magic 28, header size at +0x1C,
  compressed size at +0x20, header version at +0x24). Then:
  - version 0: the header size must be `0x40` (error `0x0B` otherwise); 24 more bytes are read (sub-header);
  - version 1: the header size must be `0x44` (error `0x0B`); 28 more bytes are read;
  - the header CRC (version 0 at +0x3C, version 1 at +0x40) is compared with the CRC of the whole header with
    that field zeroed (`FUN_6f534470`, error `0x0D` on mismatch);
  - the file must be at least the compressed size (error `0x0E`).
  - Product name: version 1 stores the product in reverse order on disk (our parser reverses it; the byte
    order was not checked against a file, UNSURE).
- `0x6F534A70` is a header-only reader used by the replay list (UI). `0x6F4434D0` and `0x6F443860` call it.
- Header writer `0x6F535960` (CONFIRMED): writes a 0x44-byte version-1 header, the CRC computed the same way,
  so the CRC rule above is the one the game writes.

**Engine status:** `crates/w3g/src/header.rs` now refuses a header whose size does not match its version
(`Error::BadHeaderSize`, unit test `header_size_must_match_the_version`). The CRC is **not** checked: the
function `FUN_6f534470` is not yet confirmed to be standard CRC-32 (reflected 0xEDB88320, init and final
xor as in `docs/spec/game-loop.md`), and an unverified CRC could refuse valid replays.

## 2. Block framing (CONFIRMED structure; CRC not enforced)

`0x6F535050` (block reader, CONFIRMED) loops over the blocks after the header. Each block has an 8-byte
header:

| offset | size | meaning | check in the original |
|---|---|---|---|
| +0 | u16 | compressed size `csize` | `csize != 0`, `csize <= max block size` (error `0x16`) |
| +2 | u16 | decompressed size `dsize` | `dsize != 0`, `dsize <= max`, `dsize >= csize` (errors `0x17`, `0x15`) |
| +4 | u16 | checksum | equals `(c >> 16) ^ c` (low 16 bits) of the CRC of the 8-byte header with bytes 4..7 zeroed (error `0x13`) |
| +6 | u16 | not compared | (bytes 6..7 are zeroed for the CRC and restored after) |

Then `csize` bytes of zlib data follow. Our `w3g::header::decompress_blocks` reads the same layout but does
not check the sizes beyond the inflate result, and does not check the checksum (it reads the field as a
`u32`). The checksum is not enforced yet (same CRC caveat as section 1).

The max block size is a field of the reader object (`param_1[0xE]`); its value is not traced (UNSURE).

## 3. The leading records of the body (CONFIRMED reader order; layout UNCONFIRMED, conflicts with `GameInfo`)

**Result.** The game setup builder `0x6F443860` reads the decompressed body from its stream object `G`
(its ECX) from byte 0, **positionally**. No record id is compared in this path: there is no `0x16`
loop and the `0x19` id byte is not read (the `0x19` payload is read as a `u16` length plus bytes).
Body position at the start is 0 (LIKELY: `G`'s vtable slot `+0x1C`, called before the read, resets it).

The order of reads, for build `B` (`B` = the value `0x6F534A40` returns, which is `G+0x238`):

| # | read | how | lands in |
|---|---|---|---|
| 1 | NUL-terminated string, max 2047 bytes | `0x6F309A00`, dst `G+0x418` | string handle at `G+0x420` (via `0x6F4C5CF0`, interned; empty = no store) |
| 2 | same, **only if `B >= 0x178B`** | `0x6F309A00`, dst `G+0x424` | handle at `G+0x42C` |
| 3 | 184-byte block | `0x6F6521B0`, dst = stack `esp+0xDC` (not in `G`) | see below |
| 4 | `u16` length (max 128) + bytes | `0x6F443720`, dst = stack `esp+0x58` | the **start blob** (`0x19` payload) |
| 5 | two dwords | `0x6F4C2D30` | `G+0x430`, `G+0x434` |
| 6 | 20 raw bytes, **only if `B >= 0x17AA`** | `0x6F650770` (→ `0x6F4C3420`, read N bytes) | `G+0x438..+0x44B` |
| 7 | two dwords | `0x6F4C2D30` | `G+0x404`, `G+0x408` |
| 8 | NUL-terminated string, max 2047 | `0x6F309A00`, dst `G+0x40C` | handle at `G+0x414` |

Then `0x6F5BE710` / `0x6F5BE740` validate the container at `G+0x3EC` (blob) and the game flag `G+0x400`
is set only when both succeed (CONFIRMED). For `B < 0x1770` the container is also parsed from the blob
(7- or 8-byte source records, see below); for `B >= 0x1770` the blob is only stored.

**The 184-byte block (step 3, `0x6F6521B0`, CONFIRMED):** cstr <= 32 at `+0x00`, cstr <= 16 at `+0x20`,
cstr <= 128 at `+0x30` (the encoded stat string: it has no zero bytes, so NUL termination is consistent),
dword at `+0xB0`, dword at `+0xB4`. The block lives on the setup's stack. Its three strings are
not stored in `G`; the setup later passes the block as EDX to `0x6F5BE710`, which ignores EDX (CONFIRMED),
so the stat string is **not consumed by this function** (the consumer is not in `0x6F443860`; open).
`+0xB0` is the stack dword the `B < 0x11DC` branch uses as its record count (UNSURE which record).

**`B`:** `0x6F534A40` = `[ECX+0x238]`, written by the block reader `0x6F535050` from a header field (CONFIRMED
that `G+0x238` is stored there; the header field is LIKELY the build dword at header `+0x38`; the low
16 bits are the build and the high 16 are the flags; whether the setup uses the whole dword is UNSURE.
Multiplayer replays have flag `0x8000`, so the whole-dword reading would put them all in the `>= 0x1770`
branch; this needs the real file to settle).

**Primitives (CONFIRMED by disassembly):**
- `0x6F4C2B30(pos, n)`: fails when `pos + n > limit` and sets `pos = limit + 1`; otherwise maps the chunk
  (vtable slot 2 when the range is outside the mapped chunk; not needed for one chunk).
- `0x6F4C2E10(dst, max)`: NUL-terminated read; copies at most `max` bytes including the NUL. No NUL within
  `max` bytes → `pos = limit + 1`. dst[0] = 0 on failure.
- `0x6F4C2FD0(dst, n)`: exact read (the `u16` blob reader uses it through `0x6F443720`; the length check
  `<= 0x80` is in `0x6F443720`).
- `0x6F4C2C10/C70/D30`: exact byte/word/dword reads; a short read fails as above.

**Model and emulator (CONFIRMED, `tools/emu/replay_leading_emu.py`).** The emulator runs the reads above
in the same order on a synthetic body placed in one chunk of `G` (the `0x6F4C5CF0` string store is hooked
to record the bytes; `memcpy` through the IAT is hooked; `Storm` ordinal 465 is only on error paths).
Python `model_read` implements the same semantics. **400 random bodies (builds 5999, 6000, 6040, 6059,
including failing reads), 0 mismatches in landings, position and error flag** (`--cases 400 --seed 11`;
300 with seed 7 also 0).

### 3.1 Comparison with `crates/w3g` (the disagreement)

`crates/w3g::GameInfo` reads: `u32`, host record (`0x00`, pid, name, extra), game name, empty string,
settings string, `u32` slots, `u32` type, `u32` language, `0x16` player records, then `0x19`. The
original's reader (above) reads: string, string (build >= 6027), 3 strings and 2 dwords (the block), the
blob, 2 dwords, 20 bytes (build >= 6058), 2 dwords, a string. The two layouts are different.

Case: the body `crates/w3g/tests/synthetic.rs` writes, as build 6059. The original stops inside the start
blob with `pos = 142`, `limit = 141` (the first string reads `\x10\x01`, the second is empty, the block's
strings are garbage). `crates/w3g/tests/leading.rs::the_gameinfo_layout_is_refused_by_the_original_reader`
asserts the refusal. The `GameInfo` parser accepts the same bytes (its own test).

**Conclusion.** Either the real 1.26a body is the layout the original reads, and `GameInfo` (and the
synthetic replays built on it) is wrong for real files, or the setup builder is not the replay loader for
these files. Both readings are open until a real `.w3g` (1.26a) is parsed. The `0x19` payload itself
(count byte, slot records, seed, mode, start spots) matches the container reader (section 4 of
game-start.md), so the blob parse is the least uncertain part.

### 3.2 Rust port

`crates/w3g/src/leading.rs`: `read_leading(body, build) -> Result<Leading, LeadingError>` follows the
order above (same error rules). Tests: `crates/w3g/tests/leading.rs` (24 Unicorn vectors in
`tests/leading_vectors.rs`, generated by `tools/emu/embed_leading_vectors.py`) and unit tests in the module.
`GameInfo` is **not** changed: the disagreement is not resolved, so replacing its layout would be a guess.

### 3.3 Other fields (CONFIRMED layout or structure, consumers open)

- `0x6F655880` / `0x6F655910` (container with 7 or 8-byte source records, used for `B < 0x11DC` and
  otherwise, reached only through the `B < 0x1770` branch): count byte `< 16`, records, dword seed, byte at
  `+0x10`, byte at `+0x11`. The setup copies each source record into a 9-byte record with status 1 at byte 7 and
  handicap **100** at byte 8 (CONFIRMED defaults in that branch). So a replay of build < 6000 has handicap 100
  for every slot regardless of the record.
- `B >= 0x1770`: no container parse in the setup; the container is validated from the blob and handicap
  is not read here. Handicap per slot: **not applied** (no confirmed source for builds 6000 and later).
- Game speed, visibility, observers, teams-together, lock-teams, full-shared-control, random-hero,
  random-races: no consumer found in `0x6F443860`. The stat string (block step 3) is not read after the
  block parse in this function. The consumers must be in a later reader of `G` or of the game object
  (not located). The fixed-seed bit `0x8000` of game settings `+0x38` is still confirmed only for the seed
  tail (game-start.md section 2); whether the replay's stat-string flags are the same word is UNSURE.
- Map checksum, map path, host name: the first two strings of the body and the stat string are the only
  candidates (their mapping to the stat fields is open). Not applied.

### 3.4 What the engine applies

`Engine::boot_from_replay` is **unchanged**. Nothing new is confirmed as a field the engine does not already
use: game speed and visibility have no located consumer, handicap for builds >= 6000 is not read here, and
the blob/slot records are the container layout already used (`crates/engine/src/replay_setup.rs`). The open
disagreement (section 3.1) is the first thing to settle on a real replay.

---

## 4. Stat string (the encoded settings): encoder CONFIRMED, decoder = its inverse

`0x6F651860` (CONFIRMED) is the encoder. It reads raw bytes from the stream reader `0x6F4C2C10` until the end
test (vtable +0x18), and writes into the output buffer (`param_2`):

- for each group of 8 output bytes, byte 0 is a mask that starts at 1 (bit 0 set);
- each raw byte `b` is written as `b | 1`;
- mask bit `j` (j = 1..7, the position within the group) is set to `b & 1`;
- the output ends with a NUL byte.

Since `b | 1` is odd, the encoded data never contains a zero byte (which is why it can be a C string).

The decoder is therefore: for data byte `j` of a group, value = stored byte if mask bit `j` is set, else
stored byte minus 1. This is exactly `w3g::game::decode_string` (CONFIRMED by algebra; the decoder's own
machine code was not located). The round trip is tested (`string_codec_roundtrip`).

Caller: `0x6F654710` (`Net::CGameDescStore`, the Battle.net game-description path, CONFIRMED): the encoded
stat string sits at the description object `+0x30`. The replay path uses the same encoding (LIKELY, since
the replay writer and the description share the stat string).

### Decoded field layout (UNSURE)

Our parser reads: `u32 flags`, `u8 zero`, `u16 map width`, `u16 map height`, `u32 map checksum`, cstring map
path, cstring host name. This matches the public layout. The consumers of the decoded fields were not
located, so the field order and the meaning of each field are **not confirmed**. Specifically open:

- which bits of `flags` mean speed, visibility, observers, teams together, lock teams, full shared control,
  random hero and random races (the public list; no bit is confirmed here);
- whether the `0x8000` and `0x80000` bits tested by the setup (game-start.md sections 2 and 5) are these
  flag bits. Our link is LIKELY (the seed tail reads the settings object `+0x38`, and `+0x38` is assumed to be
  the decoded flag word), not CONFIRMED;
- map checksum: the Storm map-checksum algorithm is not implemented, so a map is not verified against the
  replay (the replay's map path is kept for display only).

## 5. Setup and seeding (links to game-start.md)

- The seed field of the `0x19` body is the container `+0x0C` (game-start.md section 4, CONFIRMED layout).
- With the fixed-seed bit (`0x8000` of the settings flags, LIKELY the decoded flag word): the seed is the
  constant `0x77617233` (CONFIRMED rule, game-start.md section 2). `sim::rng::game_start_seed` implements
  the rule; the engine applies it in `setup_of`.
- Without the bit: the seed is the record seed (LIKELY for replays; the blob loader that makes the record
  reach the container is still open, see game-start.md section 8.2).

## 6. What the engine now does (`crates/engine/src/replay_setup.rs`)

- `setup_of(replay)`: refuses a record with no slots or with more start spots than slots (the acceptance
  rules of game-start.md section 4, CONFIRMED); seed = `sim::rng::game_start_seed(flags, record seed)`;
  first occupied human slot = local player; keeps the host name, map path, slot records and player records.
- `apply_slots(game, setup)`: slot `n` = JASS player `n` (LIKELY mapping: the slot index is the player index,
  the setup loops over player ids through the slot lookup `0x6F54F3F0`, CONFIRMED loop, LIKELY meaning).
  Sets name (from the player record with the slot's player id), race (`0x01` human, `0x02` orc, `0x04` night
  elf, `0x08` undead; LIKELY JASS values; other bits keep the map value), color, controller (computer flag,
  occupied status), slot state (1 when status is 2 = occupied, LIKELY), team, start location (the slot index,
  a placeholder). Players past the record are emptied.
- `Engine::boot_from_replay(replay_bytes, inputs)`: parses the file, sets `Inputs::rng_seed` to the replay
  seed, boots the map as `Engine::boot` does, then applies the slots over the w3i players and sets the local
  player. The notes say when no `Game.dll` is given (the RNG is then not seeded).

Not applied (gaps, each a note or a documented field):

- **game speed**: the speed bits are not located (section 4). The game state keeps its default.
- **handicap and AI strength**: the simulation's player object has no handicap or AI-strength field (the
  `natives/*` files are owned by other agents). They are kept in `ReplaySetup::slots`.
- **random race and random start positions**: the original draws them with the local generator
  (game-start.md section 7, not resolved). A random slot keeps the map's race and start location.
- **map checksum and map identity**: not verified.
- **duplicate and fold paths** of the container acceptance (game-start.md sections 3, 4): not modelled. A
  refused record returns `SetupError` instead of booting.

## 7. Open items (in order of value)

1. **Settle the leading layout (section 3.1)** on a real 1.26a replay: parse the body with `w3g::leading::read_leading` (or the Unicorn reader) and compare with `GameInfo`. Until then, `GameInfo` is UNCONFIRMED and so is every field read from it.
2. Decode the flags word: find the consumers of the decoded settings object (`game +0x30`, flag word `+0x38`)
   and map speed, visibility, teams and fixed seed bits. Check one replay byte-by-byte if available.
3. Confirm `FUN_6f534470` as CRC-32 (Unicorn: run it on a known header) and then enforce the header and
   block checksums in `w3g` (sections 1 and 2).
4. Confirm `B = FUN_6f534a40` is the build number, and the 7-byte source record (section 3).
5. Random race and start-location draws (game-start.md section 7).

## 8. Tests

- `crates/w3g/src/leading.rs` (unit, 4 tests) and `crates/w3g/tests/leading.rs` (24 Unicorn vectors in `tests/leading_vectors.rs`, plus the GameInfo-layout refusal case).
- `crates/w3g`: `header_size_must_match_the_version` (header.rs); the existing header, block and record tests.
- `crates/engine/src/replay_setup.rs` (unit, 7 tests) and `crates/engine/tests/replay_boot.rs` (integration, 4 tests), unchanged.
- Emulator: `tools/emu/replay_leading_emu.py` (400 random bodies, 0 mismatches), `tools/emu/embed_leading_vectors.py` (writes the Rust vectors).
- Workspace: `cargo test --workspace` with and without `GAME_DLL`; clippy `-D warnings`.

## 9. Pass 2 (2026-10-11): container CRCs CONFIRMED, the body consumer, settings on the real file

Supersedes the "CRC not enforced" statements of sections 1 and 2. Labels as in section 0.

### 9.1 CRC rules (CONFIRMED on the real 1.26a file: 152 blocks, 0 failures; enforced in `crates/w3g`)

| check | what is compared | original | Rust |
|---|---|---|---|
| header | the whole header, read as 0x44 bytes, with its own u32 at `+0x40` zeroed, CRC-32 (zlib polynomial, zlib init/final), must equal the stored u32 at `+0x40` | `FUN_6f534750`, error `0x0D` | `checksum::header_check`, `Error::HeaderCrc` |
| block header | the 8 block-header bytes with bytes 4..7 zeroed, CRC-32, folded to 16 bits `(c>>16)^c`, must equal the u16 at block `+4` | block reader `FUN_6f535050`, error `0x13` | `Error::BlockHeaderCrc` |
| block payload | the `csize` compressed bytes, CRC-32, folded, must equal the u16 at block `+6` (this field is **compared**; section 2 said it was not) | same reader, after the payload read, error `0x19` | `Error::BlockDataCrc` |

- The version-0 header CRC field is at `+0x3C` (section 1, from the reader's size check). Not checked on a file.
- The folded CRC is the one the original computes with its own table code: `FUN_6f649360` builds the reflected
  0xEDB88320 table (256 entries at object+0x8E0), `FUN_6f649410` updates it byte by byte from a caller-supplied
  start value (CONFIRMED). The block and header values equal zlib's CRC-32 with the standard init and final xor.
- Other block-reader checks (CONFIRMED in the decompile, not all tested): `csize == 0` or `csize > max` → `0x16`;
  `dsize == 0` or `dsize > max` → `0x17`; `dsize < csize` → `0x15`; the total decompressed size against the header
  sum (`0x10`); the header size `0x44` for version 1 (section 1). `max` is the reader's field `+0x3C`, value UNSURE.
- Test: `crates/w3g/tests/checksums.rs` (synthetic flips of the header, a block header and a payload byte give the
  three errors; real file under `W3G_REPLAY`).

### 9.2 The body's consumer (NOT located; two negative results)

- The loader path: `FUN_6f4061f0` (load-replay path, CONFIRMED caller chain) → `FUN_6f535410` (header then
  `FUN_6f535050`, which fills the body buffer with the inflated blocks; copies 0x104 bytes, LIKELY `MAX_PATH`
  for the file name, into object+0x90) → the body buffer is then handed on. The consumer of the buffer is not found yet.
- Negative 1: the positional reader `0x6F443860` (section 3) does not fit the real body. Its first read is a
  NUL-terminated string from byte 0, which on the real file is `10 01` (the `0x110` dword is the first 4 bytes).
  Starting at byte 4 instead gives an empty first string and `03 "ingamero"` as the second, so the 184-byte block
  that follows is misaligned. So `0x6F443860` is the lobby or in-memory setup reader, not the replay body reader.
- Negative 2: the record ids `0x16` and `0x19` are never compared as immediates in Game.dll (a linear scan of every
  `cmp`/`mov`/`push`/`sub` with those values finds only table registrations and error codes). A replay reader
  that branches on record ids is therefore not a simple compare; the likely form is a dispatch through the
  message factory table (`0x6F005720` owner, `0x6F005BB0` registration, one entry per message id, LIKELY). The
  decompile of the block reader's `0x16`/`0x17`/`0x19` hits is error codes (`iStack = 0x16` etc.), not ids.
- So `crates/w3g::GameInfo` remains the reference layout for real files (it decodes all 15,721 actions of the real
  file), and the Game.dll reader of it is open.

### 9.3 Settings on the real file (CONFIRMED values, bit meanings UNSURE)

- Real file (DotA Imba, build 6059): settings flags `0x40064802`, map 1984 x 1984, slot count 12, game type `0x1`,
  language word `0x049E9390` (as parsed), 10 player records, 11 occupied slots (slot records with status 2).
- Set bits of the flag word: `0x00000002`, `0x00000800`, `0x00004000`, `0x00020000`, `0x00040000`, `0x40000000`.
- **No `0x8000` and no `0x80000`**: the fixed-seed rules of game-start.md section 2 do not apply, so the setup seed
  is the `0x19` record seed `0x08F1814A` (`crates/engine/tests/replay_real.rs`, env `W3G_REPLAY`). This confirms the
  "no fixed seed in a normal replay" case on a real file; the rule itself is unchanged.
- Speed (LEAD, not a bit): the speed clamp `0x6F53E890` (`0..2`, section lockstep 9) is called from `0x6F3AFE90`
  with the byte at `[object+0x47]`, where the object is the game-settings holder of that routine. The flag-to-byte
  mapping is not traced. Teams, visibility and observers: no consumer identified yet.

### 9.4 Encoder note

- `crates/w3g::encode_string` writes groups of 7 bytes with a mask byte and **no** terminating NUL. The original
  stat string is read as a C string (section 4 says the encoder ends it with NUL, CONFIRMED). Fixtures append the
  NUL themselves. Not changed here (no caller depends on the missing NUL outside tests).

### 9.5 Tests and tools

- `crates/w3g/src/checksum.rs` (CRC-32 check value, fold), `crates/w3g/src/writer.rs` (`write_file`: a version-1
  file with the correct CRC fields, used by all synthetic fixtures in `w3g`, `engine` and `engine/tests`).
- `crates/w3g/examples/checksums.rs` (`--tsv` per-slot export of time and 0x22 values, do not commit its output).
- `crates/w3g/examples/dump.rs` prints the game name, settings flags, map size, slot count, game type and language.
