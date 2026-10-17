//! Human hero abilities: Thunder Clap (`AHtc`), Bash (`AHbh`), Avatar (`AHav`), Divine Shield (`AHds`),
//! Resurrection (`AHre`) and Summon Water Elemental (`AHwe`), on the spell cast core of [`crate::cast`].
//! Spec: `docs/spec/abilities-human-heroes.md` (one section per ability with CONFIRMED / LIKELY / UNSURE labels).
//!
//! Classes (RTTI, vtables of the user's `Game.dll`): `CAbilityThunderClap` `0x6F8F67C4`, `CAbilityBash`
//! `0x6F90A414`, `CAbilityAvatar` `0x6F90998C`, `CAbilityDivineShield` `0x6F894AEC`, `CAbilityResurrection`
//! `0x6F91C1CC`, `CAbilityWaterElemental` `0x6F89BF84`; buffs `CBuffThunderClap` `0x6F8E79B4`, `CBuffPause` (Bash's
//! stun) `0x6F8B3BEC`, `CBuffAvatar` `0x6F9039A4`, `CBuffDivineShield` `0x6F894794`, `CBuffWaterElemental`
//! `0x6F8EBDBC` (the timed life buff `BHwe`).
//!
//! Summary of what the model does (details and labels in the spec):
//!
//! * **Thunder Clap**: an immediate cast. Every enemy ground non-structure unit in `Area` around the caster takes
//!   `DataA` damage (damage type 15, class `0x8000`) in cell-walk order, and a survivor gets the `BHtc` slow of
//!   `-DataC` movement and `-DataD` attack speed for `HeroDur` (hero) or `Dur` (CONFIRMED structure: `0x6F1608B0`,
//!   `0x6F1607B0`, `0x6F18A930`).
//! * **Bash**: a passive with a pseudo-random distribution (CONFIRMED: `0x6F1A3470`, table `0x6F1D21F0`, stream 3).
//!   Armed per swing; a damage multiplier `DataB`, the stun `BPSE` (`Dur`/`HeroDur`) and the flat `DataC` damage
//!   (type 17, class `0x20000`) on hit.
//! * **Avatar**: an immediate cast; a `BHav` buff of `Dur` with armor `DataA`, maximum life `DataB` (life sink mode 3),
//!   damage `int(DataC)` and one magic immunity count.
//! * **Divine Shield**: an immediate cast; a `BHds` buff of `Dur`/`HeroDur` (by the caster) with one invulnerability
//!   count and one magic immunity count; `undivineshield` removes it when `DataA` allows.
//! * **Resurrection**: an immediate cast; up to `int(DataA)` allied corpses in `Area` come back with full life.
//! * **Water Elemental**: an immediate cast; `int(DataA)` units of the level's `UnitID` appear with a `BHwe` timed life
//!   of `Dur`.

use std::cell::RefCell;

use softfloat::{arith, conv, Tables, F32};

use crate::abilities::{buff_predicates, code, Buff, BuffPredicates};
use crate::auras::BuffEffect;
use crate::cast::{begin_cast, effect_duration, CastRec, CastTarget, SpellLevel};
use crate::heroes::life_sink;
use crate::natives::game::{Game, ObjKey, Object, ScriptRuntime};
use crate::natives::{circle_candidates, dispatch_unit_death_by};
use crate::rng::{Rng, RngTable};

/// Base codes.
pub const THUNDER_CLAP: u32 = code(b"AHtc");
pub const BASH: u32 = code(b"AHbh");
pub const AVATAR: u32 = code(b"AHav");
pub const DIVINE_SHIELD: u32 = code(b"AHds");
pub const RESURRECTION: u32 = code(b"AHre");
pub const WATER_ELEMENTAL: u32 = code(b"AHwe");
pub const BLIZZARD: u32 = code(b"AHbz");
pub const MASS_TELEPORT: u32 = code(b"AHmt");
pub const BANISH: u32 = code(b"AHbn");
pub const SIPHON_MANA: u32 = code(b"AHdr");
pub const FLAME_STRIKE: u32 = code(b"AHfs");
pub const PHOENIX: u32 = code(b"AHpx");

/// Buff codes (CONFIRMED by the code getters of the buff classes, slot 7).
pub const BUFF_THUNDER_CLAP: u32 = code(b"BHtc");
/// Bash's stun is a `CBuffPause` (`BPSE`, the code `0x6F0D2280` returns), not a `BSTN` stun.
pub const BUFF_PAUSE: u32 = code(b"BPSE");
pub const BUFF_AVATAR: u32 = code(b"BHav");
pub const BUFF_DIVINE_SHIELD: u32 = code(b"BHds");
/// The timed life buff of the water elemental (one of the seven special timed life ids).
pub const BUFF_WATER_ELEMENTAL: u32 = code(b"BHwe");
/// The Banish buff (`CBuffBanish`, `0x6F903D04`: ethereal and slowed).
pub const BUFF_BANISH: u32 = code(b"BHbn");
/// Wind Walk's buff: Bash does not roll while its owner has it (`0x6F15A110` returns `BOwk`).
pub const BUFF_WIND_WALK: u32 = code(b"BOwk");

/// Cast order ids from the original slot 0x348 (Unicorn-checked by tools/analysis/ability_orders.py).
pub const ORDER_THUNDER_CLAP: u32 = 852_096;
pub const ORDER_AVATAR: u32 = 852_086;
pub const ORDER_DIVINE_SHIELD: u32 = 852_090;
/// CONFIRMED (S35): slot 0x318 of `CAbilityDivineShield` (`0x6F0AE2C0`) returns `0xD007B` when `0x6F089A50` allows the
/// cancel, else the cell `0x6F92ED9C`. (852056 is `undefend`.)
pub const ORDER_UNDIVINE_SHIELD: u32 = 852_091;
pub const ORDER_RESURRECTION: u32 = 852_094;
pub const ORDER_WATER_ELEMENTAL: u32 = 852_097;

pub const ORDER_BLIZZARD: u32 = 852_089;
pub const ORDER_MASS_TELEPORT: u32 = 852_093;
/// Cast order ids of the Blood Mage spells (slot `0x308`/`0x348`, `tools/analysis/ability_orders.py`).
pub const ORDER_BANISH: u32 = 852_486;
pub const ORDER_SIPHON_MANA: u32 = 852_487;
pub const ORDER_FLAME_STRIKE: u32 = 852_488;
pub const ORDER_PHOENIX: u32 = 852_489;

/// The `(order id, ability)` pairs the cast core registers.
pub const SPELL_ORDERS: [(u32, u32); 9] = [
    (ORDER_BANISH, BANISH),
    (ORDER_PHOENIX, PHOENIX),
    (ORDER_BLIZZARD, BLIZZARD),
    (ORDER_MASS_TELEPORT, MASS_TELEPORT),
    (ORDER_THUNDER_CLAP, THUNDER_CLAP),
    (ORDER_AVATAR, AVATAR),
    (ORDER_DIVINE_SHIELD, DIVINE_SHIELD),
    (ORDER_RESURRECTION, RESURRECTION),
    (ORDER_WATER_ELEMENTAL, WATER_ELEMENTAL),
];

/// Damage type numbers (the JASS constants): Thunder Clap class `0x8000` is `DAMAGE_TYPE_SONIC` (15), Bash's flat
/// damage class `0x20000` is `DAMAGE_TYPE_FORCE` (17).
const THUNDER_CLAP_DAMAGE_TYPE: u32 = 15;
const BASH_DAMAGE_TYPE: u32 = 17;

/// Combat RNG stream of Bash's roll (CONFIRMED: `0x6F1B2EC0` calls `0x6F2853D0` with EDX = 3).
pub const BASH_STREAM: usize = 3;

/// Whether `base` is cast without a target.
pub fn is_immediate_spell(base: u32) -> bool {
    matches!(
        base,
        THUNDER_CLAP | AVATAR | DIVINE_SHIELD | RESURRECTION | WATER_ELEMENTAL | PHOENIX
    )
}

// ---------------------------------------------------------------------------------------------------------------
// The pseudo-random distribution of Bash (CONFIRMED, Unicorn: `tools/emu/human_heroes_emu.py`).

/// The numerators of the table the start-up initialiser of `0x6F1D21F0` builds (over 10000), one per 5 percent step.
pub const PRD_NUMERATORS: [i32; 21] = [
    0, 38, 147, 320, 547, 819, 1126, 1461, 1815, 2184, 2562, 2953, 3333, 3781, 4203, 4607, 5000, 5799, 6666, 7706,
    10000,
];

