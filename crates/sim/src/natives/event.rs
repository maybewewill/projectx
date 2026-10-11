//! Event registrations, the event-response getters, and the generic event firing (`fire_event`).
//!
//! Addresses are 1.26a `Game.dll` functions. Sources: `docs/spec/groups-events.md` (sections 5 to 8) and
//! `groups-events-2.md` (section 7), plus the native bodies read in the scratchpad (not in the repo). The
//! labels are CONFIRMED (read in the body or the spec, checked), LIKELY (strong reading) or UNSURE.
//!
//! ## Firing and response slots (CONFIRMED structure, spec 8)
//! * The event id of a firing is slot 6. Every response getter first checks it against the ids it accepts,
//!   and returns the null value otherwise. The payload of the firing is the set of [`Field`]s it carries. In
//!   the original those are read from the payload objects that slots 4 and 5 hold (getters check slot 4
//!   with `FUN_6f3c1d40` and slot 5 with `FUN_6f3c1d60`). Here a field stands for one of those reads, and the
//!   "slot 4 must be present" checks are modelled by the field being present (LIKELY).
//! * Slot 0 is the trigger unit ([`EventSlots::unit`], `Ctx::Trigger::unit`), slot 3 the player (a field), slot
//!   7 the filter unit and slot 10 the enum unit (see `game.rs`).
//! * [`fire_event`] is the one entry point: it takes an observer table (the agent and the registration key),
//!   the event id and the slot values, and runs every matching listener oldest first. A listener whose
//!   registration has a matching rule (state operator, chat text, variable name) runs only when the firing
//!   satisfies it; a listener with a filter runs it with `GetFilterUnit` = the trigger unit.
//!
//! ## Registration keys (CONFIRMED where marked)
//! | native | agent (observer table) | key | event id | accepted ids | confidence |
//! |---|---|---|---|---|---|
//! | TriggerRegisterPlayerEvent | player | id + 0x80200 | id | 0x0B..=0x11, 0x105..=0x10C | CONFIRMED ids |
//! | TriggerRegisterPlayerStateEvent | player | op + 0x80101 | 0x0B | state 0..=25 | CONFIRMED (FUN_6f437240), op map LIKELY |
//! | TriggerRegisterPlayerChatEvent | player | 0x80260 | 0x60 | (text, exact) | CONFIRMED key, match rule UNSURE |
//! | TriggerRegisterPlayerAllianceChange | player | 0x8020C | 0x0C | any type | LIKELY (no firing site found) |
//! | TriggerRegisterGameEvent | game | id + 0x80200 | id | 0..=10, 0x100..=0x104 | CONFIRMED ids, LIKELY agent |
//! | TriggerRegisterGameStateEvent | game | op + 0x80101 | 3 | state 0, 1, 2 | LIKELY (state 2 uses 0x80106 in the body, see below) |
//! | TriggerRegisterUnitStateEvent | unit | op + 0x80101 | 0x3B | state 0..=3, registers 0 and 2 | CONFIRMED (spec 7.1) |
//! | TriggerRegisterFilterUnitEvent | unit | id + 0x80200 (+ filter) | id | 0x3C..=0x58, 0x11E..=0x129 | CONFIRMED ids, LIKELY agent |
//! | TriggerRegisterUnitInRange | unit | 0x8023D | 0x3D | range (+ filter) | CONFIRMED key (FUN_6f3d25f0), distance rule UNSURE |
//! | TriggerRegisterVariableEvent | game | 0x80290 (synthetic) | none | variable name | registration only, the host fires |
//! | TriggerRegisterDialogEvent | dialog | 0x8025C | 0x5C | any | LIKELY (body adds 0x8025C/0x8025D) |
//! | TriggerRegisterDialogButtonEvent | button | 0x8025A | 0x5A | any | LIKELY (body adds 0x8025A/0x8025B) |
//! | TriggerRegisterTrackableHitEvent / TrackEvent | trackable | 0x80207 / 0x80208 | 7 / 8 | any | LIKELY; no trackable object exists yet, so the null event |
//!
//! The dispatch of the state events compares the new value with the limit (see [`compare`]). The operator
//! numbers are the JASS `limitop` values 0..=5 (LESS_THAN .. NOT_EQUAL, LIKELY). Operators 6 and up never fire.
//!
//! ## Native bodies, decisions and divergences
//! * TriggerRegisterPlayerEvent with id 0x0C would go to the alliance route in the body; the third argument
//!   of that route is not passed by the 3-argument native, so here 0x0C is registered as a plain player event
//!   (UNSURE).
//! * TriggerRegisterGameStateEvent: states 0 and 1 use the operator key (as the body). For state 2 the body
//!   registers the fixed key 0x80106; this model uses the operator key for it too, so the operator is kept
//!   (UNSURE).
//! * TriggerRegisterUnitStateEvent: the body rejects a state outside 0..=3 and an operator at or above 0x80106,
//!   and it returns an event handle for states 1 and 3 that registers nothing (CONFIRMED). The task text asks
//!   for the max states to be rejected; this follows the body and returns a handle without a registration.
//! * Chat matching: a listener matches when the message equals the registered text (exact) or contains it
//!   (not exact), ignoring ASCII case (UNSURE; the matching routine is not located). The matched text is what
//!   `GetEventPlayerChatStringMatched` returns, the whole message is `GetEventPlayerChatString` (LIKELY).
//! * GetEventDetectingPlayer (0x3C3CB0) on 0x37 returns the player of the payload (LIKELY). The decompile of
//!   the non-null branch returns the null handle, so the detecting player in the body is UNSURE.
//! * Spell getters use the spell ids 0x110..=0x114 (player) and 0x121..=0x125 (unit). The caster, ability and
//!   target values come from the payload (LIKELY). GetSpellAbility's object type is UNSURE (see the spec).
//!
//! ## Getter table (addresses CONFIRMED; accepted ids CONFIRMED from the switch cases of each body)
//! | getter | address | accepted ids | field |
//! |---|---|---|---|
//! | GetTriggerWidget | 0x6F3BB260 | any | Widget (slot 1) |
//! | GetAttacker | 0x6F3C20F0 | 0x12, 0x3E | Attacker |
//! | GetEventDamage | 0x6F3C3C40 | any | Damage |
//! | GetEventDamageSource | 0x6F3C3C60 | any | DamageSource |
//! | GetEventTargetUnit | 0x6F3C3D00 | 0x3C, 0x3D | EventTarget |
//! | GetKillingUnit | 0x6F3C23F0 | 0x14, 0x35 | Killer |
//! | GetSpellAbilityUnit | 0x6F3C3470 | spell ids | SpellCaster |
//! | GetSpellAbility | 0x6F3C3380 | spell ids | SpellAbility |
//! | GetSpellAbilityId | 0x6F3C32A0 | spell ids | SpellAbilityCode |
//! | GetSpellTargetUnit / Item / Destructable | 0x6F3C3A80 / 0x6F3C38F0 / 0x6F3C3760 | spell ids | SpellTarget (typed) |
//! | GetSpellTargetX / Y / Loc | 0x6F3C3580 / 0x6F3C3670 / 0x6F2090 | spell ids | SpellX / SpellY |
//! | GetLevelingUnit | 0x6F3C2150 | 0x29, 0x4E | Leveler |
//! | GetLearningUnit | 0x6F3C21B0 | 0x2A, 0x4F | Learner |
//! | GetLearnedSkill / Level | 0x6F3C2210 / 0x6F3C2250 | 0x2A, 0x4F | Skill / SkillLevel |
//! | GetRevivableUnit | 0x6F3C2290 | 0x2B, 0x50 | Revivable |
//! | GetRevivingUnit | 0x6F3C22F0 | 0x2C..=0x2E, 0x51..=0x53 | Reviver |
//! | GetSummoningUnit | 0x6F3C28F0 | 0x2F, 0x54 | Summoner |
//! | GetSummonedUnit | 0x6F3C2950 | 0x2F, 0x54 | Summoned |
//! | GetTransportUnit | 0x6F3C29A0 | 0x33, 0x58 | Transport |
//! | GetLoadedUnit | 0x6F3C2A00 | 0x33, 0x58 | Loaded |
//! | GetManipulatingUnit | 0x6F3C2A60 | 0x30..=0x32, 0x55..=0x57 | Manipulator |
//! | GetManipulatedItem | 0x6F3C2B00 | 0x30..=0x32, 0x55..=0x57 | Manipulated |
//! | GetTrainedUnit | 0x6F3C2500 | 0x22, 0x47 | Trained |
//! | GetTrainedUnitType | 0x6F3C2450 | 0x20..=0x22, 0x45..=0x47 | TrainedType |
//! | GetResearched | 0x6F3C2870 | 0x23..=0x25, 0x48..=0x4A | Researched |
//! | GetSoldUnit | 0x6F3C2750 | 0x10D, 0x11E | Sold |
//! | GetSoldItem | 0x6F3C25F0 | 0x10F, 0x115, 0x120, 0x126 | SoldItem |
//! | GetBuyingUnit | 0x6F3C27B0 | 0x10D, 0x10F, 0x115, 0x11E, 0x120, 0x126 | Buyer |
//! | GetChangingUnit | 0x6F3C2680 | 0x10E, 0x11F | Changing |
//! | GetChangingUnitPrevOwner | 0x6F3C26E0 | 0x10E, 0x11F | PrevOwner (player) |
//! | GetEventDetectingPlayer | 0x6F3C3CB0 | 0x37 | Detector (player, UNSURE) |
//! | GetClickedButton | 0x6F3C1DE0 | 0x5A, 0x5C | Button |
//! | GetClickedDialog | 0x6F3C1E30 | 0x5A, 0x5C | Dialog |
//! | GetTriggeringTrackable | 0x6F3C3DA0 | 0x07, 0x08 | Trackable |
//! | GetEventPlayerChatString | 0x6F3C20B0 | 0x60 | ChatText |
//! | GetEventPlayerChatStringMatched | 0x6F3C2080 | 0x60 | ChatMatched |
//! | GetEventPlayerState | 0x6F3C2050 | 0x0B | PlayerState (returned if < 26) |
//! | GetEventUnitState | 0x6F3C3C10 | 0x3B | UnitState (returned if < 4) |
//! | GetEventGameState | 0x6F3C2020 | 0x03 | GameState (returned if < 3) |
//! | GetDecayingUnit | 0x6F3BB430 | 0x15, 0x36 | trigger unit (slot 0) |
//! | GetConstructingStructure | 0x6F3BB450 | 0x1A | trigger unit |
//! | GetCancelledStructure | 0x6F3BB470 | 0x1B, 0x40 | trigger unit |
//! | GetConstructedStructure | 0x6F3BB490 | 0x1C, 0x41 | trigger unit |
//! | GetResearchingUnit | 0x6F3BB4D0 | 0x23..=0x25, 0x48..=0x4A | trigger unit |
//! | GetDetectedUnit | 0x6F3BB4B0 | 0x16 | trigger unit |
//! | GetOrderedUnit | 0x6F3BB530 | 0x26..=0x28, 0x4B..=0x4D | trigger unit |
//!
//! ## Open issues
//! * The payload checks of the decompile (slot 4 and slot 5 presence) are approximated by the fields.
//! * The state-event comparators (0x6F0E1BA0 life, 0x6F02DCC0 mana) and the player-state conversion for states
//!   14 to 16 are not decoded; the comparison uses the JASS operator on the real value.
//! * Firing records are never freed (one per firing, like the order records).

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};
use softfloat::{arith, F32};

