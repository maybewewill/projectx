//! Abilities and buffs: the ability objects owned by units, the buff list, invulnerability and timed life.
//!
//! Spec: `docs/spec/abilities.md` (with its addendum) and the factory table `docs/spec/ability_factory_1.26a.tsv`
//! (417 base codes). Labels as elsewhere. CONFIRMED = read in the spec's decompile notes; LIKELY = strong inference;
//! UNSURE = a guess or an open point. Each decision that is not CONFIRMED is marked where it is made.
//!
//! What is modelled:
//!
//! * **Ability objects** ([`Ability`]). Each has its base code (the factory's raw code), a [`BaseKind`] (the
//!   behaviour the model knows; every other code is [`BaseKind::Generic`], data only), flags (`+0x20`: removed `0x08`,
//!   unit-owned `0x20`, listener `0x80`), the permanence counter (`+0x38`), the zero-based level index (`+0x50`,
//!   `GetUnitAbilityLevel` = index + 1) and a copy of the definition it was created from (`+0x54`; the max level
//!   comes from it, default 1).
//! * **Fixed slots** ([`FixedAbilities`]). The move ability `Amov` (`+0x1EC`), the attack ability `Aatk` (`+0x1E8`), the
//!   hero ability `AHer` (`+0x1F0`), the build abilities (`+0x1F4`, six codes) and `AInv` (`+0x1F8`) are fixed fields
//!   of the unit, not list entries (CONFIRMED). Attaching one stores it in its slot. `Amov` and `Aatk` have no factory
//!   leaf (CONFIRMED from the factory table), so `UnitAddAbility` refuses them (LIKELY; not tested in the original).
//! * **Invulnerability** (CONFIRMED structure): the unit is invulnerable while it has the ability `Avul` in its list.
//!   `SetUnitInvulnerable(true)` adds it when missing and `(false)` removes it. Unlike `UnitAddAbility`, adding it does
//!   not require a registry record (UNSURE: the original's `Avul` wrapper is called directly by the unit code, so the
//!   record lookup is probably not on that path); a record is copied when one is registered. Whether `Avul` lives in
//!   the buff list or the ability list is UNSURE; it is kept in the ability list, so the buff natives do not see it.
//! * **Buffs** ([`Buff`]). A per-unit list, separate from the ability list (the original keeps buffs in the same list;
//!   UNSURE, see above). A buff has its base code, its polarity and filter data ([`BuffPredicates`]) and, for timed
//!   life, its duration state. The predicates of the codes the model knows are in [`buff_predicates`]; the counting
//!   and removal rules are in [`BuffSelect`] and are the spec's (CONFIRMED rules, LIKELY meaning of the filters).
//! * **Timed life** (`UnitApplyTimedLife`, `UnitPauseTimedLife`): a duration strictly above `0.0` starts a one-shot
//!   expiry timer (message `0xD01C4`, queue A; CONFIRMED by Unicorn, `docs/spec/abilities-2.md`). Other durations start
//!   none. Expiry removes the buff (CONFIRMED path). The unit's death after the removal is still modelled as `KillUnit`
//!   (`natives/ability.rs`); the route from the removal to the death in the original is UNSURE (abilities-2.md section 3.3).
//!
//! Not modelled: ability cooldowns (storage and countdown not found, `docs/spec/abilities-2.md` section 1), the level
//! records (`+0x4C`, 0x68 bytes), the supply and food adjustment of timed life, the removal notifications (slot 106),
//! the buff classes beyond the predicates above, and the periodic buff effects. Stacking: each application adds an
//! entry (the creator does not search for an existing buff, CONFIRMED read; no refresh rule found).

use std::collections::BTreeMap;
use std::ops::Index;

use softfloat::{arith, F32};

use crate::natives::game::{Game, ObjKey, Payload};
use crate::timer::TimerId;