/// The table index of a chance in percent: `f2i(chance * 0.2 + 0.5)` clamped to `0..=20` (`0x6F1D21F0`).
pub fn prd_index(chance: F32) -> usize {
    let scaled = arith::mul(chance, F32::from_bits(0x3E4C_CCCD));
    let rounded = arith::add(scaled, F32::from_bits(0x3F00_0000));
    conv::f2i(rounded).clamp(0, 20) as usize
}

/// The distribution constant of a chance: `DIV(I2R(numerator), I2R(10000))` of the indexed entry.
pub fn prd_constant(chance: F32, tables: &Tables) -> F32 {
    let n = PRD_NUMERATORS[prd_index(chance)];
    arith::div(conv::i2r(n), conv::i2r(10000), tables)
}

/// The effective chance of the current swing (`0x6F1A3470`): the constant times the number of swings since the last
/// proc (the counter starts at 1).
pub fn prd_effective_chance(chance: F32, count: i32, tables: &Tables) -> F32 {
    arith::mul(conv::i2r(count), prd_constant(chance, tables))
}

/// The proc test of `0x6F1B2EC0`: the draw `r` (a real in `[0, 1)`) procs when `r <= effective` (`0x6F01FD60`; NaN
/// never procs).
pub fn bash_procs(draw: F32, effective: F32) -> bool {
    arith::le(draw, effective)
}

/// Per-owner Bash state: the armed flag (ability flag `0x200`), the swing counter (`+0x84`, starts at 1) and the
/// stored multiplied damage (`+0x80`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BashState {
    pub armed: bool,
    pub count: i32,
    pub stored: F32,
}

impl Default for BashState {
    fn default() -> Self {
        BashState {
            armed: false,
            count: 1,
            stored: F32::ZERO,
        }
    }
}

fn nonzero(x: F32) -> bool {
    // The original tests `x == 0.0` with an unordered-aware compare: NaN counts as non-zero.
    !arith::eq(x, F32::ZERO) || x.to_f32().is_nan()
}

impl Game {
    /// Sets Misc `MassTeleportCluster` (default off); with it and a level `DataC != 0` Mass Teleport puts every unit
    /// on the destination position.
    pub fn set_mass_teleport_cluster(&mut self, on: bool) {
        self.spells.human.mass_teleport_cluster = on;
    }

    /// The Bash state of `unit` (defaults when it never rolled).
    pub fn bash_state(&self, unit: ObjKey) -> BashState {
        self.spells.bash.get(&unit).copied().unwrap_or_default()
    }

    fn has_ability(&self, unit: ObjKey, base: u32) -> bool {
        self.unit(unit).abilities.get(base).is_some_and(|a| !a.is_removed())
    }

    /// Bash, slot 73 (`0x6F1B2EC0`): the swing starts against `target`. Clears the armed flag, then rolls.
    pub(crate) fn bash_on_attack_start(&mut self, attacker: ObjKey, target: ObjKey) {
        if !self.has_ability(attacker, BASH) {
            return;
        }
        self.spells.bash.entry(attacker).or_default().armed = false;
        if !self.alive(target) {
            return;
        }
        // Wind Walk: the owner's `BOwk` buff stops the roll.
        if self.buff_position(attacker, BUFF_WIND_WALK).is_some() {
            return;
        }
        let (a_owner, t_owner) = (self.unit(attacker).owner, self.unit(target).owner);
        // The target filter of the ability (`0x6F02E000` -> `0x6F26D590`): an enemy (UNSURE: the stock `targs`).
        if !self.is_enemy(a_owner, t_owner) {
            return;
        }
        let Some(level) = self.ability_level_record(attacker, BASH) else {
            return;
        };
        let count = self.spells.bash.entry(attacker).or_default().count;
        let Some(effective) = self
            .order_tables()
            .map(|t| prd_effective_chance(level.data[0], count, t))
        else {
            return;
        };
        let Some((table, streams)) = self.rng_parts() else {
            return;
        };
        let u = streams.stream_mut(BASH_STREAM).random_fraction(table);
        // `0x6F2853D0`: the real is `add(0.0, mul(1.0, U))`, i.e. U itself.
        let draw = arith::add(F32::ZERO, arith::mul(F32::ONE, u));
        let st = self.spells.bash.entry(attacker).or_default();
        if bash_procs(draw, effective) {
            st.armed = true;
            st.count = 1;
        } else {
            st.count += 1;
        }
    }

    /// Bash, slot 77 (`0x6F1A3590`): the damage roll of an armed swing. Returns the damage with the multiplier's
    /// extra added (`DataB`: stored `damage * DataB`, the record gains `stored - damage`).
    pub(crate) fn bash_modify_damage(&mut self, attacker: ObjKey, damage: F32) -> F32 {
        if !self.has_ability(attacker, BASH) {
            return damage;
        }
        let st = self.spells.bash.entry(attacker).or_default();
        st.stored = F32::ZERO;
        if !st.armed {
            return damage;
        }
        let Some(level) = self.ability_level_record(attacker, BASH) else {
            return damage;
        };
        let b = level.data[1];
        if !nonzero(b) {
            return damage;
        }
        let stored = arith::mul(damage, b);
        self.spells.bash.entry(attacker).or_default().stored = stored;
        let extra = arith::sub(stored, damage);
        arith::add(damage, extra)
    }

    /// Bash, slot 85 (`0x6F1CC530`): the hit lands on `target`. Applies the pause (`Dur`/`HeroDur`), then the flat
    /// `DataC` damage unless the owner is an illusion. Returns the unit this killed.
    pub(crate) fn bash_on_hit(&mut self, attacker: ObjKey, target: ObjKey) -> Option<ObjKey> {
        if !self.has_ability(attacker, BASH) {
            return None;
        }
        let st = self.spells.bash.entry(attacker).or_default();
        if !st.armed {
            return None;
        }
        st.stored = F32::ZERO;
        let level = self.ability_level_record(attacker, BASH)?;
        if !self.alive(target) {
            return None;
        }
        let hero = self.target_hero(target);
        let dur = effect_duration(&level, hero, F32::ZERO);
        if nonzero(dur) {
            self.apply_stun_as(BUFF_PAUSE, target, dur);
        }
        let flat = level.data[2];
        if nonzero(flat) && !self.unit(attacker).illusion {
            return self.unit_damage_target(attacker, target, flat, false, false, 0, BASH_DAMAGE_TYPE);
        }
        None
    }

    /// Bash, slot 74 (`0x6F1B3060`): the swing ended without a hit; the armed flag goes.
    pub(crate) fn bash_clear(&mut self, attacker: ObjKey) {
        if let Some(st) = self.spells.bash.get_mut(&attacker) {
            st.armed = false;
        }
    }

    // ---- Avatar -------------------------------------------------------------------------------------------------

    /// `CBuffAvatar::apply` (`0x6F1A03B0`): the three stat sinks and one magic immunity count.
    pub(crate) fn apply_avatar(&mut self, unit: ObjKey, duration: F32, armor: F32, life: F32, damage: i32) -> u32 {
        let serial = self.next_buff_serial();
        self.unit_mut(unit).buffs.push(Buff {
            serial,
            base: BUFF_AVATAR,
            predicates: BuffPredicates {
                positive: true,
                ..buff_predicates(BUFF_AVATAR)
            },
            timed_life: None,
            effect: Some(BuffEffect::Avatar { armor, life, damage }),
            expiry: None,
        });
        self.set_buff_duration(unit, serial, duration);
        self.add_armor(unit, armor);
        self.avatar_life(unit, life);
        self.add_damage(unit, damage);
        // `0x6F07A3A0` with the flag: the first immunity count removes the purge debuff and dispels (section 17).
        self.magic_immunity_add(unit, true);
        serial
    }

    /// The life sink `0x6F2AC450` in mode 3 with the buff's `DataB`.
    fn avatar_life(&mut self, unit: ObjKey, value: F32) {
        let flagged = self.unit(unit).dead;
        let mut cells = self.life_cells(unit);
        life_sink(&mut cells, flagged, value, 3, self.order_tables());
        self.put_life_cells(unit, cells);
    }

    /// `CBuffAvatar::remove` (`0x6F1A0480`): the negated sinks and one magic immunity count off.
    pub(crate) fn undo_avatar(&mut self, unit: ObjKey, armor: F32, life: F32, damage: i32) {
        self.add_armor(unit, arith::neg(armor));
        self.avatar_life(unit, arith::neg(life));
        self.add_damage(unit, damage.saturating_neg());
        self.magic_immunity_remove(unit, true);
    }

    // ---- Divine Shield ------------------------------------------------------------------------------------------

