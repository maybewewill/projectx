//! Kill experience in the death path (`docs/spec/heroes.md` section 8 and section 12).
//!
//! The death handler `0x6F2A4880` (ECX = the dying unit) gives experience when the unit has a killer. The model follows
//! the disassembly (verification labels as in the spec):
//!
//! 1. no killer, or the killer removed: nothing (the killer comes from the `+0x250` pair, `0x6F4786B0`; CONFIRMED);
//! 2. `0x6F285B00(killer, dying)` is `not (killer owner's mask +0x38 has the dying owner's bit)`: the +0x38 mask is the
//!    passive alliance row (type 0, CONFIRMED: `GetPlayerAlliance` indexes `(type << 4) + 0x38`), so a killer passively
//!    allied to the victim's owner gives nothing;
//! 3. an illusion victim (bit 30 of `+0x5C`) gives nothing (CONFIRMED, `0x6F2A48F8`);
//! 4. the candidate builder `0x6F2A46F0` stops when the killer is a structure (bit `0x10000` of the killer's `+0x5C`,
//!    `0x6F26DE50(0)`) unless Misc `BuildingKillsGiveExp` (CONFIRMED by the disassembly: ECX of that call is the killer);
//! 5. every hero of the game (the 'AHer' category walk, predicate `0x6F290770` always returns 1) is a candidate when
//!    `0x6F28B990` accepts it: eligibility, the global list before the range test, range (`heroes::is_candidate`,
//!    `heroes::in_range`, CONFIRMED in Unicorn);
//! 6. the recipients are the in-range list, or the global list when that is empty; the split and the engine are
//!    [`crate::heroes::distribute_kill`] (CONFIRMED in Unicorn).
//!
//! Model decisions that are NOT confirmed (marked UNSURE in the spec): the 'AHer' list insert is not located, so the
//! candidate order is the unit creation order (the numbers do not depend on it); the shared-experience test treats the
//! killer's own heroes as sharing (the +0x68 mask is the shared-XP alliance row, type 3, CONFIRMED, but whether a
//! player's row holds its own bit is not decoded); the upgrade bonus (`0x6F40B340`) counts as zero because the upgrade
//! model has no `HERO` research; the experience is granted before the death events run.

use softfloat::{arith, F32};

use crate::heroes::{self, BonusEnv, EngineEnv, KillEnv, KillVictim};
use crate::natives::game::{Game, HeroObj, ObjKey};

/// The unit flag bit 30 (illusion) and the removal-sequence bit are the two candidate exclusions; the model holds them
/// as the unit's `illusion` and `removed` fields.
fn passive_ally(game: &Game, killer_owner: u32, victim_owner: u32) -> bool {
    if killer_owner >= 16 || victim_owner >= 16 {
        return false;
    }
    game.player(killer_owner).alliances[0] & (1 << victim_owner) != 0
}

/// Whether the killer's shared-experience row reaches the candidate's owner (`0x6F3A34E0`: type 3). A player's own
/// heroes always count (UNSURE, see the module notes).
fn shares_xp(game: &Game, killer_owner: u32, cand_owner: u32) -> bool {
    if killer_owner == cand_owner {
        return true;
    }
    if killer_owner >= 16 || cand_owner >= 16 {
        return false;
    }
    game.player(killer_owner).alliances[3] & (1 << cand_owner) != 0
}

/// The skill point cap of a hero's slots against the abilities its unit holds (`heroes::skill_point_cap`), in the
/// `(min, max)` pair form `EngineEnv` takes. A hero without slots has no cap: the pairs make a slack that never binds.
fn caps_of(unit: &crate::natives::game::UnitObj) -> [(i32, i32); 5] {
    let Some(h) = unit.hero.as_ref() else {
        return [(0, 0); 5];
    };
    if h.slots.iter().all(|s| s.code == 0) {
        return [(0, 1_000_000), (0, 0), (0, 0), (0, 0), (0, 0)];
    }
    let learned = |code: u32| unit.abilities.get(code).map(|a| a.level_index);
    let mut caps = [(0, 0); 5];
    for (i, s) in h.slots.iter().enumerate() {
        caps[i] = (heroes::learned_count(learned(s.code)), heroes::slot_max(&h.slots, s.code));
    }
    caps
}