/// Builds a raw code from its four characters (the original's big-endian `'abcd'` integer, CONFIRMED).
pub const fn code(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

/// `'Amov'`: the move ability (fixed field `+0x1EC`).
pub const MOVE: u32 = code(b"Amov");
/// `'Aatk'`: the attack ability (fixed field `+0x1E8`).
pub const ATTACK: u32 = code(b"Aatk");
/// `'Avul'`: invulnerability (factory leaf, the ability `SetUnitInvulnerable` adds and removes).
pub const INVULNERABLE: u32 = code(b"Avul");
/// `'AEev'`: the evasion ability (class `CAbilityEvasion`, vtable `0x6F89D62C`; its code getter returns this).
pub const EVASION: u32 = code(b"AEev");
/// `'Aloc'`: the locust ability.
pub const LOCUST: u32 = code(b"Aloc");
/// `'AHer'`: the hero ability (fixed field `+0x1F0`).
pub const HERO: u32 = code(b"AHer");
/// `'AInv'`: the inventory ability (fixed field `+0x1F8`).
pub const INVENTORY: u32 = code(b"AInv");
/// `'BTLF'`: the timed life buff (the default creator of `UnitApplyTimedLife`).
pub const TIMED_LIFE: u32 = code(b"BTLF");
/// The seven special timed life ids of `UnitApplyTimedLife` (CONFIRMED ids; their classes are not read, UNSURE).
pub const TIMED_LIFE_SPECIALS: [u32; 7] = [
    code(b"BUan"),
    code(b"BFig"),
    code(b"BEfn"),
    code(b"Bhwd"),
    code(b"Bplg"),
    code(b"Brai"),
    code(b"BHwe"),
];
/// The six build ability codes that share the build fixed field `+0x1F4`.
pub const BUILD_CODES: [u32; 6] = [
    code(b"ANbu"),
    code(b"AHbu"),
    code(b"AEbu"),
    code(b"AGbu"),
    code(b"AObu"),
    code(b"AUbu"),
];

/// Flag `0x08` of `+0x20`: the ability was removed (set by the removal routine before it is unlinked).
pub const FLAG_REMOVED: u32 = 0x08;
/// Flag `0x20` of `+0x20`: the ability is owned by a unit (set by the factory before it is attached).
pub const FLAG_UNIT_OWNED: u32 = 0x20;
/// Flag `0x80` of `+0x20`: an event listener is registered for the ability.
pub const FLAG_LISTENER: u32 = 0x80;

/// Whether `base` may be added to a unit by `UnitAddAbility` through the factory. `Amov` and `Aatk` have no factory
/// leaf (CONFIRMED from the factory table), so they are never added this way.
pub fn is_factory_code(base: u32) -> bool {
    base != MOVE && base != ATTACK
}

/// Whether `base` belongs to a fixed slot (see [`FixedAbilities`]).
pub fn is_fixed_code(base: u32) -> bool {
    matches!(base, MOVE | ATTACK | HERO | INVENTORY) || BUILD_CODES.contains(&base)
}

/// The behaviour the model knows for a base code. Every other code is [`BaseKind::Generic`]: data only, with no
/// behaviour beyond its flags, permanence and level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseKind {
    /// `Amov`: the move ability. Accepts the move family of orders (`orders.rs`).
    Move,
    /// `Aatk`: the attack ability. Accepts the attack family of orders.
    Attack,
    /// `Avul`: invulnerability.
    Invulnerable,
    /// `Aloc`: the locust ability (no behaviour yet).
    Locust,
    /// One of the six build abilities (`ANbu`, `AHbu`, `AEbu`, `AGbu`, `AObu`, `AUbu`, fixed field `+0x1F4`). Accepts
    /// the build orders (an order id that is a unit type id), see `orders.rs`.
    Build,
    /// `BTLF`: the timed life buff.
    TimedLife,
    /// Any other base code: data only.
    Generic,
}

impl BaseKind {
    /// The kind of base code `base`.
    pub fn of(base: u32) -> Self {
        match base {
            MOVE => BaseKind::Move,
            ATTACK => BaseKind::Attack,
            INVULNERABLE => BaseKind::Invulnerable,
            LOCUST => BaseKind::Locust,
            b if BUILD_CODES.contains(&b) => BaseKind::Build,
            TIMED_LIFE => BaseKind::TimedLife,
            _ => BaseKind::Generic,
        }
    }
}

/// An ability type as the simulation needs it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AbilityType {
    /// The maximum level from the ability record (its `+0x3C`, read only when the record's flag `+0x2C` is set).
    /// `None` means no limit data: the original's max then is 1 (`natives/unit_attr.rs`).
    pub max_level: Option<i32>,
}

/// The ability type registry, keyed by the raw ability id. An id that is not registered cannot be added to a
/// unit through the factory (`UnitAddAbility` fails, LIKELY: the original's factory finds no record).
///
/// TODO(gamedata): the engine should load it from the ability rows of the SLK data at map load.
#[derive(Debug, Clone, Default)]
pub struct AbilityTypes {
    by_id: BTreeMap<u32, AbilityType>,
}

impl AbilityTypes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces the ability type `id`. Returns the type it replaced.
    pub fn insert(&mut self, id: u32, ty: AbilityType) -> Option<AbilityType> {
        self.by_id.insert(id, ty)
    }

    /// The ability type with raw id `id`.
    pub fn get(&self, id: u32) -> Option<&AbilityType> {
        self.by_id.get(&id)
    }
}

/// An ability object owned by a unit (the ability of the original, `CAbility`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ability {
    /// The base raw code (the factory's key, `+0x30` of the definition).
    pub base: u32,
    /// The behaviour kind of `base`.
    pub kind: BaseKind,
    /// The flag word (`+0x20`), see [`FLAG_REMOVED`], [`FLAG_UNIT_OWNED`], [`FLAG_LISTENER`].
    pub flags: u32,
    /// The permanence counter (`+0x38`). `UnitMakeAbilityPermanent` adds or subtracts one; no clamp.
    pub permanent: i32,
    /// The zero-based level index (`+0x50`). The level is this plus one.
    pub level_index: i32,
    /// The definition the ability was created from (`+0x54`). A copy: the original keeps a pointer, and the
    /// registry does not change during a game.
    pub definition: Option<AbilityType>,
    /// Per-player bit mask at `+0x6C`. Only the rescue ability `Arsc` uses it: bit `p` set means player `p` can
    /// rescue the unit (CONFIRMED by disassembly of `0x6F07B3E0`).
    pub mask: u32,
}

/// The rescue ability's base code `'Arsc'` (`0x6F07B3E9` pushes it).
pub const RESCUE: u32 = 0x4172_7363;

impl Ability {
    /// A new ability at level 1 with permanence 0 and the unit-owned flag (the factory's state before attach).
    pub fn new(base: u32, definition: Option<AbilityType>) -> Self {
        Ability {
            base,
            kind: BaseKind::of(base),
            flags: FLAG_UNIT_OWNED,
            permanent: 0,
            level_index: 0,
            definition,
            mask: 0,
        }
    }