    /// Applies or refreshes the `BHds` buff of `unit` (`0x6F0B64C0`: a buff found by its code gets the set-duration
    /// rule, otherwise a new one is created and applied).
    pub(crate) fn apply_divine_shield(&mut self, unit: ObjKey, duration: F32) -> u32 {
        if let Some(i) = self.buff_position(unit, BUFF_DIVINE_SHIELD) {
            let serial = self.unit(unit).buffs[i].serial;
            self.set_buff_duration(unit, serial, duration);
            return serial;
        }
        let serial = self.next_buff_serial();
        self.unit_mut(unit).buffs.push(Buff {
            serial,
            base: BUFF_DIVINE_SHIELD,
            predicates: BuffPredicates {
                positive: true,
                ..buff_predicates(BUFF_DIVINE_SHIELD)
            },
            timed_life: None,
            effect: Some(BuffEffect::DivineShield),
            expiry: None,
        });
        self.set_buff_duration(unit, serial, duration);
        // `0x6F07D180`: purge removal and dispel first, then the invulnerability and immunity counts.
        self.magic_immunity_dispel(unit);
        self.unit_mut(unit).invuln_count += 1;
        self.magic_immunity_add(unit, true);
        serial
    }

    /// Whether the Divine Shield of `unit` may be cancelled (`0x6F089A50`, CONFIRMED): `DataA` is not zero **and** Misc
    /// `CanDeactivateDivineShield` is non-zero (the reader `0x6F009F90` with the default 0; the key is absent from the
    /// shipped `MiscData.txt`, so in the stock game the shield cannot be cancelled).
    pub fn can_cancel_divine_shield(&self, unit: ObjKey) -> bool {
        self.spells.human.can_deactivate_divine_shield
            && self
                .ability_level_record(unit, DIVINE_SHIELD)
                .is_some_and(|l| nonzero(l.data[0]))
    }

    /// The magic-immunity apply of Divine Shield (`0x6F07D180`) and of Avatar on the first immunity count (`0x6F07A3A0`),
    /// CONFIRMED by reading and by the walker's Unicorn run: the purge debuff `Bprg` (`0x6F0D9B60`) is removed, then the
    /// walker `0x6F079FE0` removes the magical, non-aura, dispellable buffs of **both** polarities
    /// ([`MAGIC_IMMUNITY_DISPEL`]). The sim knows the polarity and magic nature of the spell buffs it creates only by
    /// code ([`spell_buff_dispel_flags`]); buffs of unknown codes are left alone (UNSURE).
    pub(crate) fn magic_immunity_dispel(&mut self, unit: ObjKey) {
        if let Some(i) = self.buff_position(unit, code(b"Bprg")) {
            let mut b = self.unit_mut(unit).buffs.remove(i);
            self.finish_buff(unit, &mut b);
        }
        for (serial, base) in self.spell_buffs_of(unit) {
            if let Some(flags) = spell_buff_dispel_flags(base) {
                if dispel_selects(&MAGIC_IMMUNITY_DISPEL, &flags) {
                    self.drop_spell_buff(serial);
                }
            }
        }
        let select = crate::abilities::BuffSelect {
            remove_positive: true,
            remove_negative: true,
            magic: true,
            physical: false,
            timed_life: true,
            aura: false,
            dispellable: true,
        };
        let _ = self.remove_buffs(unit, select);
    }

    /// Sets Misc `CanDeactivateDivineShield` (default off).
    pub fn set_can_deactivate_divine_shield(&mut self, on: bool) {
        self.spells.human.can_deactivate_divine_shield = on;
    }

    /// `undivineshield`: removes the `BHds` buff when the ability allows it. Returns whether a buff went.
    pub fn cancel_divine_shield(&mut self, unit: ObjKey) -> bool {
        if !self.has_ability(unit, DIVINE_SHIELD) || !self.can_cancel_divine_shield(unit) {
            return false;
        }
        let Some(i) = self.buff_position(unit, BUFF_DIVINE_SHIELD) else {
            return false;
        };
        let mut b = self.unit_mut(unit).buffs.remove(i);
        self.finish_buff(unit, &mut b);
        true
    }

    // ---- Resurrection -------------------------------------------------------------------------------------------

    /// Brings a corpse back: not dead, full life (the revive passes the fraction 1.0, `0x6F1B5A60`).
    pub(crate) fn resurrect_unit(&mut self, k: ObjKey) {
        let max = self.unit(k).max_life;
        self.unit_mut(k).dead = false;
        self.write_life(k, max);
        self.stats_set_dead(k, false);
    }
}

/// The slow Thunder Clap applies (`0x6F18A930`, CONFIRMED, **Unicorn 400/400**): the duration is `HeroDur` for a hero and
/// `add(Dur, extra)` otherwise (`extra` = ability `+0x9C`); a duration that is exactly zero (ordered compare, NaN counts
/// as non-zero) applies nothing; the buff gets `-DataC` as movement factor and `-DataD` as attack-speed factor.
pub fn thunder_clap_slow(l: &SpellLevel, hero: bool, extra: F32) -> Option<(F32, F32, F32)> {
    let dur = effect_duration(l, hero, extra);
    nonzero(dur).then(|| {
        (
            dur,
            F32::from_bits(l.data[2].to_bits() ^ 0x8000_0000),
            F32::from_bits(l.data[3].to_bits() ^ 0x8000_0000),
        )
    })
}

/// Thunder Clap (`0x6F1608B0` instant slot, per-unit body `0x6F1607B0`): **the same class slots as War Stomp** except the
/// slot `0x430` (here the slow `0x6F18A930`, Stomp: the stun `0x6F18ABF0`) and the order id (`tools/analysis`/V5: the two
/// vtables were compared slot by slot, `0x428`, `0x42C`, `0x434` are equal). So the area filter is War Stomp's
/// (`orc::stomp_word`, `orc::area_predicate`: enemy and neutral relations, `targs` kind bits), the damage record is
/// `DataA`, class `0x8000`, flags `0x40` (`0x50` and amount 0.0 for an invulnerable or spell-immune unit) and the slow
/// runs only for a resolved, non-structure unit on the normal path. CONFIRMED: `tools/emu/stomp_emu.py` with the Thunder
/// Clap vtable (`THUNDER_VT=1`, V5, 400 cases, 0 mismatches, `tests/data/human_heroes/thunder_unit.txt`) plus the slow's own
/// run (`thunder_slow.txt`). The earlier model filtered "enemy, no building, no flyer" by hand.
fn thunder_clap(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    use crate::orc::{area_predicate, stomp_unit, stomp_word};
    let level = rec.level;
    let mut killed = Vec::new();
    {
        let mut gm = game.borrow_mut();
        let (cx, cy) = {
            let u = gm.unit(rec.caster);
            (u.x, u.y)
        };
        let level_index =
            usize::try_from(gm.unit(rec.caster).abilities.get(THUNDER_CLAP).map_or(0, |a| a.level_index)).unwrap_or(0);
        let word = stomp_word(gm.orc_targs(THUNDER_CLAP, level_index));
        for k in circle_candidates(&gm, cx, cy, level.area) {
            {
                let u = gm.unit(k);
                if u.removed || u.hidden {
                    continue;
                }
            }
            if !area_predicate(&gm, rec.caster, k, word) {
                continue;
            }
            let zero_record = gm.unit_is_invulnerable(k)
                || crate::natives::unit_attr::is_unit_type(&gm, gm.unit(k), crate::unitclass::ty::MAGIC_IMMUNE);
            let amount = if zero_record { F32::ZERO } else { level.data[0] };
            if let Some(t) = gm.unit_damage_target(rec.caster, k, amount, false, false, 0, THUNDER_CLAP_DAMAGE_TYPE) {
                killed.push(t);
            }
            let target_type = gm.target_flag_word(k) & 0x1E;
            let resolved = gm.alive(k);
            if stomp_unit(zero_record, resolved, target_type, gm.unit(k).owner).stun {
                let hero = gm.target_hero(k);
                if let Some((dur, move_factor, attack_factor)) = thunder_clap_slow(&level, hero, F32::ZERO) {
                    gm.apply_slow_as(BUFF_THUNDER_CLAP, k, dur, move_factor, attack_factor);
                }
            }
        }
    }
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(rec.caster));
    }
}

