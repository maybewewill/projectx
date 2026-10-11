//! AI natives: the main-VM side (`StartMeleeAI`, `StartCampaignAI`, `CommandAI`, `PauseCompAI`) and the 124 natives
//! of the AI script table (`docs/spec/natives-1.26a.tsv`, table `ai`), which the AI VMs call.
//!
//! The real bodies are registered by `0x6F2E3D90` (the table `ai` of the spec has their addresses; the older
//! stub at `0x6F71D9D0` is a `xor eax, eax; ret` used for checking common.ai syntax, and is not the behaviour). The
//! bodies were read from a Ghidra decompile of `Game.dll` (kept outside the repo; only the behaviour is recorded here).
//! Every AI native
//! body first gets the AI state of the calling player (`FUN_6f4786b0`) and then either writes a field of that state
//! or calls an engine routine on it. The state fields that this model keeps are: the command stack (`+0x1DC`
//! count, `+0x1E4` commands, `+0x218` data), the flag word (`+0x2D0`), the replacement count (`+0x1D4`), the code
//! setting `SetHeroLevels` (`+0x40`), the player index (`+0x24`) and the captain record flag word (`+0x6C`, reached
//! through `FUN_6f2cfeb0`).
//!
//! Where a body calls an engine routine that the model does not hold (towns, expansions, captain groups, vision,
//! creeps, production), the row is logged and returns the value the body fixes, or is NOT IMPLEMENTED with the
//! reason. A native with `implemented: false` is registered too: a call logs its name in
//! `AiShared::unimplemented` and returns the default for its return type, so the AI thread goes on.
//!
//! Levels: CONFIRMED (the body shows the value or effect), LIKELY (the mechanism is read, a detail is inferred),
//! UNSURE (the body could not be decoded fully, or the model chose a value).
//!
//! Units are counted by scanning the game's objects (O(objects) per call). That is acceptable for the model and is
//! noted as a cost, not a semantic.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{clamp_sleep, NativeCtx, NativeRegistry, NativeResult, Value};
use softfloat::F32;

use super::game::{Game, ObjKey, Object, PLAYER_SLOTS};
use crate::ai::{
    AiArg, AiCommand, AiEvent, AiShared, SharedAi, StartKind, StartRequest, COMMAND_QUEUE_LIMIT,
};

/// The level of a row's knowledge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Read in the binary: the value or the effect is shown by the body.
    Confirmed,
    /// The mechanism is read; a detail is inferred from the code shape or the usual use.
    Likely,
    /// Not fully decoded, or the model chose the value.
    Unsure,
}

/// One row of a native module table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiNative {
    /// The native's name, as scripts call it.
    pub name: &'static str,
    /// The signature, as in `docs/spec/natives-1.26a.tsv`.
    pub sig: &'static str,
    /// The address of the real body (`docs/spec/natives-1.26a.tsv`), 0 when not recorded.
    pub addr: u32,
    /// The level of the model's behaviour for this native.
    pub level: Level,
    /// Whether the model acts on the call (`false`: logged as not implemented).
    pub implemented: bool,
    /// What the model does, or why it does not.
    pub note: &'static str,
}

/// The bits of the AI flag word (`+0x2D0`) that the boolean setters change (CONFIRMED: each setter sets or clears
/// one bit of that word).
pub const FLAG_BITS: &[(&str, u32)] = &[
    ("SetNewHeroes", 0x4),
    ("SetHeroesFlee", 0x8),
    ("SetUnitsFlee", 0x10),
    ("SetGroupsFlee", 0x20),
    ("SetSlowChopping", 0x40),
    ("SetCaptainChanges", 0x80),
    ("GroupTimedLife", 0x100),
    ("SetHeroesTakeItems", 0x400),
    ("SetPeonsRepair", 0x800),
    ("SetIgnoreInjured", 0x1000),
    ("SetWatchMegaTargets", 0x2000),
    ("SetRandomPaths", 0x8_0000),
    ("SetDefendPlayer", 0x20_0000),
    ("SetHeroesBuyItems", 0x40_0000),
];

/// The melee bit of the flag word: `SetMeleeAI` sets it, `SetCampaignAI` clears it (CONFIRMED).
pub const MELEE_BIT: u32 = 0x4000;
/// `MeleeDifficulty` reads these bits (CONFIRMED mapping): the body tests `0x8000` first and returns 1; otherwise a
/// set `0x10000` (the `0x18000` mask, tested after `0x8000`) gives 3; otherwise 2. The engine sets the bits; no
/// native here does, so the model sets them only when a host does.
pub const DIFFICULTY_EASY_BIT: u32 = 0x8000;
/// See [`DIFFICULTY_EASY_BIT`].
pub const DIFFICULTY_HARD_BIT: u32 = 0x1_0000;

/// Captain record bits (`+0x6C`): full, retreating, pathing disabled (`DisablePathing`), amphibious (`SetAmphibious`).
pub const CAPTAIN_FULL: u32 = 0x1;
/// See [`CAPTAIN_FULL`].
pub const CAPTAIN_RETREATING: u32 = 0x2;
/// See [`CAPTAIN_FULL`].
pub const CAPTAIN_NO_PATHING: u32 = 0x4000;
/// See [`CAPTAIN_FULL`].
pub const CAPTAIN_AMPHIBIOUS: u32 = 0x8000;

/// The main-VM natives that concern the AI, with their levels.
pub const MAIN_NATIVES: &[AiNative] = &[
    AiNative { name: "StartMeleeAI", sig: "(Hplayer;S)V", addr: 0x6F3C_B740, level: Level::Confirmed, implemented: true, note: "gate and helper calls read (decompile, earlier pass); the start is a request served by AiHost::step" },
    AiNative { name: "StartCampaignAI", sig: "(Hplayer;S)V", addr: 0x6F3C_B770, level: Level::Confirmed, implemented: true, note: "same body as StartMeleeAI (decompile, earlier pass); the kind is recorded" },
    AiNative { name: "CommandAI", sig: "(Hplayer;II)V", addr: 0x6F3C_B7A0, level: Level::Confirmed, implemented: true, note: "pushes (command, data) on the AI's command stack (FUN_6f2d4990: pushes while the count is not 12, else drops)" },
    AiNative { name: "PauseCompAI", sig: "(Hplayer;B)V", addr: 0x6F3C_B7C0, level: Level::Likely, implemented: true, note: "counter of the player's AI: +1 for true, -1 for false (not below 0: UNSURE); a count above 0 skips its step (UNSURE, see ai.rs)" },
];