    /// The level (`GetUnitAbilityLevel`): the index plus one.
    pub fn level(&self) -> i32 {
        self.level_index + 1
    }

    /// The max level: the definition's max when it has one, and 1 otherwise (CONFIRMED, `FUN_6F02E570`).
    pub fn max_level(&self) -> i32 {
        self.definition.and_then(|d| d.max_level).unwrap_or(1)
    }

    /// Whether flag `flag` (one of the `FLAG_*` constants) is set.
    pub fn has_flag(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    /// Whether the ability was removed.
    pub fn is_removed(&self) -> bool {
        self.has_flag(FLAG_REMOVED)
    }

    /// One level up (`IncUnitAbilityLevel`, CONFIRMED): the guard compares the zero-based index with the max, so
    /// the level can reach max plus one (the spec's reading of the decompile, which the tests pin).
    pub fn inc_level(&mut self) {
        if self.level_index < self.max_level() {
            self.level_index += 1;
        }
    }

    /// One level down (`DecUnitAbilityLevel`, CONFIRMED): stops at index 0.
    pub fn dec_level(&mut self) {
        if self.level_index > 0 {
            self.level_index -= 1;
        }
    }

    /// `SetUnitAbilityLevel` (CONFIRMED in the spec): the target is at least 1, and it is the max when the max is at
    /// or below it. Returns the level that was set.
    pub fn set_level(&mut self, requested: i32) -> i32 {
        let max = self.max_level();
        let mut target = requested.max(1);
        if max <= target {
            target = max;
        }
        self.level_index = target - 1;
        target
    }

    /// `UnitMakeAbilityPermanent`: adds one for `true`, subtracts one for `false`.
    pub fn adjust_permanent(&mut self, permanent: bool) {
        self.permanent += if permanent { 1 } else { -1 };
    }
}

/// The fixed ability slots of a unit (see the module notes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixedAbilities {
    /// `+0x1E8`: `Aatk`.
    pub attack: Option<Ability>,
    /// `+0x1EC`: `Amov`.
    pub mv: Option<Ability>,
    /// `+0x1F0`: `AHer`.
    pub hero: Option<Ability>,
    /// `+0x1F4`: one of the six build codes.
    pub build: Option<Ability>,
    /// `+0x1F8`: `AInv`.
    pub inventory: Option<Ability>,
}

/// The abilities of a unit: the fixed slots, then the list in attach order (CONFIRMED structure, LIKELY order).
///
/// The list is keyed by base code: a unit has at most one ability with a given code. Lookups by code go to the
/// fixed slot for a fixed code and to the list otherwise (the original's `FUN_6F0787D0`, CONFIRMED).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitAbilities {
    fixed: FixedAbilities,
    list: Vec<Ability>,
}

impl UnitAbilities {
    fn fixed_slot(&self, base: u32) -> Option<&Option<Ability>> {
        match base {
            MOVE => Some(&self.fixed.mv),
            ATTACK => Some(&self.fixed.attack),
            HERO => Some(&self.fixed.hero),
            INVENTORY => Some(&self.fixed.inventory),
            b if BUILD_CODES.contains(&b) => Some(&self.fixed.build),
            _ => None,
        }
    }

    fn fixed_slot_mut(&mut self, base: u32) -> Option<&mut Option<Ability>> {
        match base {
            MOVE => Some(&mut self.fixed.mv),
            ATTACK => Some(&mut self.fixed.attack),
            HERO => Some(&mut self.fixed.hero),
            INVENTORY => Some(&mut self.fixed.inventory),
            b if BUILD_CODES.contains(&b) => Some(&mut self.fixed.build),
            _ => None,
        }
    }

    /// The ability with base code `base`, if the unit has it.
    pub fn get(&self, base: u32) -> Option<&Ability> {
        match self.fixed_slot(base) {
            Some(slot) => slot.as_ref().filter(|a| a.base == base),
            None => self.list.iter().find(|a| a.base == base),
        }
    }

    /// The ability with base code `base`, mutable.
    pub fn get_mut(&mut self, base: u32) -> Option<&mut Ability> {
        if is_fixed_code(base) {
            self.fixed_slot_mut(base)?
                .as_mut()
                .filter(|a| a.base == base)
        } else {
            self.list.iter_mut().find(|a| a.base == base)
        }
    }

    /// Whether the unit has the ability `base`.
    pub fn contains(&self, base: u32) -> bool {
        self.get(base).is_some()
    }

    /// Attaches `ability`: a fixed code goes to its slot (replacing what was there), any other code is appended.
    /// The caller checks for duplicates (the factory does, see `natives/unit_attr.rs`).
    pub fn attach(&mut self, ability: Ability) {
        match self.fixed_slot_mut(ability.base) {
            Some(slot) => *slot = Some(ability),
            None => self.list.push(ability),
        }
    }

    /// Removes the ability `base` and returns it. A fixed slot is cleared (UNSURE: whether the original clears the
    /// fixed field on removal is not read).
    pub fn remove(&mut self, base: u32) -> Option<Ability> {
        if is_fixed_code(base) {
            let slot = self.fixed_slot_mut(base)?;
            if slot.as_ref().is_some_and(|a| a.base == base) {
                slot.take()
            } else {
                None
            }
        } else {
            let i = self.list.iter().position(|a| a.base == base)?;
            Some(self.list.remove(i))
        }
    }