/// What Avatar's cast slot `0x6F1CA180` hands to its buff apply `0x6F1A03B0` and what the apply passes on (CONFIRMED,
/// **Unicorn 400/400**, `tools/emu/spell_buff_emu.py` check `avatar`: the real cast slot, the real `CBuffAvatar` apply slot
/// 214, the real stat helpers `0x6F196A40/60/80` and the real damage counter `0x6F0C6C10`; stubs: the timer start, the buff
/// allocator, the holders, the armor and life sinks and the immunity counter, which record their arguments): the duration is
/// `Dur` plus the ability's extra term (`+0x9C`, soft add) with NO hero test; the armor sink gets `DataA`, the life sink
/// `0x6F2AC450` gets `DataB` with mode 3, both weapon slots' damage counters (mask -1) grow by `F2I(DataC)`, `DataD` is
/// only stored in a holder (`+0xE8`, no sink reads it), and the magic immunity routine `0x6F07A3A0` is called once with 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AvatarValues {
    pub dur: F32,
    pub armor: F32,
    pub life: F32,
    pub damage: i32,
}

pub fn avatar_values(l: &SpellLevel, extra: F32) -> AvatarValues {
    AvatarValues {
        dur: arith::add(l.dur, extra),
        armor: l.data[0],
        life: l.data[1],
        damage: conv::f2i(l.data[2]),
    }
}

fn avatar(game: &RefCell<Game>, rec: &CastRec) {
    let v = avatar_values(&rec.level, F32::ZERO);
    game.borrow_mut().apply_avatar(rec.caster, v.dur, v.armor, v.life, v.damage);
}

/// The counters one application of the Divine Shield buff changes on the unit (`CBuffDivineShield` slot `0x31C` =
/// `0x6F07D180`, CONFIRMED, **Unicorn 400/400**, `tools/emu/spell_buff_emu.py` check `divine_shield`: the real cast slot
/// `0x6F0B64C0`, the real apply, the real counters `0x6F0796F0` and `0x6F07A3A0` on a fake caster; stubbed: the buff
/// lookup / allocator, the base apply, the purge removal, the dispel walker and the cancel test, which record their
/// arguments). Order: the set-duration rule, the purge-debuff lookup and removal, the dispel walker with
/// `(0,1,0,0,0,1,1,0,1)`, the invulnerability count (`+0xE8` plus one; the unit flag `0x8` is set when it becomes 1), then the
/// magic immunity counters (`+0x15C` plus one and, for the flag argument 1, `+0x160` plus one); when either of those becomes
/// exactly 1 the purge removal and the same dispel walker run once more. Returns the counters after and the number of dispel
/// walker calls (each one preceded by a purge lookup).
pub fn divine_shield_apply_counters(inv: i32, flags: u32, imm: i32, imm_flagged: i32) -> (i32, u32, i32, i32, u32) {
    let mut dispels = 1;
    let inv = inv.wrapping_add(1);
    let flags = if inv == 1 { flags | 8 } else { flags };
    let imm = imm.wrapping_add(1);
    let imm_flagged = imm_flagged.wrapping_add(1);
    if imm == 1 || imm_flagged == 1 {
        dispels += 1;
    }
    (inv, flags, imm, imm_flagged, dispels)
}

fn divine_shield(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    // The hero test is on the caster (`0x6F0B64C0`: `0x6F1D2660(owner)`).
    let hero = gm.target_hero(rec.caster);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    gm.apply_divine_shield(rec.caster, dur);
}

/// A candidate record of the Resurrection sort (`0x6F1D1720` fills 20 bytes): the corpse, an integer key (`+4`, meaning
/// NOT FOUND: the filler `0x6F1CFF70` was not located), a real (`+8`), a flag (`+0xC`) and the caster's player
/// (`+0x10`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResurrectionRec {
    pub owner: i32,
    pub key: i32,
    pub real: F32,
    pub flag: i32,
    pub player: i32,
    /// The corpse's agent id (`unit + 0xC`), the final tie break.
    pub id: i32,
}

/// The sort comparator `0x6F19EA20` (CONFIRMED, Unicorn: 400 cases): negative when `a` goes first. The corpse of the
/// caster's own player first; then the larger key first (`0x6F19E970`, with the wrapped difference test of the original),
/// then a zero flag first, then the smaller real first (a NaN difference puts `a` last), then the larger agent id first.
pub fn resurrection_compare(a: &ResurrectionRec, b: &ResurrectionRec) -> i32 {
    if a.owner != b.owner {
        if a.owner == a.player {
            return -1;
        }
        if b.owner == b.player {
            return 1;
        }
    }
    let d = a.key.wrapping_sub(b.key);
    if d != 0 {
        return if d <= 0 { 1 } else { -1 };
    }
    if a.flag != b.flag {
        return if b.flag == 0 { 1 } else { -1 };
    }
    let v = arith::sub(a.real, b.real).to_f32();
    if v.is_nan() {
        1
    } else if v == 0.0 {
        b.id.wrapping_sub(a.id)
    } else if v > 0.0 {
        1
    } else {
        -1
    }
}

fn resurrection(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let max = conv::f2i(rec.level.data[0]);
    let (cx, cy, owner) = {
        let c = gm.unit(rec.caster);
        (c.x, c.y, c.owner)
    };
    let area2 = arith::mul(rec.level.area, rec.level.area);
    let mut recs = Vec::new();
    for i in 0..gm.object_count() {
        let k = ObjKey(i as u32);
        if !matches!(gm.object(k), Object::Unit(_)) || k == rec.caster {
            continue;
        }
        let u = gm.unit(k);
        if !u.dead || u.removed {
            continue;
        }
        let (dx, dy) = (arith::sub(u.x, cx), arith::sub(u.y, cy));
        let d2 = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
        if !arith::le(d2, area2) || gm.is_enemy(owner, u.owner) {
            continue;
        }
        // The record fields (`0x6F1C1B90`, CONFIRMED by reading, distance Unicorn 400/400): `key` (`+4`) = the unit
        // level of `0x6F29DCA0` (the type's level, 0 when not positive, at most Misc `MaxUnitLevel`), `real` (`+8`) =
        // the squared 2D distance to the caster, `flag` (`+0xC`) = 0 for a plain corpse. Heroes (uppercase type id) take
        // another path through the `Acar` corpse buff and are not modelled.
        if (u.type_id >> 24) as u8 >= b'A' && (u.type_id >> 24) as u8 <= b'Z' {
            continue;
        }
        let level = gm.unit_types().get(u.type_id).map_or(0, |t| t.level);
        let cap = gm.kill_xp_config().max_unit_level;
        let key = if level <= 0 { 0 } else { level.min(cap) };
        recs.push((
            k,
            ResurrectionRec {
                owner: u.owner as i32,
                key,
                real: d2,
                flag: 0,
                player: owner as i32,
                id: k.0 as i32,
            },
        ));
    }
    recs.sort_by(|a, b| resurrection_compare(&a.1, &b.1).cmp(&0));
    let count = usize::try_from(max).unwrap_or(0);
    for (k, _) in recs.into_iter().take(count) {
        gm.resurrect_unit(k);
    }
}

fn water_elemental(game: &RefCell<Game>, rec: &CastRec) {
    summon(game, rec, BUFF_WATER_ELEMENTAL);
}

/// Summon Phoenix (`CAbilitySummonPhoenix` is a `CAbilityWaterElemental`: the effect `0x6F11BA10` registers a callback and
/// jumps into the same summon loop `0x6F163390`): `f2i(DataA)` units of `UnitID` at the caster. The post-create hook
/// `0x6F128BE0` (slot `0x3E4`) does not start a timed life; it only stores the summoner in the unit's `0x6F1132A0` buff.
/// The stock Phoenix has `Dur` 0, so the summon is permanent (`BHwe` with a zero duration starts no timer).
fn phoenix(game: &RefCell<Game>, rec: &CastRec) {
    summon_with(game, rec, None);
}

/// The summon loop of the Water Elemental family (`0x6F163390`, vtable slot `0x3C0` of `CAbilityWaterElemental`,
/// `CAbilityScout`, `CAbilitySpiritOfVengeance`; the Phoenix effect `0x6F11BA10` enters the same loop). CONFIRMED, V5:
/// the real loop on a fake ability, 450 cases over the three vtables, 0 mismatches (`tools/emu/v5_summon_emu.py`,
/// `tests/data/human_heroes/summon_loop.txt`): `n = F2I(DataA)` units of the level's `UnitID` for the caster's owner, the
/// creator flag word `0x502` when `Dur + ability+0x9C` is exactly zero else `2`, every unit is requested at the point
/// `caster + M * (Area, 0, 0)` (the creator wrapper `0x6F1D27A0` gets the `Area` pointer and the constant 1, i.e. the
/// caster's facing; `orc::feral_place`), then the link `0x6F052EC0`, the class slot `0x3E4` with a pointer to the
/// duration (the timed-life buff) and the release. The count and duration arithmetic is Feral Spirit's
/// (`orc::feral_plan`). The earlier model put the units on the caster and passed radians as degrees.
pub(crate) fn summon(game: &RefCell<Game>, rec: &CastRec, timed_life: u32) {
    summon_with(game, rec, Some(timed_life));
}

