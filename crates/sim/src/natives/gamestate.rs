//! Game-state natives: game speed, difficulty, placement, game type and map flags, the density settings,
//! the float and integer game-state slots, the map name and description, the pause, end, restart and reload
//! requests, the cheat flags, the version, the tournament values, and the campaign availability setters.
//!
//! Model. The original keeps these values in its game-state object (`DAT_6fab65f4`, the global at
//! `0x6FAB65F4`, field `+0x30`). The offsets are read from the natives that access them: game type supported
//! `+0x30`, game type selected `+0x34`, map flags `+0x38`, creature density `+0x49`, resource density `+0x4a`,
//! game difficulty `+0x46`, game speed `+0x47`, placement `+0x48`. The model stores the same values in
//! [`GameState`]. A host sets the ones that a map or a lobby sets (see the setters on [`Game`]).
//!
//! Behaviour notes. Addresses are the 1.26a `Game.dll` natives (`docs/spec/natives-1.26a.tsv`). Labels: CONFIRMED =
//! read in the decompile of the native (`natives.c`) or of the helper it calls, LIKELY = strong inference from
//! the code shape, UNSURE = guessed or not read. The table in the module-level section below lists each native.
//!
//! | Native | Address | Status | Notes |
//! |---|---|---|---|
//! | `GetGameSpeed` | 0x6f3baec0 | CONFIRMED | field `+0x47`; default UNSURE (set by the map). |
//! | `SetGameSpeed` | 0x6f3baed0 | CONFIRMED gate, LIKELY store | no effect while map flag `0x4000` is set (`+0x38`). The inner setter (`0x6f3db270`) is not read. |
//! | `GetGameDifficulty` | 0x6f3baf20 | CONFIRMED | field `+0x46`. |
//! | `SetGameDifficulty` | 0x6f3baf10 | CONFIRMED | field `+0x46`. |
//! | `GetDefaultDifficulty` | 0x6f3b33a0 | CONFIRMED structure, LIKELY value | returns 1 (normal) when the lobby conditions fail; the in-game branch is not read. |
//! | `SetDefaultDifficulty` | 0x6f3b33d0 | CONFIRMED gate, UNSURE store | the original only writes under the lobby conditions; the model always stores. |
//! | `GetAIDifficulty` | 0x6f3cb7e0 | CONFIRMED structure | per player (the player field `+0x26c`); the model keeps a per-slot value set by the host. |
//! | `GetGamePlacement` | 0x6f3baf00 | CONFIRMED | field `+0x48`. |
//! | `SetGamePlacement` | 0x6f3baef0 | CONFIRMED | field `+0x48`. |
//! | `GetGameTypeSelected` | 0x6f3bae60 | CONFIRMED | `+0x34` with bit `0x8` cleared. |
//! | `IsGameTypeSupported` | 0x6f3bae40 | CONFIRMED | bit test on `+0x30`. |
//! | `SetGameTypeSupported` | 0x6f3bae10 | CONFIRMED | bit set or clear on `+0x30`. |
//! | `IsMapFlagSet` | 0x6f3baea0 | CONFIRMED | bit test on `+0x38`. |
//! | `SetMapFlag` | 0x6f3bae70 | CONFIRMED | bit set or clear on `+0x38`. |
//! | `GetResourceDensity` | 0x6f3baf40 | CONFIRMED | field `+0x4a`. |
//! | `SetResourceDensity` | 0x6f3baf30 | CONFIRMED | field `+0x4a`. |
//! | `GetCreatureDensity` | 0x6f3baf60 | CONFIRMED | field `+0x49`. |
//! | `SetCreatureDensity` | 0x6f3baf50 | CONFIRMED | field `+0x49`. |
//! | `GetFloatGameState` | 0x6f3bc310 | CONFIRMED index range (only index 2 is read) | returns 0.0 for another index. |
//! | `SetFloatGameState` | 0x6f3bc2e0 | CONFIRMED index range | stores for index 2 only. |
//! | `GetIntegerGameState` | 0x6f3bc380 | CONFIRMED index range (0 and 1) | returns 0 otherwise. |
//! | `SetIntegerGameState` | 0x6f3bc350 | CONFIRMED index range (0 and 1) | stores for indices 0 and 1 only. |
//! | `SetMapName` | 0x6f3bada0 | LIKELY | stored text; the helper that writes it is not read. |
//! | `SetMapDescription` | 0x6f3badc0 | LIKELY | stored text; same helper as the name. |
//! | `GetSaveBasicFilename` | 0x6f3c1f80 | UNSURE | returns "" (no save is loaded in the model). |
//! | `CopySaveGame` | 0x6f3bc180 | UNSURE | returns false: the model has no save files. |
//! | `EndGame` | 0x6f3bbbb0 | CONFIRMED store of the result, LIKELY effect | stores the result flag; the end-of-game helper (`0x6f39f240`) is not read. |
//! | `RestartGame` | 0x6f3bbc00 | CONFIRMED gate, LIKELY effect | logged as a request (only when not in a menu). |
//! | `ReloadGame` | 0x6f3bbbe0 | CONFIRMED gate, LIKELY effect | logged as a request (only when not in a menu). |
//! | `PauseGame` | 0x6f3bc4d0 | LIKELY | stored flag; the original sends a command (`0x6f2fa7e0`), not read. |
//! | `DisableRestartMission` | 0x6f3b38c0 | UNSURE | stored flag (the helper `0x6f2f6b20` is not read). |
//! | `SuspendTimeOfDay` | 0x6f3bc460 | LIKELY | stored flag (the time-of-day helpers are not read). |
//! | `IsNoVictoryCheat` | 0x6f3bc8c0 | CONFIRMED read of flag `0x80` | stored flag, default false (set only by the host). |
//! | `IsNoDefeatCheat` | 0x6f3bc8f0 | CONFIRMED read of flag `0x40` | stored flag, default false (set only by the host). |
//! | `VersionGet` | 0x6f3b32f0 | LIKELY | the version is set at start-up (the static value is -1); the model returns 1 (`VERSION_FROZEN_THRONE`), UNSURE. |
//! | `VersionCompatible` | 0x6f3b3300 | UNSURE | the helper (`0x6f0098f0`) reads a version table that is filled at start-up. The model: 0 and the current version. |
//! | `VersionSupported` | 0x6f3b3310 | UNSURE | the helper (`0x6f009910`) was skipped in the decompile. The model: the same as compatible. |
//! | `GetTournamentFinishSoonTimeRemaining` | 0x6f3c1ea0 | CONFIRMED structure | 0.0 unless the tournament phase is `0x101`. |
//! | `GetTournamentFinishNowRule` | 0x6f3c1ed0 | CONFIRMED structure | 0 unless the phase is `0x102`. |
//! | `GetTournamentFinishNowPlayer` | 0x6f3c1f00 | CONFIRMED structure | null unless the phase is `0x102`; the player handle has flag 0. |
//! | `GetTournamentScore` | 0x6f3c1f60 | CONFIRMED structure, UNSURE value | 0 unless the host sets a score. |
//! | `SetCampaignAvailable` | 0x6f3b3370 | CONFIRMED gate, UNSURE store | stored; the campaign menu is not modelled. |
//! | `SetMissionAvailable` | 0x6f3b3340 | CONFIRMED gate, UNSURE store | stored. |
//! | `SetOpCinematicAvailable` | 0x6f3b3400 | CONFIRMED gate, UNSURE store | stored. |
//! | `SetEdCinematicAvailable` | 0x6f3b3460 | CONFIRMED gate, UNSURE store | stored. |
//! | `SetTutorialCleared` | 0x6f3b3430 | CONFIRMED gate, UNSURE store | stored. |
//! | `SetCampaignMenuRace` | 0x6f3bc3d0 | CONFIRMED gate, UNSURE store | stored (only when the version global is 0). |
//! | `SetCampaignMenuRaceEx` | 0x6f3bc3b0 | UNSURE store | stored. |
//! | `ForceCampaignSelectScreen` | 0x6f3bc400 | LIKELY | logged as a request. |
//!
//! Not in this module: `GetWorldBounds` (the rect comes from the world extents, the helpers were not read), `Cheat`
//! (needs the cheat table), `GetWinningPlayer` (its helper `0x6f4280b0` was not read), `ExecuteFunc` (needs the VM's
//! function table by name).
//!
//! The campaign and cinematic setters do not depend on units, combat or orders. The values are kept so that a host
//! can read them back. The tests are in `tests/gamestate_natives.rs`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use jass::vm::{NativeCtx, NativeRegistry, NativeResult, Value};
use softfloat::F32;