    /// Every ability in order: move, attack, hero, build and inventory (the fixed slots), then the list.
    pub fn iter(&self) -> impl Iterator<Item = &Ability> {
        let f = &self.fixed;
        [&f.mv, &f.attack, &f.hero, &f.build, &f.inventory]
            .into_iter()
            .flatten()
            .chain(self.list.iter())
    }
}

impl Index<&u32> for UnitAbilities {
    type Output = Ability;

    /// The ability `base`; panics when the unit does not have it (for tests and checked callers).
    fn index(&self, base: &u32) -> &Ability {
        self.get(*base).expect("the unit has no such ability")
    }
}

/// The polarity and filter data of a buff (the predicates of the original's buff class).
///
/// `positive` and `negative` are the polarity predicates (slots 121 and 120, LIKELY meaning). `magic`, `physical`,
/// `aura` and `dispellable` are slots 118, 119, 122 and 204; `timed_life` is slot 203 (LIKELY from the native notes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuffPredicates {
    pub positive: bool,
    pub negative: bool,
    pub magic: bool,
    pub physical: bool,
    pub aura: bool,
    pub timed_life: bool,
    pub dispellable: bool,
}

/// The predicates of the buff with base code `base`, for the codes the model knows.
///
/// `BTLF` is harmful (negative) and timed (UNSURE: the polarity is inferred, not read). The seven special timed life
/// ids are timed and have no polarity (UNSURE: their classes are not read). Any other code gets no predicates; the
/// caller passes them when it adds such a buff.
pub fn buff_predicates(base: u32) -> BuffPredicates {
    if base == TIMED_LIFE {
        BuffPredicates {
            negative: true,
            timed_life: true,
            ..BuffPredicates::default()
        }
    } else if TIMED_LIFE_SPECIALS.contains(&base) {
        BuffPredicates {
            timed_life: true,
            ..BuffPredicates::default()
        }
    } else {
        BuffPredicates::default()
    }
}

/// The selection of the buff natives: the seven booleans in WC3 order (CONFIRMED mapping, spec section 3.3):
/// `remove_positive`, `remove_negative`, `magic`, `physical`, `timed_life`, `aura`, `dispellable`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuffSelect {
    pub remove_positive: bool,
    pub remove_negative: bool,
    pub magic: bool,
    pub physical: bool,
    pub timed_life: bool,
    pub aura: bool,
    pub dispellable: bool,
}

impl BuffSelect {
    /// Whether the type filters pass for `p`. Magic, physical and dispellable select by type when their flag is set
    /// (the predicate is required). Timed life and aura are excluded when their flag is clear (the predicate must be
    /// absent). This is the reading of the spec's "skipped when" notes (LIKELY).
    fn filters_pass(&self, p: &BuffPredicates) -> bool {
        (!self.magic || p.magic)
            && (!self.physical || p.physical)
            && (self.timed_life || !p.timed_life)
            && (self.aura || !p.aura)
            && (!self.dispellable || p.dispellable)
    }

    /// Whether a buff with predicates `p` counts for `UnitHasBuffsEx` and `UnitCountBuffsEx` (CONFIRMED rules).
    /// A positive buff passes unless `remove_negative` alone is set; a negative buff passes unless `remove_positive`
    /// alone is set. With both flags false every buff counts.
    pub fn counts(&self, p: &BuffPredicates) -> bool {
        let polarity = (!p.positive || self.remove_positive || !self.remove_negative)
            && (!p.negative || self.remove_negative || !self.remove_positive);
        polarity && self.filters_pass(p)
    }

    /// Whether a buff with predicates `p` is removed by `UnitRemoveBuffs(Ex)` (CONFIRMED rules). Only a positive buff
    /// with `remove_positive`, or a negative buff with `remove_negative`, is removed; neutral buffs never are. With
    /// both flags false nothing is removed.
    pub fn removes(&self, p: &BuffPredicates) -> bool {
        ((p.positive && self.remove_positive) || (p.negative && self.remove_negative))
            && self.filters_pass(p)
    }
}

/// Message id of the timed life expiry timer (`0xD01C4`, CONFIRMED: the timer started by slot 211 of the timed life
/// class at `0x6F0D30D0`, and the message on which its handler `0x6F0D3130` removes the buff).
pub const TIMED_LIFE_EXPIRY: u32 = 0xD01C4;

/// Whether a timed life buff with duration `duration` starts its expiry timer (CONFIRMED by Unicorn, see
/// `docs/spec/abilities-2.md` section 2.1): only a duration strictly greater than `0.0` does. Zero, negative zero,
/// negative values and NaN start no timer, so the buff has no expiry until it is removed some other way.
pub fn timed_life_starts_timer(duration: F32) -> bool {
    duration.to_f32() > 0.0
}

/// The state of a timed life buff's duration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimedLifeState {
    /// Running: the expiry is on the timer queue under this id.
    Running(TimerId),
    /// Paused, with the time that was left (softfloat seconds).
    Paused(F32),
    /// No timer was started, because the duration was not greater than zero. The buff has no expiry.
    Unlimited,
}

/// The outcome of [`Game::remove_buffs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemovedBuffs {
    /// The number of buffs removed.
    pub count: usize,
    /// A removed timed life buff had a running duration: the unit's life must be set to zero.
    pub kill: bool,
}

