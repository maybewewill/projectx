//! The per-time-slot sync value (block `0x22`), computed from simulation state and the replay stream.
//!
//! Source: docs/spec/lockstep.md sections 13 and 15 (Game.dll 1.26a). The value is written by `0x6F545840` into
//! `CNetData+0x1C64`; the instruction order was re-read from the **machine code** (section 15 corrects the earlier
//! reading, which had `X` twice and dropped the RNG term):
//!
//! ```text
//! A      = fold over the 20 engine-global slots of registry 13 (walk 19 -> 0, skip null, acc = rol(v ^ acc, 3))
//! c      = rol(A ^ 0x17AB, 3)
//! v      = T ^ c                                  T = CNetData+0x1C70, the tick clock
//! v      = rol(v,3) ^ X                           X = world+0x40, or world+0x1C when that is -1 ("next presence tag")
//! v      = rol(v,3) ^ Y                           Y = world+0x50 ("next birth tag")
//! v      = rol(v,3) ^ R                           R = byte-hash sum of the 8-byte global generator state at [0x6FAB73D8]
//! sync   = rol(v,3)
//! ```
//!
//! so `sync = rol12(T) ^ rol12(c) ^ rol9(X) ^ rol6(Y) ^ rol3(R)`: linear over GF(2) in every input.
//!
//! This module has the arithmetic, the slot hashes that are fully decoded (the net checksum of the received blocks,
//! the rand-streams hash, the cheat flags, the game/player fold) and [`SyncState`], which tracks a replay stream.
//! The hash of the units/pathing side (`X`, `Y`) and the data tables is not reproduced; they are inputs.

/// Constant XORed into the registry fold (`0x6F545840`: `xor eax, 0x17ab`).
pub const REGISTRY_XOR: u32 = 0x17AB;

/// Number of participant slots of registry 13 (`0x6F007D90` allocates 20 pointers).
pub const REGISTRY_SLOTS: usize = 20;

/// Game ticks advance `CNetData+0x1C70` by this many game milliseconds (`add [esi+0x1c70], ebp`, `0x6F553770`).
///
/// CONFIRMED (machine code, section 13.7): `ebp` is loaded from `[CNetData+0x139C]` at `0x6F553516`, but on the
/// path that runs ticks it is overwritten at `0x6F553676` by `lea ebp, [eax-0x14]` with `eax = 0x19`, i.e. the
/// 25 ms unit minus 20 = 5. The tick loop is only entered when the accumulator is at least 25 ms (`0x6F55366A`).
pub const TICK_CLOCK_STEP: u32 = TICK_UNIT_MS - TICK_UNIT_MS_REMAINDER;

/// The unit of the frame driver's tick accumulator, in ms (`cmp [esi+0x2250], 0x19`, `0x6F55366A`).
pub const TICK_UNIT_MS: u32 = 0x19;

/// The value subtracted from the unit to get the tick step (`lea ebp, [eax-0x14]`, `0x6F553676`).
pub const TICK_UNIT_MS_REMAINDER: u32 = 0x14;

/// Fold over the registry 13 participant slots, as `FUN_6f005e20` does.
///
/// The walk runs from the last slot down to the first, skips empty slots, and for each participant value `v`
/// computes `acc = rol(v ^ acc, 3)` starting from `acc = 0`.
pub fn registry_fold(slots: &[Option<u32>]) -> u32 {
    let mut acc = 0u32;
    for v in slots.iter().rev().flatten() {
        acc = (v ^ acc).rotate_left(3);
    }
    acc
}

/// `c = rol(A ^ 0x17AB, 3)`.
pub fn registry_term(fold: u32) -> u32 {
    (fold ^ REGISTRY_XOR).rotate_left(3)
}

/// Value of a registry participant of class `CMiscCustom` (vtable `0x6F8750A4`), its vtable slot 0 at
/// `0x6F0020D0` (CONFIRMED arithmetic from the machine code, section 13.7).
///
/// The node has two child links (`+4`, `+8`). `a` and `b` are the child hashes (`None` for a null link):
///
/// ```text
/// s = a ? rol(H(a), 3) : 0
/// H = b ? rol(H(b) ^ s, 3) : s
/// ```
///
/// How a child hash `H(a)` is computed (the walk `0x6F6FA990` → `0x6F4D0570`) is not reproduced here.
pub fn custom_node_hash(a: Option<u32>, b: Option<u32>) -> u32 {
    let s = a.map_or(0, |h| h.rotate_left(3));
    match b {
        Some(hb) => (hb ^ s).rotate_left(3),
        None => s,
    }
}

/// The registry fold A that gives the recorded sync `value` for clock `t`, world words `x`, `y` and RNG hash `r`:
/// the inverse of [`sync_value`]. Every value implies exactly one A once the other four inputs are fixed.
pub fn implied_registry_fold(value: u32, t: u32, x: u32, y: u32, r: u32) -> u32 {
    let v3 = value.rotate_right(3) ^ r;
    let v2 = v3.rotate_right(3) ^ y;
    let v1 = v2.rotate_right(3) ^ x;
    let v0 = v1.rotate_right(3);
    let c = v0 ^ t;
    c.rotate_right(3) ^ REGISTRY_XOR
}

