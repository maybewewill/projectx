//! AI natives: the main-VM side (`StartMeleeAI`, `StartCampaignAI`, `CommandAI`, `PauseCompAI`) and the 124 natives
//! of the AI script table (`docs/spec/natives-1.26a.tsv`, table `ai`), which the AI VMs call.
//!
//! What the binary shows (CONFIRMED, from the disassembly of `Game.dll`): all 124 AI natives are registered with one
//! implementation address, `0x6F71D9D0`, and that function is `xor eax, eax; ret`. Registration is at `0x6F71D9E0`
//! (the `ecx` argument of each `0x6F455110` call). So the binary gives no body for any individual AI native, and the
//! behaviour below is the model's own, taken from the native names, the signatures and the usual use in AI scripts.
//! Every row of [`AI_NATIVES`] carries its level: CONFIRMED (read in the binary), LIKELY (strong inference) or UNSURE
//! (guessed from the name). A native with `implemented: false` is registered too: a call logs its name in
//! `AiShared::unimplemented` and returns the default for its return type, so the AI thread goes on.
//!
//! The main-VM natives are read from the decompile (`natives.c`, `chunk_014.c`, `chunk_016.c`). `CommandAI` is
//! `FUN_6f2dcd80` (player gate, then `FUN_6f2d4990`: a queue of 12 `(command, data)` pairs, a full queue drops the
//! call); `PauseCompAI` is `FUN_6f2e0400` (a counter, `+1` for true, `-1` for false). `StartMeleeAI` and
//! `StartCampaignAI` have the same body (a gate, then `FUN_6f4c4630` and `FUN_6f3b3700`). The model's start is a
//! request served by [`crate::ai::AiHost::step`] (see the module docs of `ai.rs`).
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
    /// Read in the binary.
    Confirmed,
    /// A strong inference from the code shape or the usual use of the native.
    Likely,
    /// Guessed from the name and the signature.
    Unsure,
}

/// One row of a native module table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiNative {
    /// The native's name, as scripts call it.
    pub name: &'static str,
    /// The signature, as in `docs/spec/natives-1.26a.tsv`.
    pub sig: &'static str,
    /// The level of the model's behaviour for this native.
    pub level: Level,
    /// Whether the model acts on the call (`false`: logged as not implemented).
    pub implemented: bool,
    /// What the model does, or why it does not.
    pub note: &'static str,
}

/// The main-VM natives that concern the AI, with their levels.
pub const MAIN_NATIVES: &[AiNative] = &[
    AiNative { name: "StartMeleeAI", sig: "(Hplayer;S)V", level: Level::Confirmed, implemented: true, note: "gate and helper calls read (decompile); the start is a request served by AiHost::step" },
    AiNative { name: "StartCampaignAI", sig: "(Hplayer;S)V", level: Level::Confirmed, implemented: true, note: "same body as StartMeleeAI (decompile); the kind is recorded" },
    AiNative { name: "CommandAI", sig: "(Hplayer;II)V", level: Level::Confirmed, implemented: true, note: "queues (command, data) on the AI of a player with an AI; a full queue (12) drops the call" },
    AiNative { name: "PauseCompAI", sig: "(Hplayer;B)V", level: Level::Likely, implemented: true, note: "counter of the player's AI: +1 for true, -1 for false (not below 0: UNSURE); a count above 0 skips its step (UNSURE)" },
];