/// A buff on a unit.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Buff {
    /// Identifies the buff on its unit (unique per game). The timer payload refers to it.
    pub serial: u32,
    /// The base code.
    pub base: u32,
    pub predicates: BuffPredicates,
    /// The duration state, for a timed life buff.
    pub timed_life: Option<TimedLifeState>,
    /// What the buff does to its unit (aura, slow, stun), see `crate::auras`.
    pub effect: Option<crate::auras::BuffEffect>,
    /// The expiry timer of a buff with a duration other than timed life (`crate::auras`).
    pub expiry: Option<TimerId>,
}

impl Game {
    /// Whether unit `k` is invulnerable: it has the ability `Avul` (CONFIRMED structure, see the module notes).
    pub fn unit_is_invulnerable(&self, k: ObjKey) -> bool {
        self.unit(k).abilities.contains(INVULNERABLE)
    }

    /// `SetUnitInvulnerable` on unit `k` (the body of the original's unit slot 42, CONFIRMED structure): adds the
    /// ability `Avul` when `on` and it is missing, removes it when `!on`.
    pub fn set_unit_invulnerable(&mut self, k: ObjKey, on: bool) {
        if on {
            if !self.unit(k).abilities.contains(INVULNERABLE) {
                let definition = self.ability_types().get(INVULNERABLE).copied();
                self.unit_mut(k)
                    .abilities
                    .attach(Ability::new(INVULNERABLE, definition));
            }
        } else {
            self.unit_mut(k).abilities.remove(INVULNERABLE);
        }
    }

    /// The buffs of unit `k`, in the order they were added.
    pub fn buffs(&self, k: ObjKey) -> &[Buff] {
        &self.unit(k).buffs
    }

    /// Adds a buff without a duration (the host or a spell adds it; buff creation by spells is not modelled).
    /// Returns its serial.
    pub fn add_buff(&mut self, k: ObjKey, base: u32, predicates: BuffPredicates) -> u32 {
        let serial = self.next_buff_serial();
        self.unit_mut(k).buffs.push(Buff {
            serial,
            base,
            predicates,
            timed_life: None,
            effect: None,
            expiry: None,
        });
        serial
    }

    /// `UnitApplyTimedLife` on unit `k` (the body of the original's `FUN_6F07B580`, CONFIRMED dispatch): adds a timed
    /// life buff of base code `base` whose duration timer runs from now. Each application adds an entry (UNSURE:
    /// stacking). Returns the serial.
    pub(crate) fn apply_timed_life(&mut self, k: ObjKey, base: u32, duration: F32) -> u32 {
        let serial = self.next_buff_serial();
        // The creator allocates a new buff object each time: the default creator (`0x6F07B240`) does not search the
        // unit's list for an existing buff of the same code (CONFIRMED read), so a second application adds an entry.
        let timed_life = if timed_life_starts_timer(duration) {
            // One-shot agent timer with message 0xD01C4 on queue A (CONFIRMED: `0x6F477A80` passes repeat 0 and
            // queue 0 to the timer holder's start slot).
            let timer = self.timers.start(
                duration.to_f32(),
                false,
                Payload::TimedLife { unit: k, serial },
            );
            TimedLifeState::Running(timer)
        } else {
            TimedLifeState::Unlimited
        };
        self.unit_mut(k).buffs.push(Buff {
            serial,
            base,
            predicates: buff_predicates(base),
            timed_life: Some(timed_life),
            effect: None,
            expiry: None,
        });
        serial
    }

    /// `UnitPauseTimedLife` on unit `k`: `pause` cancels the running timers of its timed life buffs and keeps the
    /// time left; `!pause` starts them again with that time, from now. The time left is the soft-float difference of
    /// the expiry and the queue time.
    pub(crate) fn set_timed_life_paused(&mut self, k: ObjKey, pause: bool) {
        let now = self.timers.now();
        let mut buffs = std::mem::take(&mut self.unit_mut(k).buffs);
        for b in buffs.iter_mut().filter(|b| b.predicates.timed_life) {
            match (b.timed_life, pause) {
                (Some(TimedLifeState::Running(id)), true) => {
                    if let Some(expiry) = self.timers.expiry_of(id) {
                        let left = arith::sub(
                            F32::from_bits(expiry.to_bits()),
                            F32::from_bits(now.to_bits()),
                        );
                        self.timers.cancel(id);
                        b.timed_life = Some(TimedLifeState::Paused(left));
                    }
                }
                (Some(TimedLifeState::Paused(left)), false) => {
                    let id = self.timers.start(
                        left.to_f32(),
                        false,
                        Payload::TimedLife {
                            unit: k,
                            serial: b.serial,
                        },
                    );
                    b.timed_life = Some(TimedLifeState::Running(id));
                }
                _ => {}
            }
        }
        self.unit_mut(k).buffs = buffs;
    }

    /// The number of buffs of unit `k` that `select` counts.
    pub fn count_buffs(&self, k: ObjKey, select: BuffSelect) -> usize {
        self.buffs(k)
            .iter()
            .filter(|b| select.counts(&b.predicates))
            .count()
    }