/// Gives the experience of `victim`'s death to the heroes (see the module notes). `killer` is the unit that took the
/// victim's life. Needs the softfloat tables (division); without them nothing is granted.
pub fn grant_kill_xp(game: &mut Game, victim: ObjKey, killer: ObjKey) {
    let cfg = *game.kill_xp_config();
    let (v_owner, v_type, v_illusion, v_summoned, v_pos, v_hero_level) = {
        let v = game.unit(victim);
        (
            v.owner,
            v.type_id,
            v.illusion,
            v.summoned,
            (v.x, v.y),
            v.hero.as_ref().map(|h| h.level),
        )
    };
    let (k_owner, k_removed, k_type) = {
        let k = game.unit(killer);
        (k.owner, k.removed, k.type_id)
    };
    if k_removed || v_illusion || passive_ally(game, k_owner, v_owner) {
        return;
    }
    if !cfg.building_kills && game.unit_types().is_building(k_type) {
        return;
    }
    let Some(tables) = game.order_tables().cloned() else {
        return;
    };
    let xp = game.hero_xp().clone();

    // The record (`0x6F2A46F0`): V = HeroExpRange squared; the candidates in creation order.
    let v_range = arith::mul(cfg.hero_exp_range, cfg.hero_exp_range);
    let mut in_range: Vec<ObjKey> = Vec::new();
    let mut global: Vec<ObjKey> = Vec::new();
    let candidates: Vec<ObjKey> = game
        .unit_objects()
        .filter(|(_, u)| u.hero.is_some())
        .map(|(k, _)| k)
        .collect();
    for c in candidates {
        let u = game.unit(c);
        let h = u.hero.as_ref().expect("filtered on hero");
        let at_max = heroes::hero_at_max(&xp, h);
        let flags_ok = !u.illusion && !u.removed;
        let shared = shares_xp(game, k_owner, u.owner);
        if !heroes::is_candidate(cfg.max_level_drain, at_max, if flags_ok { 0 } else { heroes::CANDIDATE_EXCLUDED_FLAGS }, shared) {
            continue;
        }
        if cfg.global_experience {
            global.push(c);
        }
        let d = heroes::kill_distance2(v_pos, (u.x, u.y));
        if heroes::in_range(d, v_range) {
            in_range.push(c);
        }
    }
    let list = if in_range.is_empty() { global } else { in_range };
    if list.is_empty() {
        return;
    }

    let victim_info = KillVictim {
        type_upper: crate::natives::hero::is_hero_id(v_type),
        illusion: v_illusion,
        summoned: v_summoned,
        owner: v_owner as i32,
        level: v_hero_level.unwrap_or_else(|| unit_level(game, v_type, cfg.max_unit_level)),
        ability: None,
    };
    let kill_env = KillEnv {
        max_unit: cfg.max_unit_level,
        summon_factor: cfg.summon_factor,
    };
    let no_levels = |_: u32| 0i32;
    let no_value = |_: u32, _: i32| F32::ZERO;
    let mut recipients: Vec<(HeroObj, EngineEnv)> = list
        .iter()
        .map(|&k| {
            let u = game.unit(k);
            let handicap = game.misc.players[(u.owner as usize).min(15)].handicap_xp;
            let env = EngineEnv {
                handicap,
                bonus: BonusEnv {
                    b1: 0,
                    slots: 0,
                    codes: [0; 9],
                    levels: &no_levels,
                    value: &no_value,
                },
                caps: caps_of(u),
            };
            (u.hero.clone().expect("filtered on hero"), env)
        })
        .collect();
    heroes::distribute_kill(&xp, &tables, &victim_info, &kill_env, &mut recipients);
    for (&k, (h, _)) in list.iter().zip(recipients) {
        game.unit_mut(k).hero = Some(h);
    }
}

/// The unit level `0x6F29DCA0` answers for a non-hero victim: the type's `level`, 0 when not positive, at most
/// `MaxUnitLevel` (CONFIRMED by the decompile; the ability override is not modelled).
fn unit_level(game: &Game, type_id: u32, max_unit: i32) -> i32 {
    let level = game.unit_types().get(type_id).map_or(0, |t| t.level);
    if level <= 0 {
        0
    } else if level > max_unit {
        max_unit
    } else {
        level
    }
}
