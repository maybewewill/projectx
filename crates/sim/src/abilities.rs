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
//! * **Timed life** (`UnitApplyTimedLife`, `UnitPauseTimedLife`): the buff gets a duration timer on the game's timer
//!   queue. Expiry kills the unit through the same path as `KillUnit` (see `natives/ability.rs`). The expiry-to-death
//!   link is UNSURE: the spec only says that a unit death is the expected result.
//!
//! Not modelled: cooldowns, the level records (`+0x4C`, 0x68 bytes), stacking of buffs (UNSURE: the model adds a new
//! entry each time), the supply and food adjustment of timed life, the removal notifications (slot 106), the buff
//! classes beyond the predicates above, and the periodic buff effects.

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
}

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

/// The state of a timed life buff's duration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimedLifeState {
    /// Running: the expiry is on the timer queue under this id.
    Running(TimerId),
    /// Paused, with the time that was left (softfloat seconds).
    Paused(F32),
}

/// A buff on a unit.
#[derive(Debug, Clone, PartialEq)]
pub struct Buff {
    /// Identifies the buff on its unit (unique per game). The timer payload refers to it.
    pub serial: u32,
    /// The base code.
    pub base: u32,
    pub predicates: BuffPredicates,
    /// The duration state, for a timed life buff.
    pub timed_life: Option<TimedLifeState>,
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
        });
        serial
    }

    /// `UnitApplyTimedLife` on unit `k` (the body of the original's `FUN_6F07B580`, CONFIRMED dispatch): adds a timed
    /// life buff of base code `base` whose duration timer runs from now. Each application adds an entry (UNSURE:
    /// stacking). Returns the serial.
    pub(crate) fn apply_timed_life(&mut self, k: ObjKey, base: u32, duration: F32) -> u32 {
        let serial = self.next_buff_serial();
        let timer = self.timers.start(
            duration.to_f32(),
            false,
            Payload::TimedLife { unit: k, serial },
        );
        self.unit_mut(k).buffs.push(Buff {
            serial,
            base,
            predicates: buff_predicates(base),
            timed_life: Some(TimedLifeState::Running(timer)),
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

    /// Removes the buffs of unit `k` that `select` removes, cancelling their duration timers, and returns how many
    /// were removed (the notifications of the original are not modelled).
    pub(crate) fn remove_buffs(&mut self, k: ObjKey, select: BuffSelect) -> usize {
        let buffs = std::mem::take(&mut self.unit_mut(k).buffs);
        let (gone, keep): (Vec<Buff>, Vec<Buff>) = buffs
            .into_iter()
            .partition(|b| select.removes(&b.predicates));
        for b in &gone {
            if let Some(TimedLifeState::Running(id)) = b.timed_life {
                self.timers.cancel(id);
            }
        }
        self.unit_mut(k).buffs = keep;
        gone.len()
    }

    /// The next buff serial.
    fn next_buff_serial(&mut self) -> u32 {
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