/// The summon loop with an optional timed-life buff: Summon Phoenix passes `None` (its class slot `0x3E4`, `0x6F128BE0`, only stores the
/// summoner, V7: it never starts a timed life).
pub(crate) fn summon_with(game: &RefCell<Game>, rec: &CastRec, timed_life: Option<u32>) {
    let mut gm = game.borrow_mut();
    let plan = crate::orc::feral_plan(rec.level.dur, F32::ZERO, rec.level.data[0]);
    let (x, y, owner, facing) = {
        let c = gm.unit(rec.caster);
        (c.x, c.y, c.owner, c.facing)
    };
    if rec.level.unit_id == 0 {
        return;
    }
    let (px, py) = match gm.softfloat_tables().cloned() {
        Some(t) => crate::orc::feral_place(x, y, facing, rec.level.area, &t),
        None => (x, y),
    };
    for _ in 0..plan.count.max(0) {
        // UNSURE: the unit creator `0x6F29F990` spreads units requested at one point by its own placement search.
        if let Some((k, _)) =
            crate::natives::unit::build(&mut gm, owner, rec.level.unit_id, px, py, crate::orc::rad_to_deg(facing), true)
        {
            gm.unit_mut(k).summoned = true;
            if let Some(buff) = timed_life {
                gm.apply_timed_life(k, buff, plan.duration);
            }
        }
    }
}


// ---------------------------------------------------------------------------------------------------------------
// Blizzard (`AHbz`): the effect object `XHbz` (`CEffectBlizzard`, `0x6F8FB97C`). Spec section 11.

/// Level fields as `0x6F1B9E90` hands them to the effect setup `0x6F1B9BB0` (CONFIRMED, Unicorn: 400 level records).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlizzardParams {
    /// Number of waves: `f2i(DataA)` (stored at `+0x78`/`+0x84`; the setup clamps it to at least 1).
    pub waves: i32,
    /// Wave interval: the level's `Cast` field (`+0x80`).
    pub interval: F32,
    /// Damage per wave and unit: `DataB` (`+0x8C`).
    pub damage: F32,
    /// Cap on the total damage of one wave: `DataF` (`+0x94`), 0 = none.
    pub cap: F32,
    /// Shards per wave: `f2i(DataC)` (the setup clamps it to `1..=32`, `+0x98`).
    pub shards: i32,
    /// Building factor: `DataD` (`+0xAC`).
    pub building: F32,
    /// `DataE` (`+0x354` property), `Dur + 0` and `HeroDur` (`+0x35C`/`+0x364`): the optional debuff duration.
    pub data_e: F32,
    pub dur: F32,
    pub hero_dur: F32,
    /// Damage radius: `Area` (`+0x74`).
    pub area: F32,
}

/// The mapping of a level record to the effect parameters.
pub fn blizzard_params(l: &SpellLevel) -> BlizzardParams {
    BlizzardParams {
        waves: conv::f2i(l.data[0]).max(1),
        interval: l.cast,
        damage: l.data[1],
        cap: l.data[5],
        shards: conv::f2i(l.data[2]).clamp(1, 32),
        building: l.data[3],
        data_e: l.data[4],
        dur: arith::add(l.dur, F32::ZERO),
        hero_dur: l.hero_dur,
        area: l.area,
    }
}

/// The channel time Blizzard stores in the property at ability `+0x90` at the end of `0x6F1B9E90` (CONFIRMED,
/// Unicorn 400 cases): `mul(I2R(f2i(DataA)), Cast)`, i.e. waves times the wave interval. The cast core arms the
/// `0xD01BD` timer with it (`docs/spec/abilities-human-heroes.md` section 14).
pub fn blizzard_channel_time(l: &SpellLevel) -> F32 {
    arith::mul(conv::i2r(conv::f2i(l.data[0])), l.cast)
}

/// The arguments of the dispel walker `0x6F079FE0` in stack order (`ret 0x24`): CONFIRMED by reading the loop and by the
/// 400 Unicorn runs of `tools/emu/human_heroes3_emu.py` (random argument sets, random buff predicate answers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispelArgs {
    /// arg0: only buffs answering "physical" (vtable slot `0x1DC`).
    pub physical: bool,
    /// arg1: only buffs answering "magic" (slot `0x1D8`).
    pub magic: bool,
    /// arg2: also buffs for which slot `0x32C` says "cannot be dispelled".
    pub undispellable_ok: bool,
    /// arg3: also aura buffs (slot `0x1E8`).
    pub aura_ok: bool,
    /// arg4: only buffs for which slot `0x330(arg7)` is true.
    pub typed: bool,
    /// arg5 / arg6: positive buffs (slot `0x1E4`) / negative buffs (slot `0x1E0`) are candidates.
    pub positive: bool,
    pub negative: bool,
    /// arg7: the argument of slot `0x330`.
    pub type_arg: bool,
    /// arg8: also buffs with bit `0x20000000` in their flag word (the timed-life buffs).
    pub timed_ok: bool,
}

/// The call Divine Shield (`0x6F07D180`) and Avatar (`0x6F07A3A0` on the first immunity count) make: magical buffs of
/// both polarities, no auras, nothing marked undispellable, timed life included.
pub const MAGIC_IMMUNITY_DISPEL: DispelArgs = DispelArgs {
    physical: false,
    magic: true,
    undispellable_ok: false,
    aura_ok: false,
    typed: false,
    positive: true,
    negative: true,
    type_arg: false,
    timed_ok: true,
};

/// The dispel answers of the spell buffs the sim creates (UNSURE: the classes' predicate slots were not read; these are
/// the usual game behaviour): the stun, the slow and the Thunder Clap slow are negative and magical.
pub fn spell_buff_dispel_flags(base: u32) -> Option<DispelBuff> {
    [code(b"BSTN"), code(b"Bslo"), BUFF_THUNDER_CLAP].contains(&base).then_some(DispelBuff {
        magic: true,
        negative: true,
        ..DispelBuff::default()
    })
}

/// What a buff answers to the dispel walker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DispelBuff {
    pub magic: bool,
    pub physical: bool,
    pub negative: bool,
    pub positive: bool,
    pub aura: bool,
    pub undispellable: bool,
    pub typed: bool,
    pub timed: bool,
}

/// Whether the walker `0x6F079FE0` removes a buff (CONFIRMED, Unicorn 400 cases): the buff must be positive or negative;
/// one of the allowed polarities must match; auras, undispellable and timed-life buffs only when the call allows them;
/// physical / magic / typed only when the call requires them.
pub fn dispel_selects(a: &DispelArgs, b: &DispelBuff) -> bool {
    if !(b.negative || b.positive) {
        return false;
    }
    if !((a.positive && b.positive) || (a.negative && b.negative)) {
        return false;
    }
    !((!a.aura_ok && b.aura)
        || (!a.undispellable_ok && b.undispellable)
        || (a.physical && !b.physical)
        || (a.magic && !b.magic)
        || (a.typed && !b.typed)
        || (!a.timed_ok && b.timed))
}

/// The default-filling normaliser of a target flag word (`0x6F2552F0`, CONFIRMED, **Unicorn 400 cases**): every group the
/// word leaves empty gets its default. Groups (mask -> default): relations `0x1F000` -> all; ancient `0xC0000000` ->
/// both; mechanical/organic `0xC000000` -> both; sapper `0x30000000` -> both; unit classes `0x7FE` -> `0xE` (ground, air,
/// structure); alive/dead `0x3000000` -> alive; hero `0xC00000` -> both; vulnerable/invulnerable `0x300000` ->
/// vulnerable. The result also keeps the word's own bits.
pub fn targs_normalise(w: u32) -> u32 {
    const GROUPS: [(u32, u32); 8] = [
        (0x1F000, 0x1F000),
        (0xC000_0000, 0xC000_0000),
        (0xC00_0000, 0xC00_0000),
        (0x3000_0000, 0x3000_0000),
        (0x7FE, 0xE),
        (0x300_0000, 0x100_0000),
        (0xC0_0000, 0xC0_0000),
        (0x30_0000, 0x10_0000),
    ];
    GROUPS.iter().fold(w, |acc, &(mask, default)| if w & mask == 0 { acc | default } else { acc })
}