use super::game::{Game, PLAYER_SLOTS};
use super::hashtable::{bool_at, int_at, real_at, str_at};
use super::misc::MiscEvent;
use super::player::index_at;

/// `SetGameSpeed` is ignored while this map flag (`+0x38` bit 14) is set.
const FLAG_SPEED_LOCKED: u32 = 0x4000;
/// The game-type bit that `GetGameTypeSelected` clears (`0xfffffff7`).
const GAME_TYPE_MASK: u32 = !0x8;
/// The version `VersionGet` returns in the model: `VERSION_FROZEN_THRONE` (1, UNSURE: set by the host).
pub const VERSION_FROZEN_THRONE: i32 = 1;
/// The float game-state index that the natives accept: only index 2 (`GAME_STATE_TIME_OF_DAY`), per the range
/// check `1 < index < 3`.
const FLOAT_STATE_INDEX: i32 = 2;
/// The tournament phases the getters read (CONFIRMED values from the decompile).
pub const TOURNAMENT_FINISH_SOON: u32 = 0x101;
pub const TOURNAMENT_FINISH_NOW: u32 = 0x102;

/// The campaign and cinematic availability flags the setters write. Keys name the setter and its arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CampaignKey {
    /// `SetCampaignAvailable(index, flag)`.
    Campaign(i32),
    /// `SetMissionAvailable(campaign, mission, flag)`.
    Mission(i32, i32),
    /// `SetOpCinematicAvailable(index, flag)`.
    OpCinematic(i32),
    /// `SetEdCinematicAvailable(index, flag)`.
    EdCinematic(i32),
    /// `SetTutorialCleared(flag)`.
    TutorialCleared,
}