/// The 124 AI natives of table `ai`, in the order of the table. Generated from `docs/spec/natives-1.26a.tsv`.
pub const AI_NATIVES: &[AiNative] = &[
    AiNative { name: "DebugS", sig: "(S)V", level: Level::Likely, implemented: true, note: "logged to the AI command log; no game effect" },
    AiNative { name: "DebugFI", sig: "(SI)V", level: Level::Likely, implemented: true, note: "logged to the AI command log; no game effect" },
    AiNative { name: "DebugUnitID", sig: "(SI)V", level: Level::Likely, implemented: true, note: "logged to the AI command log; no game effect" },
    AiNative { name: "DisplayText", sig: "(IS)V", level: Level::Likely, implemented: true, note: "logged to the AI command log; no game effect" },
    AiNative { name: "DisplayTextI", sig: "(ISI)V", level: Level::Likely, implemented: true, note: "logged to the AI command log; no game effect" },
    AiNative { name: "DisplayTextII", sig: "(ISII)V", level: Level::Likely, implemented: true, note: "logged to the AI command log; no game effect" },
    AiNative { name: "DisplayTextIII", sig: "(ISIII)V", level: Level::Likely, implemented: true, note: "logged to the AI command log; no game effect" },
    AiNative { name: "SuicideUnit", sig: "(II)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "SuicideUnitEx", sig: "(III)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "Sleep", sig: "(R)V", level: Level::Unsure, implemented: true, note: "the table body is a stub; sleep is the VM's own sleep, resumed by the AI step" },
    AiNative { name: "StartThread", sig: "(C)V", level: Level::Likely, implemented: true, note: "starts the code value as a new thread of the same AI VM" },
    AiNative { name: "GetAiPlayer", sig: "()I", level: Level::Likely, implemented: true, note: "returns the player slot the AI VM runs for" },
    AiNative { name: "DoAiScriptDebug", sig: "()B", level: Level::Unsure, implemented: true, note: "returns false; the debug flag is not modelled" },
    AiNative { name: "GetHeroId", sig: "()I", level: Level::Unsure, implemented: true, note: "returns the stored setting (0 when never set)" },
    AiNative { name: "GetHeroLevelAI", sig: "()I", level: Level::Unsure, implemented: true, note: "returns the stored setting (0 when never set)" },
    AiNative { name: "SetHeroLevels", sig: "(C)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "SetNewHeroes", sig: "(B)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "GetUnitCount", sig: "(I)I", level: Level::Likely, implemented: true, note: "living units of the type owned by the player (dead and removed units excluded: UNSURE)" },
    AiNative { name: "GetPlayerUnitTypeCount", sig: "(Hplayer;I)I", level: Level::Likely, implemented: true, note: "living units of the type owned by the player (dead and removed units excluded: UNSURE)" },
    AiNative { name: "GetTownUnitCount", sig: "(IIB)I", level: Level::Unsure, implemented: true, note: "town filter not modelled: all owned units of the type" },
    AiNative { name: "GetUnitCountDone", sig: "(I)I", level: Level::Unsure, implemented: true, note: "same count as GetUnitCount: construction is not modelled" },
    AiNative { name: "GetUpgradeLevel", sig: "(I)I", level: Level::Likely, implemented: true, note: "researched level of the player tech with the same id (upgrades share the tech levels)" },
    AiNative { name: "GetUnitGoldCost", sig: "(I)I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: unit and upgrade costs are not in the sim's type registry (UnitType has no cost fields)" },
    AiNative { name: "GetUnitWoodCost", sig: "(I)I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: unit and upgrade costs are not in the sim's type registry (UnitType has no cost fields)" },
    AiNative { name: "GetUnitBuildTime", sig: "(I)I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: unit and upgrade costs are not in the sim's type registry (UnitType has no cost fields)" },
    AiNative { name: "GetUpgradeGoldCost", sig: "(I)I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: unit and upgrade costs are not in the sim's type registry (UnitType has no cost fields)" },
    AiNative { name: "GetUpgradeWoodCost", sig: "(I)I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: unit and upgrade costs are not in the sim's type registry (UnitType has no cost fields)" },
    AiNative { name: "GetEnemyPower", sig: "()I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI threat model (enemy army valuation) is not in the sim" },
    AiNative { name: "GetMinesOwned", sig: "()I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: gold mine ownership is not modelled as a type query" },
    AiNative { name: "GetGoldOwned", sig: "()I", level: Level::Likely, implemented: true, note: "gold of the AI player (player state 1)" },
    AiNative { name: "TownWithMine", sig: "()I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "TownHasMine", sig: "(I)B", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "TownHasHall", sig: "(I)B", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetNextExpansion", sig: "()I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetExpansionPeon", sig: "()Hunit;", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetEnemyExpansion", sig: "()Hunit;", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "SetExpansion", sig: "(Hunit;I)B", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "GetBuilding", sig: "(Hplayer;)Hunit;", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "SetAllianceTarget", sig: "(Hunit;)V", level: Level::Unsure, implemented: true, note: "stored as the alliance target and logged" },
    AiNative { name: "GetAllianceTarget", sig: "()Hunit;", level: Level::Unsure, implemented: true, note: "returns the stored alliance target (null when never set)" },
    AiNative { name: "SetProduce", sig: "(III)B", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "MergeUnits", sig: "(IIII)B", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "ConvertUnits", sig: "(II)B", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "SetUpgrade", sig: "(I)B", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "Unsummon", sig: "(Hunit;)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "ClearHarvestAI", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "HarvestGold", sig: "(II)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "HarvestWood", sig: "(II)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "StopGathering", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "AddGuardPost", sig: "(IRR)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "FillGuardPosts", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "ReturnGuardPosts", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "CreateCaptains", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "SetCaptainHome", sig: "(IRR)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "ResetCaptainLocs", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "ShiftTownSpot", sig: "(RR)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "SetCaptainChanges", sig: "(B)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "TeleportCaptain", sig: "(RR)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "ClearCaptainTargets", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "CaptainVsUnits", sig: "(Hplayer;)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "CaptainVsPlayer", sig: "(Hplayer;)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "CaptainAttack", sig: "(RR)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "GroupTimedLife", sig: "(B)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "CaptainGoHome", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "CaptainIsHome", sig: "()B", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain groups (AI army state) are not modelled" },
    AiNative { name: "CaptainRetreating", sig: "()B", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain groups (AI army state) are not modelled" },
    AiNative { name: "CaptainIsFull", sig: "()B", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain groups (AI army state) are not modelled" },
    AiNative { name: "CaptainIsEmpty", sig: "()B", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain groups (AI army state) are not modelled" },
    AiNative { name: "CaptainGroupSize", sig: "()I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain groups (AI army state) are not modelled" },
    AiNative { name: "CaptainReadiness", sig: "()I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain groups (AI army state) are not modelled" },
    AiNative { name: "CaptainReadinessHP", sig: "()I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain groups (AI army state) are not modelled" },
    AiNative { name: "CaptainReadinessMa", sig: "()I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain groups (AI army state) are not modelled" },
    AiNative { name: "CaptainInCombat", sig: "(B)B", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain groups (AI army state) are not modelled" },
    AiNative { name: "TownThreatened", sig: "()B", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "CaptainAtGoal", sig: "()B", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the captain groups (AI army state) are not modelled" },
    AiNative { name: "CreepsOnMap", sig: "()B", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: needs the AI's creep and ignore lists, or the invisibility state" },
    AiNative { name: "RemoveInjuries", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "RemoveSiege", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "IsTowered", sig: "(Hunit;)B", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "DisablePathing", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "SetAmphibious", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "InitAssault", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "AddAssault", sig: "(II)B", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "AddDefenders", sig: "(II)B", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "GetCreepCamp", sig: "(IIB)Hunit;", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "StartGetEnemyBase", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "WaitGetEnemyBase", sig: "()B", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetMegaTarget", sig: "()Hunit;", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetEnemyBase", sig: "()Hunit;", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetExpansionFoe", sig: "()Hunit;", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetExpansionX", sig: "()I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "GetExpansionY", sig: "()I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: the AI town and expansion model (map AI data, expansion sites) is not in the sim" },
    AiNative { name: "SetStagePoint", sig: "(RR)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "AttackMoveKill", sig: "(Hunit;)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "AttackMoveXY", sig: "(II)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "LoadZepWave", sig: "(II)V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "SuicidePlayer", sig: "(Hplayer;B)B", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "SuicidePlayerUnits", sig: "(Hplayer;B)B", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "UnitAlive", sig: "(Hunit;)B", level: Level::Likely, implemented: true, note: "true for a live (not dead, not removed) unit handle" },
    AiNative { name: "UnitInvis", sig: "(Hunit;)B", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: needs the AI's creep and ignore lists, or the invisibility state" },
    AiNative { name: "IgnoredUnits", sig: "(I)I", level: Level::Unsure, implemented: false, note: "NOT IMPLEMENTED: needs the AI's creep and ignore lists, or the invisibility state" },
    AiNative { name: "CommandsWaiting", sig: "()I", level: Level::Likely, implemented: true, note: "length of the AI's command queue" },
    AiNative { name: "GetLastCommand", sig: "()I", level: Level::Unsure, implemented: true, note: "queue front (oldest first, UNSURE); pop removes the front" },
    AiNative { name: "GetLastData", sig: "()I", level: Level::Unsure, implemented: true, note: "queue front (oldest first, UNSURE); pop removes the front" },
    AiNative { name: "PopLastCommand", sig: "()V", level: Level::Unsure, implemented: true, note: "queue front (oldest first, UNSURE); pop removes the front" },
    AiNative { name: "SetCampaignAI", sig: "()V", level: Level::Unsure, implemented: true, note: "logged; no game effect" },
    AiNative { name: "SetMeleeAI", sig: "()V", level: Level::Unsure, implemented: true, note: "logged; no game effect" },
    AiNative { name: "SetTargetHeroes", sig: "(B)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "SetHeroesFlee", sig: "(B)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "SetHeroesBuyItems", sig: "(B)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "SetIgnoreInjured", sig: "(B)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "SetPeonsRepair", sig: "(B)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "SetRandomPaths", sig: "(B)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "SetDefendPlayer", sig: "(B)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "SetHeroesTakeItems", sig: "(B)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "SetUnitsFlee", sig: "(B)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "SetGroupsFlee", sig: "(B)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "SetSlowChopping", sig: "(B)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "SetSmartArtillery", sig: "(B)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "SetWatchMegaTargets", sig: "(B)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "SetReplacementCount", sig: "(I)V", level: Level::Unsure, implemented: true, note: "stored as an AI setting; no game effect" },
    AiNative { name: "PurchaseZeppelin", sig: "()V", level: Level::Unsure, implemented: true, note: "recorded in the AI command log only (no game effect); boolean results return true" },
    AiNative { name: "MeleeDifficulty", sig: "()I", level: Level::Unsure, implemented: true, note: "the game difficulty value" },
    AiNative { name: "DebugBreak", sig: "(I)V", level: Level::Likely, implemented: true, note: "logged to the AI command log; no game effect" },
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

/// The value of a command that a native returns: `true` for a boolean native (the call is recorded as accepted),
/// nothing otherwise.
fn accepted(sig: &str) -> Value {
    if sig.ends_with('B') {
        Value::Boolean(true)
    } else {
        Value::Null
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

/// Records a call in the command log and returns the call's value.
fn log(env: &Env, ctx: &NativeCtx<'_>, name: &'static str, sig: &str, a: &[Value]) -> NativeResult {
    let args = a.iter().map(|v| to_arg(ctx, *v)).collect();
    env.shared.borrow_mut().commands.push(AiCommand {
        player: env.player,
        name,
        args,
    });
    NativeResult::Return(accepted(sig))
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

/// The stored setting `name` as an integer (0 when never set).
fn setting_int(env: &Env, name: &str) -> i32 {
    match env
        .shared
        .borrow()
        .control(env.player)
        .and_then(|c| c.settings.get(name))
    {
        Some(AiArg::Int(i)) => *i,
        Some(AiArg::Bool(b)) => i32::from(*b),
        _ => 0,
    }
}

/// Dispatches a call to an AI-table native. `None` when the model does not implement the native.
fn ai_call(
    name: &'static str,
    sig: &str,
    env: &Env,
    ctx: &mut NativeCtx<'_>,
    a: &[Value],
) -> Option<NativeResult> {
    let ret = |v: Value| Some(NativeResult::Return(v));
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

        // Debug and display output: logged, no game effect.
        "DebugS" | "DebugFI" | "DebugUnitID" | "DebugBreak" | "DisplayText" | "DisplayTextI"
        | "DisplayTextII" | "DisplayTextIII" => Some(log(env, ctx, name, sig, a)),

        // Unit and player queries on the game's state.
        "GetUnitCount" | "GetUnitCountDone" | "GetTownUnitCount" => {
            let gm = env.game.borrow();
            ret(Value::Integer(count_units(
                &gm,
                env.player,
                int_arg(a, 0) as u32,
            )))
        }
        "GetPlayerUnitTypeCount" => {
            let gm = env.game.borrow();
            let owner = gm
                .player_index_of_handle(handle_arg(a, 0))
                .unwrap_or(u32::MAX);
            ret(Value::Integer(count_units(
                &gm,
                owner,
                int_arg(a, 1) as u32,
            )))
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
        "GetGoldOwned" => {
            let gm = env.game.borrow();
            // Gold is player state 1, stored times ten (the same scaling as `GetPlayerState`).
            let gold = gm.player(env.player).states[1].wrapping_div(10);
            ret(Value::Integer(gold))
        }
        "MeleeDifficulty" => {
            let gm = env.game.borrow();
            ret(Value::Integer(gm.game_state.difficulty))
        }
        "UnitAlive" => {
            let gm = env.game.borrow();
            let alive = gm
                .unit_of_handle(handle_arg(a, 0))
                .is_some_and(|k| matches!(gm.object(k), Object::Unit(u) if !u.dead && !u.removed));
            ret(Value::Boolean(alive))
        }

        // The command queue that `CommandAI` fills.
        "CommandsWaiting" => {
            let sh = env.shared.borrow();
            ret(Value::Integer(
                sh.control(env.player).map_or(0, |c| c.queue.len() as i32),
            ))
        }
        "GetLastCommand" | "GetLastData" => {
            let sh = env.shared.borrow();
            let front = sh
                .control(env.player)
                .and_then(|c| c.queue.front().copied());
            let v = match (name, front) {
                ("GetLastCommand", Some((c, _))) | ("GetLastData", Some((_, c))) => c,
                _ => 0,
            };
            ret(Value::Integer(v))
        }
        "PopLastCommand" => {
            env.shared
                .borrow_mut()
                .control_mut(env.player)
                .queue
                .pop_front();
            ret(Value::Null)
        }

        // Settings the AI reads back later.
        "SetHeroLevels"
        | "SetNewHeroes"
        | "SetTargetHeroes"
        | "SetHeroesFlee"
        | "SetHeroesBuyItems"
        | "SetIgnoreInjured"
        | "SetPeonsRepair"
        | "SetRandomPaths"
        | "SetDefendPlayer"
        | "SetHeroesTakeItems"
        | "SetUnitsFlee"
        | "SetGroupsFlee"
        | "SetSlowChopping"
        | "SetSmartArtillery"
        | "SetWatchMegaTargets"
        | "SetReplacementCount" => Some(store(env, ctx, name, a)),
        "GetHeroId" | "GetHeroLevelAI" => ret(Value::Integer(setting_int(env, name))),
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
            Some(log(env, ctx, name, sig, a))
        }

        // Commands that the model records (no game effect).
        "SetMeleeAI"
        | "SetCampaignAI"
        | "SetProduce"
        | "MergeUnits"
        | "ConvertUnits"
        | "SetUpgrade"
        | "Unsummon"
        | "ClearHarvestAI"
        | "HarvestGold"
        | "HarvestWood"
        | "StopGathering"
        | "AddGuardPost"
        | "FillGuardPosts"
        | "ReturnGuardPosts"
        | "CreateCaptains"
        | "SetCaptainHome"
        | "ResetCaptainLocs"
        | "ShiftTownSpot"
        | "SetCaptainChanges"
        | "TeleportCaptain"
        | "ClearCaptainTargets"
        | "CaptainVsUnits"
        | "CaptainVsPlayer"
        | "CaptainAttack"
        | "GroupTimedLife"
        | "CaptainGoHome"
        | "RemoveInjuries"
        | "RemoveSiege"
        | "SetStagePoint"
        | "AttackMoveKill"
        | "AttackMoveXY"
        | "LoadZepWave"
        | "SuicidePlayer"
        | "SuicidePlayerUnits"
        | "SuicideUnit"
        | "SuicideUnitEx"
        | "PurchaseZeppelin"
        | "DisablePathing"
        | "SetAmphibious"
        | "InitAssault"
        | "AddAssault"
        | "AddDefenders"
        | "StartGetEnemyBase"
        | "SetExpansion" => Some(log(env, ctx, name, sig, a)),

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
                c.queue.push_back((int_arg(a, 1), int_arg(a, 2)));
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
        let sig = row.sig;
        let fallback = default_return(sig);
        reg.register(name, move |ctx, a| {
            if let Some(result) = ai_call(name, sig, &env, ctx, a) {
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
    fn default_returns_follow_the_signature() {
        assert_eq!(default_return("(I)I"), Value::Integer(0));
        assert_eq!(default_return("(I)B"), Value::Boolean(false));
        assert_eq!(default_return("()Hunit;"), Value::Null);
        assert_eq!(default_return("(IRR)V"), Value::Null);
        assert_eq!(default_return("(S)R"), Value::Real(F32::ZERO));
    }
}