/// The world words used by the sync value, read from the world object (`[0x6FAB778C]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldWords {
    /// `world+0x1C`.
    pub f1c: u32,
    /// `world+0x40`; `0xFFFFFFFF` means "use `f1c`".
    pub f40: u32,
    /// `world+0x50`.
    pub f50: u32,
}

impl WorldWords {
    /// X: `world+0x40`, or `world+0x1C` when `world+0x40` is `-1`.
    pub fn x(&self) -> u32 {
        if self.f40 == u32::MAX {
            self.f1c
        } else {
            self.f40
        }
    }

    /// Y: `world+0x50`.
    pub fn y(&self) -> u32 {
        self.f50
    }

    /// The world words after the world constructor (CONFIRMED stores in `FUN_6f46b120` at `0x6F46B120`:
    /// `+0x40 = -1`, `+0x50 = 0`). `+0x1C` is not stored by the constructor; it is taken as 0 because the object is
    /// allocated with flag 8 at `0x6F46D9xx` (zero fill, LIKELY; the allocator is Storm, not checked).
    pub fn at_start() -> Self {
        WorldWords {
            f1c: 0,
            f40: u32::MAX,
            f50: 0,
        }
    }
}

/// The tick clock `CNetData+0x1C70` (game milliseconds, one tick = [`TICK_CLOCK_STEP`]).
///
/// Reset to 0 by the CNetData init (`0x6F5505D6`, `0x6F54C5ED`); the tick loop of the frame driver adds the step
/// **before** it dispatches the tick (`0x6F553770`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickClock {
    value: u32,
}

impl TickClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current value of `CNetData+0x1C70`.
    pub fn value(&self) -> u32 {
        self.value
    }

    /// One game tick: adds [`TICK_CLOCK_STEP`] (wrapping, as the 32-bit add does).
    pub fn tick(&mut self) {
        self.value = self.value.wrapping_add(TICK_CLOCK_STEP);
    }

    /// Reset to 0 (the CNetData init).
    pub fn reset(&mut self) {
        self.value = 0;
    }
}

/// The sync value from its five inputs (machine code `0x6F5458D2..0x6F5458F9`): the clock `t`, the registry fold,
/// the world words `x` (presence tag) and `y` (birth tag) and the RNG-state hash `r`.
pub fn sync_value(t: u32, registry_fold: u32, x: u32, y: u32, r: u32) -> u32 {
    let c = registry_term(registry_fold);
    let mut v = t ^ c;
    v = v.rotate_left(3) ^ x;
    v = v.rotate_left(3) ^ y;
    v = v.rotate_left(3) ^ r;
    v.rotate_left(3)
}

/// The sync value of the current state: clock, registry slots, world words and RNG hash.
pub fn record_value(
    clock: &TickClock,
    registry: &[Option<u32>],
    world: &WorldWords,
    rng_hash: u32,
) -> u32 {
    sync_value(
        clock.value(),
        registry_fold(registry),
        world.x(),
        world.y(),
        rng_hash,
    )
}

/// The left rotation with which the participant in `slot` enters the sync value (CONFIRMED algebra): the fold
/// rotates once for every non-null slot at or below `slot`, then `c` adds 3 and the value chain 12.
///
/// The net checksum sits in slot [`slot::NET`]; with slot 1 (combat) null and slot 0 (cheat flags) present the
/// rotation is 21, which the real 1.26a replay confirms on thousands of record pairs (lockstep.md section 15.3).
pub fn participant_rotation(slots: &[Option<u32>], slot: usize) -> u32 {
    let below = slots.iter().take(slot + 1).filter(|s| s.is_some()).count() as u32;
    12 + 3 + 3 * below
}

/// Indices of the engine-global slots of registry 13 (the table entry `0x6F4C34D0(13)` + 0x10).
///
/// CONFIRMED indices: the dump function `0x6F5455A0` reads slots 0..19 in order and prints 0, 1, 2, 3, 4 and 6 as
/// the cheat, combat, net, rand, unit and game checksums; the singletons' accessors confirm 2, 3, 5, 6, 7, 8, 9 and
/// 12..19. Slots 1, 4, 10, 11 and 15 have no other reader, i.e. they stay null (LIKELY).
pub mod slot {
    /// `CCheatData` (vtable `0x6F940A90`): hash = `flags & 0xFFFFEFFF`.
    pub const CHEAT: usize = 0;
    /// Combat data; never created in a normal game (null).
    pub const COMBAT: usize = 1;
    /// `CNetData` (vtable `0x6F9590D0`): hash = the running net checksum (`CNetData+0x227C`).
    pub const NET: usize = 2;
    /// `CRandData` (vtable `0x6F92FE18`): hash = sum over the 45 streams of the byte-hash of their 8 bytes.
    pub const RAND: usize = 3;
    /// Unit data (null: no reader outside the dump).
    pub const UNIT: usize = 4;
    /// `CAbilityDatabase` (vtable `0x6F92E0A8`): a lazily mixed file hash (static for a game).
    pub const ABILITY_DB: usize = 5;
    /// `CGameWar3` (vtable `0x6F941B10`): hash = fold over the 12 player objects.
    pub const GAME: usize = 6;
    /// First of the custom-data slots 12..=19 (`CCustom*Data`, the map's modified data tables).
    pub const CUSTOM_FIRST: usize = 12;
}