/// The tournament values the getters read. The model holds them as the host sets them; the default is "no
/// tournament", which makes every getter return its zero.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tournament {
    /// The phase: 0 (none), [`TOURNAMENT_FINISH_SOON`] or [`TOURNAMENT_FINISH_NOW`].
    pub phase: u32,
    /// Seconds before the finish (read in the finish-soon phase).
    pub finish_soon_remaining: F32,
    /// The finish-now rule (read in the finish-now phase).
    pub finish_now_rule: i32,
    /// The slot that the finish-now rule names, if any.
    pub finish_now_player: Option<u32>,
    /// The score per player slot.
    pub scores: [i32; PLAYER_SLOTS],
}

/// The game-state values. Defaults are the model's values, documented per field (UNSURE where the map decides).
#[derive(Debug, Clone, PartialEq)]
pub struct GameState {
    /// Game speed (`+0x47`). 0 slowest to 4 fastest. Default 2 (normal, UNSURE).
    pub speed: i32,
    /// Game difficulty (`+0x46`). 0 easy to 3 insane. Default 1 (normal, UNSURE).
    pub difficulty: i32,
    /// Default difficulty for the next game (lobby value). Default 1 (UNSURE).
    pub default_difficulty: i32,
    /// AI difficulty per player slot. Default 0 (UNSURE).
    pub ai_difficulty: [i32; PLAYER_SLOTS],
    /// Placement (`+0x48`). Default 0 (UNSURE).
    pub placement: i32,
    /// Creature density (`+0x49`). Default 0 (UNSURE).
    pub creature_density: i32,
    /// Resource density (`+0x4a`). Default 0 (UNSURE).
    pub resource_density: i32,
    /// The selected game type (`+0x34`, bit 3 cleared when read). Default 0 (UNSURE).
    pub game_type_selected: u32,
    /// The supported game types (`+0x30`). Default 0 (UNSURE).
    pub game_types_supported: u32,
    /// Map flags (`+0x38`). Default 0 (UNSURE: the map sets them).
    pub map_flags: u32,
    /// The float game-state value at index 2 (time of day). Default 0.0.
    pub float_state: F32,
    /// The integer game-state values at indices 0 and 1. Default 0.
    pub int_states: [i32; 2],
    /// Set by `PauseGame`.
    pub paused: bool,
    /// Set by `SuspendTimeOfDay(true)`, cleared by `SuspendTimeOfDay(false)`.
    pub time_of_day_suspended: bool,
    /// Set by `DisableRestartMission`.
    pub restart_mission_disabled: bool,
    /// The cheat flags (`IsNoVictoryCheat`, `IsNoDefeatCheat`). Set by the host only.
    pub no_victory_cheat: bool,
    pub no_defeat_cheat: bool,
    /// The result that `EndGame` gave (`Some(true)` for a victory or `Some(false)` for a defeat, as the argument
    /// was passed). `None` while the game runs.
    pub ended: Option<bool>,
    /// `SetMapName` and `SetMapDescription`.
    pub map_name: String,
    pub map_description: String,
    /// `VersionGet`. Default [`VERSION_FROZEN_THRONE`] (UNSURE).
    pub version: i32,
    /// The tournament values.
    pub tournament: Tournament,
    /// The campaign flags written by the setters.
    pub campaign: BTreeMap<CampaignKey, bool>,
    /// The campaign menu race (`SetCampaignMenuRace` and its `Ex` form, raw value).
    pub campaign_menu_race: i32,
}