/// The unclamped `f2i(DataA)` and `f2i(DataC)` the ability method passes (the vector check compares these).
pub fn blizzard_raw_counts(l: &SpellLevel) -> (i32, i32) {
    (conv::f2i(l.data[0]), conv::f2i(l.data[2]))
}

const TWO_PI: F32 = F32::from_bits(0x40C9_0FDB);
const POINT_FOUR: F32 = F32::from_bits(0x3ECC_CCCD);
const HALF: F32 = F32::from_bits(0x3F00_0000);
/// The delay of the first damage wave: the constant `0x6FAAE4F0` (0.9), passed as the start delay of the `0xD01B1`
/// timer by the setup `0x6F1B9BB0` (CONFIRMED by the argument capture; the value is the start-up value of that cell).
const FIRST_DAMAGE_DELAY: f32 = 0.9;

/// The result of one wave's shard placement (`0x6F1AF6E0`).
#[derive(Debug, Clone, PartialEq)]
pub struct ShardWave {
    /// The shuffled launch order (`+0xCC`): the first shard launched is `perm[count - 1]`.
    pub perm: Vec<u32>,
    /// The offset of every shard from the wave centre (properties `+0x150 + 8i` and `+0x254 + 8i`).
    pub offsets: Vec<(F32, F32)>,
}

fn stream_real(rng: &mut Rng, table: &RngTable) -> F32 {
    // `0x6F2853D0`: `add(0.0, mul(1.0, U))` (Unicorn-checked for stream 3 in `human_heroes_vectors.rs`).
    arith::add(F32::ZERO, arith::mul(F32::ONE, rng.random_fraction(table)))
}

/// `0x6F1AF6E0` (CONFIRMED, Unicorn: 400 cases). `n` shards (1..=32) in a circle of radius `area / 2` with a random
/// start angle and a jitter of `area * 0.4 * (U - 0.5)` per axis. With more than five shards the last one is not on
/// the circle but at a random point of the jitter square. Draws: the global generator once per `i = 1..n` for the
/// shuffle, stream 4 for everything else (extra shard x, y; start angle; x, y jitter per circle shard).
pub fn blizzard_shards(
    n: usize,
    area: F32,
    global: &mut Rng,
    stream4: &mut Rng,
    table: &RngTable,
    tables: &Tables,
) -> ShardWave {
    let mut perm: Vec<u32> = (0..n as u32).collect();
    for i in 1..n {
        let r = global.next_u32(table);
        let j = ((u64::from(r) * (i as u64 + 1)) >> 32) as usize;
        perm.swap(i, j);
    }
    let half = arith::mul(area, HALF);
    let jitter = arith::mul(area, POINT_FOUR);
    let mut offsets = vec![(F32::ZERO, F32::ZERO); n];
    let mut circle = n;
    if n > 5 {
        circle = n - 1;
        let jx = arith::mul(jitter, arith::sub(stream_real(stream4, table), HALF));
        let jy = arith::mul(jitter, arith::sub(stream_real(stream4, table), HALF));
        offsets[circle] = (jx, jy);
    }
    let step = arith::div(TWO_PI, conv::i2r(circle as i32), tables);
    let base = arith::mul(TWO_PI, stream_real(stream4, table));
    for (i, slot) in offsets.iter_mut().enumerate().take(circle) {
        let angle = arith::add(base, arith::mul(step, conv::i2r(i as i32)));
        let c = softfloat::math::cos(angle, tables);
        let s = softfloat::math::sin(angle, tables);
        let rx = arith::mul(half, c);
        let ry = arith::mul(half, s);
        let jx = arith::mul(jitter, arith::sub(stream_real(stream4, table), HALF));
        let jy = arith::mul(jitter, arith::sub(stream_real(stream4, table), HALF));
        *slot = (arith::add(rx, jx), arith::add(ry, jy));
    }
    ShardWave { perm, offsets }
}

/// The damage value one unit gets in one wave (`0x6F1AF950`, CONFIRMED, **Unicorn 400 cases**,
/// `tools/emu/human_heroes3_emu.py`): with a non-zero cap `DataF` and `cap < mul(I2R(n), d)` (ordered) the value is
/// `div(cap, I2R(n))` (`n` = the count of units of the same relation kind the area helper counted); the sign is flipped
/// when the effect's slot `0x8C` says "heal" (always 0 for Blizzard); a structure (`unit+0x5C` bit 16) or a unit with
/// `unit+0x20` bit `0x200000` (`flagged`) takes `mul(d, building)`.
pub fn blizzard_unit_damage(
    p: &BlizzardParams,
    count: i32,
    structure_like: bool,
    negate: bool,
    tables: Option<&Tables>,
) -> F32 {
    let mut d = p.damage;
    if nonzero(p.cap) {
        let total = arith::mul(conv::i2r(count), d);
        if arith::lt(p.cap, total) {
            // The division needs the soft-float tables; without them the cap cannot be applied.
            if let Some(t) = tables {
                d = arith::div(p.cap, conv::i2r(count), t);
            }
        }
    }
    if negate {
        d = F32::from_bits(d.to_bits() ^ 0x8000_0000);
    }
    if structure_like {
        d = arith::mul(p.building, d);
    }
    d
}

/// "The strongest wins": an existing `BHbz` buff keeps its value unless it is smaller than the new one (ordered
/// compare `existing < new`, CONFIRMED in `0x6F1AF950`, Unicorn 400 cases); the update also refreshes the duration.
pub fn blizzard_buff_replaces(existing: F32, new: F32) -> bool {
    arith::lt(existing, new)
}

#[derive(Debug, Clone)]
struct BlizzardFx {
    /// The cast that created the effect (the channel end destroys it).
    cast: u32,
    caster: ObjKey,
    x: F32,
    y: F32,
    p: BlizzardParams,
    visual_left: i32,
    damage_left: i32,
}

/// Mass Teleport channel in progress.
#[derive(Debug, Clone, Copy)]
struct TeleportFx {
    caster: ObjKey,
    dest: ObjKey,
    level: SpellLevel,
}

/// State of the effects this module creates (owned by the spell system).
#[derive(Debug, Clone, Default)]
pub struct State {
    next: u32,
    blizzards: std::collections::BTreeMap<u32, BlizzardFx>,
    teleports: std::collections::BTreeMap<u32, TeleportFx>,
    /// Misc `MassTeleportCluster` (a map's `war3mapMisc.txt` may set it; the shipped MiscData.txt has no such key, so the
    /// default is 0).
    pub mass_teleport_cluster: bool,
    /// Misc `CanDeactivateDivineShield` (default 0; absent from the shipped MiscData.txt).
    pub can_deactivate_divine_shield: bool,
    /// The `BHbz` marker of each unit: `(spell buff serial, stored wave value)`.
    markers: std::collections::BTreeMap<ObjKey, (u32, F32)>,
    /// Every shard wave placed, `(effect id, wave)`, for tests and the renderer.
    pub log: Vec<(u32, ShardWave)>,
}

/// Timer kinds of [`crate::cast::SpellMsg::Human`].
pub const TICK_BLIZZARD_WAVE: u8 = 1;
pub const TICK_BLIZZARD_DAMAGE: u8 = 2;

fn human_timer(gm: &mut Game, seconds: f32, kind: u8, id: u32) {
    gm.timers.start(
        seconds,
        false,
        crate::natives::game::Payload::Spell(crate::cast::SpellMsg::Human { kind, id }),
    );
}

fn blizzard_start(game: &RefCell<Game>, rec: &CastRec) {
    let CastTarget::Point(x, y) = rec.target else {
        return;
    };
    let p = blizzard_params(&rec.level);
    let id = {
        let mut gm = game.borrow_mut();
        gm.spells.human.next += 1;
        let id = gm.spells.human.next;
        gm.spells.human.blizzards.insert(
            id,
            BlizzardFx {
                cast: rec.id,
                caster: rec.caster,
                x,
                y,
                p,
                visual_left: p.waves,
                damage_left: p.waves,
            },
        );
        id
    };
    // `0x6F1B9D2D`: the first visual wave starts at once, the damage timer after 0.9 s.
    blizzard_wave(game, id);
    human_timer(&mut game.borrow_mut(), FIRST_DAMAGE_DELAY, TICK_BLIZZARD_DAMAGE, id);
}