/// The byte hash of the original (`x ^ (x * 0x7A2D)` on a byte value, 32-bit arithmetic).
pub fn byte_hash(b: u8) -> u32 {
    let b = u32::from(b);
    b.wrapping_mul(0x7A2D) ^ b
}

/// The sum of the byte hashes of the four bytes of `x` (the body of `0x6F280B90`, added to a zero accumulator).
pub fn word_hash(x: u32) -> u32 {
    x.to_le_bytes()
        .iter()
        .fold(0u32, |acc, &b| acc.wrapping_add(byte_hash(b)))
}

/// The hash of the 8 bytes of one RNG generator state (`0x6F280BE0`, called with a zero accumulator): used for the
/// global generator (the `R` term of the sync value) and for each of the 45 streams.
pub fn rng_state_hash(rng: &crate::rng::Rng) -> u32 {
    word_hash(rng.a).wrapping_add(word_hash(rng.b))
}

/// The rand slot's hash (`CRandData` vtable slot 0, `0x6F280C80`): the sum of [`rng_state_hash`] over the 45 streams.
pub fn rand_data_hash(streams: &crate::rng::Streams) -> u32 {
    (0..crate::rng::STREAM_COUNT).fold(0u32, |acc, k| {
        acc.wrapping_add(rng_state_hash(&streams.stream(k)))
    })
}

/// The cheat slot's hash (`CCheatData` vtable slot 0, `0x6F3834D0`): the flag word without bit 12.
pub fn cheat_data_hash(flags: u32) -> u32 {
    flags & 0xFFFF_EFFF
}

/// The game slot's hash (`CGameWar3` vtable slot 0, `0x6F3A1690`): starts at 12 and folds each present player's
/// hash `h` of the 12 player objects as `acc = rol(acc ^ h, 3)`.
pub fn game_data_hash(player_hashes: &[Option<u32>; 12]) -> u32 {
    player_hashes
        .iter()
        .flatten()
        .fold(12u32, |acc, &h| (acc ^ h).rotate_left(3))
}

/// The hash of a received block payload (`0x6F39E5C0`): `acc = 0`; for each little-endian dword `w`,
/// `acc = rol(w ^ acc, 3)`; then for each remaining byte `b`, `acc = rol(b ^ acc, 3)`.
pub fn payload_hash(bytes: &[u8]) -> u32 {
    let mut acc = 0u32;
    let mut chunks = bytes.chunks_exact(4);
    for c in &mut chunks {
        let w = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        acc = (w ^ acc).rotate_left(3);
    }
    for &b in chunks.remainder() {
        acc = (u32::from(b) ^ acc).rotate_left(3);
    }
    acc
}

/// Leave-game block id, folded into the net checksum (`0x6F5530D0`, lockstep.md section 15.1).
pub const BLOCK_LEAVE: u8 = 0x17;
/// Forced-game-end block id, folded into the net checksum.
pub const BLOCK_GAME_END: u8 = 0x2F;

/// The running net checksum, `CNetData+0x227C` (the value of registry slot [`slot::NET`]).
///
/// Every block the receive loop applies adds a term: for a time slot (`0x1E`/`0x1F`) the term is
/// `word_hash(id + word_hash(payload_hash(payload)))`, where `payload` is the block's bytes after its length
/// (the u16 time increment, then each player's `u8 id, u16 len, actions`). CONFIRMED arithmetic (machine code
/// `0x6F553307..0x6F5533BA`) and CONFIRMED against the real replay (section 15.3). The block is folded before it
/// is applied, so the sync value computed while applying a slot already contains that slot's term.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetChecksum {
    value: u32,
}

impl NetChecksum {
    /// A fresh checksum (`CNetData+0x227C = 0` at construction, `0x6F550639`).
    pub fn new() -> Self {
        Self::default()
    }

    /// The current value.
    pub fn value(&self) -> u32 {
        self.value
    }

    /// Adds one block term `v` (the original adds the four byte hashes of `v`).
    pub fn fold(&mut self, v: u32) {
        self.value = self.value.wrapping_add(word_hash(v));
    }

    /// A time-slot block with id `id` (0x1E or 0x1F) and the given payload.
    pub fn time_slot(&mut self, id: u8, payload: &[u8]) {
        self.fold(u32::from(id).wrapping_add(word_hash(payload_hash(payload))));
    }