    /// Removes the buffs of unit `k` that `select` removes (the removal routine `0x6F079CC0`): their timers are
    /// cancelled and their effects undone (`CBuff::vf41` -> `vf207`, `crate::auras`). Returns how many were removed and
    /// whether the unit must die: the removal notify of the timed life class (slot 106, `0x6F0D5F10`) sets the owner's
    /// life to zero when the buff had a running duration (flag `0x80`, running or paused), which is the same call as
    /// `KillUnit` (CONFIRMED, `docs/spec/buffs.md` section 5). Dispel Magic killing a summoned unit is this path.
    pub(crate) fn remove_buffs(&mut self, k: ObjKey, select: BuffSelect) -> RemovedBuffs {
        let buffs = std::mem::take(&mut self.unit_mut(k).buffs);
        let (gone, keep): (Vec<Buff>, Vec<Buff>) = buffs
            .into_iter()
            .partition(|b| select.removes(&b.predicates));
        self.unit_mut(k).buffs = keep;
        let mut kill = false;
        let count = gone.len();
        for mut b in gone {
            if matches!(
                b.timed_life,
                Some(TimedLifeState::Running(_)) | Some(TimedLifeState::Paused(_))
            ) {
                kill = true;
            }
            if let Some(TimedLifeState::Running(id)) = b.timed_life {
                self.timers.cancel(id);
            }
            self.finish_buff(k, &mut b);
        }
        RemovedBuffs { count, kill }
    }

    /// The next buff serial.
    pub(crate) fn next_buff_serial(&mut self) -> u32 {
        self.buff_serial = self.buff_serial.wrapping_add(1);
        self.buff_serial
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ab(max: Option<i32>) -> Ability {
        Ability::new(code(b"AHbz"), Some(AbilityType { max_level: max }))
    }

    /// The duration branch of the original's timed life apply (`0x6F0D4CB0`), run in Unicorn
    /// (`tools/emu/abilities2_emu.py`, 25 cases with flag 0 and 1): the expiry timer starts for a duration above
    /// zero only. Each row is (duration f32 bits, starts timer).
    #[test]
    fn timed_life_timer_follows_the_original_duration_branch() {
        let vectors: &[(u32, bool)] = &[
            (0x3F80_0000, true),          // 1.0
            (0x3F00_0000, true),          // 0.5
            (1e-38_f32.to_bits(), true),  // 1e-38 (a denormal in f32)
            (0x0000_0001, true),          // smallest positive denormal
            (0x0000_0000, false),         // 0.0
            (0x8000_0000, false),         // -0.0
            (0xBF80_0000, false),         // -1.0
            (0xFF80_0000, false),         // -inf
            (0x7F80_0000, true),          // +inf
            (0x7FC0_0000, false),         // NaN
            (3.4e38_f32.to_bits(), true), // 3.4e38
        ];
        for &(bits, want) in vectors {
            assert_eq!(
                timed_life_starts_timer(F32::from_bits(bits)),
                want,
                "duration bits {bits:#010x}"
            );
        }
        assert_eq!(TIMED_LIFE_EXPIRY, 0xD01C4);
    }

    #[test]
    fn codes_are_the_four_characters() {
        assert_eq!(MOVE, 0x416D_6F76);
        assert_eq!(ATTACK, 0x4161_746B);
        assert_eq!(INVULNERABLE, 0x4176_756C);
        assert_eq!(LOCUST, 0x416C_6F63);
        assert_eq!(TIMED_LIFE, 0x4254_4C46);
        assert_eq!(TIMED_LIFE_SPECIALS[0], 0x4255_616E);
    }

    #[test]
    fn kinds_follow_the_base_code() {
        assert_eq!(BaseKind::of(MOVE), BaseKind::Move);
        assert_eq!(BaseKind::of(ATTACK), BaseKind::Attack);
        assert_eq!(BaseKind::of(INVULNERABLE), BaseKind::Invulnerable);
        assert_eq!(BaseKind::of(LOCUST), BaseKind::Locust);
        assert_eq!(BaseKind::of(TIMED_LIFE), BaseKind::TimedLife);
        assert_eq!(BaseKind::of(TIMED_LIFE_SPECIALS[3]), BaseKind::Generic);
    }

    #[test]
    fn levels_inc_past_the_max_and_dec_stops_at_zero() {
        let mut a = ab(Some(3));
        assert_eq!(a.level(), 1);
        a.inc_level();
        a.inc_level();
        a.inc_level();
        assert_eq!(
            a.level(),
            4,
            "the guard is index < max, so max + 1 is reached"
        );
        a.inc_level();
        assert_eq!(a.level(), 4, "no further increase from max + 1");
        for _ in 0..10 {
            a.dec_level();
        }
        assert_eq!(a.level(), 1, "dec stops at index 0");
    }

    #[test]
    fn set_level_clamps_to_one_and_the_max() {
        let mut a = ab(Some(3));
        assert_eq!(a.set_level(0), 1);
        assert_eq!(a.set_level(-7), 1);
        assert_eq!(a.set_level(2), 2);
        assert_eq!(a.set_level(9), 3);
        let mut none = ab(None);
        assert_eq!(none.set_level(5), 1, "no max data means max 1");
    }

    #[test]
    fn permanence_and_flags() {
        let mut a = ab(None);
        assert!(a.has_flag(FLAG_UNIT_OWNED));
        assert!(!a.is_removed());
        a.adjust_permanent(true);
        a.adjust_permanent(true);
        a.adjust_permanent(false);
        assert_eq!(a.permanent, 1);
        a.flags |= FLAG_REMOVED;
        assert!(a.is_removed());
    }

    #[test]
    fn fixed_codes_go_to_their_slots() {
        let mut u = UnitAbilities::default();
        u.attach(Ability::new(MOVE, None));
        u.attach(Ability::new(BUILD_CODES[2], None));
        u.attach(Ability::new(code(b"AHgr"), None));
        assert!(u.contains(MOVE));
        assert!(u.contains(BUILD_CODES[2]));
        assert!(!u.contains(BUILD_CODES[0]), "one build slot holds one code");
        assert_eq!(u.iter().count(), 3);
        assert!(u.remove(MOVE).is_some());
        assert!(!u.contains(MOVE));
        assert!(u.remove(MOVE).is_none());
        assert!(
            !is_factory_code(MOVE) && !is_factory_code(ATTACK) && is_factory_code(INVULNERABLE)
        );
    }

    #[test]
    fn selection_counts_and_removes_by_the_confirmed_rules() {
        let pos = BuffPredicates {
            positive: true,
            ..Default::default()
        };
        let neg = BuffPredicates {
            negative: true,
            ..Default::default()
        };
        let neutral = BuffPredicates::default();
        let none = BuffSelect::default();
        assert!(none.counts(&pos) && none.counts(&neg) && none.counts(&neutral));
        assert!(!none.removes(&pos) && !none.removes(&neg) && !none.removes(&neutral));
        let only_pos = BuffSelect {
            remove_positive: true,
            ..Default::default()
        };
        assert!(only_pos.counts(&pos) && !only_pos.counts(&neg) && only_pos.counts(&neutral));
        assert!(only_pos.removes(&pos) && !only_pos.removes(&neg) && !only_pos.removes(&neutral));
        let only_neg = BuffSelect {
            remove_negative: true,
            ..Default::default()
        };
        assert!(!only_neg.counts(&pos) && only_neg.counts(&neg));
        assert!(!only_neg.removes(&pos) && only_neg.removes(&neg));
    }

    #[test]
    fn type_filters_select_only_the_predicate_when_set() {
        let magic = BuffPredicates {
            positive: true,
            magic: true,
            ..Default::default()
        };
        let plain = BuffPredicates {
            positive: true,
            ..Default::default()
        };
        let want_magic = BuffSelect {
            remove_positive: true,
            magic: true,
            timed_life: true,
            aura: true,
            ..Default::default()
        };
        assert!(want_magic.counts(&magic));
        assert!(!want_magic.counts(&plain));
        let timed = BuffPredicates {
            negative: true,
            timed_life: true,
            ..Default::default()
        };
        let no_timed = BuffSelect {
            remove_negative: true,
            ..Default::default()
        };
        assert!(
            !no_timed.counts(&timed),
            "timed life is excluded unless its flag is set"
        );
        let with_timed = BuffSelect {
            timed_life: true,
            ..no_timed
        };
        assert!(with_timed.counts(&timed));
    }
}

// ---- cooldown and mana state (model; the original's storage is not found, see docs/spec/abilities-3.md) ----

/// Result of the cast check of this model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastCheck {
    Ready,
    /// The ability's cooldown has not run out.
    Cooling,
    /// The unit has less mana than the cost.
    NoMana,
}