impl Default for GameState {
    fn default() -> Self {
        Self::new()
    }
}

impl GameState {
    /// The model's defaults (see the field docs).
    pub fn new() -> Self {
        GameState {
            speed: 2,
            difficulty: 1,
            default_difficulty: 1,
            ai_difficulty: [0; PLAYER_SLOTS],
            placement: 0,
            creature_density: 0,
            resource_density: 0,
            game_type_selected: 0,
            game_types_supported: 0,
            map_flags: 0,
            float_state: F32::ZERO,
            int_states: [0; 2],
            paused: false,
            time_of_day_suspended: false,
            restart_mission_disabled: false,
            no_victory_cheat: false,
            no_defeat_cheat: false,
            ended: None,
            map_name: String::new(),
            map_description: String::new(),
            version: VERSION_FROZEN_THRONE,
            tournament: Tournament::default(),
            campaign: BTreeMap::new(),
            campaign_menu_race: 0,
        }
    }
}

impl Game {
    /// The game-state values (read only).
    pub fn game_state(&self) -> &GameState {
        &self.game_state
    }

    /// Mutable access to the game-state values, for the host (map setup, lobby).
    pub fn game_state_mut(&mut self) -> &mut GameState {
        &mut self.game_state
    }
}

/// Registers the game-state natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    // Speed and difficulty.
    native(reg, game, "GetGameSpeed", |gm, _, _| {
        Value::Integer(gm.game_state.speed)
    });
    native(reg, game, "SetGameSpeed", |gm, _, a| {
        if gm.game_state.map_flags & FLAG_SPEED_LOCKED == 0 {
            gm.game_state.speed = int_at(a, 0);
        }
        Value::Null
    });
    native(reg, game, "GetGameDifficulty", |gm, _, _| {
        Value::Integer(gm.game_state.difficulty)
    });
    native(reg, game, "SetGameDifficulty", |gm, _, a| {
        gm.game_state.difficulty = int_at(a, 0);
        Value::Null
    });
    native(reg, game, "GetDefaultDifficulty", |gm, _, _| {
        Value::Integer(gm.game_state.default_difficulty)
    });
    native(reg, game, "SetDefaultDifficulty", |gm, _, a| {
        gm.game_state.default_difficulty = int_at(a, 0);
        Value::Null
    });
    native(reg, game, "GetAIDifficulty", |gm, _, a| {
        // A null player gives 0 (the original's default).
        match index_at(gm, a, 0) {
            Some(p) => Value::Integer(gm.game_state.ai_difficulty[p as usize]),
            None => Value::Integer(0),
        }
    });

    // Placement, game type and map flags.
    native(reg, game, "GetGamePlacement", |gm, _, _| {
        Value::Integer(gm.game_state.placement)
    });
    native(reg, game, "SetGamePlacement", |gm, _, a| {
        gm.game_state.placement = int_at(a, 0);
        Value::Null
    });
    native(reg, game, "GetGameTypeSelected", |gm, _, _| {
        Value::Integer((gm.game_state.game_type_selected & GAME_TYPE_MASK) as i32)
    });
    native(reg, game, "IsGameTypeSupported", |gm, _, a| {
        let bits = int_at(a, 0) as u32;
        Value::Boolean(gm.game_state.game_types_supported & bits != 0)
    });
    native(reg, game, "SetGameTypeSupported", |gm, _, a| {
        let bits = int_at(a, 0) as u32;
        if bool_at(a, 1) {
            gm.game_state.game_types_supported |= bits;
        } else {
            gm.game_state.game_types_supported &= !bits;
        }
        Value::Null
    });
    native(reg, game, "IsMapFlagSet", |gm, _, a| {
        let bits = int_at(a, 0) as u32;
        Value::Boolean(gm.game_state.map_flags & bits != 0)
    });
    native(reg, game, "SetMapFlag", |gm, _, a| {
        let bits = int_at(a, 0) as u32;
        if bool_at(a, 1) {
            gm.game_state.map_flags |= bits;
        } else {
            gm.game_state.map_flags &= !bits;
        }
        Value::Null
    });

    // Densities.
    native(reg, game, "GetResourceDensity", |gm, _, _| {
        Value::Integer(gm.game_state.resource_density)
    });
    native(reg, game, "SetResourceDensity", |gm, _, a| {
        gm.game_state.resource_density = int_at(a, 0);
        Value::Null
    });
    native(reg, game, "GetCreatureDensity", |gm, _, _| {
        Value::Integer(gm.game_state.creature_density)
    });
    native(reg, game, "SetCreatureDensity", |gm, _, a| {
        gm.game_state.creature_density = int_at(a, 0);
        Value::Null
    });

    // Game-state slots.
    native(reg, game, "GetFloatGameState", |gm, _, a| {
        let i = int_at(a, 0);
        let v = if i == FLOAT_STATE_INDEX {
            gm.game_state.float_state
        } else {
            F32::ZERO
        };
        Value::Real(v)
    });
    native(reg, game, "SetFloatGameState", |gm, _, a| {
        if int_at(a, 0) == FLOAT_STATE_INDEX {
            gm.game_state.float_state = real_at(a, 1);
        }
        Value::Null
    });
    native(reg, game, "GetIntegerGameState", |gm, _, a| {
        let i = int_at(a, 0);
        let v = usize::try_from(i)
            .ok()
            .and_then(|i| gm.game_state.int_states.get(i).copied())
            .unwrap_or(0);
        Value::Integer(v)
    });
    native(reg, game, "SetIntegerGameState", |gm, _, a| {
        let i = int_at(a, 0);
        if let Some(slot) = usize::try_from(i)
            .ok()
            .and_then(|i| gm.game_state.int_states.get_mut(i))
        {
            *slot = int_at(a, 1);
        }
        Value::Null
    });

    // Map name and description, save files.
    native(reg, game, "SetMapName", |gm, ctx, a| {
        gm.game_state.map_name = str_at(ctx, a, 0);
        Value::Null
    });
    native(reg, game, "SetMapDescription", |gm, ctx, a| {
        gm.game_state.map_description = str_at(ctx, a, 0);
        Value::Null
    });
    native(reg, game, "GetSaveBasicFilename", |_, ctx, _| {
        Value::String(ctx.intern(""))
    });
    native(reg, game, "CopySaveGame", |gm, _, _| {
        gm.misc.log.push(MiscEvent::CopySaveGame);
        Value::Boolean(false)
    });

    // Game flow.
    native(reg, game, "EndGame", |gm, _, a| {
        let result = bool_at(a, 0);
        gm.game_state.ended = Some(result);
        gm.misc.log.push(MiscEvent::EndGame { result });
        Value::Null
    });
    native(reg, game, "RestartGame", |gm, _, a| {
        gm.misc.log.push(MiscEvent::RestartGame {
            keep_settings: bool_at(a, 0),
        });
        Value::Null
    });
    native(reg, game, "ReloadGame", |gm, _, _| {
        gm.misc.log.push(MiscEvent::ReloadGame);
        Value::Null
    });
    native(reg, game, "PauseGame", |gm, _, a| {
        let paused = bool_at(a, 0);
        gm.game_state.paused = paused;
        gm.misc.log.push(MiscEvent::PauseGame { paused });
        Value::Null
    });
    native(reg, game, "DisableRestartMission", |gm, _, a| {
        gm.game_state.restart_mission_disabled = bool_at(a, 0);
        Value::Null
    });
    native(reg, game, "SuspendTimeOfDay", |gm, _, a| {
        gm.game_state.time_of_day_suspended = bool_at(a, 0);
        Value::Null
    });
    native(reg, game, "IsNoVictoryCheat", |gm, _, _| {
        Value::Boolean(gm.game_state.no_victory_cheat)
    });
    native(reg, game, "IsNoDefeatCheat", |gm, _, _| {
        Value::Boolean(gm.game_state.no_defeat_cheat)
    });

    // Version.
    native(reg, game, "VersionGet", |gm, _, _| {
        Value::Integer(gm.game_state.version)
    });
    native(reg, game, "VersionCompatible", |gm, _, a| {
        Value::Boolean(version_ok(gm, int_at(a, 0)))
    });
    native(reg, game, "VersionSupported", |gm, _, a| {
        Value::Boolean(version_ok(gm, int_at(a, 0)))
    });

    // Tournament (zero unless the host sets a phase).
    native(
        reg,
        game,
        "GetTournamentFinishSoonTimeRemaining",
        |gm, _, _| {
            let t = &gm.game_state.tournament;
            let v = if t.phase == TOURNAMENT_FINISH_SOON {
                t.finish_soon_remaining
            } else {
                F32::ZERO
            };
            Value::Real(v)
        },
    );
    native(reg, game, "GetTournamentFinishNowRule", |gm, _, _| {
        let t = &gm.game_state.tournament;
        let v = if t.phase == TOURNAMENT_FINISH_NOW {
            t.finish_now_rule
        } else {
            0
        };
        Value::Integer(v)
    });
    native(reg, game, "GetTournamentFinishNowPlayer", |gm, _, _| {
        let t = &gm.game_state.tournament;
        match (t.phase == TOURNAMENT_FINISH_NOW, t.finish_now_player) {
            (true, Some(p)) => Value::Handle(gm.player_handle(p)),
            _ => Value::Handle(0),
        }
    });
    native(reg, game, "GetTournamentScore", |gm, _, a| {
        match index_at(gm, a, 0) {
            Some(p) => Value::Integer(gm.game_state.tournament.scores[p as usize]),
            None => Value::Integer(0),
        }
    });

    // Campaign and cinematic availability (stored; the campaign menu is not modelled).
    native(reg, game, "SetCampaignAvailable", |gm, _, a| {
        set_campaign(gm, CampaignKey::Campaign(int_at(a, 0)), bool_at(a, 1));
        Value::Null
    });
    native(reg, game, "SetMissionAvailable", |gm, _, a| {
        let key = CampaignKey::Mission(int_at(a, 0), int_at(a, 1));
        set_campaign(gm, key, bool_at(a, 2));
        Value::Null
    });
    native(reg, game, "SetOpCinematicAvailable", |gm, _, a| {
        set_campaign(gm, CampaignKey::OpCinematic(int_at(a, 0)), bool_at(a, 1));
        Value::Null
    });
    native(reg, game, "SetEdCinematicAvailable", |gm, _, a| {
        set_campaign(gm, CampaignKey::EdCinematic(int_at(a, 0)), bool_at(a, 1));
        Value::Null
    });
    native(reg, game, "SetTutorialCleared", |gm, _, a| {
        set_campaign(gm, CampaignKey::TutorialCleared, bool_at(a, 0));
        Value::Null
    });
    native(reg, game, "SetCampaignMenuRace", |gm, _, a| {
        gm.game_state.campaign_menu_race = int_at(a, 0);
        Value::Null
    });
    native(reg, game, "SetCampaignMenuRaceEx", |gm, _, a| {
        gm.game_state.campaign_menu_race = int_at(a, 0);
        Value::Null
    });
    native(reg, game, "ForceCampaignSelectScreen", |gm, _, _| {
        gm.misc.log.push(MiscEvent::ForceCampaignSelect);
        Value::Null
    });
}

/// Stores a campaign flag. Writes to the map are not logged: the flags have no getter in the natives.
fn set_campaign(gm: &mut Game, key: CampaignKey, flag: bool) {
    gm.game_state.campaign.insert(key, flag);
}

/// Whether version `v` is accepted by `VersionCompatible` and `VersionSupported`: the first version (RoC, 0) and the
/// current one (UNSURE, see the module table).
fn version_ok(gm: &Game, v: i32) -> bool {
    v == 0 || v == gm.game_state.version
}

/// Registers one native whose body is `f`. `f` gets the game, the native context (for strings) and the arguments.
pub(super) fn native<F>(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>, name: &str, f: F)
where
    F: Fn(&mut Game, &mut NativeCtx<'_>, &[Value]) -> Value + 'static,
{
    let g = Rc::clone(game);
    reg.register(name, move |ctx: &mut NativeCtx<'_>, a: &[Value]| {
        let mut gm = g.borrow_mut();
        NativeResult::Return(f(&mut gm, ctx, a))
    });
}
