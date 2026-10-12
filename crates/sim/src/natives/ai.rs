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

use jass::vm::{NativeCtx, NativeRegistry, NativeResult, Value};
use softfloat::F32;

use super::game::{Game, ObjKey, Object, PLAYER_SLOTS};
use crate::captains::{self, Captain};
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
    AiNative { name: "Sleep", sig: "(R)V", addr: 0x6F2D_E380, level: Level::Likely, implemented: true, note: "hands the duration to the thread's agent timer (FUN_6f2d0f90: a virtual call with the duration); the model sleeps with timer::sleep_seconds (the AI's clamp of a negative value: UNSURE)" },
    AiNative { name: "StartThread", sig: "(C)V", addr: 0x6F2D_E2A0, level: Level::Likely, implemented: true, note: "starts the code as a thread of this AI (FUN_6f2da590). The binary keeps 6 thread slots (+0x254, index 0 to 5) and drops a thread when none is free; the slot freeing rule is UNSURE, so no cap is enforced" },
    AiNative { name: "GetAiPlayer", sig: "()I", addr: 0x6F2D_CFE0, level: Level::Confirmed, implemented: true, note: "returns the AI state's player index (+0x24); -1 outside an AI context (not reachable from an AI VM)" },
    AiNative { name: "DoAiScriptDebug", sig: "()B", addr: 0x6F2D_C0D0, level: Level::Confirmed, implemented: true, note: "body is 'return 0': always false" },
    AiNative { name: "GetHeroId", sig: "()I", addr: 0x6F2D_DF50, level: Level::Unsure, implemented: true, note: "the type code of the AI's first living hero, 0 without one. The body reads the hero record of the AI state (+0x1F0, id at +0x30) and, inside a hero-level callback, the hero being levelled (ability lookup 0x6F0787D0, owner compared); the model has no callback (UNSURE)" },
    AiNative { name: "GetHeroLevelAI", sig: "()I", addr: 0x6F2D_E0E0, level: Level::Unsure, implemented: true, note: "the level of the AI's first living hero, 0 without one. The body is (agile value of the hero record + 1 - record+0x90) + 1; read as the hero level (UNSURE)" },
    AiNative { name: "SetHeroLevels", sig: "(C)V", addr: 0x6F2E_0B00, level: Level::Confirmed, implemented: true, note: "stores the code value in the AI state (+0x40); no getter native" },
    AiNative { name: "SetNewHeroes", sig: "(B)V", addr: 0x6F2E_3020, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x4 of the AI flag word (+0x2D0)" },
    AiNative { name: "GetUnitCount", sig: "(I)I", addr: 0x6F2D_D050, level: Level::Likely, implemented: true, note: "typed count incl. in progress + queued trainees + pending builds (ai_manager::unit_count; mask 0x36 and FUN_6f2d1c10 CONFIRMED)" },
    AiNative { name: "GetPlayerUnitTypeCount", sig: "(Hplayer;I)I", addr: 0x6F2E_2B60, level: Level::Likely, implemented: true, note: "owned units of the type with mask 0x36 (CONFIRMED mask); 0 for a null or invalid player (CONFIRMED gate); the model counts owned live units" },
    AiNative { name: "GetTownUnitCount", sig: "(IIB)I", addr: 0x6F2D_D0C0, level: Level::Unsure, implemented: true, note: "workers on their way to build the type (0x6F2D1C10, CONFIRMED) plus the AI's units of the type that belong to the town (0x6F2D5430, CONFIRMED structure; units under construction left out when the flag is set); membership in the model is 'within 1024 of the town's anchor' (UNSURE)" },
    AiNative { name: "GetUnitCountDone", sig: "(I)I", addr: 0x6F2D_D010, level: Level::Likely, implemented: true, note: "typed count of completed units (mask 0x30 CONFIRMED)" },
    AiNative { name: "GetUpgradeLevel", sig: "(I)I", addr: 0x6F2D_D130, level: Level::Likely, implemented: true, note: "researched level of the tech; the body adds 1 when an entity with mask 2 of the tech exists (in-progress research: not modelled)" },
    AiNative { name: "GetUnitGoldCost", sig: "(I)I", addr: 0x6F2D_D1E0, level: Level::Confirmed, implemented: true, note: "the gold cost of the unit type (UnitType::gold_cost, the type record field +0x20); 0 for an unknown id. Research ids (uppercase first letter) give 0 here: their gate (FUN_6f473170) is not decoded" },
    AiNative { name: "GetUnitWoodCost", sig: "(I)I", addr: 0x6F2D_D240, level: Level::Confirmed, implemented: true, note: "the wood cost of the unit type (UnitType::wood_cost, the type record field +0x24); 0 for an unknown id; same research gate as the gold cost" },
    AiNative { name: "GetUnitBuildTime", sig: "(I)I", addr: 0x6F2D_D190, level: Level::Likely, implemented: true, note: "the type data: the type bldtm (the production records FUN_6f2d2fb0 that the body searches first are not modelled: UNSURE)" },
    AiNative { name: "GetUpgradeGoldCost", sig: "(I)I", addr: 0x6F2D_D2A0, level: Level::Likely, implemented: true, note: "base + mod * level of UpgradeData for the level after the researched one (shape CONFIRMED by the body: base plus increment times level minus one; the in-progress +1 of FUN_6f2dd130 is not added)" },
    AiNative { name: "GetUpgradeWoodCost", sig: "(I)I", addr: 0x6F2D_D2C0, level: Level::Likely, implemented: true, note: "as GetUpgradeGoldCost, with the lumber columns" },
    AiNative { name: "GetEnemyPower", sig: "()I", addr: 0x6F2D_C150, level: Level::Confirmed, implemented: true, note: "body is 'return 0': always 0" },
    AiNative { name: "GetMinesOwned", sig: "()I", addr: 0x6F2D_DFD0, level: Level::Likely, implemented: true, note: "counts the live mine references of the four town slots' sites (FUN_6f2d11a0); body CONFIRMED (docs/spec/ai-natives.md section 6); the town sites and slots are the model of ai_towns.rs, whose registration of units (when, and which site) is UNSURE" },
    AiNative { name: "GetGoldOwned", sig: "()I", addr: 0x6F2D_E000, level: Level::Likely, implemented: true, note: "sums the remaining gold ('Agmb' +0x6C) of those mines (FUN_6f2d1210), not the player's gold; body CONFIRMED (docs/spec/ai-natives.md section 6); the town sites and slots are the model of ai_towns.rs, whose registration of units (when, and which site) is UNSURE" },
    AiNative { name: "TownWithMine", sig: "()I", addr: 0x6F2D_E030, level: Level::Likely, implemented: true, note: "first slot whose first mine is live and, for human/orc, whose hall is live; night elf needs an entangled mine; -1 when none (FUN_6f2d5380); body CONFIRMED (docs/spec/ai-natives.md section 6); the town sites and slots are the model of ai_towns.rs, whose registration of units (when, and which site) is UNSURE" },
    AiNative { name: "TownHasMine", sig: "(I)B", addr: 0x6F2D_E060, level: Level::Likely, implemented: true, note: "either mine reference of the slot's site is live; slot above 3 or empty gives false (FUN_6f2d12b0); body CONFIRMED (docs/spec/ai-natives.md section 6); the town sites and slots are the model of ai_towns.rs, whose registration of units (when, and which site) is UNSURE" },
    AiNative { name: "TownHasHall", sig: "(I)B", addr: 0x6F2D_E0A0, level: Level::Likely, implemented: true, note: "the slot's hall reference is live; slot above 3 or empty gives false (FUN_6f2d1310); body CONFIRMED (docs/spec/ai-natives.md section 6); the town sites and slots are the model of ai_towns.rs, whose registration of units (when, and which site) is UNSURE" },
    AiNative { name: "GetNextExpansion", sig: "()I", addr: 0x6F2E_2BC0, level: Level::Unsure, implemented: true, note: "index of the town site of the next expansion, -1 without one (0x6F2E10D0: mine above 600 gold, not an AI site of the side, ranked in three classes by path cost; the model ranks by straight distance, see captains::next_expansion)" },
    AiNative { name: "GetExpansionPeon", sig: "()Hunit;", addr: 0x6F2D_D530, level: Level::Likely, implemented: true, note: "0x6F2D1F40 CONFIRMED structure: the nearest idle worker (mode 0/7) to the expansion's mine, else a gold harvester, else a lumber harvester; distance is straight, not path cost" },
    AiNative { name: "GetEnemyExpansion", sig: "()Hunit;", addr: 0x6F2D_D4F0, level: Level::Unsure, implemented: true, note: "the latest town hall of an enemy player that has two or more halls, else null" },
    AiNative { name: "SetExpansion", sig: "(Hunit;I)B", addr: 0x6F2E_0570, level: Level::Unsure, implemented: true, note: "0x6F2D7F90 CONFIRMED structure: the peon leaves its captain, the build-spot search runs around the expansion site (0x6F2E50E0) and the peon gets the build order (the haunted-mine type builds on the mine); false when no spot is found or the order is refused. The site is the model's mine position" },
    AiNative { name: "GetBuilding", sig: "(Hplayer;)Hunit;", addr: 0x6F2E_2E80, level: Level::Likely, implemented: true, note: "0x6F2E2E80 CONFIRMED: the first structure of the player (creation order here) without the marker that Unsummon sets; null for a null player" },
    AiNative { name: "SetAllianceTarget", sig: "(Hunit;)V", addr: 0x6F2E_0630, level: Level::Likely, implemented: true, note: "stores the unit handle as the AI's alliance target (the body keeps a unit reference in the AI state)" },
    AiNative { name: "GetAllianceTarget", sig: "()Hunit;", addr: 0x6F2D_D570, level: Level::Likely, implemented: true, note: "returns the stored alliance target; null when none" },
    AiNative { name: "SetProduce", sig: "(III)B", addr: 0x6F2D_D450, level: Level::Likely, implemented: true, note: "CONFIRMED loop (zero returns true, the first failed request returns false); each request is the train or build path of ai_manager.rs (spec ai-natives section 7); the order is issued at once" },
    AiNative { name: "MergeUnits", sig: "(IIII)B", addr: 0x6F2D_D320, level: Level::Unsure, implemented: true, note: "true when the owned units of the fourth type already reach the count (CONFIRMED guard, with the pending count of the second type taken as 0: UNSURE); otherwise the merge (FUN_6f2d2580) is not modelled: false" },
    AiNative { name: "ConvertUnits", sig: "(II)B", addr: 0x6F2D_D2E0, level: Level::Unsure, implemented: true, note: "the body returns no value (CONFIRMED); the model returns false. The conversion (FUN_6f2d2440) is recorded" },
    AiNative { name: "SetUpgrade", sig: "(I)B", addr: 0x6F2D_D4B0, level: Level::Likely, implemented: true, note: "CONFIRMED constant return false; the research order goes to the first usable building (ai_manager::set_upgrade)" },
    AiNative { name: "Unsummon", sig: "(Hunit;)V", addr: 0x6F2E_08A0, level: Level::Likely, implemented: true, note: "unsummon request (FUN_6f2d2790) recorded; no game effect" },
    AiNative { name: "ClearHarvestAI", sig: "()V", addr: 0x6F2D_D5B0, level: Level::Confirmed, implemented: true, note: "previous-cycle bits (CONFIRMED, 0x6F2DD5B0)" },
    AiNative { name: "HarvestGold", sig: "(II)V", addr: 0x6F2D_D5E0, level: Level::Likely, implemented: true, note: "CONFIRMED structure (0x6F2D9710): gate, retention of the previous cycle, new assignments; the walk and trip times are stand-ins" },
    AiNative { name: "HarvestWood", sig: "(II)V", addr: 0x6F2D_D630, level: Level::Likely, implemented: true, note: "as HarvestGold; wood on town 0 first sends every idle worker (CONFIRMED quirk)" },
    AiNative { name: "StopGathering", sig: "()V", addr: 0x6F2D_CE00, level: Level::Likely, implemented: true, note: "gathering stop (FUN_6f2d7710) recorded; no game effect" },
    AiNative { name: "AddGuardPost", sig: "(IRR)V", addr: 0x6F2D_CE30, level: Level::Likely, implemented: true, note: "guard post request (FUN_6f2daa40) recorded; no game effect" },
    AiNative { name: "FillGuardPosts", sig: "()V", addr: 0x6F2D_CF80, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "ReturnGuardPosts", sig: "()V", addr: 0x6F2D_CFB0, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "CreateCaptains", sig: "()V", addr: 0x6F2D_D960, level: Level::Likely, implemented: true, note: "creates both captains at the AI's first town hall (FUN_6f2da810)" },
    AiNative { name: "SetCaptainHome", sig: "(IRR)V", addr: 0x6F2D_D7B0, level: Level::Likely, implemented: true, note: "which = 1 attack, 2 defense, 3 both: sets the captain's home point" },
    AiNative { name: "ResetCaptainLocs", sig: "()V", addr: 0x6F2D_D780, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "ShiftTownSpot", sig: "(RR)V", addr: 0x6F2D_D740, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "SetCaptainChanges", sig: "(B)V", addr: 0x6F2E_2D20, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x80 of the AI flag word (+0x2D0)" },
    AiNative { name: "TeleportCaptain", sig: "(RR)V", addr: 0x6F2D_D680, level: Level::Likely, implemented: true, note: "captain state checked (FUN_6f2cfeb0), then the engine move (FUN_6f2e6380): recorded" },
    AiNative { name: "ClearCaptainTargets", sig: "()V", addr: 0x6F2D_D6C0, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "CaptainVsUnits", sig: "(Hplayer;)V", addr: 0x6F2E_2BF0, level: Level::Likely, implemented: true, note: "player gate (valid player), then the engine's attack on the player's units (FUN_6f2d1520): recorded; the attack is not modelled" },
    AiNative { name: "CaptainVsPlayer", sig: "(Hplayer;)V", addr: 0x6F2E_2C80, level: Level::Likely, implemented: true, note: "as CaptainVsUnits, the other engine mode: recorded; the attack is not modelled" },
    AiNative { name: "CaptainAttack", sig: "(RR)V", addr: 0x6F2D_D700, level: Level::Likely, implemented: true, note: "captain state checked, then the engine attack order (FUN_6f2ed520): recorded" },
    AiNative { name: "GroupTimedLife", sig: "(B)V", addr: 0x6F2E_2D60, level: Level::Confirmed, implemented: true, note: "sets or clears flag bit 0x100 of the AI flag word (+0x2D0)" },
    AiNative { name: "CaptainGoHome", sig: "()V", addr: 0x6F2D_D800, level: Level::Likely, implemented: true, note: "the attack group's members walk to the captain's home" },
    AiNative { name: "CaptainIsHome", sig: "()B", addr: 0x6F2D_D870, level: Level::Unsure, implemented: true, note: "the attack group's centre is within 500 units of home (the body tests the captain's position against home, FUN_6f2e76e0; the scale of the compare is UNSURE)" },
    AiNative { name: "CaptainRetreating", sig: "()B", addr: 0x6F2D_D830, level: Level::Likely, implemented: true, note: "bit 2 of the attack captain's flag word; nothing in the model sets it (the retreat rule is not decoded)" },
    AiNative { name: "CaptainIsFull", sig: "()B", addr: 0x6F2D_DB10, level: Level::Confirmed, implemented: true, note: "bit 1 of the attack captain's flag word: set by InitAssault, cleared by an unsatisfied AddAssault" },
    AiNative { name: "CaptainIsEmpty", sig: "()B", addr: 0x6F2D_DB50, level: Level::Likely, implemented: true, note: "no living member in the attack group (CONFIRMED body: the group size at +0xBC is 0; the size is the count of living members)" },
    AiNative { name: "CaptainGroupSize", sig: "()I", addr: 0x6F2D_DB90, level: Level::Likely, implemented: true, note: "number of living members of the attack group (+0xBC)" },
    AiNative { name: "CaptainReadiness", sig: "()I", addr: 0x6F2D_DA40, level: Level::Unsure, implemented: true, note: "min(life %, mana %) of the attack group (FUN_6f2e91a0 flags 1,1; the original splits heroes from other units, the model takes the totals); 100 for an empty group" },
    AiNative { name: "CaptainReadinessHP", sig: "()I", addr: 0x6F2D_DA90, level: Level::Unsure, implemented: true, note: "life % of the attack group (flags 1,0)" },
    AiNative { name: "CaptainReadinessMa", sig: "()I", addr: 0x6F2D_DAD0, level: Level::Unsure, implemented: true, note: "mana % of the attack group (flags 0,1)" },
    AiNative { name: "CaptainInCombat", sig: "(B)B", addr: 0x6F2D_D8A0, level: Level::Unsure, implemented: true, note: "true when a member of the chosen captain has a living attack target (the original: either enemy reference of the captain, +0x114/+0x120, resolves)" },
    AiNative { name: "TownThreatened", sig: "()B", addr: 0x6F2D_D8E0, level: Level::Unsure, implemented: true, note: "true when the player record's threat reference (+0x164) resolves (CONFIRMED body, FUN_6f2dd8e0); AiControl::town_threatened, false unless a host sets it (UNSURE: no threat scan)" },
    AiNative { name: "CaptainAtGoal", sig: "()B", addr: 0x6F2D_D930, level: Level::Unsure, implemented: true, note: "the group's centre is within the goal radius of the goal; true when the goal unit is dead; false without a goal (FUN_6f2e76e0 on the goal point; scale UNSURE)" },
    AiNative { name: "CreepsOnMap", sig: "()B", addr: 0x6F2D_C0F0, level: Level::Unsure, implemented: true, note: "any living neutral hostile non-structure unit (the callback 0x6F2DC0E0 flags the first unit the builder gives it; the builder's mode 0x1B/8/0x1F filter is not decoded)" },
    AiNative { name: "RemoveInjuries", sig: "()V", addr: 0x6F2D_DDA0, level: Level::Likely, implemented: true, note: "non-hero members of the attack group with life at most 50 % (CONFIRMED constant 0x6FAAE4D0) move to the defense captain and go to its home (FUN_6f2eee50)" },
    AiNative { name: "RemoveSiege", sig: "()V", addr: 0x6F2D_DE00, level: Level::Likely, implemented: true, note: "recorded; no game effect" },
    AiNative { name: "IsTowered", sig: "(Hunit;)B", addr: 0x6F2E_0950, level: Level::Unsure, implemented: true, note: "more than one enemy structure that can attack within 900 units of the unit (the body counts units near it with an attack and a predicate, radius from vtable slot 0xEC; the model's radius and predicate are UNSURE)" },
    AiNative { name: "DisablePathing", sig: "()V", addr: 0x6F2E_06E0, level: Level::Confirmed, implemented: true, note: "sets bit 0x4000 of the captain record (+0x6C); recorded" },
    AiNative { name: "SetAmphibious", sig: "()V", addr: 0x6F2E_0720, level: Level::Confirmed, implemented: true, note: "sets bit 0x8000 of the captain record (+0x6C); recorded" },
    AiNative { name: "InitAssault", sig: "()V", addr: 0x6F2D_D990, level: Level::Confirmed, implemented: true, note: "sets bit 1 (full) of the attack captain's flag word (0x6F2D4DF0)" },
    AiNative { name: "AddAssault", sig: "(II)B", addr: 0x6F2D_D9C0, level: Level::Confirmed, implemented: true, note: "the request 0x6F2D6EA0 (control flow CONFIRMED, Unicorn 600/600): six passes over the group and the idle units, true when the quantity is met, else the full bit is cleared and false; the pass bodies are LIKELY (see captains.rs)" },
    AiNative { name: "AddDefenders", sig: "(II)B", addr: 0x6F2D_DA00, level: Level::Likely, implemented: true, note: "the same request on the defense captain (0x6F2D96D0)" },
    AiNative { name: "GetCreepCamp", sig: "(IIB)Hunit;", addr: 0x6F2E_3D50, level: Level::Likely, implemented: true, note: "camp choice 0x6F2E3980 CONFIRMED (Unicorn 600/600); camps are formed from the neutral hostile units (CreepCampPathingCellDistance 26 cells), levels and path costs are the model's (UNSURE); returns the first living creep of the chosen camp" },
    AiNative { name: "StartGetEnemyBase", sig: "()V", addr: 0x6F2D_DCC0, level: Level::Unsure, implemented: true, note: "the search (FUN_6f2db410) is run at once: the result is the nearest enemy town hall (else structure) by straight distance, read back by GetEnemyBase" },
    AiNative { name: "WaitGetEnemyBase", sig: "()B", addr: 0x6F2D_DCF0, level: Level::Unsure, implemented: true, note: "always false (the search finished); the original's search takes time (timing UNSURE)" },
    AiNative { name: "GetMegaTarget", sig: "()Hunit;", addr: 0x6F2D_DD60, level: Level::Unsure, implemented: true, note: "reference at AI state +0x2C4 written by 0x6F2D7EC0 (rule CONFIRMED, see captains::mega_target): needs SetWatchMegaTargets, allied strength above the enemy's, own strength at least 20, first enemy whose strength is below half of the own; the model re-evaluates at each call" },
    AiNative { name: "GetEnemyBase", sig: "()Hunit;", addr: 0x6F2D_DD20, level: Level::Unsure, implemented: true, note: "the result of StartGetEnemyBase (the original reads +0x2EC of the AI state); the target choice is the model's" },
    AiNative { name: "GetExpansionFoe", sig: "()Hunit;", addr: 0x6F2E_2E40, level: Level::Unsure, implemented: true, note: "the first living creep of the camp nearest to the expansion site within 1100 units, else null" },
    AiNative { name: "GetExpansionX", sig: "()I", addr: 0x6F2E_2DA0, level: Level::Unsure, implemented: true, note: "x of the nearest gold mine with gold that is not within 900 units of a town hall, -1 when none (the original ranks sites by path cost)" },
    AiNative { name: "GetExpansionY", sig: "()I", addr: 0x6F2E_2DF0, level: Level::Unsure, implemented: true, note: "y of the same site, -1 when none" },
    AiNative { name: "SetStagePoint", sig: "(RR)V", addr: 0x6F2D_DBD0, level: Level::Likely, implemented: true, note: "stores the point where recruited units gather" },
    AiNative { name: "AttackMoveKill", sig: "(Hunit;)V", addr: 0x6F2E_0A40, level: Level::Likely, implemented: true, note: "no-op when the captain already has that target (CONFIRMED, 0x6F2E7530); else mode 2, retreat bit cleared, the members attack-move to the target and attack it when near (the captain mover is replaced by plain orders, UNSURE)" },
    AiNative { name: "AttackMoveXY", sig: "(II)V", addr: 0x6F2D_DC10, level: Level::Likely, implemented: true, note: "mode 2, retreat bit cleared, goal radius 200; the members attack-move to the point" },
    AiNative { name: "LoadZepWave", sig: "(II)V", addr: 0x6F2D_DC50, level: Level::Likely, implemented: true, note: "captain state checked, then the zeppelin wave (FUN_6f2e7cd0): recorded" },
    AiNative { name: "SuicidePlayer", sig: "(Hplayer;B)B", addr: 0x6F2E_2F00, level: Level::Unsure, implemented: true, note: "gate as before (valid player; with check_full the full bit); when it passes, the attack group attack-moves to the nearest structure of that player. The original's engine suicide (FUN_6f2ef160) is not decoded" },
    AiNative { name: "SuicidePlayerUnits", sig: "(Hplayer;B)B", addr: 0x6F2E_2F90, level: Level::Unsure, implemented: true, note: "as SuicidePlayer, the target is the nearest unit of the player" },
    AiNative { name: "UnitAlive", sig: "(Hunit;)B", addr: 0x6F2E_0760, level: Level::Likely, implemented: true, note: "true for a unit that is not dead and not removed; the body tests the unit flag 0x100 at +0x5C (the mapping to dead or removed is UNSURE); null gives false (CONFIRMED)" },
    AiNative { name: "UnitInvis", sig: "(Hunit;)B", addr: 0x6F2E_07C0, level: Level::Unsure, implemented: true, note: "false: the sim has no invisibility; the body is 'not visible to the AI player' and a unit without the invisibility flag 0x1000000 passes the gate (CONFIRMED gate)" },
    AiNative { name: "IgnoredUnits", sig: "(I)I", addr: 0x6F2D_CE80, level: Level::Unsure, implemented: true, note: "0x6F2DCE80 / callback 0x6F2D3360 CONFIRMED: the AI's units of the type whose AI record holds none of its three references; the model counts units in no captain that neither harvest nor build" },
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

/// The player controller value of a computer slot (the engine's `map::CONTROL_COMPUTER`).
const CONTROLLER_COMPUTER: u32 = 1;

/// Registers the objects created since the last call with the town model (see `ai_towns`): every gold mine, and the
/// town halls and haunted mines of players that run an AI (started, or a computer slot).
fn sync_towns(env: &Env) {
    let gm = env.game.borrow();
    let mut sh = env.shared.borrow_mut();
    let from = sh.towns.scanned();
    let total = gm.object_count();
    if from >= total {
        return;
    }
    let ai_players: std::collections::BTreeSet<u32> = (0..PLAYER_SLOTS as u32)
        .filter(|&p| sh.is_active(p) || gm.player(p).controller == CONTROLLER_COMPUTER)
        .collect();
    let mut units = Vec::new();
    for i in from..total {
        let k = ObjKey(i as u32);
        if let Object::Unit(u) = gm.object(k) {
            if u.removed {
                continue;
            }
            let ty = u.type_id;
            let is_mine = gm.mines.contains_key(&k)
                || ty == crate::ai_towns::GOLD_MINE
                || ty == crate::ai_towns::HAUNTED_MINE
                || ty == crate::ai_towns::ENTANGLED_MINE;
            units.push(crate::ai_towns::UnitInfo {
                key: k,
                type_id: ty,
                owner: u.owner,
                x: f32::from_bits(u.x.to_bits()),
                y: f32::from_bits(u.y.to_bits()),
                class_flags: u.class_flags,
                is_mine,
            });
        }
    }
    let live = |k: ObjKey| matches!(gm.object(k), Object::Unit(u) if !u.removed);
    sh.towns.register(&units, &ai_players, &live);
    sh.towns.set_scanned(total);
}

/// The town in slot `slot` of this AI, as the manager needs it (`None` when the slot is above 3 or empty).
fn town_info(env: &Env, slot: i32) -> Option<crate::ai_manager::TownInfo> {
    if !(0..crate::ai_towns::TOWN_SLOTS as i32).contains(&slot) {
        return None;
    }
    sync_towns(env);
    let gm = env.game.borrow();
    let sh = env.shared.borrow();
    let site = sh.towns.site_of_slot(env.player, i64::from(slot))?;
    let live = |k: ObjKey| matches!(gm.object(k), Object::Unit(u) if !u.removed);
    let hall = site.hall.filter(|&k| live(k));
    let mine = site.mines.iter().flatten().copied().find(|&k| live(k));
    let (x, y) = match hall {
        Some(h) => {
            let u = gm.unit(h);
            (u.x.to_f32(), u.y.to_f32())
        }
        None => (site.x, site.y),
    };
    Some(crate::ai_manager::TownInfo { slot: slot as usize, hall, mine, x, y })
}

/// The five town natives on the model.
fn town_call(name: &'static str, env: &Env, a: &[Value]) -> NativeResult {
    sync_towns(env);
    let gm = env.game.borrow();
    let sh = env.shared.borrow();
    let live = |k: ObjKey| matches!(gm.object(k), Object::Unit(u) if !u.removed);
    let p = env.player;
    let v = match name {
        "GetMinesOwned" => Value::Integer(sh.towns.mines_owned(p, &live)),
        "GetGoldOwned" => Value::Integer(
            sh.towns
                .gold_owned(p, &live, &|k| gm.resource_amount(k).unwrap_or(0)),
        ),
        "TownWithMine" => {
            let ty = |k: ObjKey| match gm.object(k) {
                Object::Unit(u) => u.type_id,
                _ => 0,
            };
            let building = |k: ObjKey| {
                matches!(gm.object(k), Object::Unit(u) if u.count_state == crate::unit_counts::REGISTERED_IN_PROGRESS)
            };
            Value::Integer(sh.towns.town_with_mine(p, gm.player(p).race, &live, &ty, &building))
        }
        "TownHasMine" => Value::Boolean(sh.towns.town_has_mine(p, i64::from(int_arg(a, 0)), &live)),
        _ => Value::Boolean(sh.towns.town_has_hall(p, i64::from(int_arg(a, 0)), &live)),
    };
    NativeResult::Return(v)
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

/// Runs `f` on the game, this AI's two captains and the AI flag word. The captains are moved out of the shared state
/// for the call, so `f` may borrow the shared state again only for other things (it must not touch the captains).
fn with_caps<R>(env: &Env, f: impl FnOnce(&mut Game, &mut [Captain; 2], u32) -> R) -> R {
    let (mut caps, flags) = {
        let mut sh = env.shared.borrow_mut();
        let c = sh.control_mut(env.player);
        (std::mem::take(&mut c.captains), c.flags)
    };
    let r = f(&mut env.game.borrow_mut(), &mut caps, flags);
    env.shared.borrow_mut().control_mut(env.player).captains = caps;
    r
}

/// A unit result: the handle of `unit`, or null.
fn unit_value(env: &Env, unit: Option<ObjKey>) -> Value {
    match unit {
        Some(k) => match env.game.borrow().handle_of(k) {
            0 => Value::Null,
            h => Value::Handle(h),
        },
        None => Value::Null,
    }
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
        "Sleep" => Some(NativeResult::Sleep(crate::timer::sleep_seconds(real_arg(a, 0)))),
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

        // Captain and threat records (AiControl fields; nothing in the sim writes them yet).
        "CaptainIsEmpty" => ret(Value::Boolean(with_caps(env, |gm, caps, _| captains::group_size(gm, &caps[0]) == 0))),
        "CaptainGroupSize" => ret(Value::Integer(with_caps(env, |gm, caps, _| captains::group_size(gm, &caps[0])))),
        "CaptainInCombat" => {
            // Argument true: the attack captain; false: the defense captain.
            let which = if bool_arg(a, 0) { captains::ATTACK_CAPTAIN } else { captains::DEFENSE_CAPTAIN };
            ret(Value::Boolean(with_caps(env, |_, caps, _| caps[which].in_combat)))
        }
        "CaptainIsFull" => ret(Value::Boolean(with_caps(env, |_, caps, _| caps[0].flags & captains::FULL != 0))),
        "CaptainRetreating" => {
            ret(Value::Boolean(with_caps(env, |_, caps, _| caps[0].flags & captains::RETREATING != 0)))
        }
        "CaptainReadiness" | "CaptainReadinessHP" | "CaptainReadinessMa" => {
            let life = name != "CaptainReadinessMa";
            ret(Value::Integer(with_caps(env, |gm, caps, _| captains::readiness(gm, &caps[0], life))))
        }
        "CaptainIsHome" => ret(Value::Boolean(with_caps(env, |gm, caps, _| captains::is_home(gm, &caps[0])))),
        "CaptainAtGoal" => ret(Value::Boolean(with_caps(env, |gm, caps, _| captains::at_goal(gm, &caps[0])))),
        "CreateCaptains" => {
            with_caps(env, |gm, caps, _| {
                let home = captains::anchor_of(gm, env.player);
                for c in caps.iter_mut() {
                    c.created = true;
                    c.home = home;
                }
            });
            Some(log(env, ctx, name, a, Value::Null))
        }
        "SetCaptainHome" => {
            let which = int_arg(a, 0);
            let p = (real_arg(a, 1).to_f32(), real_arg(a, 2).to_f32());
            with_caps(env, |_, caps, _| {
                if which & 1 != 0 {
                    caps[0].home = Some(p);
                }
                if which & 2 != 0 {
                    caps[1].home = Some(p);
                }
            });
            Some(log(env, ctx, name, a, Value::Null))
        }
        "SetStagePoint" => {
            let p = (real_arg(a, 0).to_f32(), real_arg(a, 1).to_f32());
            with_caps(env, |_, caps, _| caps[0].stage = Some(p));
            Some(log(env, ctx, name, a, Value::Null))
        }
        "InitAssault" => {
            with_caps(env, |_, caps, _| caps[0].flags |= captains::FULL);
            Some(log(env, ctx, name, a, Value::Null))
        }
        "AddAssault" | "AddDefenders" => {
            let (qty, typ) = (int_arg(a, 0), int_arg(a, 1) as u32);
            let which = usize::from(name == "AddDefenders");
            let ok = with_caps(env, |gm, caps, ai_flags| {
                let ctx = captains::RequestCtx { player: env.player, ai_flags };
                let (first, second) = caps.split_at_mut(1);
                if which == 0 {
                    captains::request(gm, &mut first[0], &mut [&mut second[0]], ctx, qty, typ)
                } else {
                    captains::request(gm, &mut second[0], &mut [&mut first[0]], ctx, qty, typ)
                }
            });
            Some(log(env, ctx, name, a, Value::Boolean(ok)))
        }
        "RemoveInjuries" => {
            with_caps(env, |gm, caps, _| {
                let (first, second) = caps.split_at_mut(1);
                captains::remove_injuries(gm, &mut first[0], &mut second[0]);
            });
            Some(log(env, ctx, name, a, Value::Null))
        }
        "CaptainGoHome" => {
            with_caps(env, |gm, caps, _| captains::go_home(gm, &mut caps[0]));
            Some(log(env, ctx, name, a, Value::Null))
        }
        "AttackMoveXY" => {
            let p = (int_arg(a, 0) as f32, int_arg(a, 1) as f32);
            let now = env.shared.borrow().elapsed;
            with_caps(env, |gm, caps, _| captains::attack_move_xy(gm, &mut caps[0], p, now));
            Some(log(env, ctx, name, a, Value::Null))
        }
        "AttackMoveKill" => {
            let now = env.shared.borrow().elapsed;
            let target = env.game.borrow().unit_of_handle(handle_arg(a, 0));
            if let Some(t) = target {
                with_caps(env, |gm, caps, _| captains::attack_move_kill(gm, &mut caps[0], t, now));
            }
            Some(log(env, ctx, name, a, Value::Null))
        }

        // Targets: creep camps, enemy bases, expansion sites (captains.rs).
        "GetCreepCamp" => {
            let (mn, mx, fly) = (int_arg(a, 0), int_arg(a, 1), bool_arg(a, 2));
            let from = with_caps(env, |gm, caps, _| captains::center(gm, &caps[0]));
            let found = {
                let gm = env.game.borrow();
                let from = from.or_else(|| captains::anchor_of(&gm, env.player)).unwrap_or((0.0, 0.0));
                env.shared.borrow_mut().camps.creep_camp(&gm, from, mn, mx, fly)
            };
            ret(unit_value(env, found))
        }
        "CreepsOnMap" => ret(Value::Boolean(captains::Camps::creeps_on_map(&env.game.borrow()))),
        "IsTowered" => {
            let gm = env.game.borrow();
            let towered = gm.unit_of_handle(handle_arg(a, 0)).is_some_and(|k| captains::is_towered(&gm, k));
            ret(Value::Boolean(towered))
        }
        "StartGetEnemyBase" => {
            let found = {
                let gm = env.game.borrow();
                let from = captains::anchor_of(&gm, env.player).unwrap_or((0.0, 0.0));
                captains::enemy_base(&gm, env.player, from)
            };
            let handle = found.map_or(0, |k| env.game.borrow().handle_of(k));
            env.shared.borrow_mut().control_mut(env.player).enemy_base = handle;
            Some(log(env, ctx, name, a, Value::Null))
        }
        "WaitGetEnemyBase" => ret(Value::Boolean(false)),
        "GetEnemyBase" => {
            let h = env.shared.borrow().control(env.player).map_or(0, |c| c.enemy_base);
            let alive = env.game.borrow().unit_of_handle(h).is_some();
            ret(if h != 0 && alive { Value::Handle(h) } else { Value::Null })
        }
        "GetEnemyExpansion" => {
            let found = captains::enemy_expansion(&env.game.borrow(), env.player);
            ret(unit_value(env, found))
        }
        "GetExpansionX" | "GetExpansionY" => {
            let site = captains::next_expansion(&env.game.borrow(), env.player);
            let v = site.map_or(-1, |(_, p)| if name == "GetExpansionX" { p.0 as i32 } else { p.1 as i32 });
            ret(Value::Integer(v))
        }
        "GetExpansionFoe" => {
            let found = {
                let gm = env.game.borrow();
                match captains::next_expansion(&gm, env.player) {
                    Some((_, p)) => env.shared.borrow_mut().camps.foe_near(&gm, p),
                    None => None,
                }
            };
            ret(unit_value(env, found))
        }
        "GetNextExpansion" => {
            sync_towns(env);
            let site = captains::next_expansion(&env.game.borrow(), env.player);
            let sh = env.shared.borrow();
            let idx = site.and_then(|(mine, p)| {
                sh.towns
                    .sites
                    .iter()
                    .position(|s| s.mines.contains(&Some(mine)))
                    .or_else(|| sh.towns.site_near(p.0, p.1))
            });
            ret(Value::Integer(idx.map_or(-1, |i| i as i32)))
        }
        "GetExpansionPeon" => {
            let found = {
                let gm = env.game.borrow();
                captains::next_expansion(&gm, env.player).and_then(|(_, p)| captains::expansion_peon(&gm, env.player, p))
            };
            ret(unit_value(env, found))
        }
        "SetExpansion" => {
            let peon = env.game.borrow().unit_of_handle(handle_arg(a, 0));
            let type_id = int_arg(a, 1) as u32;
            let ok = match peon {
                Some(peon) => {
                    let mut gm = env.game.borrow_mut();
                    let site = captains::next_expansion(&gm, env.player);
                    let spot = site.and_then(|(_, p)| {
                        if type_id == crate::ai_towns::HAUNTED_MINE {
                            Some(p)
                        } else {
                            gm.find_build_site(env.player, type_id, p)
                        }
                    });
                    let started = match spot {
                        Some((x, y)) if gm.unit(peon).owner == env.player => gm.start_build_job(peon, type_id, x, y).is_ok(),
                        _ => false,
                    };
                    if started {
                        // The peon leaves its captain (0x6F2EC850).
                        drop(gm);
                        let mut sh = env.shared.borrow_mut();
                        for c in sh.control_mut(env.player).captains.iter_mut() {
                            c.members.retain(|&m| m != peon);
                        }
                    }
                    started
                }
                None => false,
            };
            ret(Value::Boolean(ok))
        }
        "GetTownUnitCount" => {
            sync_towns(env);
            let (type_id, town, only_done) = (int_arg(a, 0) as u32, int_arg(a, 1), bool_arg(a, 2));
            let anchor = town_info(env, town).map(|t| (t.x, t.y));
            let gm = env.game.borrow();
            let n = match anchor {
                // a town slot above 3 or empty: only the builders count (0x6F2D5430 returns 0)
                None => captains::town_unit_count(&gm, env.player, type_id, (f32::MAX, f32::MAX), 0.0, only_done),
                Some(p) => captains::town_unit_count(&gm, env.player, type_id, p, crate::ai_towns::SITE_RADIUS, only_done),
            };
            ret(Value::Integer(n))
        }
        "GetMegaTarget" => {
            let flags = env.shared.borrow().control(env.player).map_or(0, |c| c.flags);
            let found = captains::mega_target(&env.game.borrow(), env.player, flags);
            ret(unit_value(env, found))
        }
        "IgnoredUnits" => {
            let n = with_caps(env, |gm, caps, _| captains::ignored_units(gm, env.player, int_arg(a, 0) as u32, &*caps));
            ret(Value::Integer(n))
        }
        "GetBuilding" => {
            let owner = env.game.borrow().player_index_of_handle(handle_arg(a, 0));
            let found = owner.and_then(|p| {
                let sh = env.shared.borrow();
                let marked = sh.control(env.player).map(|c| c.unsummoned.clone()).unwrap_or_default();
                captains::first_building(&env.game.borrow(), p, &marked)
            });
            ret(unit_value(env, found))
        }
        "GetHeroId" | "GetHeroLevelAI" => {
            let gm = env.game.borrow();
            let hero = captains::ai_hero(&gm, env.player);
            let v = hero.map_or(0, |k| {
                if name == "GetHeroId" {
                    gm.unit(k).type_id as i32
                } else {
                    gm.unit(k).hero.as_ref().map_or(0, |h| h.level)
                }
            });
            ret(Value::Integer(v))
        }
        "Unsummon" => {
            if let Some(k) = env.game.borrow().unit_of_handle(handle_arg(a, 0)) {
                env.shared.borrow_mut().control_mut(env.player).unsummoned.push(k);
            }
            Some(log(env, ctx, name, a, Value::Null))
        }
        "UnitInvis" => ret(Value::Boolean(false)),
        "TownThreatened" => {
            let sh = env.shared.borrow();
            ret(Value::Boolean(sh.control(env.player).is_some_and(|c| c.town_threatened)))
        }

        // AI towns (ai_towns.rs): the registration is lazy, in object order.
        "GetMinesOwned" | "GetGoldOwned" | "TownWithMine" | "TownHasMine" | "TownHasHall" => {
            Some(town_call(name, env, a))
        }

        // Unit and player queries on the game's state.
        "GetUnitCount" | "GetUnitCountDone" => {
            let gm = env.game.borrow();
            ret(Value::Integer(crate::ai_manager::unit_count(
                &gm,
                env.player,
                int_arg(a, 0) as u32,
                name == "GetUnitCountDone",
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
        "ConvertUnits" => {
            Some(log(env, ctx, name, a, Value::Boolean(false)))
        }
        "SetUpgrade" => {
            // The native returns the constant 0 (CONFIRMED); the order goes out before it returns.
            crate::ai_manager::set_upgrade(&mut env.game.borrow_mut(), env.player, int_arg(a, 0) as u32);
            Some(log(env, ctx, name, a, Value::Boolean(false)))
        }
        "SetProduce" => {
            let town = town_info(env, int_arg(a, 2));
            let ok = {
                let mut gm = env.game.borrow_mut();
                crate::ai_manager::set_produce(&mut gm, env.player, int_arg(a, 0), int_arg(a, 1) as u32, town)
            };
            Some(log(env, ctx, name, a, Value::Boolean(ok)))
        }
        "GetUpgradeGoldCost" | "GetUpgradeWoodCost" => {
            let gm = env.game.borrow();
            let tech = int_arg(a, 0) as u32;
            // The level the cost is for: the researched level plus one, plus one more while it is in progress
            // (`0x6F2DD130` adds 1 for a research in progress); the cost is `base + mod * (level - 1)`.
            let level = gm.tech_level(env.player, tech) + 1;
            let v = gm.production().upgrades.get(&tech).map_or(0, |u| {
                if name == "GetUpgradeGoldCost" {
                    u.gold_for(level - 1)
                } else {
                    u.lumber_for(level - 1)
                }
            });
            ret(Value::Integer(v))
        }
        "GetUnitBuildTime" => {
            let gm = env.game.borrow();
            ret(Value::Integer(
                gm.production().build_seconds.get(&(int_arg(a, 0) as u32)).copied().unwrap_or(0),
            ))
        }
        "ClearHarvestAI" => {
            crate::ai_manager::clear_harvest(&mut env.game.borrow_mut(), env.player);
            Some(log(env, ctx, name, a, Value::Null))
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
                let town = town_info(env, int_arg(a, 0));
                crate::ai_manager::harvest(
                    &mut env.game.borrow_mut(),
                    env.player,
                    town,
                    int_arg(a, 1),
                    name == "HarvestGold",
                );
                Some(log(env, ctx, name, a, Value::Null))
            } else {
                ret(Value::Null)
            }
        }
        "SuicidePlayer" | "SuicidePlayerUnits" => {
            // The body's gate: a valid player, and for `true` the captain's full bit.
            let valid = player_of(&env.game, a).is_some();
            let flag = bool_arg(a, 1);
            let full = with_caps(env, |_, caps, _| caps[0].flags & captains::FULL != 0);
            let gate = valid && (!flag || full);
            if gate {
                // The group attack-moves to the nearest structure (SuicidePlayer) or unit of the player (UNSURE: the
                // original's suicide routine 0x6F2EF160 is not decoded).
                let now = env.shared.borrow().elapsed;
                let victim = player_of(&env.game, a);
                let units_only = name == "SuicidePlayerUnits";
                let from = with_caps(env, |gm2, caps, _| captains::center(gm2, &caps[0])).unwrap_or((0.0, 0.0));
                let target = {
                    let gm = env.game.borrow();
                    victim.and_then(|p| captains::nearest_of(&gm, p, from, units_only))
                };
                if let Some(t) = target {
                    with_caps(env, |gm, caps, _| captains::attack_move_kill(gm, &mut caps[0], t, now));
                }
            }
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
        "StopGathering"
        | "AddGuardPost"
        | "FillGuardPosts"
        | "ReturnGuardPosts"
        | "ResetCaptainLocs"
        | "ShiftTownSpot"
        | "TeleportCaptain"
        | "ClearCaptainTargets"
        | "CaptainAttack"
        | "RemoveSiege"
        | "LoadZepWave"
        | "PurchaseZeppelin" => Some(log(env, ctx, name, a, Value::Null)),

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
    fn no_ai_native_is_left_unbound() {
        // Every row of table `ai` is bound since S19 (docs/spec/ai-natives.md section 10); a new unbound row has to come
        // with a reason and a change here.
        let unbound: Vec<&str> = AI_NATIVES.iter().filter(|r| !r.implemented).map(|r| r.name).collect();
        assert!(unbound.is_empty(), "unbound: {unbound:?}");
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