/// Cooldowns of one unit's abilities. A running cooldown is a one-shot timer on [`crate::timer::TimerQueue`]: its
/// expiry is the time the ability is ready again, and its payload is the ability's base code. The 1.26a binary
/// keeps the level cooldown in the level record (`Cool`, offset `+0x14`, clamped by the builder), but the countdown
/// and the ready check are NOT FOUND, so this is a model (UNSURE).
#[derive(Debug, Clone, Default)]
pub struct Cooldowns {
    queue: crate::timer::TimerQueue<u32>,
    running: std::collections::BTreeMap<u32, crate::timer::TimerId>,
}

impl Cooldowns {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current queue time (the game clock of the cooldown timers).
    pub fn now(&self) -> f32 {
        self.queue.now()
    }

    /// `true` when no cooldown of `base` is running.
    pub fn is_ready(&self, base: u32) -> bool {
        !self.running.contains_key(&base)
    }

    /// Seconds left on the cooldown of `base` (0 when ready).
    pub fn remaining(&self, base: u32) -> f32 {
        self.running
            .get(&base)
            .and_then(|&id| self.queue.expiry_of(id))
            .map_or(0.0, |e| (e - self.queue.now()).max(0.0))
    }

    /// Starts the cooldown of `base` for `seconds` from now, replacing a running one. A value that is not
    /// greater than 0.0 starts nothing (the ability is ready at once).
    pub fn start(&mut self, base: u32, seconds: f32) {
        if let Some(old) = self.running.remove(&base) {
            self.queue.cancel(old);
        }
        if seconds.is_nan() || seconds <= 0.0 {
            return;
        }
        let id = self.queue.start(seconds, false, base);
        self.running.insert(base, id);
    }

    /// Clears the cooldown of `base`. Returns `true` when one was running.
    pub fn reset(&mut self, base: u32) -> bool {
        match self.running.remove(&base) {
            Some(id) => self.queue.cancel(id),
            None => false,
        }
    }

    /// Advances the queue to `target` and returns the abilities that became ready, in expiry order.
    pub fn advance_to(&mut self, target: f32) -> Vec<u32> {
        let mut ready = Vec::new();
        while let Some(fired) = self.queue.pop_due(target) {
            if self.running.get(&fired.payload) == Some(&fired.id) {
                self.running.remove(&fired.payload);
            }
            ready.push(fired.payload);
        }
        ready
    }
}

/// The cost value the cast path hands to the unit-side check (CONFIRMED, `tools/emu/abilities4_emu.py`,
/// docs/spec/abilities-4.md section 3): the level's integer `Cost` through the original I2R (24-bit
/// mantissa truncation), plus the zero term of vtable slot 0x36C, by the soft add. A negative cost is
/// **not** clamped on this path (`-5` gives `-5.0`).
pub fn cast_cost_value(cost: i32) -> F32 {
    arith::add(softfloat::conv::i2r(cost), F32(0))
}