use super::game::{
    Ctx, Event, EventSource, Game, HandleKind, LocationObj, ObjKey, Object, Observer, GAME_AGENT,
};
use super::player::raw_at;
use super::trigger::fire_with_slots;
use super::unit_event::{filter_passes, register_observer, trigger_unit_handle, KEY_BASE};
use super::ScriptRuntime;

/// The key base of the player-state and limit operators: operator `n` is key `0x80101 + n`.
pub const KEY_OPERATOR_BASE: u32 = 0x8_0101;
/// Player chat (`TriggerRegisterPlayerChatEvent`), event id 0x60.
pub const KEY_CHAT: u32 = 0x8_0260;
/// Alliance change, event id 0x0C.
pub const KEY_ALLIANCE: u32 = 0x8_020C;
/// Unit comes within range (`TriggerRegisterUnitInRange`), event id 0x3D.
pub const KEY_UNIT_IN_RANGE: u32 = 0x8_023D;
/// Dialog button click, event id 0x5A.
pub const KEY_DIALOG_BUTTON: u32 = 0x8_025A;
/// Dialog click, event id 0x5C.
pub const KEY_DIALOG: u32 = 0x8_025C;
/// Global variable changes (`TriggerRegisterVariableEvent`). A synthetic key: no JASS event id maps to it.
pub const KEY_VARIABLE: u32 = 0x8_0290;
/// Trackable hit and track (event ids 7 and 8).
pub const KEY_TRACKABLE_HIT: u32 = 0x8_0207;
pub const KEY_TRACKABLE_TRACK: u32 = 0x8_0208;