    /// A leave-game block (`0x17`): `0x17` plus the word hashes of the reason, the player id and the result
    /// (the fourth field is not hashed; `0x6F5531BB..0x6F553226`).
    pub fn leave(&mut self, reason: u32, player: u8, result: u32) {
        let acc = u32::from(BLOCK_LEAVE)
            .wrapping_add(word_hash(reason))
            .wrapping_add(word_hash(u32::from(player)))
            .wrapping_add(word_hash(result));
        self.fold(acc);
    }

    /// A forced-game-end block (`0x2F`): `0x2F` plus the word hashes of the mode and the countdown.
    pub fn game_end(&mut self, mode: u32, countdown: u32) {
        let acc = u32::from(BLOCK_GAME_END)
            .wrapping_add(word_hash(mode))
            .wrapping_add(word_hash(countdown));
        self.fold(acc);
    }
}

/// `T` for a time slot: the tick clock `CNetData+0x1C70` when the slot is applied.
///
/// The frame driver runs ticks of 5 ms in units of 25 ms (section 11), so the clock is the elapsed time of the
/// earlier slots rounded down to a multiple of 25 ms. Checked on the real replay: of the 638 record pairs whose
/// remaining hidden state was unchanged or nearly so, 490 match this model exactly (section 15.3); the rest are
/// pairs where a hidden word also changed.
pub fn tick_clock_at_slot(elapsed_before_ms: u64) -> u32 {
    (elapsed_before_ms / u64::from(TICK_UNIT_MS) * u64::from(TICK_UNIT_MS)) as u32
}

/// Replay stream tracker: feeds the blocks in file order and gives the sync record the original would write.
///
/// The record is recomputed only when the slot's increment is not zero (the time applier skips the computation
/// for a zero amount, CONFIRMED: the file repeats the previous value on those slots, and the first slot's record
/// is 0). The hidden inputs the simulation does not produce yet are set by the caller: `registry` (slots other
/// than the net slot), `world` (X, Y) and `rng_hash` (R).
#[derive(Clone, Debug)]
pub struct SyncState {
    /// Registry slots; [`slot::NET`] is overwritten with the net checksum at every computation.
    pub registry: [Option<u32>; REGISTRY_SLOTS],
    /// The world words X and Y.
    pub world: WorldWords,
    /// R, the hash of the global generator state.
    pub rng_hash: u32,
    net: NetChecksum,
    elapsed_ms: u64,
    last: u32,
    /// The tick clock `T` (`CNetData+0x1C70`) at the next slot: 25 ms units consumed so far (see [`SyncState::pause`]).
    clock: u32,
    /// Milliseconds received and not yet consumed (below 25 unless paused).
    buffer: u64,
    paused: bool,
    /// 25 ms units the last slot released to the tick loop.
    last_units: u64,
}

impl Default for SyncState {
    fn default() -> Self {
        let mut registry = [None; REGISTRY_SLOTS];
        registry[slot::CHEAT] = Some(0);
        registry[slot::NET] = Some(0);
        SyncState {
            registry,
            world: WorldWords::at_start(),
            rng_hash: 0,
            net: NetChecksum::new(),
            elapsed_ms: 0,
            last: 0,
            clock: 0,
            buffer: 0,
            paused: false,
            last_units: 0,
        }
    }
}

impl SyncState {
    /// A tracker in the start state (net checksum 0, clock 0, record 0).
    pub fn new() -> Self {
        Self::default()
    }

    /// The net checksum so far.
    pub fn net(&self) -> &NetChecksum {
        &self.net
    }

    /// Elapsed time of the slots seen so far.
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }

    /// The record the last slot produced.
    pub fn last_record(&self) -> u32 {
        self.last
    }

    /// The registry with the net slot filled in.
    pub fn registry_now(&self) -> [Option<u32>; REGISTRY_SLOTS] {
        let mut r = self.registry;
        r[slot::NET] = Some(self.net.value());
        r
    }

    /// Processes one time-slot block: folds it into the net checksum, computes the record when the increment is
    /// not zero and returns the record to expect after the slot. `payload` starts with the u16 increment.
    pub fn time_slot(&mut self, id: u8, payload: &[u8]) -> u32 {
        self.net.time_slot(id, payload);
        let inc = payload
            .get(..2)
            .map_or(0, |b| u64::from(u16::from_le_bytes([b[0], b[1]])));
        if inc != 0 {
            let t = self.clock;
            let reg = self.registry_now();
            self.last = sync_value(
                t,
                registry_fold(&reg),
                self.world.x(),
                self.world.y(),
                self.rng_hash,
            );
        }
        self.elapsed_ms += inc;
        self.buffer += inc;
        self.last_units = 0;
        if !self.paused {
            self.last_units = self.buffer / u64::from(TICK_UNIT_MS);
            self.buffer %= u64::from(TICK_UNIT_MS);
            self.clock = self.clock.wrapping_add(self.last_units as u32 * TICK_UNIT_MS);
        }
        self.last
    }

    /// The tick clock `T` the next slot's record will use.
    pub fn clock(&self) -> u32 {
        self.clock
    }

    /// 25 ms units the last slot released to the tick loop (zero while paused).
    pub fn last_units(&self) -> u64 {
        self.last_units
    }

    /// Whether the game is paused.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// The pause action (0x01): the tick loop stops. Slots keep arriving and their time piles up in the buffer, the
    /// clock stays (CONFIRMED on the EchoIsles replay: every pair inside a pause has a zero residual with `T` frozen).
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// The resume action (0x02): the whole units that piled up during the pause are dropped (only the remainder below
    /// 25 ms is kept) and the clock advances one unit at once. Fitted on the EchoIsles replay (five pauses, 2,766 of
    /// 5,503 pairs exact, against 406 without the rule); the one extra unit is the least understood part.
    pub fn resume(&mut self) {
        if self.paused {
            self.paused = false;
            self.buffer %= u64::from(TICK_UNIT_MS);
            self.clock = self.clock.wrapping_add(TICK_UNIT_MS);
        }
    }

    /// Processes a leave-game block (folded into the net checksum; no record is computed).
    pub fn leave(&mut self, reason: u32, player: u8, result: u32) {
        self.net.leave(reason, player, result);
    }

    /// Processes a forced-game-end block.
    pub fn game_end(&mut self, mode: u32, countdown: u32) {
        self.net.game_end(mode, countdown);
    }
}

