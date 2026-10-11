//! Natives of the common work list that are not registered yet, each with the reason. A script that calls one of them
//! leaves it unbound (the VM reports it as missing), so nothing is faked. The table [`NATIVES`] lists them; the
//! registered ones are in the module tables of `pool.rs`, `defeat.rs`, `economy.rs`, `object.rs`, `animation.rs`,
//! `item.rs` (`NATIVES`), `order.rs` (`GROUP_NATIVES`), `hero.rs`, `gamestate.rs` and `trigger.rs`.
//!
//! The reasons name the system that is missing. Addresses are in `docs/spec/natives-1.26a.tsv`.
//!
//! The support table is checked by `tests/pending_natives.rs`: every row is `NotImplemented` and every name is a native
//! of the 1.26a table.

use super::unit_attr::{NativeEntry, Support};

/// Reason for the shop stock natives.
const STOCK: &str = "shop stock lists (the per-shop arrays of the unit object and the default lists that FUN_6f46d1b0 fills) and the slot counts are not modelled";
/// Reason for the local selection natives.
const SELECTION: &str = "selection is local interface state (the local player object's selection set, guarded by its interface flag at +0x34); the simulation has no interface object";
/// Reason for the neutral order natives.
const NEUTRAL: &str = "the neutral order helpers (FUN_6f3b2ef0, FUN_6f3b2f30, FUN_6f3b3090) take the unit, the order and the issuing player in registers the decompile does not show";
/// Reason for the build order natives.
const BUILD: &str =
    "the build placement validator (FUN_6f2a50c0) and the build order record are not modelled";
/// Reason for the sync and trigger wait natives.
const SYNC: &str = "the multiplayer sync barriers and the wait-on-trigger threads (FUN_6f44b2f0, FUN_6f44b3a0, FUN_6f4482d0) are not read";
/// Reason for the name lookups that need the object or localisation tables.
const NAMES: &str = "the editor object tables and the localised string tables are not loaded (the lookups return names from them)";
/// Reason for the guard position natives.
const GUARD: &str =
    "the guard position list of the AI (creep leash) is not modelled; the helpers are FUN_6f2e5410, FUN_6f2dcf00, FUN_6f2dcec0 (natives-pending.md section 5)";
/// Reason for the player unit counts: the counter object at player+0x2D0 (+0x60 completed, +0x5C in progress) is read,
/// but the sites that change those counters are not found (docs/spec/natives-pending.md section 1).
const COUNTS: &str = "the player unit counter (player+0x2D0: +0x60 base, +0x5C added by the flag) is read, but the increment and decrement sites are not found, so the model cannot say which units a count includes";
/// Reason for the player natives.
const PLAYER: &str = "the player statistics, unit and structure counts and the slot removal read counters and unit flags that the model does not keep";
/// Reason for the event response getters whose payload is not produced.
const EVENT: &str = "the event that carries this payload has no firing site in the model (see event.rs), so the getter has nothing to read";