/// Event ids used by the firings in this module.
pub const EVENT_GAME_STATE: u32 = 0x03;
pub const EVENT_PLAYER_STATE: u32 = 0x0B;
pub const EVENT_PLAYER_CHAT: u32 = 0x60;
pub const EVENT_UNIT_STATE: u32 = 0x3B;
pub const EVENT_UNIT_IN_RANGE: u32 = 0x3D;
pub const EVENT_OWNER_PLAYER: u32 = 0x10E;
pub const EVENT_OWNER_UNIT: u32 = 0x11F;

/// The player states a player-state event may name (0..=25, body of `TriggerRegisterPlayerStateEvent`).
const PLAYER_STATE_COUNT: i32 = 26;
/// The spell event ids: 0x110..=0x114 (player unit) and 0x121..=0x125 (unit).
const SPELL_IDS: &[u32] = &[
    0x110, 0x111, 0x112, 0x113, 0x114, 0x121, 0x122, 0x123, 0x124, 0x125,
];

/// A value a firing carries. `Object` is an arena object (its handle is created when read), `Player` a player
/// slot index (its handle is the player handle).
#[derive(Debug, Clone, PartialEq)]
pub enum Datum {
    Object(ObjKey),
    Player(u32),
    Int(i32),
    Real(F32),
    Text(String),
}

/// The payload fields of a firing. Each field is one of the reads the getters make (see the table above).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Widget,
    Player,
    Damage,
    DamageSource,
    Attacker,
    EventTarget,
    Killer,
    SpellCaster,
    SpellAbility,
    SpellAbilityCode,
    SpellTarget,
    SpellX,
    SpellY,
    Learner,
    Leveler,
    Skill,
    SkillLevel,
    Revivable,
    Reviver,
    Summoner,
    Summoned,
    Transport,
    Loaded,
    Manipulator,
    Manipulated,
    Trained,
    TrainedType,
    Researched,
    Sold,
    SoldItem,
    Buyer,
    Changing,
    PrevOwner,
    Detector,
    Button,
    Dialog,
    Trackable,
    ChatText,
    ChatMatched,
    PlayerState,
    UnitState,
    GameState,
}

/// One firing: the event id (slot 6) and its payload. Stored in `Game::firings`, read by the getters through
/// the `firing` index of the running `Ctx::Trigger`.
#[derive(Debug, Clone, PartialEq)]
pub struct Firing {
    pub id: u32,
    pub data: Vec<(Field, Datum)>,
}

/// What a caller passes to [`fire_event`]: the event id, the trigger unit (slot 0), and the payload.
/// `state`, `value` and `text` are the values the matching rules use (see [`accepts`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventSlots {
    pub id: u32,
    pub unit: Option<ObjKey>,
    pub state: Option<u32>,
    pub value: Option<F32>,
    pub text: Option<String>,
    pub data: Vec<(Field, Datum)>,
}

/// The comparison of a state event (`limit` operator `op` of the JASS limitop constants). Equality uses the
/// VM's real `==` (approximate, LIKELY). Operators above 5 never fire.
pub fn compare(op: u32, value: F32, limit: F32) -> bool {
    match op {
        0 => arith::lt(value, limit),
        1 => arith::le(value, limit),
        2 => arith::approx_eq(value, limit),
        3 => !arith::lt(value, limit),
        4 => arith::gt(value, limit),
        5 => !arith::approx_eq(value, limit),
        _ => false,
    }
}

/// The chat matching rule (UNSURE, see the module notes): exact equality, or containment, ignoring ASCII case.
pub fn chat_matches(message: &str, pattern: &str, exact: bool) -> bool {
    let m = message.to_ascii_lowercase();
    let p = pattern.to_ascii_lowercase();
    if exact {
        m == p
    } else {
        m.contains(&p)
    }
}