/// The part of a record difference that the net checksum and the clock do not explain.
///
/// For two consecutive computed records `a` and `b` the XOR of the values is
/// `rol12(Ta ^ Tb) ^ rol(Na ^ Nb, rotation) ^ rest`, where `rest` carries the hidden words (X, Y, R and the other
/// registry slots). A pair whose `rest` is zero is predicted exactly from the replay stream alone.
pub fn residual(a: &RecordPoint, b: &RecordPoint, net_rotation: u32) -> u32 {
    a.value
        ^ b.value
        ^ (a.clock ^ b.clock).rotate_left(12)
        ^ (a.net ^ b.net).rotate_left(net_rotation)
}

/// What the stream alone gives for one computed record: the recorded value, the tick clock model and the net checksum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordPoint {
    /// The recorded (or predicted) sync value.
    pub value: u32,
    /// `T` from [`tick_clock_at_slot`].
    pub clock: u32,
    /// The net checksum after the slot's own block.
    pub net: u32,
}

/// The pathing-object registry behind the two world words X and Y (docs/spec/lockstep.md section 16).
///
/// CONFIRMED against the original (`tools/emu/pr_registry_emu.py`, 400 random sequences, 24,598 operations, 0
/// mismatches of the whole table state): every `CPrRgEntry`-derived object takes a slot in an entry table
/// (`0x6F4AEFF0` registers, `0x6F4AEF00` unregisters). Slots come from a free list threaded through the entries
/// (the most recently freed slot is reused first, like the handle table), else the table grows by one. A
/// registration with no descriptor (or descriptor slot -1) also takes the **birth tag** `Y` (`world+0x50`, then
/// incremented); `Y` never decreases. `X` is the head of the free list when it is not -1, else the table length
/// (the dump prints `world+0x40`, or `world+0x1C` when that is -1).
///
/// The second table (descriptor slot -2) hands out the tag from `world+0x54` and is not part of the sync value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrRegistry {
    used0: Vec<bool>,
    free0: Vec<u32>,
    y: u32,
    y2: u32,
    used1: Vec<bool>,
    free1: Vec<u32>,
}

/// A registration handle: the slot and the tag the object received.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrHandle {
    /// Table slot; bit 31 set = the flagged table.
    pub slot: u32,
    /// The tag (birth tag for table 0).
    pub tag: u32,
}

impl PrRegistry {
    /// An empty registry (the world constructor state: no entries, `Y = 0`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an object in table 0 and gives it a birth tag (`0x6F4AEFF0` without a descriptor).
    pub fn register(&mut self) -> PrHandle {
        let slot = match self.free0.pop() {
            Some(s) => s,
            None => {
                self.used0.push(false);
                (self.used0.len() - 1) as u32
            }
        };
        self.used0[slot as usize] = true;
        let tag = self.y;
        self.y = self.y.wrapping_add(1);
        PrHandle { slot, tag }
    }

    /// Registers an object in the flagged table (descriptor slot -2); it does not touch `Y`.
    pub fn register_flagged(&mut self) -> PrHandle {
        let slot = match self.free1.pop() {
            Some(s) => s,
            None => {
                self.used1.push(false);
                (self.used1.len() - 1) as u32
            }
        };
        self.used1[slot as usize] = true;
        let tag = self.y2;
        self.y2 = self.y2.wrapping_add(1);
        PrHandle {
            slot: slot | 0x8000_0000,
            tag,
        }
    }

