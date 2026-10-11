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

## 3. Open: the game-info records (UNSURE, the main gap)

The decompressed body starts with the game-info records (host record, game name, empty string, encoded
settings, slot count, game type, language, `0x16` player records). Our parser (`w3g::game::parse_game_info`)
reads them in that order (CONFIRMED against the public layout and our synthetic tests, not against the
original). **The machine code that reads these records from the decompressed body was not located.** The
searches that did not lead there: the `0x16`/`0x19` switch cases (`0x6F54CD40` is the Battle.net event
switch; `0x6F3012E0` is an error-string table), and the callers of the container parser `0x6F5BE740`
(`0x6F3A49E0`, `0x6F3AF8D0`, `0x6F443860`, `0x6F58AC20`, `0x6F4061F0`; these are game-setup and UI callers).

What was found on the way:

- `0x6F443860` (setup builder, CONFIRMED structure): calls `0x6F4434D0` (validation), `0x6F535410` (opens the
  file and reads the body through `0x6F535050`), `0x6F534A40` (returns a value `B`), then branches on `B`:
  - `B < 0x11DC`: a 7-byte source record per slot (UNSURE: the copies read 9 bytes per record, overlapping;
    decompile may be mistyped), 9-byte output records, AI strength byte `+7` = 1 and handicap byte `+8` = 100
    (CONFIRMED defaults);
  - otherwise: an 8-byte source record per slot, handicap `+8` = 100 (CONFIRMED), AI strength `+7` copied
    from the source (CONFIRMED).
  `B` is LIKELY the replay build number (the branch constants 6000, 6058 and 0x17AA are build-like). The
  9-byte slot record layout is therefore build-dependent; `w3g` derives the size from the record length
  (7, 8 or 9), which covers the same cases.
- `0x6F443860` then calls `0x6F5BE710` and `0x6F5BE740` (the container parse) and sets game `+0x400 = 1`
  only when **both** return non-zero (CONFIRMED). So a refused container also blocks the setup flag.

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

1. Find the code that reads the game-info records from the decompressed body (section 3). Route: the
   callers of `0x6F535050` (the block reader) and of `0x6F535410`; the function that calls `0x6F5BE740`
   with the body in its stream, and the writer of player object 0's `+0x1F4` blob (game-start.md 8.2).
2. Decode the flags word: find the consumers of the decoded settings object (`game +0x30`, flag word `+0x38`)
   and map speed, visibility, teams and fixed seed bits. Check one replay byte-by-byte if available.
3. Confirm `FUN_6f534470` as CRC-32 (Unicorn: run it on a known header) and then enforce the header and
   block checksums in `w3g` (sections 1 and 2).
4. Confirm `B = FUN_6f534a40` is the build number, and the 7-byte source record (section 3).
5. Random race and start-location draws (game-start.md section 7).

## 8. Tests

- `crates/w3g`: `header_size_must_match_the_version` (header.rs); the existing header, block and record tests.
- `crates/engine/src/replay_setup.rs` (unit, 7 tests): race bits, record seed vs fixed-seed bit, local
  player, refusals (no slots, start spots above slots), slot application (names, races, teams, controllers,
  empty slots), random race keeps the map value.
- `crates/engine/tests/replay_boot.rs` (integration, 4 tests): a synthetic map and a synthetic replay boot
  through `Engine::boot_from_replay`; slots replace the map players in the report; the fixed-seed bit; a
  refused record is an error; garbage is a replay error.
- Workspace: `cargo test --workspace` passes without and with `GAME_DLL`; clippy `-D warnings` is clean.