/// Message `0xD01B0` (`0x6F1B6C00`): a visual wave: one placement, the counter, the next wave.
fn blizzard_wave(game: &RefCell<Game>, id: u32) {
    let mut gm = game.borrow_mut();
    let Some(mut fx) = gm.spells.human.blizzards.get(&id).cloned() else {
        return;
    };
    let tables = gm.order_tables().cloned();
    let mut placed = None;
    if let Some(tables) = tables {
        if let Some((table, streams)) = gm.rng_parts() {
            let table = table.clone();
            let mut global = streams.global;
            let mut s4 = streams.stream(4);
            let wave = blizzard_shards(fx.p.shards as usize, fx.p.area, &mut global, &mut s4, &table, &tables);
            streams.global = global;
            *streams.stream_mut(4) = s4;
            placed = Some(wave);
        }
    }
    if let Some(w) = placed {
        gm.spells.human.log.push((id, w));
    }
    fx.visual_left -= 1;
    let again = fx.visual_left > 0;
    let interval = fx.p.interval.to_f32();
    gm.spells.human.blizzards.insert(id, fx);
    if again {
        human_timer(&mut gm, interval, TICK_BLIZZARD_WAVE, id);
    }
}

/// Message `0xD01B1` (`0x6F1B9DB0`): the damage wave over the area.
fn blizzard_damage(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let Some(fx) = game.borrow().spells.human.blizzards.get(&id).cloned() else {
        return;
    };
    // UNSURE: the effect ends with its caster (the channel end `0x6F1AFBE0` destroys it).
    if !game.borrow().alive(fx.caster) {
        game.borrow_mut().spells.human.blizzards.remove(&id);
        return;
    }
    let (targets, tables) = {
        let gm = game.borrow();
        let owner = gm.unit(fx.caster).owner;
        // The enumeration filter is the ability's flag word (`'_'` = `0xFDD1F00E`, section 15): alive, vulnerable
        // (the `invu` bit is clear), ground/air/structure, all relations. UNSURE: where allies are excluded (the
        // original does not hurt them in play, the word does not say); the model hits enemies only.
        let t: Vec<ObjKey> = circle_candidates(&gm, fx.x, fx.y, fx.p.area)
            .into_iter()
            .filter(|&k| gm.alive(k) && !gm.unit_is_invulnerable(k) && gm.is_enemy(owner, gm.unit(k).owner))
            .collect();
        (t, gm.order_tables().cloned())
    };
    let count = targets.len() as i32;
    for t in targets {
        let amount = {
            let gm = game.borrow();
            if !gm.alive(t) {
                continue;
            }
            let structure = gm.unit_types().is_building(gm.unit(t).type_id);
            blizzard_unit_damage(&fx.p, count, structure, false, tables.as_ref())
        };
        // The `BHbz` marker decides (`0x6F1AF950`, `0x6F2C31D0`): a unit without the marker gets one (duration
        // `interval - 0.01`) and takes the damage at once; a unit that has it takes none, and a larger value only
        // replaces the stored one and refreshes the duration ("strongest wins": Blizzards do not stack).
        let fresh = blizzard_marker(game, t, amount, fx.p.interval);
        if !fresh {
            continue;
        }
        let died = game
            .borrow_mut()
            .unit_damage_target(fx.caster, t, amount, false, false, 0, BLIZZARD_DAMAGE_TYPE);
        if let Some(k) = died {
            dispatch_unit_death_by(game, rt, k, Some(fx.caster));
        }
    }
    let mut gm = game.borrow_mut();
    if let Some(e) = gm.spells.human.blizzards.get_mut(&id) {
        e.damage_left -= 1;
        let (again, iv, done) = (e.damage_left > 0, e.p.interval.to_f32(), e.visual_left <= 0);
        if again {
            human_timer(&mut gm, iv, TICK_BLIZZARD_DAMAGE, id);
        } else if done {
            gm.spells.human.blizzards.remove(&id);
        }
    }
}

/// Damage type of Blizzard: the effect's class word is `0x200` (`0x6F1B45F0`), `1 << 9` = `DAMAGE_TYPE_COLD`.
const BLIZZARD_DAMAGE_TYPE: u32 = 9;

/// The marker buff `BHbz` (`CBuffBlizzardAoe`, parent `CBuffAoe`): it holds the wave value and ends after
/// `interval - 0.01` (`0x6F0D6040` subtracts the constant `0x6FAAE480`).
pub const BUFF_BLIZZARD_AOE: u32 = code(b"BHbz");