    /// Unregisters an object (`0x6F4AEF00`): its slot goes on the free list; `Y` is unchanged.
    pub fn unregister(&mut self, h: PrHandle) {
        let (used, free, i) = if h.slot & 0x8000_0000 != 0 {
            (&mut self.used1, &mut self.free1, h.slot & 0x7FFF_FFFF)
        } else {
            (&mut self.used0, &mut self.free0, h.slot)
        };
        if let Some(u) = used.get_mut(i as usize) {
            if *u {
                *u = false;
                free.push(i);
            }
        }
    }

    /// `Y`, the next birth tag.
    pub fn y(&self) -> u32 {
        self.y
    }

    /// `X`: the head of the free list, or the table length when the list is empty.
    pub fn x(&self) -> u32 {
        self.free0
            .last()
            .copied()
            .unwrap_or(self.used0.len() as u32)
    }

    /// Live objects in table 0.
    pub fn live(&self) -> usize {
        self.used0.iter().filter(|u| **u).count()
    }

    /// The world words for the sync value.
    pub fn world_words(&self) -> WorldWords {
        WorldWords {
            f1c: self.used0.len() as u32,
            f40: self.free0.last().copied().unwrap_or(u32::MAX),
            f50: self.y,
        }
    }
}

/// Whether a run of "Y only" residuals `r_i = rotr6(D_i)` can come from one monotone counter: a start `y0 < 2^16`
/// with `y_{i+1} = y_i ^ r_i` and `y_i < y_{i+1} <= y_i + max_step` for every step.
pub fn y_chain_feasible(run: &[u32], max_step: u32) -> bool {
    'start: for y0 in 0..(1u32 << 16) {
        let mut y = y0;
        for &r in run {
            let ny = y ^ r;
            if !(y < ny && ny <= y.saturating_add(max_step)) {
                continue 'start;
            }
            y = ny;
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::{Rng, Streams};

    #[test]
    fn pr_registry_reuses_most_recently_freed_slot() {
        let mut r = PrRegistry::new();
        let a = r.register();
        let b = r.register();
        let c = r.register();
        assert_eq!((a.slot, b.slot, c.slot, r.y(), r.x()), (0, 1, 2, 3, 3));
        r.unregister(a);
        r.unregister(c);
        assert_eq!(r.x(), 2);
        let d = r.register();
        assert_eq!((d.slot, d.tag, r.y(), r.x()), (2, 3, 4, 0));
        r.unregister(d);
        r.unregister(d);
        assert_eq!(r.x(), 2);
        let f = r.register_flagged();
        assert_eq!(f.slot, 0x8000_0000);
        assert_eq!(r.y(), 4);
        assert_eq!(r.live(), 1);
    }

    #[test]
    fn y_chain_detects_monotone_runs() {
        assert!(y_chain_feasible(&[1, 3], 8));
        assert!(!y_chain_feasible(&[0x8000, 0x8000], 8));
    }

    /// Step-by-step version written in the order of the machine code, used as the reference.
    fn asm_order(t: u32, a: u32, x: u32, y: u32, r: u32) -> u32 {
        let c = (a ^ 0x17AB).rotate_left(3);
        let mut e = t ^ c;
        e = e.rotate_left(3);
        e ^= x;
        e = e.rotate_left(3);
        e ^= y;
        e = e.rotate_left(3);
        e ^= r;
        e.rotate_left(3)
    }

    #[test]
    fn value_matches_machine_code_order() {
        for (t, a, x, y, r) in [
            (0u32, 0u32, 0u32, 0u32, 0u32),
            (110, 0x1234_5678, 0x9ABC_DEF0, 0x0F0F_0F0F, 0x0123_4567),
            (9_749_884, 0xDEAD_BEEF, 0xFFFF_FFFF, 1, 2),
            (u32::MAX, 7, 3, 5, 9),
        ] {
            assert_eq!(sync_value(t, a, x, y, r), asm_order(t, a, x, y, r));
        }
    }

    #[test]
    fn value_is_linear_in_every_input() {
        // sync = rol12(T) ^ rol12(c) ^ rol9(X) ^ rol6(Y) ^ rol3(R)
        let (a, x, y, r) = (0x1357_9BDF, 0x2468_ACE0, 0x0BAD_F00D, 0x00C0_FFEE);
        let base = sync_value(0, a, x, y, r);
        for t in [5u32, 110, 9_749_884] {
            assert_eq!(sync_value(t, a, x, y, r) ^ base, t.rotate_left(12));
        }
        assert_eq!(sync_value(0, a, x ^ 0x10, y, r) ^ base, 0x10u32.rotate_left(9));
        assert_eq!(sync_value(0, a, x, y ^ 0x10, r) ^ base, 0x10u32.rotate_left(6));
        assert_eq!(sync_value(0, a, x, y, r ^ 0x10) ^ base, 0x10u32.rotate_left(3));
    }

    #[test]
    fn registry_fold_walks_from_the_last_slot_and_skips_empty_ones() {
        // Walk order: slot 2 first, then slot 0 (slot 1 is empty).
        let slots = [Some(0x11u32), None, Some(0x22)];
        let acc1 = 0x22u32.rotate_left(3);
        let acc2 = (0x11 ^ acc1).rotate_left(3);
        assert_eq!(registry_fold(&slots), acc2);
        assert_eq!(registry_fold(&[None; REGISTRY_SLOTS]), 0);
    }

    #[test]
    fn registry_term_uses_the_constant_and_rotation() {
        assert_eq!(registry_term(0), 0x17AB_u32.rotate_left(3));
    }

    #[test]
    fn world_x_falls_back_to_1c_only_for_minus_one() {
        let w = WorldWords { f1c: 7, f40: 9, f50: 3 };
        assert_eq!((w.x(), w.y()), (9, 3));
        let w = WorldWords { f1c: 7, f40: u32::MAX, f50: 3 };
        assert_eq!(w.x(), 7);
        let w = WorldWords { f1c: 7, f40: 0, f50: 3 };
        assert_eq!(w.x(), 0);
    }

    #[test]
    fn tick_clock_adds_five_per_tick_and_resets() {
        let mut c = TickClock::new();
        assert_eq!(c.value(), 0);
        for _ in 0..22 {
            c.tick();
        }
        assert_eq!(c.value(), 110);
        c.reset();
        assert_eq!(c.value(), 0);
        let mut w = TickClock { value: u32::MAX - 2 };
        w.tick();
        assert_eq!(w.value(), 2);
    }

    #[test]
    fn tick_step_is_the_25ms_unit_minus_20() {
        assert_eq!(TICK_UNIT_MS, 25);
        assert_eq!(TICK_CLOCK_STEP, 5);
        assert_eq!(TICK_UNIT_MS - TICK_UNIT_MS_REMAINDER, TICK_CLOCK_STEP);
    }

    #[test]
    fn clock_at_a_slot_is_the_elapsed_time_in_whole_25ms_units() {
        assert_eq!(tick_clock_at_slot(0), 0);
        assert_eq!(tick_clock_at_slot(24), 0);
        assert_eq!(tick_clock_at_slot(25), 25);
        assert_eq!(tick_clock_at_slot(127_630), 127_625);
    }

    #[test]
    fn custom_node_hash_follows_the_machine_code() {
        // Leaf: no children gives 0 (the object is empty).
        assert_eq!(custom_node_hash(None, None), 0);
        // Only the first link: rol(H(a), 3).
        assert_eq!(custom_node_hash(Some(0x1234_5678), None), 0x1234_5678u32.rotate_left(3));
        // Both links: rol(H(b) ^ rol(H(a),3), 3).
        let (ha, hb) = (0xDEAD_BEEFu32, 0x0BAD_CAFEu32);
        assert_eq!(custom_node_hash(Some(ha), Some(hb)), (hb ^ ha.rotate_left(3)).rotate_left(3));
        // Only the second link: its hash is not rotated before the final rol, and the first-link term is 0.
        assert_eq!(custom_node_hash(None, Some(hb)), hb.rotate_left(3));
    }

    #[test]
    fn implied_fold_inverts_the_sync_value() {
        for (a, t, x, y, r) in [
            (0u32, 0u32, 0u32, 0u32, 0u32),
            (0x1A25_8A4E, 0, 0, 0, 0),
            (0x1357_9BDF, 110, 0x2468_ACE0, 0x0BAD_F00D, 0x0102_0304),
            (u32::MAX, u32::MAX, 3, 5, 7),
        ] {
            let v = sync_value(t, a, x, y, r);
            assert_eq!(implied_registry_fold(v, t, x, y, r), a);
        }
    }

    #[test]
    fn world_words_at_start_match_the_constructor() {
        let w = WorldWords::at_start();
        assert_eq!(w.x(), 0);
        assert_eq!(w.y(), 0);
        assert_eq!(w.f40, u32::MAX);
    }

    #[test]
    fn record_value_combines_the_components() {
        let mut clock = TickClock::new();
        clock.tick();
        let mut slots = [None; REGISTRY_SLOTS];
        slots[4] = Some(0xABCD);
        let world = WorldWords { f1c: 1, f40: 2, f50: 3 };
        let expected = sync_value(5, registry_fold(&slots), 2, 3, 0x77);
        assert_eq!(record_value(&clock, &slots, &world, 0x77), expected);
    }

    #[test]
    fn byte_and_word_hashes_follow_the_machine_code() {
        assert_eq!(byte_hash(0), 0);
        assert_eq!(byte_hash(1), 0x7A2D ^ 1);
        assert_eq!(byte_hash(0xFF), (0xFFu32 * 0x7A2D) ^ 0xFF);
        assert_eq!(word_hash(0), 0);
        assert_eq!(word_hash(0x0000_0201), byte_hash(1) + byte_hash(2));
    }

    #[test]
    fn payload_hash_chains_dwords_then_bytes() {
        assert_eq!(payload_hash(&[]), 0);
        // Two bytes: rol(b1 ^ rol(b0 ^ 0, 3), 3).
        let b0 = 0x26u32.rotate_left(3);
        assert_eq!(payload_hash(&[0x26, 0x00]), b0.rotate_left(3));
        assert_eq!(payload_hash(&[0x26, 0x01]), (1 ^ b0).rotate_left(3));
        // One dword then one byte.
        let w = 0x0403_0201u32;
        let acc = w.rotate_left(3);
        assert_eq!(payload_hash(&[1, 2, 3, 4, 9]), (9 ^ acc).rotate_left(3));
    }

    #[test]
    fn net_checksum_of_the_first_replay_slots() {
        // The first two time slots of the real 1.26a replay (block id 0x1F): an empty slot and a 38 ms slot.
        // Values from the Python model, whose primitives match the original under Unicorn
        // (tools/emu/net_checksum_emu.py: receive-loop cases, 0 mismatches).
        let mut n = NetChecksum::new();
        n.time_slot(0x1F, &[0, 0]);
        assert_eq!(n.value(), 969_580);
        n.time_slot(0x1F, &[0x26, 0]);
        assert_eq!(n.value(), 11_885_160);
    }

    #[test]
    fn leave_and_end_blocks_add_their_fields() {
        let mut n = NetChecksum::new();
        n.leave(1, 2, 3);
        let want = word_hash(0x17 + word_hash(1) + word_hash(2) + word_hash(3));
        assert_eq!(n.value(), want);
        let mut m = NetChecksum::new();
        m.game_end(5, 6);
        assert_eq!(m.value(), word_hash(0x2F + word_hash(5) + word_hash(6)));
    }

    #[test]
    fn rand_cheat_and_game_hashes() {
        let rng = Rng { a: 0x0000_0102, b: 0 };
        assert_eq!(rng_state_hash(&rng), byte_hash(2) + byte_hash(1));
        let mut s = Streams::default();
        s.stream_mut(7).a = 0x0000_0001;
        assert_eq!(rand_data_hash(&s), byte_hash(1));
        assert_eq!(cheat_data_hash(0xFFFF_FFFF), 0xFFFF_EFFF);
        let mut players = [None; 12];
        assert_eq!(game_data_hash(&players), 12);
        players[3] = Some(5);
        assert_eq!(game_data_hash(&players), (12u32 ^ 5).rotate_left(3));
    }

    #[test]
    fn participant_rotation_counts_non_null_slots_below() {
        let mut slots = [None; REGISTRY_SLOTS];
        slots[slot::CHEAT] = Some(0);
        slots[slot::NET] = Some(0);
        // Net + cheat: 12 + 3 + 3 * 2 = 21, the value seen on the real replay.
        assert_eq!(participant_rotation(&slots, slot::NET), 21);
        slots[slot::COMBAT] = Some(0);
        assert_eq!(participant_rotation(&slots, slot::NET), 24);
    }

    #[test]
    fn tracker_repeats_the_record_on_zero_increment_slots() {
        let mut s = SyncState::new();
        // First slot: increment 0, record stays 0.
        assert_eq!(s.time_slot(0x1F, &[0, 0]), 0);
        // 38 ms: a record is computed from clock 0, net slot, cheat 0.
        let r1 = s.time_slot(0x1F, &[0x26, 0]);
        let mut reg = [None; REGISTRY_SLOTS];
        reg[slot::CHEAT] = Some(0);
        reg[slot::NET] = Some(11_885_160);
        assert_eq!(r1, sync_value(0, registry_fold(&reg), 0, 0, 0));
        // Zero increment again: the old value is repeated and the net checksum still moves.
        let net_before = s.net().value();
        assert_eq!(s.time_slot(0x1F, &[0, 0]), r1);
        assert_ne!(s.net().value(), net_before);
        // Next real slot sees 38 ms elapsed: clock model 25.
        let r2 = s.time_slot(0x1F, &[0x26, 0]);
        reg[slot::NET] = Some(s.net().value());
        assert_eq!(r2, sync_value(25, registry_fold(&reg), 0, 0, 0));
        assert_eq!(s.elapsed_ms(), 76);
    }

    #[test]
    fn residual_vanishes_when_only_net_and_clock_change() {
        let (x, y, r) = (5u32, 6u32, 7u32);
        let mut slots = [None; REGISTRY_SLOTS];
        slots[slot::CHEAT] = Some(0);
        let rot = {
            let mut s = slots;
            s[slot::NET] = Some(0);
            participant_rotation(&s, slot::NET)
        };
        let mk = |t: u32, n: u32| {
            let mut s = slots;
            s[slot::NET] = Some(n);
            RecordPoint {
                value: sync_value(t, registry_fold(&s), x, y, r),
                clock: t,
                net: n,
            }
        };
        let p = mk(25, 0x1234_5678);
        let q = mk(50, 0x0F0F_F0F0);
        assert_eq!(residual(&p, &q, rot), 0);
        let mut changed = q;
        changed.value ^= 0x40;
        assert_eq!(residual(&p, &changed, rot), 0x40);
    }
}