/// The 124 AI natives of table `ai`, in the order of the table. Generated from `docs/spec/natives-1.26a.tsv`, with the
/// levels and notes of the real bodies (see the module docs).
pub const AI_NATIVES: &[AiNative] = &[
    AiNative { name: "DebugS", sig: "(S)V", addr: 0x6F2D_C0A0, level: Level::Confirmed, implemented: true, note: "empty body; the call is recorded in the command log for tests, no game effect" },
    AiNative { name: "DebugFI", sig: "(SI)V", addr: 0x6F2D_C0B0, level: Level::Confirmed, implemented: true, note: "empty body; recorded only" },
    AiNative { name: "DebugUnitID", sig: "(SI)V", addr: 0x6F2D_C0C0, level: Level::Confirmed, implemented: true, note: "empty body; recorded only" },
    AiNative { name: "DisplayText", sig: "(IS)V", addr: 0x6F2D_C060, level: Level::Confirmed, implemented: true, note: "empty body; recorded only" },
    AiNative { name: "DisplayTextI", sig: "(ISI)V", addr: 0x6F2D_C070, level: Level::Confirmed, implemented: true, note: "empty body; recorded only" },
    AiNative { name: "DisplayTextII", sig: "(ISII)V", addr: 0x6F2D_C080, level: Level::Confirmed, implemented: true, note: "empty body; recorded only" },
    AiNative { name: "DisplayTextIII", sig: "(ISIII)V", addr: 0x6F2D_C090, level: Level::Confirmed, implemented: true, note: "empty body; recorded only" },
    AiNative { name: "SuicideUnit", sig: "(II)V", addr: 0x6F2D_E330, level: Level::Likely, implemented: true, note: "does nothing when the count is 0 or less (CONFIRMED); otherwise the engine sends up to count units of the type to suicide (FUN_6f2d4e00, not modelled): recorded" },
    AiNative { name: "SuicideUnitEx", sig: "(III)V", addr: 0x6F2D_E2E0, level: Level::Likely, implemented: true, note: "as SuicideUnit; the third argument goes to the unit record (+0x54) of each unit, not modelled: recorded" },
    AiNative { name: "Sleep", sig: "(R)V", addr: 0x6F2D_E380, level: Level::Likely, implemented: true, note: "hands the duration to the thread's agent timer (FUN_6f2d0f90: a virtual call with the duration); the model sleeps with clamp_sleep (the AI's clamp of a negative value: UNSURE)" },
    AiNative { name: "StartThread", sig: "(C)V", addr: 0x6F2D_E2A0, level: Level::Likely, implemented: true, note: "starts the code as a thread of this AI (FUN_6f2da590). The binary keeps 6 thread slots (+0x254, index 0 to 5) and drops a thread when none is free; the slot freeing rule is UNSURE, so no cap is enforced" },
    AiNative { name: "GetAiPlayer", sig: "()I", addr: 0x6F2D_CFE0, level: Level::Confirmed, implemented: true, note: "returns the AI state's player index (+0x24); -1 outside an AI context (not reachable from an AI VM)" },
    AiNative { name: "DoAiScriptDebug", sig: "()B", addr: 0x6F2D_C0D0, level: Level::Confirmed, implemented: true, note: "body is 'return 0': always false" },
    AiNative { name: "GetHeroId", sig: "()I", addr: 0x6F2D_DF50, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: looks up the AI's hero through the hero selection (FUN_6f0787d0) and the AI state (+0x30); the hero record is not modelled" },
    AiNative { name: "GetHeroLevelAI", sig: "()I", addr: 0x6F2D_E0E0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the body is (FUN_6f473170() - hero+0x90) + 2 on the AI's hero record; the hero record and FUN_6f473170 are not modelled" },
    AiNative { name: "SetHeroLevels", sig: "(C)V", addr: 0x6F2E_0B00, level: Level::Confirmed, implemented: true, note: "stores the code value in the AI state (+0x40); no getter native" },
    AiNative { name: "SetNewHeroes", sig: "(B)V", addr: 0x6F2E_3020, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x4 of the AI flag word (+0x2D0)" },
    AiNative { name: "GetUnitCount", sig: "(I)I", addr: 0x6F2D_D050, level: Level::Unsure, implemented: true, note: "body = units under construction of the type (FUN_6f2d1c10, not modelled: no construction state) + owned units with mask 0x36; the model counts the owned live units of the type" },
    AiNative { name: "GetPlayerUnitTypeCount", sig: "(Hplayer;I)I", addr: 0x6F2E_2B60, level: Level::Likely, implemented: true, note: "owned units of the type with mask 0x36 (CONFIRMED mask); 0 for a null or invalid player (CONFIRMED gate); the model counts owned live units" },
    AiNative { name: "GetTownUnitCount", sig: "(IIB)I", addr: 0x6F2D_D0C0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: body = units under construction (FUN_6f2d1c10) + units of the type inside the given town (FUN_6f2d5430); the town membership is not modelled" },
    AiNative { name: "GetUnitCountDone", sig: "(I)I", addr: 0x6F2D_D010, level: Level::Unsure, implemented: true, note: "body counts with mask 0x30 (finished units, CONFIRMED mask); the sim has no construction state, so this is the owned live count" },
    AiNative { name: "GetUpgradeLevel", sig: "(I)I", addr: 0x6F2D_D130, level: Level::Likely, implemented: true, note: "researched level of the tech; the body adds 1 when an entity with mask 2 of the tech exists (in-progress research: not modelled)" },
    AiNative { name: "GetUnitGoldCost", sig: "(I)I", addr: 0x6F2D_D1E0, level: Level::Confirmed, implemented: true, note: "the gold cost of the unit type (UnitType::gold_cost, the type record field +0x20); 0 for an unknown id. Research ids (uppercase first letter) give 0 here: their gate (FUN_6f473170) is not decoded" },
    AiNative { name: "GetUnitWoodCost", sig: "(I)I", addr: 0x6F2D_D240, level: Level::Confirmed, implemented: true, note: "the wood cost of the unit type (UnitType::wood_cost, the type record field +0x24); 0 for an unknown id; same research gate as the gold cost" },
    AiNative { name: "GetUnitBuildTime", sig: "(I)I", addr: 0x6F2D_D190, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the body searches the player's production records first (FUN_6f2d2fb0) and only then the type data; the production records are not modelled" },
    AiNative { name: "GetUpgradeGoldCost", sig: "(I)I", addr: 0x6F2D_D2A0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the body is base + increment * (level - 1) from the upgrade tables (FUN_6f2b0260); the sim holds no upgrade cost records" },
    AiNative { name: "GetUpgradeWoodCost", sig: "(I)I", addr: 0x6F2D_D2C0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the body is base + increment * (level - 1) from the upgrade tables (FUN_6f2b0280); the sim holds no upgrade cost records" },
    AiNative { name: "GetEnemyPower", sig: "()I", addr: 0x6F2D_C150, level: Level::Confirmed, implemented: true, note: "body is 'return 0': always 0" },
    AiNative { name: "GetMinesOwned", sig: "()I", addr: 0x6F2D_DFD0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: counts the mines of the AI's towns (FUN_6f2d11a0 over the four town slots); the town and mine model is not in the sim" },
    AiNative { name: "GetGoldOwned", sig: "()I", addr: 0x6F2D_E000, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the body sums a value over the AI's town slots (FUN_6f2d1210), not the player's gold state; the town model is not in the sim" },
    AiNative { name: "TownWithMine", sig: "()I", addr: 0x6F2D_E030, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "TownHasMine", sig: "(I)B", addr: 0x6F2D_E060, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "TownHasHall", sig: "(I)B", addr: 0x6F2D_E0A0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetNextExpansion", sig: "()I", addr: 0x6F2E_2BC0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetExpansionPeon", sig: "()Hunit;", addr: 0x6F2D_D530, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetEnemyExpansion", sig: "()Hunit;", addr: 0x6F2D_D4F0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "SetExpansion", sig: "(Hunit;I)B", addr: 0x6F2E_0570, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the result is the engine's expansion placement (FUN_6f2d8130); the town model is not in the sim" },
    AiNative { name: "GetBuilding", sig: "(Hplayer;)Hunit;", addr: 0x6F2E_2E80, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: searches the player's buildings through a callback (FUN_6f2a3270; the callback is not decoded)" },
    AiNative { name: "SetAllianceTarget", sig: "(Hunit;)V", addr: 0x6F2E_0630, level: Level::Likely, implemented: true, note: "stores the unit handle as the AI's alliance target (the body keeps a unit reference in the AI state)" },
    AiNative { name: "GetAllianceTarget", sig: "()Hunit;", addr: 0x6F2D_D570, level: Level::Likely, implemented: true, note: "returns the stored alliance target; null when none" },
    AiNative { name: "SetProduce", sig: "(III)B", addr: 0x6F2D_D450, level: Level::Unsure, implemented: true, note: "a count of 0 or less returns true (CONFIRMED: the loop does not run); a positive count returns the engine's production result (FUN_6f2d8130, not modelled): false; the request is recorded" },
    AiNative { name: "MergeUnits", sig: "(IIII)B", addr: 0x6F2D_D320, level: Level::Unsure, implemented: true, note: "true when the owned units of the fourth type already reach the count (CONFIRMED guard, with the pending count of the second type taken as 0: UNSURE); otherwise the merge (FUN_6f2d2580) is not modelled: false" },
    AiNative { name: "ConvertUnits", sig: "(II)B", addr: 0x6F2D_D2E0, level: Level::Unsure, implemented: true, note: "the body returns no value (CONFIRMED); the model returns false. The conversion (FUN_6f2d2440) is recorded" },
    AiNative { name: "SetUpgrade", sig: "(I)B", addr: 0x6F2D_D4B0, level: Level::Confirmed, implemented: true, note: "body returns 0 after the request (CONFIRMED): always false; the request is recorded" },
    AiNative { name: "Unsummon", sig: "(Hunit;)V", addr: 0x6F2E_08A0, level: Level::Likely, implemented: true, note: "unsummon request (FUN_6f2d2790) recorded; no game effect" },
    AiNative { name: "ClearHarvestAI", sig: "()V", addr: 0x6F2D_D5B0, level: Level::Likely, implemented: true, note: "harvest state reset (FUN_6f2d5e40) recorded; no game effect" },
    AiNative { name: "HarvestGold", sig: "(II)V", addr: 0x6F2D_D5E0, level: Level::Confirmed, implemented: true, note: "the request is made only when the count is above 0 (CONFIRMED guard); recorded" },
    AiNative { name: "HarvestWood", sig: "(II)V", addr: 0x6F2D_D630, level: Level::Confirmed, implemented: true, note: "the request is made only when the count is above 0 (CONFIRMED guard); recorded" },
    AiNative { name: "StopGathering", sig: "()V", addr: 0x6F2D_CE00, level: Level::Likely, implemented: true, note: "gathering stop (FUN_6f2d7710) recorded; no game effect" },
    AiNative { name: "AddGuardPost", sig: "(IRR)V", addr: 0x6F2D_CE30, level: Level::Likely, implemented: true, note: "guard post request (FUN_6f2daa40) recorded; no game effect" },
    AiNative { name: "FillGuardPosts", sig: "()V", addr: 0x6F2D_CF80, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "ReturnGuardPosts", sig: "()V", addr: 0x6F2D_CFB0, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "CreateCaptains", sig: "()V", addr: 0x6F2D_D960, level: Level::Likely, implemented: true, note: "captain creation (FUN_6f2da810) recorded; no game effect" },
    AiNative { name: "SetCaptainHome", sig: "(IRR)V", addr: 0x6F2D_D7B0, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "ResetCaptainLocs", sig: "()V", addr: 0x6F2D_D780, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "ShiftTownSpot", sig: "(RR)V", addr: 0x6F2D_D740, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "SetCaptainChanges", sig: "(B)V", addr: 0x6F2E_2D20, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x80 of the AI flag word (+0x2D0)" },
    AiNative { name: "TeleportCaptain", sig: "(RR)V", addr: 0x6F2D_D680, level: Level::Likely, implemented: true, note: "captain state checked (FUN_6f2cfeb0), then the engine move (FUN_6f2e6380): recorded" },
    AiNative { name: "ClearCaptainTargets", sig: "()V", addr: 0x6F2D_D6C0, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "CaptainVsUnits", sig: "(Hplayer;)V", addr: 0x6F2E_2BF0, level: Level::Likely, implemented: true, note: "player gate (valid player), then the engine's attack on the player's units (FUN_6f2d1520): recorded; the attack is not modelled" },
    AiNative { name: "CaptainVsPlayer", sig: "(Hplayer;)V", addr: 0x6F2E_2C80, level: Level::Likely, implemented: true, note: "as CaptainVsUnits, the other engine mode: recorded; the attack is not modelled" },
    AiNative { name: "CaptainAttack", sig: "(RR)V", addr: 0x6F2D_D700, level: Level::Likely, implemented: true, note: "captain state checked, then the engine attack order (FUN_6f2ed520): recorded" },
    AiNative { name: "GroupTimedLife", sig: "(B)V", addr: 0x6F2E_2D60, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x100 of the AI flag word (+0x2D0)" },
    AiNative { name: "CaptainGoHome", sig: "()V", addr: 0x6F2D_D800, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "CaptainIsHome", sig: "()B", addr: 0x6F2D_D870, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain group's home state (FUN_6f2d1080) is not modelled" },
    AiNative { name: "CaptainRetreating", sig: "()B", addr: 0x6F2D_D830, level: Level::Likely, implemented: true, note: "captain record bit 0x2 (CONFIRMED mask); no engine writer is modelled, so false" },
    AiNative { name: "CaptainIsFull", sig: "()B", addr: 0x6F2D_DB10, level: Level::Likely, implemented: true, note: "captain record bit 0x1 (CONFIRMED mask); no engine writer is modelled, so false" },
    AiNative { name: "CaptainIsEmpty", sig: "()B", addr: 0x6F2D_DB50, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain group size record (+0xBC) is not modelled" },
    AiNative { name: "CaptainGroupSize", sig: "()I", addr: 0x6F2D_DB90, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain group size record (+0xBC) is not modelled" },
    AiNative { name: "CaptainReadiness", sig: "()I", addr: 0x6F2D_DA40, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: readiness is computed by the captain group (FUN_6f2e91a0) from its units; not modelled" },
    AiNative { name: "CaptainReadinessHP", sig: "()I", addr: 0x6F2D_DA90, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: captain group readiness (FUN_6f2e91a0), not modelled" },
    AiNative { name: "CaptainReadinessMa", sig: "()I", addr: 0x6F2D_DAD0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: captain group readiness (FUN_6f2e91a0), not modelled" },
    AiNative { name: "CaptainInCombat", sig: "(B)B", addr: 0x6F2D_D8A0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain combat state (FUN_6f2d0ff0) is not modelled" },
    AiNative { name: "TownThreatened", sig: "()B", addr: 0x6F2D_D8E0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the town threat masks of the AI state (+0x164, +0x168) are not modelled" },
    AiNative { name: "CaptainAtGoal", sig: "()B", addr: 0x6F2D_D930, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain's goal point (FUN_6f2d1090) is not modelled" },
    AiNative { name: "CreepsOnMap", sig: "()B", addr: 0x6F2D_C0F0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: scans the units through a callback (not decoded), which needs the creep and ignore rules" },
    AiNative { name: "RemoveInjuries", sig: "()V", addr: 0x6F2D_DDA0, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "RemoveSiege", sig: "()V", addr: 0x6F2D_DE00, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "IsTowered", sig: "(Hunit;)B", addr: 0x6F2E_0950, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: counts the towers near the unit through the unit's vtable query; the body is true for more than one tower" },
    AiNative { name: "DisablePathing", sig: "()V", addr: 0x6F2E_06E0, level: Level::Confirmed, implemented: true, note: "sets bit 0x4000 of the captain record (+0x6C); recorded" },
    AiNative { name: "SetAmphibious", sig: "()V", addr: 0x6F2E_0720, level: Level::Confirmed, implemented: true, note: "sets bit 0x8000 of the captain record (+0x6C); recorded" },
    AiNative { name: "InitAssault", sig: "()V", addr: 0x6F2D_D990, level: Level::Likely, implemented: true, note: "assault start (FUN_6f2d4df0) recorded; no game effect" },
    AiNative { name: "AddAssault", sig: "(II)B", addr: 0x6F2D_D9C0, level: Level::Unsure, implemented: true, note: "the body returns no value (CONFIRMED); the model returns false. The request (FUN_6f2d96f0) is recorded" },
    AiNative { name: "AddDefenders", sig: "(II)B", addr: 0x6F2D_DA00, level: Level::Unsure, implemented: true, note: "the body returns no value (CONFIRMED); the model returns false. The request (FUN_6f2d96d0) is recorded" },
    AiNative { name: "GetCreepCamp", sig: "(IIB)Hunit;", addr: 0x6F2E_3D50, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the creep camp search (FUN_6f2e3980) is not modelled" },
    AiNative { name: "StartGetEnemyBase", sig: "()V", addr: 0x6F2D_DCC0, level: Level::Likely, implemented: true, note: "enemy base search (FUN_6f2db410) recorded; the result is WaitGetEnemyBase's, not modelled" },
    AiNative { name: "WaitGetEnemyBase", sig: "()B", addr: 0x6F2D_DCF0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: reads the enemy-base result of the AI state (+0x2EC), not modelled" },
    AiNative { name: "GetMegaTarget", sig: "()Hunit;", addr: 0x6F2D_DD60, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetEnemyBase", sig: "()Hunit;", addr: 0x6F2D_DD20, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetExpansionFoe", sig: "()Hunit;", addr: 0x6F2E_2E40, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetExpansionX", sig: "()I", addr: 0x6F2E_2DA0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetExpansionY", sig: "()I", addr: 0x6F2E_2DF0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "SetStagePoint", sig: "(RR)V", addr: 0x6F2D_DBD0, level: Level::Likely, implemented: true, note: "captain state checked, then the stage point (FUN_6f2e6d80): recorded" },
    AiNative { name: "AttackMoveKill", sig: "(Hunit;)V", addr: 0x6F2E_0A40, level: Level::Likely, implemented: true, note: "captain state checked, then the attack-move (FUN_6f2ec700): recorded" },
    AiNative { name: "AttackMoveXY", sig: "(II)V", addr: 0x6F2D_DC10, level: Level::Likely, implemented: true, note: "captain state checked, then the attack-move (FUN_6f2ec7d0): recorded" },
    AiNative { name: "LoadZepWave", sig: "(II)V", addr: 0x6F2D_DC50, level: Level::Likely, implemented: true, note: "captain state checked, then the zeppelin wave (FUN_6f2e7cd0): recorded" },
    AiNative { name: "SuicidePlayer", sig: "(Hplayer;B)B", addr: 0x6F2E_2F00, level: Level::Likely, implemented: true, note: "player gate; the boolean is a gate on the captain full bit (true needs CAPTAIN_FULL, which no engine writer sets here, so false); the engine suicide (FUN_6f2ef160) is recorded" },
    AiNative { name: "SuicidePlayerUnits", sig: "(Hplayer;B)B", addr: 0x6F2E_2F90, level: Level::Likely, implemented: true, note: "as SuicidePlayer, for the units only (the engine call takes the other mode): the same gate; recorded" },
    AiNative { name: "UnitAlive", sig: "(Hunit;)B", addr: 0x6F2E_0760, level: Level::Likely, implemented: true, note: "true for a unit that is not dead and not removed; the body tests the unit flag 0x100 at +0x5C (the mapping to dead or removed is UNSURE); null gives false (CONFIRMED)" },
    AiNative { name: "UnitInvis", sig: "(Hunit;)B", addr: 0x6F2E_07C0, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the body is 'not visible to the AI player' (FUN_6f3a38f0, a vision query); the sim has no vision model" },
    AiNative { name: "IgnoredUnits", sig: "(I)I", addr: 0x6F2D_CE80, level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI's ignore list (FUN_6f2d33c0) is not modelled" },
    AiNative { name: "CommandsWaiting", sig: "()I", addr: 0x6F2D_CDD0, level: Level::Confirmed, implemented: true, note: "the count of the AI's command stack (+0x1DC)" },
    AiNative { name: "GetLastCommand", sig: "()I", addr: 0x6F2E_0450, level: Level::Confirmed, implemented: true, note: "the command on top of the stack (the newest pushed); -1 when the stack is empty" },
    AiNative { name: "GetLastData", sig: "()I", addr: 0x6F2E_0490, level: Level::Confirmed, implemented: true, note: "the data on top of the stack (the newest pushed); -1 when the stack is empty" },
    AiNative { name: "PopLastCommand", sig: "()V", addr: 0x6F2E_04D0, level: Level::Confirmed, implemented: true, note: "removes the top of the stack; nothing when it is empty" },
    AiNative { name: "SetCampaignAI", sig: "()V", addr: 0x6F2D_DEA0, level: Level::Confirmed, implemented: true, note: "clears the melee bit 0x4000 of the AI flag word (FUN_6f2d4d40 with 1); the unit flags of the engine are not changed" },
    AiNative { name: "SetMeleeAI", sig: "()V", addr: 0x6F2D_DEE0, level: Level::Confirmed, implemented: true, note: "sets the melee bit 0x4000 of the AI flag word (FUN_6f2d4d40 with 0); the unit flags of the engine are not changed" },
    AiNative { name: "SetTargetHeroes", sig: "(B)V", addr: 0x6F2D_DE60, level: Level::Likely, implemented: true, note: "stored as an AI setting; the engine's use (FUN_6f2d4c90) is not modelled" },
    AiNative { name: "SetHeroesFlee", sig: "(B)V", addr: 0x6F2E_3120, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x8 of the AI flag word (+0x2D0)" },
    AiNative { name: "SetHeroesBuyItems", sig: "(B)V", addr: 0x6F2E_3160, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x400000 of the AI flag word (+0x2D0)" },
    AiNative { name: "SetIgnoreInjured", sig: "(B)V", addr: 0x6F2E_31E0, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x1000 of the AI flag word (+0x2D0)" },
    AiNative { name: "SetPeonsRepair", sig: "(B)V", addr: 0x6F2E_3060, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x800 of the AI flag word (+0x2D0)" },
    AiNative { name: "SetRandomPaths", sig: "(B)V", addr: 0x6F2E_30A0, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x80000 of the AI flag word (+0x2D0)" },
    AiNative { name: "SetDefendPlayer", sig: "(B)V", addr: 0x6F2E_30E0, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x200000 of the AI flag word (+0x2D0)" },
    AiNative { name: "SetHeroesTakeItems", sig: "(B)V", addr: 0x6F2E_3220, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x400 of the AI flag word (+0x2D0)" },
    AiNative { name: "SetUnitsFlee", sig: "(B)V", addr: 0x6F2E_3260, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x10 of the AI flag word (+0x2D0)" },
    AiNative { name: "SetGroupsFlee", sig: "(B)V", addr: 0x6F2E_32A0, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x20 of the AI flag word (+0x2D0)" },
    AiNative { name: "SetSlowChopping", sig: "(B)V", addr: 0x6F2E_32E0, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x40 of the AI flag word (+0x2D0)" },
    AiNative { name: "SetSmartArtillery", sig: "(B)V", addr: 0x6F2D_DF10, level: Level::Likely, implemented: true, note: "stored as an AI setting; the engine's use (FUN_6f2d4c20) is not modelled" },
    AiNative { name: "SetWatchMegaTargets", sig: "(B)V", addr: 0x6F2E_31A0, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x2000 of the AI flag word (+0x2D0)" },
    AiNative { name: "SetReplacementCount", sig: "(I)V", addr: 0x6F2E_0B30, level: Level::Confirmed, implemented: true, note: "stores the integer in the AI state (+0x1D4)" },
    AiNative { name: "PurchaseZeppelin", sig: "()V", addr: 0x6F2D_D410, level: Level::Likely, implemented: true, note: "captain state checked, then the engine purchase (FUN_6f2ed480): recorded" },
    AiNative { name: "MeleeDifficulty", sig: "()I", addr: 0x6F2E_0500, level: Level::Likely, implemented: true, note: "1 when bit 0x8000 is set; else 3 when 0x10000 is set; else 2 (CONFIRMED mapping); the engine sets these bits, the source is not decoded, so the model sets none unless a host does" },
    AiNative { name: "DebugBreak", sig: "(I)V", addr: 0x6F2D_C050, level: Level::Confirmed, implemented: true, note: "empty body; recorded only" },
];

/// What a call to a native of the AI table, or to a main-VM AI native, has as its environment.
struct Env {
    game: Rc<RefCell<Game>>,
    shared: SharedAi,
    player: u32,
}

/// The default return value for a native with signature `sig` (used by a not-implemented call).
pub fn default_return(sig: &str) -> Value {
    let ret = sig.rsplit_once(')').map_or("V", |(_, r)| r);
    match ret.as_bytes().first() {
        Some(b'I') => Value::Integer(0),
        Some(b'B') => Value::Boolean(false),
        Some(b'R') => Value::Real(F32::ZERO),
        Some(b'S') => Value::String(0),
        _ => Value::Null,
    }
}

fn int_arg(a: &[Value], i: usize) -> i32 {
    a.get(i).and_then(|v| v.as_integer()).unwrap_or(0)
}

fn real_arg(a: &[Value], i: usize) -> F32 {
    a.get(i).and_then(|v| v.as_real()).unwrap_or(F32::ZERO)
}

fn bool_arg(a: &[Value], i: usize) -> bool {
    a.get(i).and_then(|v| v.as_boolean()).unwrap_or(false)
}

fn handle_arg(a: &[Value], i: usize) -> u32 {
    a.get(i).and_then(|v| v.as_handle()).unwrap_or(0)
}

fn to_arg(ctx: &NativeCtx<'_>, v: Value) -> AiArg {
    match v {
        Value::Null => AiArg::Null,
        Value::Integer(i) => AiArg::Int(i),
        Value::Real(r) => AiArg::Real(r),
        Value::Boolean(b) => AiArg::Bool(b),
        Value::String(id) => AiArg::Str(ctx.string(id).to_string()),
        Value::Handle(h) => AiArg::Handle(h),
        Value::Code(c) => AiArg::Code(c),
    }
}

/// Counts the units owned by `owner` of raw type `type_id` that are alive and in the game.
fn count_units(gm: &Game, owner: u32, type_id: u32) -> i32 {
    let mut n = 0;
    for i in 0..gm.object_count() {
        if let Object::Unit(u) = gm.object(ObjKey(i as u32)) {
            if u.owner == owner && u.type_id == type_id && !u.removed && !u.dead {
                n += 1;
            }
        }
    }
    n
}

/// Records a call in the command log and returns `ret` as the call's value.
fn log(
    env: &Env,
    ctx: &NativeCtx<'_>,
    name: &'static str,
    a: &[Value],
    ret: Value,
) -> NativeResult {
    let args = a.iter().map(|v| to_arg(ctx, *v)).collect();
    env.shared.borrow_mut().commands.push(AiCommand {
        player: env.player,
        name,
        args,
    });
    NativeResult::Return(ret)
}

/// Sets or clears the bit `bit` of this AI's flag word.
fn set_flag(env: &Env, bit: u32, on: bool) {
    let mut sh = env.shared.borrow_mut();
    let c = sh.control_mut(env.player);
    if on {
        c.flags |= bit;
    } else {
        c.flags &= !bit;
    }
}

/// The flag bit of a boolean setter, if `name` is one.
fn flag_bit(name: &str) -> Option<u32> {
    FLAG_BITS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|&(_, bit)| bit)
}

/// Stores the value of the first argument as a setting of this AI.
fn store(env: &Env, ctx: &NativeCtx<'_>, name: &'static str, a: &[Value]) -> NativeResult {
    let value = a.first().map_or(AiArg::Null, |v| to_arg(ctx, *v));
    env.shared
        .borrow_mut()
        .control_mut(env.player)
        .settings
        .insert(name, value);
    NativeResult::Return(Value::Null)
}

/// The captain record flags of this AI (0 when the AI has no control record yet).
fn captain_flags(env: &Env) -> u32 {
    env.shared
        .borrow()
        .control(env.player)
        .map_or(0, |c| c.captain_flags)
}

/// Dispatches a call to an AI-table native. `None` when the model does not implement the native.
fn ai_call(
    name: &'static str,
    env: &Env,
    ctx: &mut NativeCtx<'_>,
    a: &[Value],
) -> Option<NativeResult> {
    let ret = |v: Value| Some(NativeResult::Return(v));

    // Boolean setters of the AI flag word: the value is the argument's truth.
    if let Some(bit) = flag_bit(name) {
        set_flag(env, bit, bool_arg(a, 0));
        return ret(Value::Null);
    }

    match name {
        // Identity and time.
        "GetAiPlayer" => ret(Value::Integer(env.player as i32)),
        "Sleep" => Some(NativeResult::Sleep(clamp_sleep(real_arg(a, 0)))),
        "StartThread" => {
            if let Some(Value::Code(f)) = a.first() {
                ctx.spawn(*f);
            }
            ret(Value::Null)
        }
        "DoAiScriptDebug" => ret(Value::Boolean(false)),

        // Debug and display output: recorded, no game effect (the bodies are empty).
        "DebugS" | "DebugFI" | "DebugUnitID" | "DebugBreak" | "DisplayText" | "DisplayTextI"
        | "DisplayTextII" | "DisplayTextIII" => Some(log(env, ctx, name, a, Value::Null)),

        // Unit and player queries on the game's state.
        "GetUnitCount" | "GetUnitCountDone" => {
            let gm = env.game.borrow();
            ret(Value::Integer(count_units(
                &gm,
                env.player,
                int_arg(a, 0) as u32,
            )))
        }
        "GetPlayerUnitTypeCount" => {
            let gm = env.game.borrow();
            let owner = gm.player_index_of_handle(handle_arg(a, 0));
            let n = owner.map_or(0, |p| count_units(&gm, p, int_arg(a, 1) as u32));
            ret(Value::Integer(n))
        }
        "GetUpgradeLevel" => {
            let gm = env.game.borrow();
            let level = gm
                .misc
                .players
                .get(env.player as usize)
                .and_then(|p| p.tech.get(&(int_arg(a, 0) as u32)))
                .map_or(0, |t| t.researched);
            ret(Value::Integer(level))
        }
        "GetUnitGoldCost" | "GetUnitWoodCost" => {
            let gm = env.game.borrow();
            let cost = gm.unit_types().get(int_arg(a, 0) as u32).map_or(0, |t| {
                if name == "GetUnitGoldCost" {
                    t.gold_cost
                } else {
                    t.wood_cost
                }
            });
            ret(Value::Integer(cost))
        }
        "GetEnemyPower" => ret(Value::Integer(0)),
        "MeleeDifficulty" => {
            let flags = env
                .shared
                .borrow()
                .control(env.player)
                .map_or(0, |c| c.flags);
            let level = if flags & DIFFICULTY_EASY_BIT != 0 {
                1
            } else if flags & DIFFICULTY_HARD_BIT != 0 {
                3
            } else {
                2
            };
            ret(Value::Integer(level))
        }
        "UnitAlive" => {
            let gm = env.game.borrow();
            let alive = gm
                .unit_of_handle(handle_arg(a, 0))
                .is_some_and(|k| matches!(gm.object(k), Object::Unit(u) if !u.dead && !u.removed));
            ret(Value::Boolean(alive))
        }
        "CaptainIsFull" => ret(Value::Boolean(captain_flags(env) & CAPTAIN_FULL != 0)),
        "CaptainRetreating" => ret(Value::Boolean(captain_flags(env) & CAPTAIN_RETREATING != 0)),

        // The command stack that `CommandAI` fills: the newest pair is read first.
        "CommandsWaiting" => {
            let sh = env.shared.borrow();
            ret(Value::Integer(
                sh.control(env.player).map_or(0, |c| c.queue.len() as i32),
            ))
        }
        "GetLastCommand" | "GetLastData" => {
            let sh = env.shared.borrow();
            let top = sh.control(env.player).and_then(|c| c.queue.last().copied());
            let v = match (name, top) {
                ("GetLastCommand", Some((c, _))) | ("GetLastData", Some((_, c))) => c,
                _ => -1,
            };
            ret(Value::Integer(v))
        }
        "PopLastCommand" => {
            env.shared.borrow_mut().control_mut(env.player).queue.pop();
            ret(Value::Null)
        }

        // Settings the AI reads back later, or stores for the engine.
        "SetHeroLevels" | "SetTargetHeroes" | "SetSmartArtillery" | "SetReplacementCount" => {
            Some(store(env, ctx, name, a))
        }
        "GetAllianceTarget" => {
            let h = env
                .shared
                .borrow()
                .control(env.player)
                .map_or(0, |c| c.alliance_target);
            ret(if h == 0 {
                Value::Null
            } else {
                Value::Handle(h)
            })
        }
        "SetAllianceTarget" => {
            let h = handle_arg(a, 0);
            env.shared
                .borrow_mut()
                .control_mut(env.player)
                .alliance_target = h;
            Some(log(env, ctx, name, a, Value::Null))
        }
        "SetMeleeAI" => {
            set_flag(env, MELEE_BIT, true);
            Some(log(env, ctx, name, a, Value::Null))
        }
        "SetCampaignAI" => {
            set_flag(env, MELEE_BIT, false);
            Some(log(env, ctx, name, a, Value::Null))
        }
        "DisablePathing" | "SetAmphibious" => {
            let bit = if name == "DisablePathing" {
                CAPTAIN_NO_PATHING
            } else {
                CAPTAIN_AMPHIBIOUS
            };
            env.shared
                .borrow_mut()
                .control_mut(env.player)
                .captain_flags |= bit;
            Some(log(env, ctx, name, a, Value::Null))
        }

        // Requests that the body returns a fixed value for.
        "SetUpgrade" | "ConvertUnits" | "AddAssault" | "AddDefenders" => {
            Some(log(env, ctx, name, a, Value::Boolean(false)))
        }
        "SetProduce" => {
            let ok = int_arg(a, 0) <= 0;
            Some(log(env, ctx, name, a, Value::Boolean(ok)))
        }
        "MergeUnits" => {
            // The guard of the body: enough units of the fourth type already exist, so nothing is done.
            let want = int_arg(a, 0);
            let have = {
                let gm = env.game.borrow();
                count_units(&gm, env.player, int_arg(a, 3) as u32)
            };
            if have >= want {
                return ret(Value::Boolean(true));
            }
            let both = {
                let gm = env.game.borrow();
                count_units(&gm, env.player, int_arg(a, 1) as u32) > 0
                    && count_units(&gm, env.player, int_arg(a, 2) as u32) > 0
            };
            if both {
                Some(log(env, ctx, name, a, Value::Boolean(false)))
            } else {
                ret(Value::Boolean(false))
            }
        }
        "SuicideUnit" | "SuicideUnitEx" => {
            if int_arg(a, 0) > 0 {
                Some(log(env, ctx, name, a, Value::Null))
            } else {
                ret(Value::Null)
            }
        }
        "HarvestGold" | "HarvestWood" => {
            if int_arg(a, 1) > 0 {
                Some(log(env, ctx, name, a, Value::Null))
            } else {
                ret(Value::Null)
            }
        }
        "SuicidePlayer" | "SuicidePlayerUnits" => {
            // The body's gate: a valid player, and for `true` the captain's full bit.
            let valid = player_of(&env.game, a).is_some();
            let flag = bool_arg(a, 1);
            let gate = valid && (!flag || captain_flags(env) & CAPTAIN_FULL != 0);
            Some(log(env, ctx, name, a, Value::Boolean(gate)))
        }
        "CaptainVsUnits" | "CaptainVsPlayer" => {
            let valid = player_of(&env.game, a).is_some();
            if valid {
                Some(log(env, ctx, name, a, Value::Null))
            } else {
                ret(Value::Null)
            }
        }

        // Commands that the model records (no game effect).
        "ClearHarvestAI"
        | "StopGathering"
        | "AddGuardPost"
        | "FillGuardPosts"
        | "ReturnGuardPosts"
        | "CreateCaptains"
        | "SetCaptainHome"
        | "ResetCaptainLocs"
        | "ShiftTownSpot"
        | "TeleportCaptain"
        | "ClearCaptainTargets"
        | "CaptainAttack"
        | "CaptainGoHome"
        | "RemoveInjuries"
        | "RemoveSiege"
        | "Unsummon"
        | "SetStagePoint"
        | "AttackMoveKill"
        | "AttackMoveXY"
        | "LoadZepWave"
        | "PurchaseZeppelin"
        | "InitAssault"
        | "StartGetEnemyBase" => Some(log(env, ctx, name, a, Value::Null)),

        _ => None,
    }
}

/// Registers the main-VM AI natives on `reg`, bound to `game` and the shared AI state.
pub fn register_main(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>, shared: &SharedAi) {
    let g = Rc::clone(game);
    let s = Rc::clone(shared);
    reg.register("StartMeleeAI", move |ctx, a| {
        start(&g, &s, ctx, a, StartKind::Melee)
    });
    let g = Rc::clone(game);
    let s = Rc::clone(shared);
    reg.register("StartCampaignAI", move |ctx, a| {
        start(&g, &s, ctx, a, StartKind::Campaign)
    });

    let g = Rc::clone(game);
    let s = Rc::clone(shared);
    reg.register("CommandAI", move |_, a| {
        let Some(p) = player_of(&g, a) else {
            return NativeResult::Return(Value::Null);
        };
        let mut sh = s.borrow_mut();
        if sh.is_active(p) {
            let c = sh.control_mut(p);
            if c.queue.len() < COMMAND_QUEUE_LIMIT {
                c.queue.push((int_arg(a, 1), int_arg(a, 2)));
            } else {
                c.dropped_commands += 1;
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    let s = Rc::clone(shared);
    reg.register("PauseCompAI", move |_, a| {
        let Some(p) = player_of(&g, a) else {
            return NativeResult::Return(Value::Null);
        };
        let mut sh = s.borrow_mut();
        if sh.is_active(p) {
            let c = sh.control_mut(p);
            c.pause = if bool_arg(a, 1) {
                c.pause + 1
            } else {
                (c.pause - 1).max(0)
            };
        }
        NativeResult::Return(Value::Null)
    });
}

/// The player index of the player-handle argument 0, if it is a valid slot.
fn player_of(game: &Rc<RefCell<Game>>, a: &[Value]) -> Option<u32> {
    let p = game.borrow().player_index_of_handle(handle_arg(a, 0))?;
    ((p as usize) < PLAYER_SLOTS).then_some(p)
}

/// `StartMeleeAI` and `StartCampaignAI`: marks the player as AI-controlled and queues the start request.
fn start(
    game: &Rc<RefCell<Game>>,
    shared: &SharedAi,
    ctx: &mut NativeCtx<'_>,
    a: &[Value],
    kind: StartKind,
) -> NativeResult {
    let script = a
        .get(1)
        .and_then(|v| v.as_string_id())
        .map(|id| ctx.string(id).to_string())
        .unwrap_or_default();
    let player = player_of(game, a);
    let mut sh = shared.borrow_mut();
    match player {
        None => sh.events.push(AiEvent::Refused {
            player: u32::MAX,
            script,
            reason: "no player slot",
        }),
        Some(p) if sh.is_active(p) => sh.events.push(AiEvent::Refused {
            player: p,
            script,
            reason: "the player already runs an AI",
        }),
        Some(p) => {
            let c = sh.control_mut(p);
            c.active = true;
            c.kind = Some(kind);
            sh.requests.push(StartRequest {
                player: p,
                script,
                kind,
            });
        }
    }
    NativeResult::Return(Value::Null)
}

/// Registers the 124 AI-table natives for the AI VM of `player`. Implemented rows dispatch to the model; the rest
/// count their calls in `AiShared::unimplemented` and return the default value.
pub fn register_ai(
    reg: &mut NativeRegistry,
    game: &Rc<RefCell<Game>>,
    shared: &SharedAi,
    player: u32,
) {
    for row in AI_NATIVES {
        let env = Env {
            game: Rc::clone(game),
            shared: Rc::clone(shared),
            player,
        };
        let name = row.name;
        let fallback = default_return(row.sig);
        reg.register(name, move |ctx, a| {
            if let Some(result) = ai_call(name, &env, ctx, a) {
                return result;
            }
            let mut sh: std::cell::RefMut<'_, AiShared> = env.shared.borrow_mut();
            *sh.unimplemented.entry(name).or_insert(0) += 1;
            NativeResult::Return(fallback)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_124_rows_and_unique_names() {
        assert_eq!(AI_NATIVES.len(), 124);
        let mut names: Vec<&str> = AI_NATIVES.iter().map(|r| r.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 124);
    }

    #[test]
    fn every_row_has_its_real_address() {
        for row in AI_NATIVES {
            assert!(row.addr >= 0x6F00_0000, "{} has no address", row.name);
        }
    }

    #[test]
    fn default_returns_follow_the_signature() {
        assert_eq!(default_return("(I)I"), Value::Integer(0));
        assert_eq!(default_return("(I)B"), Value::Boolean(false));
        assert_eq!(default_return("()Hunit;"), Value::Null);
        assert_eq!(default_return("(IRR)V"), Value::Null);
        assert_eq!(default_return("(S)R"), Value::Real(F32::ZERO));
    }

    #[test]
    fn unbound_ai_natives_are_pinned() {
        // The 36 rows not bound yet, each blocked by a subsystem the AI model does not hold (docs/spec/ai-natives.md
        // sections 2 and 5). A change here must come with a change of that document.
        let unbound: Vec<&str> = AI_NATIVES.iter().filter(|r| !r.implemented).map(|r| r.name).collect();
        assert_eq!(unbound.len(), 36);
        for row in AI_NATIVES.iter().filter(|r| !r.implemented) {
            assert_eq!(row.level, Level::Unsure, "{} is unbound but not UNSURE", row.name);
            assert!(row.note.starts_with("NOT IMPLEMENTED"), "{} has no reason", row.name);
        }
        for name in ["GetHeroId", "GetGoldOwned", "CreepsOnMap", "UnitInvis", "GetUpgradeGoldCost", "SetExpansion"] {
            assert!(unbound.contains(&name), "{name} should be unbound");
        }
    }

    #[test]
    fn flag_bits_are_distinct_and_cover_the_setters() {
        let mut bits: Vec<u32> = FLAG_BITS.iter().map(|&(_, b)| b).collect();
        bits.sort_unstable();
        bits.dedup();
        assert_eq!(bits.len(), FLAG_BITS.len());
        assert!(!bits.contains(&MELEE_BIT));
        assert!(!bits.contains(&DIFFICULTY_EASY_BIT));
        assert!(!bits.contains(&DIFFICULTY_HARD_BIT));
    }
}