/// The cast check: the cooldown first, then the mana (the order is UNSURE). The mana comparison is still
/// a model (the original's x87 equality in `0x6F283C70` is not decoded); the cost value is confirmed.
pub fn cast_check(cooldowns: &Cooldowns, base: u32, mana: f32, cost: i32) -> CastCheck {
    if !cooldowns.is_ready(base) {
        CastCheck::Cooling
    } else if mana < cast_cost_value(cost).to_f32() {
        CastCheck::NoMana
    } else {
        CastCheck::Ready
    }
}

/// Casts `base`: checks it, pays `cost` and starts the cooldown of `cool` seconds. Returns the mana left.
pub fn cast(
    cooldowns: &mut Cooldowns,
    base: u32,
    mana: f32,
    cost: i32,
    cool: f32,
) -> Result<f32, CastCheck> {
    match cast_check(cooldowns, base, mana, cost) {
        CastCheck::Ready => {
            cooldowns.start(base, cool);
            Ok(mana - cost.max(0) as f32)
        }
        refused => Err(refused),
    }
}

#[cfg(test)]
mod cooldown_tests {
    use super::*;

    const A: u32 = code(b"AHbz");
    const B: u32 = code(b"AHbb");

    #[test]
    fn cooldown_runs_down_and_reports_ready_once() {
        let mut c = Cooldowns::new();
        c.start(A, 5.0);
        assert!(!c.is_ready(A));
        assert_eq!(c.remaining(A), 5.0);
        assert!(c.advance_to(4.0).is_empty());
        assert!(!c.is_ready(A));
        assert!((c.remaining(A) - 1.0).abs() < 1e-5);
        assert_eq!(c.advance_to(5.0), vec![A]);
        assert!(c.is_ready(A));
        assert!(c.advance_to(100.0).is_empty());
    }

    #[test]
    fn zero_or_negative_cooldown_never_blocks() {
        let mut c = Cooldowns::new();
        c.start(A, 0.0);
        c.start(B, -2.0);
        assert!(c.is_ready(A) && c.is_ready(B));
        assert_eq!(cast_check(&c, A, 10.0, 0), CastCheck::Ready);
    }

    #[test]
    fn restarting_replaces_the_old_timer() {
        let mut c = Cooldowns::new();
        c.start(A, 5.0);
        c.start(A, 2.0);
        assert_eq!(c.advance_to(2.0), vec![A]);
        // the 5 s timer was cancelled: nothing fires later
        assert!(c.advance_to(10.0).is_empty());
    }

    #[test]
    fn reset_clears_a_running_cooldown() {
        let mut c = Cooldowns::new();
        c.start(A, 5.0);
        assert!(c.reset(A));
        assert!(c.is_ready(A));
        assert!(!c.reset(A));
        assert!(c.advance_to(10.0).is_empty());
    }

    #[test]
    fn abilities_have_independent_cooldowns() {
        let mut c = Cooldowns::new();
        c.start(A, 3.0);
        c.start(B, 1.0);
        assert_eq!(c.advance_to(1.0), vec![B]);
        assert!(!c.is_ready(A));
        assert_eq!(c.advance_to(3.0), vec![A]);
    }

    #[test]
    fn cast_pays_mana_and_starts_the_cooldown() {
        let mut c = Cooldowns::new();
        let mana = cast(&mut c, A, 100.0, 75, 4.0).expect("ready with enough mana");
        assert_eq!(mana, 25.0);
        assert!(!c.is_ready(A));
        assert_eq!(cast(&mut c, A, 100.0, 75, 4.0), Err(CastCheck::Cooling));
        c.advance_to(4.0);
        assert_eq!(cast(&mut c, A, 20.0, 75, 4.0), Err(CastCheck::NoMana));
        assert!(c.is_ready(A), "a refused cast starts no cooldown");
    }

    #[test]
    fn exact_mana_is_enough() {
        let c = Cooldowns::new();
        assert_eq!(cast_check(&c, A, 75.0, 75), CastCheck::Ready);
        assert_eq!(cast_check(&c, A, 74.9, 75), CastCheck::NoMana);
    }
}

/// Unicorn vectors of the cast cost value (`tools/emu/abilities4_emu.py`, original `Game.dll` 1.26a):
/// (Cost, bits of the float that reaches the unit check at `0x6F283C70`). Each equals the original's I2R.
#[cfg(test)]
mod cast_cost_vectors {
    use super::cast_cost_value;

    const VECTORS: &[(i32, u32)] = &[
        (0, 0x0000_0000),
        (1, 0x3F80_0000),
        (75, 0x4296_0000),
        (1_234_567, 0x4996_B438),
        (-5, 0xC0A0_0000),
        (16_777_219, 0x4B80_0001),
        (2_147_483_647, 0x4EFF_FFFF),
        (-2_147_483_648, 0xCF00_0000),
    ];

    #[test]
    fn cast_cost_value_matches_original_vectors() {
        for &(cost, bits) in VECTORS {
            assert_eq!(cast_cost_value(cost).0, bits, "cost {cost}");
        }
    }

    #[test]
    fn negative_cost_is_not_clamped_on_the_cast_path() {
        assert_eq!(cast_cost_value(-5).to_f32(), -5.0);
    }
}