/// Fires event `id` on the observer table of `source` under `key`: every listener of that key runs, oldest
/// first, when its matching rule accepts the slots and its filter (if any) passes with `GetFilterUnit` = the
/// trigger unit. Each running listener gets its own firing record in `Game::firings`.
pub fn fire_event(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    source: ObjKey,
    key: u32,
    slots: EventSlots,
) {
    let nodes: Vec<Observer> = game
        .borrow()
        .observers(source)
        .iter()
        .filter(|o| o.key == key)
        .copied()
        .collect();
    for node in nodes {
        let extra = {
            let gm = game.borrow();
            accepts(&gm, node, &slots)
        };
        if let Some(extra) = extra {
            fire_node(game, rt, node, &slots, extra);
        }
    }
}

/// The extra payload a listener adds when `slots` satisfy its matching rule, or `None` when they do not.
/// Listeners whose source has no rule accept with no extra payload.
fn accepts(gm: &Game, node: Observer, slots: &EventSlots) -> Option<Vec<(Field, Datum)>> {
    let Object::Event(ev) = gm.object(node.event) else {
        return None;
    };
    match ev.source {
        EventSource::Limit { state, limit, .. } => {
            let (Some(s), Some(v)) = (slots.state, slots.value) else {
                return None;
            };
            let op = node.key.wrapping_sub(KEY_OPERATOR_BASE);
            (s == state && compare(op, v, limit)).then(Vec::new)
        }
        EventSource::Chat { exact, text, .. } => {
            let message = slots.text.as_deref()?;
            let pattern = gm.strings.get(text as usize)?;
            chat_matches(message, pattern, exact)
                .then(|| vec![(Field::ChatMatched, Datum::Text(pattern.clone()))])
        }
        EventSource::Variable { name, op, limit } => {
            let (Some(n), Some(v)) = (slots.text.as_deref(), slots.value) else {
                return None;
            };
            let same = gm.strings.get(name as usize).is_some_and(|s| s == n);
            (same && compare(op, v, limit)).then(Vec::new)
        }
        _ => Some(Vec::new()),
    }
}

/// Runs one listener with a new firing record. Returns whether its conditions passed.
fn fire_node(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    node: Observer,
    slots: &EventSlots,
    extra: Vec<(Field, Datum)>,
) -> bool {
    if let (Some(filter), Some(unit)) = (node.filter, slots.unit) {
        if !filter_passes(game, rt, filter, node, unit) {
            return false;
        }
    }
    let index = {
        let mut gm = game.borrow_mut();
        let mut data = slots.data.clone();
        data.extend(extra);
        gm.firings.push(Firing { id: slots.id, data });
        (gm.firings.len() - 1) as u32
    };
    fire_with_slots(
        game,
        rt,
        node.trigger,
        Some(node.event),
        slots.unit,
        None,
        Some(index),
    )
}

/// A player-state change (`SetPlayerState`): fires the player's state listeners for state `state` with the new
/// value `value` (as the JASS real). Called by `natives/player.rs`.
pub(crate) fn fire_player_state(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    player: u32,
    state: u32,
    value: F32,
) {
    for op in 0..6 {
        fire_event(
            game,
            rt,
            ObjKey(player),
            KEY_OPERATOR_BASE + op,
            EventSlots {
                id: EVENT_PLAYER_STATE,
                state: Some(state),
                value: Some(value),
                data: vec![
                    (Field::Player, Datum::Player(player)),
                    (Field::PlayerState, Datum::Int(state as i32)),
                ],
                ..Default::default()
            },
        );
    }
}

/// A unit's life or mana change: fires the unit's state listeners for `state` (0 life, 2 mana) with the value
/// it has now. Called by `natives/unit.rs` after the change (the unit may have died; that is fired separately).
pub(crate) fn fire_unit_state(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    unit: ObjKey,
    state: u32,
) {
    let value = {
        let gm = game.borrow();
        let u = gm.unit(unit);
        match state {
            0 => u.life,
            2 => u.mana,
            _ => return,
        }
    };
    for op in 0..6 {
        fire_event(
            game,
            rt,
            unit,
            KEY_OPERATOR_BASE + op,
            EventSlots {
                id: EVENT_UNIT_STATE,
                unit: Some(unit),
                state: Some(state),
                value: Some(value),
                data: vec![(Field::UnitState, Datum::Int(state as i32))],
                ..Default::default()
            },
        );
    }
}

/// A chat line from `player` (host input). Fires the player's chat listeners whose text matches.
pub fn fire_player_chat(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, player: u32, text: &str) {
    fire_event(
        game,
        rt,
        ObjKey(player),
        KEY_CHAT,
        EventSlots {
            id: EVENT_PLAYER_CHAT,
            text: Some(text.to_string()),
            data: vec![
                (Field::Player, Datum::Player(player)),
                (Field::ChatText, Datum::Text(text.to_string())),
            ],
            ..Default::default()
        },
    );
}

/// An owner change of `unit` from `from` to `to` (`SetUnitOwner`): fires the new owner's player-unit listeners
/// (0x10E) and the unit's own listeners (0x11F). `GetChangingUnit` is the unit, `GetChangingUnitPrevOwner` the
/// old owner. Called by `natives/unit.rs`.
pub(crate) fn fire_owner_change(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    unit: ObjKey,
    from: u32,
    to: u32,
) {
    let data = vec![
        (Field::Changing, Datum::Object(unit)),
        (Field::PrevOwner, Datum::Player(from)),
        (Field::Player, Datum::Player(to)),
    ];
    fire_event(
        game,
        rt,
        ObjKey(to),
        KEY_BASE + EVENT_OWNER_PLAYER,
        EventSlots {
            id: EVENT_OWNER_PLAYER,
            unit: Some(unit),
            data: data.clone(),
            ..Default::default()
        },
    );
    fire_event(
        game,
        rt,
        unit,
        KEY_BASE + EVENT_OWNER_UNIT,
        EventSlots {
            id: EVENT_OWNER_UNIT,
            unit: Some(unit),
            data,
            ..Default::default()
        },
    );
}