/// The marker rule of `0x6F1AF950` / `0x6F1CADD0` / `0x6F0D6040`. Returns whether the unit takes this wave's damage
/// (it had no marker). With a marker: a smaller stored value is replaced and the duration refreshed, no damage.
fn blizzard_marker(game: &RefCell<Game>, unit: ObjKey, value: F32, interval: F32) -> bool {
    let mut gm = game.borrow_mut();
    let dur = if arith::lt(F32::ZERO, interval) {
        arith::sub(interval, F32::from_bits(0x3C23_D70A))
    } else {
        F32::ZERO
    };
    let existing = gm
        .spells
        .human
        .markers
        .get(&unit)
        .copied()
        .filter(|(serial, _)| gm.spell_buff_alive(unit, *serial));
    match existing {
        None => {
            let serial = gm.add_spell_buff(unit, BUFF_BLIZZARD_AOE, crate::cast::BuffEffect::Marker, dur);
            gm.spells.human.markers.insert(unit, (serial, value));
            true
        }
        Some((serial, old)) => {
            if blizzard_buff_replaces(old, value) {
                gm.drop_spell_buff(serial);
                let serial = gm.add_spell_buff(unit, BUFF_BLIZZARD_AOE, crate::cast::BuffEffect::Marker, dur);
                gm.spells.human.markers.insert(unit, (serial, value));
            }
            false
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Mass Teleport (`AHmt`). Spec section 12.

/// A candidate record of the selection (`0x6F056890`, 20 bytes): the unit, `f2i` of the squared distance to the
/// caster, the level key, the hero flag, the "same owner as the caster" flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeleportRec {
    pub dist: i32,
    pub key: i32,
    pub hero: i32,
    pub own: i32,
    /// The unit's agent id (`unit + 0xC`), the final tie break.
    pub id: i32,
}

/// The sort comparator `0x6F026120` (CONFIRMED, Unicorn: 400 cases): negative when `a` goes first. Own units first,
/// heroes first, higher key first, nearer first, then the larger agent id first.
pub fn teleport_compare(a: &TeleportRec, b: &TeleportRec) -> i32 {
    let c = b.own.wrapping_sub(a.own);
    if c != 0 {
        return c;
    }
    let c = b.hero.wrapping_sub(a.hero);
    if c != 0 {
        return c;
    }
    let c = b.key.wrapping_sub(a.key);
    if c != 0 {
        return c;
    }
    let c = a.dist.wrapping_sub(b.dist);
    if c != 0 {
        return c;
    }
    b.id.wrapping_sub(a.id)
}

/// The count rule of `0x6F069D60` (CONFIRMED structure): with `limit = f2i(DataA) - 1` (compared as unsigned, so
/// `DataA < 1` never trims), more candidates than the limit are sorted and cut to the limit.
pub fn teleport_select(mut recs: Vec<(ObjKey, TeleportRec)>, data_a: i32) -> Vec<(ObjKey, TeleportRec)> {
    let limit = data_a.wrapping_sub(1) as u32;
    if recs.len() as u32 > limit {
        recs.sort_by(|a, b| teleport_compare(&a.1, &b.1).cmp(&0));
        recs.truncate(limit as usize);
    }
    recs
}

fn teleport_start(game: &RefCell<Game>, rec: &CastRec) {
    let CastTarget::Unit(dest) = rec.target else {
        return;
    };
    // The delay is the channel time of the cast core (`DataB`, section 14); the teleport runs at its completion
    // (`on_channel_complete`), keyed by the cast id.
    game.borrow_mut().spells.human.teleports.insert(
        rec.id,
        TeleportFx {
            caster: rec.caster,
            dest,
            level: rec.level,
        },
    );
}

/// Blizzard sets the stage-1 delay of the cast to 0 in its level init (`0x6F197E90`), so its `Cast` field is only
/// the wave interval (CONFIRMED by disassembly).
pub(crate) fn cast_field_is_wave_interval(base: u32) -> bool {
    base == BLIZZARD
}

/// The class value of the channel time property (ability `+0x90`), if the class sets one: Blizzard `waves * Cast`
/// (Unicorn 400 cases), Mass Teleport `DataB` (level init `0x6F060AE0`, reads the `DataB` getter `0x6F02EA50`).
pub fn channel_time_override(base: u32, level: &SpellLevel) -> Option<F32> {
    match base {
        BLIZZARD => Some(blizzard_channel_time(level)),
        MASS_TELEPORT => Some(level.data[1]),
        _ => None,
    }
}

/// Slot `0x39C(1)`, class part: Mass Teleport teleports when its channel time has run out.
pub(crate) fn on_channel_complete(game: &RefCell<Game>, rec: &CastRec) {
    if rec.base == MASS_TELEPORT {
        teleport_finish(game, rec.id);
    }
}

/// Slot `0x388`, class part: the channel-end hook of the cast `rec.id` (after FINISH, before ENDCAST). Blizzard
/// destroys its effect (the waves stop); Mass Teleport drops a teleport that did not complete.
pub(crate) fn on_cast_end(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    gm.spells.human.blizzards.retain(|_, fx| fx.cast != rec.id);
    gm.spells.human.teleports.remove(&rec.id);
}

fn teleport_finish(game: &RefCell<Game>, id: u32) {
    let Some(fx) = game.borrow_mut().spells.human.teleports.remove(&id) else {
        return;
    };
    let mut gm = game.borrow_mut();
    if !gm.alive(fx.caster) || !gm.alive(fx.dest) {
        return;
    }
    let (cx, cy, owner) = {
        let c = gm.unit(fx.caster);
        (c.x, c.y, c.owner)
    };
    let (dx, dy) = {
        let d = gm.unit(fx.dest);
        (d.x, d.y)
    };
    let mut recs = Vec::new();
    for k in circle_candidates(&gm, cx, cy, fx.level.area) {
        if k == fx.caster || !gm.alive(k) {
            continue;
        }
        let u = gm.unit(k);
        // The group filter word `0x40303006` (section 15): nonancient, **player** (the caster's own units only, no
        // allies), vulnerable and invulnerable, air and ground (so no structures), alive by default.
        if u.owner != owner || gm.unit_types().is_building(u.type_id) {
            continue;
        }
        let (ex, ey) = (arith::sub(u.x, cx), arith::sub(u.y, cy));
        let d2 = arith::add(arith::add(arith::mul(ex, ex), arith::mul(ey, ey)), F32::ZERO);
        let hero = u.hero.as_ref().map(|h| h.level);
        recs.push((
            k,
            TeleportRec {
                dist: conv::f2i(d2),
                key: hero.unwrap_or(0),
                hero: i32::from(hero.is_some() && !u.illusion),
                own: i32::from(u.owner == owner),
                id: k.0 as i32,
            },
        ));
    }
    let chosen = teleport_select(recs, conv::f2i(fx.level.data[0]));
    // Clustering = Misc `MassTeleportCluster` (default 0; absent from the shipped MiscData.txt) and `DataC != 0`
    // (`0x6F069F23..0x6F069F45`, CONFIRMED: the reader `0x6F009F90` is given the default 0). With it every unit is
    // placed at the destination position itself (Unicorn: `0x6F056980` hands the destination pair to the move call
    // `0x6F048AE0`); without it (the stock game) the unit gets an offset-based position (LIKELY: it keeps its offset
    // from the caster, UNSURE: the formula was not decoded).
    let cluster = gm.spells.human.mass_teleport_cluster && nonzero(fx.level.data[2]);
    let (px, py) = gm.clamp_unit_move(dx, dy);
    gm.store_unit_position(fx.caster, px, py);
    for (k, _) in chosen {
        let (ux, uy) = {
            let u = gm.unit(k);
            (u.x, u.y)
        };
        let (tx, ty) = if cluster {
            (dx, dy)
        } else {
            (arith::add(dx, arith::sub(ux, cx)), arith::add(dy, arith::sub(uy, cy)))
        };
        let (px, py) = gm.clamp_unit_move(tx, ty);
        gm.store_unit_position(k, px, py);
    }
}

/// The three values Banish hands to its new buff (`0x6F1CA2B0`, CONFIRMED, **Unicorn 400/400**): the duration (`HeroDur`
/// for a hero, else `add(Dur, extra)` with `extra` = ability `+0x9C`), the movement factor `-DataA` and the attack-speed
/// factor `-DataB`.
pub fn banish_values(l: &SpellLevel, hero: bool, extra: F32) -> (F32, F32, F32) {
    (
        effect_duration(l, hero, extra),
        F32::from_bits(l.data[0].to_bits() ^ 0x8000_0000),
        F32::from_bits(l.data[1].to_bits() ^ 0x8000_0000),
    )
}

/// Banish (`AHbn`, `CAbilityBanish`, unit effect `0x6F1CA2B0`, CONFIRMED by reading): the duration is `HeroDur` for a hero
/// and `Dur` plus the `+0x9C` term otherwise (the usual rule); the buff `BHbn` (`CBuffBanish`, a `CBuffEthereal`, a
/// `CBuffSpeed`) gets the movement factor **`-DataA`** and the attack-speed factor **`-DataB`**, with the caster as source.
/// An existing `BHbn` is refreshed through its slot `0x35C`, else a new buff is created and applied (slot `0x354` =
/// `0x6F09BEB0`): the speed values go in, the caster is remembered, **a running attack is stopped** (`0x6F2A4B80`) and the
/// ethereal counter of the unit goes up by one (`0x6F07A340`). The removal (`0x6F07F630`) lowers the counter again and
/// sends a zero-damage hit of class `0x210` from the remembered caster (not modelled). Ethereal units cannot be hit by
/// physical attacks and take the Misc ethereal damage factor (`combat.rs`/`damage.rs`).
fn banish(game: &RefCell<Game>, rec: &CastRec) {
    let CastTarget::Unit(target) = rec.target else {
        return;
    };
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let (dur, move_delta, attack_delta) = banish_values(&rec.level, hero, F32::ZERO);
    let effect = crate::cast::BuffEffect::Slow { move_delta, attack_delta };
    let serial = gm.add_spell_buff(target, BUFF_BANISH, effect, dur);
    if serial != 0 {
        gm.unit_mut(target).ethereal += 1;
        gm.combat_end(target);
    }
}

/// The class effects of the point and unit target spells of this module; `false` when `rec.base` is not one.
pub(crate) fn run_target_effect(game: &RefCell<Game>, rec: &CastRec) -> bool {
    match rec.base {
        BANISH => banish(game, rec),
        BLIZZARD => blizzard_start(game, rec),
        MASS_TELEPORT => teleport_start(game, rec),
        _ => return false,
    }
    true
}

/// A timer of [`crate::cast::SpellMsg::Human`].
pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, id: u32) {
    match kind {
        TICK_BLIZZARD_WAVE => blizzard_wave(game, id),
        TICK_BLIZZARD_DAMAGE => blizzard_damage(game, rt, id),
        _ => {}
    }
}

/// The effect of an immediate spell (the class effect slot `0x3C0` / `0x428`), called by the cast core.
pub(crate) fn run_immediate_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    match rec.base {
        THUNDER_CLAP => thunder_clap(game, rt, rec),
        AVATAR => avatar(game, rec),
        DIVINE_SHIELD => divine_shield(game, rec),
        RESURRECTION => resurrection(game, rec),
        WATER_ELEMENTAL => water_elemental(game, rec),
        PHOENIX => phoenix(game, rec),
        _ => {}
    }
}

/// An immediate order of `unit`: a spell order starts the cast, `undivineshield` removes the shield. `None` when the
/// id is not one of these (the order system takes it).
pub(crate) fn immediate_order(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    unit: ObjKey,
    id: u32,
) -> Option<bool> {
    if id == ORDER_UNDIVINE_SHIELD {
        return Some(game.borrow_mut().cancel_divine_shield(unit));
    }
    let base = game.borrow().spell_for_order(unit, id)?;
    if !is_immediate_spell(base) {
        return None;
    }
    Some(begin_cast(game, rt, unit, base, CastTarget::None).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prd_index_rounds_to_five_percent_steps() {
        let f = |v: f32| F32::from_bits(v.to_bits());
        assert_eq!(prd_index(f(0.0)), 0);
        assert_eq!(prd_index(f(25.0)), 5);
        assert_eq!(prd_index(f(15.0)), 3);
        assert_eq!(prd_index(f(100.0)), 20);
        assert_eq!(prd_index(f(500.0)), 20);
        assert_eq!(prd_index(f(-40.0)), 0);
    }

    #[test]
    fn immediate_spells_are_the_five_casts() {
        assert!(is_immediate_spell(THUNDER_CLAP));
        assert!(!is_immediate_spell(BASH));
        assert!(!is_immediate_spell(code(b"AHtb")));
    }

    #[test]
    fn codes_are_the_four_characters() {
        assert_eq!(BUFF_PAUSE, 0x4250_5345);
        assert_eq!(THUNDER_CLAP, 0x4148_7463);
    }
}