/// The support table of the natives this crate does not register yet.
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry { name: "AddItemToAllStock", support: Support::NotImplemented(STOCK) },
    NativeEntry { name: "AddItemToStock", support: Support::NotImplemented(STOCK) },
    NativeEntry { name: "AddUnitToAllStock", support: Support::NotImplemented(STOCK) },
    NativeEntry { name: "AddUnitToStock", support: Support::NotImplemented(STOCK) },
    NativeEntry { name: "RemoveItemFromAllStock", support: Support::NotImplemented(STOCK) },
    NativeEntry { name: "RemoveItemFromStock", support: Support::NotImplemented(STOCK) },
    NativeEntry { name: "RemoveUnitFromAllStock", support: Support::NotImplemented(STOCK) },
    NativeEntry { name: "RemoveUnitFromStock", support: Support::NotImplemented(STOCK) },
    NativeEntry { name: "SetItemTypeSlots", support: Support::NotImplemented(STOCK) },
    NativeEntry { name: "SetAllItemTypeSlots", support: Support::NotImplemented(STOCK) },
    NativeEntry { name: "SetAllUnitTypeSlots", support: Support::NotImplemented(STOCK) },
    NativeEntry { name: "ClearSelection", support: Support::NotImplemented(SELECTION) },
    NativeEntry { name: "SelectUnit", support: Support::NotImplemented(SELECTION) },
    NativeEntry { name: "SyncSelections", support: Support::NotImplemented(SELECTION) },
    NativeEntry { name: "GroupEnumUnitsSelected", support: Support::NotImplemented(SELECTION) },
    NativeEntry { name: "IssueNeutralImmediateOrder", support: Support::NotImplemented(NEUTRAL) },
    NativeEntry { name: "IssueNeutralImmediateOrderById", support: Support::NotImplemented(NEUTRAL) },
    NativeEntry { name: "IssueNeutralPointOrder", support: Support::NotImplemented(NEUTRAL) },
    NativeEntry { name: "IssueNeutralPointOrderById", support: Support::NotImplemented(NEUTRAL) },
    NativeEntry { name: "IssueNeutralTargetOrder", support: Support::NotImplemented(NEUTRAL) },
    NativeEntry { name: "IssueNeutralTargetOrderById", support: Support::NotImplemented(NEUTRAL) },
    NativeEntry { name: "IssueBuildOrder", support: Support::NotImplemented(BUILD) },
    NativeEntry { name: "IssueBuildOrderById", support: Support::NotImplemented(BUILD) },
    NativeEntry { name: "TriggerSyncReady", support: Support::NotImplemented(SYNC) },
    NativeEntry { name: "TriggerSyncStart", support: Support::NotImplemented(SYNC) },
    NativeEntry { name: "TriggerExecuteWait", support: Support::NotImplemented(SYNC) },
    NativeEntry { name: "TriggerWaitForSound", support: Support::NotImplemented(SYNC) },
    NativeEntry { name: "AbilityId", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "AbilityId2String", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "GetObjectName", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "GetLocalizedString", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "GetLocalizedHotkey", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "GetAbilityEffect", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "GetAbilityEffectById", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "GetAbilitySound", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "GetAbilitySoundById", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "GetDestructableName", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "GetItemName", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "RecycleGuardPosition", support: Support::NotImplemented(GUARD) },
    NativeEntry { name: "RemoveAllGuardPositions", support: Support::NotImplemented(GUARD) },
    NativeEntry { name: "RemoveGuardPosition", support: Support::NotImplemented(GUARD) },
    NativeEntry {
        name: "CreateCorpse",
        support: Support::NotImplemented("the corpse type is passed in registers the decompile does not show (FUN_6f24f1f0 is called without it), and the corpse flags 0x200 and 0x1000 are not modelled"),
    },
    NativeEntry {
        name: "CreateBlightedGoldmine",
        support: Support::NotImplemented("the goldmine type is set in registers the decompile does not show before the unit builder, and the mine's amount comes from its ability data"),
    },
    NativeEntry {
        name: "ChooseRandomCreep",
        support: Support::NotImplemented("the candidate filter needs the creep flags of the unit types (not in the registry); the walk is FUN_6f29fd90 on stream 32 (natives-pending.md section 3)"),
    },
    NativeEntry {
        name: "ChooseRandomNPBuilding",
        support: Support::NotImplemented("the candidate filter needs the neutral building flags of the unit types (not in the registry); the walk is FUN_6f294740 on stream 34 (rng.md 6)"),
    },
    NativeEntry { name: "CripplePlayer", support: Support::NotImplemented(PLAYER) },
    NativeEntry { name: "RemovePlayer", support: Support::NotImplemented(PLAYER) },
    NativeEntry { name: "SetPlayerUnitsOwner", support: Support::NotImplemented(PLAYER) },
    NativeEntry { name: "CachePlayerHeroData", support: Support::NotImplemented(PLAYER) },
    NativeEntry { name: "GetPlayerScore", support: Support::NotImplemented(PLAYER) },
    NativeEntry { name: "GetPlayerUnitCount", support: Support::NotImplemented(COUNTS) },
    NativeEntry { name: "GetPlayerStructureCount", support: Support::NotImplemented(COUNTS) },
    NativeEntry { name: "GetPlayerTypedUnitCount", support: Support::NotImplemented(COUNTS) },
    NativeEntry { name: "GetWinningPlayer", support: Support::NotImplemented(EVENT) },
    NativeEntry { name: "GetRescuer", support: Support::NotImplemented(EVENT) },
    NativeEntry { name: "GetSellingUnit", support: Support::NotImplemented(EVENT) },
    NativeEntry { name: "GetOrderTargetDestructable", support: Support::NotImplemented(EVENT) },
    NativeEntry {
        name: "GetWorldBounds",
        support: Support::NotImplemented("the four bounds come from the global object (+0x70) and the pathing object (+0x54) through about ten helpers that are not read; the rect is allocated last (docs/spec/natives-pending.md section 2)"),
    },
    NativeEntry {
        name: "Cheat",
        support: Support::NotImplemented("the cheat command runs the chat-command parser (FUN_6f7b0da0), which is not read"),
    },
    NativeEntry {
        name: "ChangeLevel",
        support: Support::NotImplemented("it switches the campaign level through the campaign loader (FUN_6f3a4c70), which is not modelled"),
    },
];