/// A game state change (`state` 0, 1 or 2; the value is the JASS real). Fires the game-state listeners.
pub fn fire_game_state(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, state: u32, value: F32) {
    for op in 0..6 {
        fire_event(
            game,
            rt,
            GAME_AGENT,
            KEY_OPERATOR_BASE + op,
            EventSlots {
                id: EVENT_GAME_STATE,
                state: Some(state),
                value: Some(value),
                data: vec![(Field::GameState, Datum::Int(state as i32))],
                ..Default::default()
            },
        );
    }
}

/// A global variable was set to `value` (the host calls this from its variable store). Fires the variable
/// listeners of the variable `name` whose comparison holds.
pub fn fire_variable_changed(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    name: &str,
    value: F32,
) {
    fire_event(
        game,
        rt,
        GAME_AGENT,
        KEY_VARIABLE,
        EventSlots {
            id: 0,
            value: Some(value),
            text: Some(name.to_ascii_lowercase()),
            ..Default::default()
        },
    );
}

/// A unit moved from `from` to its current position: fires the range listeners (`TriggerRegisterUnitInRange`)
/// of the other units whose range the unit has entered (the squared distance test of the groups spec, with
/// the world units: UNSURE). Called by the position natives through `region::unit_moved`.
pub(crate) fn unit_range_moved(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    unit: ObjKey,
    from: (F32, F32),
) {
    let entered: Vec<Observer> = {
        let gm = game.borrow();
        let u = gm.unit(unit);
        let now = (u.x, u.y);
        let mut found = Vec::new();
        for agent in gm.observer_agents() {
            if agent == unit || !gm.is_alive(agent) {
                continue;
            }
            let Object::Unit(center) = gm.object(agent) else {
                continue;
            };
            let center = (center.x, center.y);
            for node in gm.observers(agent) {
                if node.key != KEY_UNIT_IN_RANGE {
                    continue;
                }
                let Object::Event(ev) = gm.object(node.event) else {
                    continue;
                };
                let EventSource::InRange { range, .. } = ev.source else {
                    continue;
                };
                let r2 = arith::mul(range, range);
                let was = arith::le(arith::dist2(from.0, from.1, center.0, center.1), r2);
                let is = arith::le(arith::dist2(now.0, now.1, center.0, center.1), r2);
                if !was && is {
                    found.push(*node);
                }
            }
        }
        found
    };
    for node in entered {
        let slots = EventSlots {
            id: EVENT_UNIT_IN_RANGE,
            unit: Some(unit),
            ..Default::default()
        };
        fire_node(game, rt, node, &slots, Vec::new());
    }
}

/// Registers the event natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    register_registrations(reg, game);
    register_getters(reg, game);
}

fn register_registrations(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("TriggerRegisterPlayerEvent", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = raw_at(a, 2) as u32;
        let ok_id = matches!(id, 0x0B..=0x11 | 0x105..=0x10C);
        let (Some(t), Some(p), true) = (trigger_arg(&gm, a, 0), player_arg(&gm, a, 1), ok_id)
        else {
            return NativeResult::Return(Value::Handle(0));
        };
        let key = id + KEY_BASE;
        let source = EventSource::Keyed { key, value: 0 };
        let h = register_observer(&mut gm, ObjKey(p), key, t, None, source);
        NativeResult::Return(Value::Handle(h))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterPlayerStateEvent", move |_, a| {
        let mut gm = g.borrow_mut();
        let state = raw_at(a, 2);
        let (Some(t), Some(p), true) = (
            trigger_arg(&gm, a, 0),
            player_arg(&gm, a, 1),
            (0..PLAYER_STATE_COUNT).contains(&state),
        ) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let op = raw_at(a, 3) as u32;
        let key = op.wrapping_add(KEY_OPERATOR_BASE);
        let source = EventSource::Limit {
            key,
            state: state as u32,
            limit: real_arg(a, 4),
        };
        let h = register_observer(&mut gm, ObjKey(p), key, t, None, source);
        NativeResult::Return(Value::Handle(h))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterPlayerChatEvent", move |ctx, a| {
        let mut gm = g.borrow_mut();
        let (Some(t), Some(p)) = (trigger_arg(&gm, a, 0), player_arg(&gm, a, 1)) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let pattern = match a.get(2) {
            Some(Value::String(id)) => ctx.string(*id).to_string(),
            _ => String::new(),
        };
        let exact = a.get(3).and_then(|v| v.as_boolean()).unwrap_or(false);
        gm.strings.push(pattern);
        let text = (gm.strings.len() - 1) as u32;
        let source = EventSource::Chat {
            key: KEY_CHAT,
            exact,
            text,
        };
        let h = register_observer(&mut gm, ObjKey(p), KEY_CHAT, t, None, source);
        NativeResult::Return(Value::Handle(h))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterPlayerAllianceChange", move |_, a| {
        let mut gm = g.borrow_mut();
        let (Some(t), Some(p)) = (trigger_arg(&gm, a, 0), player_arg(&gm, a, 1)) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let value = raw_at(a, 2);
        let source = EventSource::Keyed {
            key: KEY_ALLIANCE,
            value,
        };
        let h = register_observer(&mut gm, ObjKey(p), KEY_ALLIANCE, t, None, source);
        NativeResult::Return(Value::Handle(h))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterGameEvent", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = raw_at(a, 1) as u32;
        let ok_id = matches!(id, 0..=0x0A | 0x100..=0x104);
        let (Some(t), true) = (trigger_arg(&gm, a, 0), ok_id) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let key = id + KEY_BASE;
        let source = EventSource::Keyed { key, value: 0 };
        let h = register_observer(&mut gm, GAME_AGENT, key, t, None, source);
        NativeResult::Return(Value::Handle(h))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterGameStateEvent", move |_, a| {
        let mut gm = g.borrow_mut();
        let state = raw_at(a, 1);
        let Some(t) = trigger_arg(&gm, a, 0) else {
            return NativeResult::Return(Value::Handle(0));
        };
        if !(0..=2).contains(&state) {
            return NativeResult::Return(Value::Handle(0));
        }
        let key = (raw_at(a, 2) as u32).wrapping_add(KEY_OPERATOR_BASE);
        let source = EventSource::Limit {
            key,
            state: state as u32,
            limit: real_arg(a, 3),
        };
        let h = register_observer(&mut gm, GAME_AGENT, key, t, None, source);
        NativeResult::Return(Value::Handle(h))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterUnitStateEvent", move |_, a| {
        let mut gm = g.borrow_mut();
        let state = raw_at(a, 2);
        let op = raw_at(a, 3);
        let (Some(t), Some(u)) = (trigger_arg(&gm, a, 0), unit_arg(&gm, a, 1)) else {
            return NativeResult::Return(Value::Handle(0));
        };
        if !(0..4).contains(&state) || op >= 0x8_0106 {
            return NativeResult::Return(Value::Handle(0));
        }
        let key = (op as u32).wrapping_add(KEY_OPERATOR_BASE);
        let source = EventSource::Limit {
            key,
            state: state as u32,
            limit: real_arg(a, 4),
        };
        // States 1 and 3 (the maxima) return an event that no table holds (CONFIRMED, spec 6.2).
        if state == 0 || state == 2 {
            let h = register_observer(&mut gm, u, key, t, None, source);
            return NativeResult::Return(Value::Handle(h));
        }
        let (_, h) = gm.add_object(Object::Event(Event { trigger: t, source }), 1);
        NativeResult::Return(Value::Handle(h))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterFilterUnitEvent", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = raw_at(a, 2) as u32;
        let ok_id = matches!(id, 0x3C..=0x58 | 0x11E..=0x129);
        let (Some(t), Some(u), true) = (trigger_arg(&gm, a, 0), unit_arg(&gm, a, 1), ok_id) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let filter = boolexpr_arg(&gm, a, 3);
        let key = id + KEY_BASE;
        let source = EventSource::Keyed { key, value: 0 };
        let h = register_observer(&mut gm, u, key, t, filter, source);
        NativeResult::Return(Value::Handle(h))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterUnitInRange", move |_, a| {
        let mut gm = g.borrow_mut();
        let (Some(t), Some(u)) = (trigger_arg(&gm, a, 0), unit_arg(&gm, a, 1)) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let filter = boolexpr_arg(&gm, a, 3);
        let source = EventSource::InRange {
            key: KEY_UNIT_IN_RANGE,
            range: real_arg(a, 2),
        };
        let h = register_observer(&mut gm, u, KEY_UNIT_IN_RANGE, t, filter, source);
        NativeResult::Return(Value::Handle(h))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterVariableEvent", move |ctx, a| {
        let mut gm = g.borrow_mut();
        let Some(t) = trigger_arg(&gm, a, 0) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let name = match a.get(1) {
            Some(Value::String(id)) => ctx.string(*id).to_ascii_lowercase(),
            _ => String::new(),
        };
        let op = raw_at(a, 2) as u32;
        gm.strings.push(name);
        let name = (gm.strings.len() - 1) as u32;
        let source = EventSource::Variable {
            name,
            op,
            limit: real_arg(a, 3),
        };
        let h = register_observer(&mut gm, GAME_AGENT, KEY_VARIABLE, t, None, source);
        NativeResult::Return(Value::Handle(h))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterDialogEvent", move |_, a| {
        let mut gm = g.borrow_mut();
        let (Some(t), Some(d)) = (
            trigger_arg(&gm, a, 0),
            handle_kind_arg(&gm, a, 1, HandleKind::Dialog),
        ) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let source = EventSource::Keyed {
            key: KEY_DIALOG,
            value: 0,
        };
        let h = register_observer(&mut gm, d, KEY_DIALOG, t, None, source);
        NativeResult::Return(Value::Handle(h))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterDialogButtonEvent", move |_, a| {
        let mut gm = g.borrow_mut();
        let (Some(t), Some(b)) = (
            trigger_arg(&gm, a, 0),
            handle_kind_arg(&gm, a, 1, HandleKind::Button),
        ) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let source = EventSource::Keyed {
            key: KEY_DIALOG_BUTTON,
            value: 0,
        };
        let h = register_observer(&mut gm, b, KEY_DIALOG_BUTTON, t, None, source);
        NativeResult::Return(Value::Handle(h))
    });

    // No trackable object is modelled yet (there is no CreateTrackable), so these natives return the null
    // event. The keys they would register are KEY_TRACKABLE_HIT and KEY_TRACKABLE_TRACK (LIKELY).
    reg.register("TriggerRegisterTrackableHitEvent", move |_, _| {
        NativeResult::Return(Value::Handle(0))
    });
    reg.register("TriggerRegisterTrackableTrackEvent", move |_, _| {
        NativeResult::Return(Value::Handle(0))
    });
}

/// The response value that a getter returns.
#[derive(Clone, Copy)]
enum Ret {
    Handle,
    Int,
    Real,
    Text,
}

fn register_getters(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    register_field(
        reg,
        game,
        "GetTriggerWidget",
        None,
        Field::Widget,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetAttacker",
        Some(&[0x12, 0x3E]),
        Field::Attacker,
        Ret::Handle,
    );
    register_field(reg, game, "GetEventDamage", None, Field::Damage, Ret::Real);
    register_field(
        reg,
        game,
        "GetEventDamageSource",
        None,
        Field::DamageSource,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetEventTargetUnit",
        Some(&[0x3C, 0x3D]),
        Field::EventTarget,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetKillingUnit",
        Some(&[0x14, 0x35]),
        Field::Killer,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetSpellAbilityUnit",
        Some(SPELL_IDS),
        Field::SpellCaster,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetSpellAbility",
        Some(SPELL_IDS),
        Field::SpellAbility,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetSpellAbilityId",
        Some(SPELL_IDS),
        Field::SpellAbilityCode,
        Ret::Int,
    );
    register_field(
        reg,
        game,
        "GetSpellTargetX",
        Some(SPELL_IDS),
        Field::SpellX,
        Ret::Real,
    );
    register_field(
        reg,
        game,
        "GetSpellTargetY",
        Some(SPELL_IDS),
        Field::SpellY,
        Ret::Real,
    );
    register_field(
        reg,
        game,
        "GetLevelingUnit",
        Some(&[0x29, 0x4E]),
        Field::Leveler,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetLearningUnit",
        Some(&[0x2A, 0x4F]),
        Field::Learner,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetLearnedSkill",
        Some(&[0x2A, 0x4F]),
        Field::Skill,
        Ret::Int,
    );
    register_field(
        reg,
        game,
        "GetLearnedSkillLevel",
        Some(&[0x2A, 0x4F]),
        Field::SkillLevel,
        Ret::Int,
    );
    register_field(
        reg,
        game,
        "GetRevivableUnit",
        Some(&[0x2B, 0x50]),
        Field::Revivable,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetRevivingUnit",
        Some(&[0x2C, 0x2D, 0x2E, 0x51, 0x52, 0x53]),
        Field::Reviver,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetSummoningUnit",
        Some(&[0x2F, 0x54]),
        Field::Summoner,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetSummonedUnit",
        Some(&[0x2F, 0x54]),
        Field::Summoned,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetTransportUnit",
        Some(&[0x33, 0x58]),
        Field::Transport,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetLoadedUnit",
        Some(&[0x33, 0x58]),
        Field::Loaded,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetManipulatingUnit",
        Some(&[0x30, 0x31, 0x32, 0x55, 0x56, 0x57]),
        Field::Manipulator,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetManipulatedItem",
        Some(&[0x30, 0x31, 0x32, 0x55, 0x56, 0x57]),
        Field::Manipulated,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetTrainedUnit",
        Some(&[0x22, 0x47]),
        Field::Trained,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetTrainedUnitType",
        Some(&[0x20, 0x21, 0x22, 0x45, 0x46, 0x47]),
        Field::TrainedType,
        Ret::Int,
    );
    register_field(
        reg,
        game,
        "GetResearched",
        Some(&[0x23, 0x24, 0x25, 0x48, 0x49, 0x4A]),
        Field::Researched,
        Ret::Int,
    );
    register_field(
        reg,
        game,
        "GetSoldUnit",
        Some(&[0x10D, 0x11E]),
        Field::Sold,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetSoldItem",
        Some(&[0x10F, 0x115, 0x120, 0x126]),
        Field::SoldItem,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetBuyingUnit",
        Some(&[0x10D, 0x10F, 0x115, 0x11E, 0x120, 0x126]),
        Field::Buyer,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetChangingUnit",
        Some(&[0x10E, 0x11F]),
        Field::Changing,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetChangingUnitPrevOwner",
        Some(&[0x10E, 0x11F]),
        Field::PrevOwner,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetEventDetectingPlayer",
        Some(&[0x37]),
        Field::Detector,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetClickedButton",
        Some(&[0x5A, 0x5C]),
        Field::Button,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetClickedDialog",
        Some(&[0x5A, 0x5C]),
        Field::Dialog,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetTriggeringTrackable",
        Some(&[0x07, 0x08]),
        Field::Trackable,
        Ret::Handle,
    );
    register_field(
        reg,
        game,
        "GetEventPlayerChatString",
        Some(&[0x60]),
        Field::ChatText,
        Ret::Text,
    );
    register_field(
        reg,
        game,
        "GetEventPlayerChatStringMatched",
        Some(&[0x60]),
        Field::ChatMatched,
        Ret::Text,
    );
    register_field(
        reg,
        game,
        "GetEventPlayerState",
        Some(&[0x0B]),
        Field::PlayerState,
        Ret::Int,
    );
    register_field(
        reg,
        game,
        "GetEventUnitState",
        Some(&[0x3B]),
        Field::UnitState,
        Ret::Int,
    );
    register_field(
        reg,
        game,
        "GetEventGameState",
        Some(&[0x03]),
        Field::GameState,
        Ret::Int,
    );

    register_spell_target(reg, game, "GetSpellTargetUnit", HandleKind::Unit);
    register_spell_target(reg, game, "GetSpellTargetItem", HandleKind::Item);
    register_spell_target(
        reg,
        game,
        "GetSpellTargetDestructable",
        HandleKind::Destructable,
    );

    let g = Rc::clone(game);
    reg.register("GetSpellTargetLoc", move |ctx, _| {
        let mut gm = g.borrow_mut();
        let thread = ctx.thread_key();
        let x = response(&gm, thread, Some(SPELL_IDS), Field::SpellX);
        let y = response(&gm, thread, Some(SPELL_IDS), Field::SpellY);
        let id = match (x, y) {
            (Some(Datum::Real(x)), Some(Datum::Real(y))) => {
                gm.add_object(Object::Location(LocationObj { x, y }), 1).1
            }
            _ => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    // Trigger-unit getters: the unit of the running dispatch (slot 0), when the event id is accepted.
    register_trigger_unit(reg, game, "GetDecayingUnit", &[0x15, 0x36]);
    register_trigger_unit(reg, game, "GetConstructingStructure", &[0x1A]);
    register_trigger_unit(reg, game, "GetCancelledStructure", &[0x1B, 0x40]);
    register_trigger_unit(reg, game, "GetConstructedStructure", &[0x1C, 0x41]);
    register_trigger_unit(
        reg,
        game,
        "GetResearchingUnit",
        &[0x23, 0x24, 0x25, 0x48, 0x49, 0x4A],
    );
    register_trigger_unit(reg, game, "GetDetectedUnit", &[0x16]);
    register_trigger_unit(
        reg,
        game,
        "GetOrderedUnit",
        &[0x26, 0x27, 0x28, 0x4B, 0x4C, 0x4D],
    );
}

/// A getter that returns the trigger unit when the running event id is in `ids`, else the null handle.
fn register_trigger_unit(
    reg: &mut NativeRegistry,
    game: &Rc<RefCell<Game>>,
    name: &str,
    ids: &'static [u32],
) {
    let g = Rc::clone(game);
    reg.register(name, move |ctx, _| {
        let gm = g.borrow();
        let thread = ctx.thread_key();
        let accepted =
            super::unit_event::current_event_id(&gm, thread).is_some_and(|id| ids.contains(&id));
        let id = if accepted {
            trigger_unit_handle(&gm, thread)
        } else {
            0
        };
        NativeResult::Return(Value::Handle(id))
    });
}

/// A getter that returns field `field` of the running firing when its event id is in `ids` (`None`: any).
fn register_field(
    reg: &mut NativeRegistry,
    game: &Rc<RefCell<Game>>,
    name: &str,
    ids: Option<&'static [u32]>,
    field: Field,
    ret: Ret,
) {
    let g = Rc::clone(game);
    reg.register(name, move |ctx, _| {
        let mut gm = g.borrow_mut();
        let datum = response(&gm, ctx.thread_key(), ids, field);
        let v = match (ret, datum) {
            (Ret::Handle, Some(d)) => Value::Handle(datum_handle(&mut gm, &d)),
            (Ret::Int, Some(Datum::Int(n))) => Value::Integer(n),
            (Ret::Real, Some(Datum::Real(r))) => Value::Real(r),
            (Ret::Text, Some(Datum::Text(s))) => Value::String(ctx.intern(&s)),
            (Ret::Handle, None) => Value::Handle(0),
            (Ret::Int, _) => Value::Integer(0),
            (Ret::Real, _) => Value::Real(F32::ZERO),
            (Ret::Text, _) => Value::String(ctx.intern("")),
        };
        NativeResult::Return(v)
    });
}

/// A typed spell target getter: the target handle, when it is an object of `kind`.
fn register_spell_target(
    reg: &mut NativeRegistry,
    game: &Rc<RefCell<Game>>,
    name: &str,
    kind: HandleKind,
) {
    let g = Rc::clone(game);
    reg.register(name, move |ctx, _| {
        let mut gm = g.borrow_mut();
        let id = match response(&gm, ctx.thread_key(), Some(SPELL_IDS), Field::SpellTarget) {
            Some(Datum::Object(k)) if gm.object(k).handle_kind() == kind => gm.handle_or_create(k),
            _ => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });
}

/// The firing of the running thread, if it was fired through [`fire_event`].
fn firing_of(gm: &Game, thread: u32) -> Option<&Firing> {
    match gm.context(thread) {
        Some(Ctx::Trigger {
            firing: Some(i), ..
        }) => gm.firings.get(i as usize),
        _ => None,
    }
}

/// The value of `field` in the running firing, when the event id is in `ids` (`None`: any event id).
fn response(gm: &Game, thread: u32, ids: Option<&[u32]>, field: Field) -> Option<Datum> {
    if let Some(ids) = ids {
        let id = super::unit_event::current_event_id(gm, thread)?;
        if !ids.contains(&id) {
            return None;
        }
    }
    let f = firing_of(gm, thread)?;
    f.data
        .iter()
        .find(|(f, _)| *f == field)
        .map(|(_, d)| d.clone())
}

/// The handle id of a value: objects get their handle (created when needed), players their player handle.
fn datum_handle(gm: &mut Game, d: &Datum) -> u32 {
    match d {
        Datum::Object(k) => gm.handle_or_create(*k),
        Datum::Player(p) => gm.player_handle(*p),
        _ => 0,
    }
}

fn trigger_arg(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.handle_of_kind(handle_arg(a, i), HandleKind::Trigger)
}

fn player_arg(gm: &Game, a: &[Value], i: usize) -> Option<u32> {
    gm.player_index_of_handle(handle_arg(a, i))
}

fn unit_arg(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.unit_of_handle(handle_arg(a, i))
}

fn handle_kind_arg(gm: &Game, a: &[Value], i: usize, kind: HandleKind) -> Option<ObjKey> {
    gm.handle_of_kind(handle_arg(a, i), kind)
}

/// The boolexpr of argument `i`. A null handle is no filter (LIKELY); an invalid one is treated the same way.
fn boolexpr_arg(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.handle_of_kind(handle_arg(a, i), HandleKind::BoolExpr)
}

fn handle_arg(a: &[Value], i: usize) -> u32 {
    a.get(i).and_then(|v| v.as_handle()).unwrap_or(0)
}

fn real_arg(a: &[Value], i: usize) -> F32 {
    a.get(i).and_then(|v| v.as_real()).unwrap_or(F32::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operators_follow_the_limitop_order() {
        let (a, b) = (F32::from_bits(0x4120_0000), F32::from_bits(0x4180_0000)); // 10, 16
        assert!(compare(0, a, b));
        assert!(compare(1, a, a));
        assert!(!compare(2, a, b));
        assert!(compare(3, b, a));
        assert!(compare(4, b, a));
        assert!(compare(5, a, b));
        assert!(!compare(6, a, b));
    }

    #[test]
    fn chat_rule_exact_or_contained() {
        assert!(chat_matches("-GO", "-go", true));
        assert!(!chat_matches("-go 3", "-go", true));
        assert!(chat_matches("-go 3", "-go", false));
        assert!(!chat_matches("hello", "-go", false));
    }
}
