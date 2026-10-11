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

/// Reason for the neutral order natives.
const NEUTRAL: &str = "the neutral order helpers (FUN_6f3b2ef0, FUN_6f3b2f30, FUN_6f3b3090) take the unit, the order and the issuing player in registers the decompile does not show";
/// Reason for the sync and trigger wait natives.
const SYNC: &str = "the multiplayer sync barriers and the wait-on-trigger threads (FUN_6f44b2f0, FUN_6f44b3a0, FUN_6f4482d0) are not read";
/// Reason for the name lookups that need the object or localisation tables.
const NAMES: &str = "the editor object tables and the localised string tables are not loaded (the lookups return names from them)";
/// Reason for the event response getters whose payload is not produced.
const EVENT: &str = "the event that carries this payload has no firing site in the model (see event.rs), so the getter has nothing to read";

/// The support table of the natives this crate does not register yet.
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry { name: "IssueNeutralImmediateOrder", support: Support::NotImplemented(NEUTRAL) },
    NativeEntry { name: "IssueNeutralImmediateOrderById", support: Support::NotImplemented(NEUTRAL) },
    NativeEntry { name: "IssueNeutralPointOrder", support: Support::NotImplemented(NEUTRAL) },
    NativeEntry { name: "IssueNeutralPointOrderById", support: Support::NotImplemented(NEUTRAL) },
    NativeEntry { name: "IssueNeutralTargetOrder", support: Support::NotImplemented(NEUTRAL) },
    NativeEntry { name: "IssueNeutralTargetOrderById", support: Support::NotImplemented(NEUTRAL) },
    NativeEntry { name: "TriggerSyncReady", support: Support::NotImplemented(SYNC) },
    NativeEntry { name: "TriggerSyncStart", support: Support::NotImplemented(SYNC) },
    NativeEntry { name: "TriggerExecuteWait", support: Support::NotImplemented(SYNC) },
    NativeEntry { name: "AbilityId", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "AbilityId2String", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "GetAbilityEffect", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "GetAbilitySound", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "GetDestructableName", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "GetItemName", support: Support::NotImplemented(NAMES) },
    NativeEntry { name: "GetWinningPlayer", support: Support::NotImplemented(EVENT) },
    NativeEntry { name: "GetOrderTargetDestructable", support: Support::NotImplemented(EVENT) },
    NativeEntry {
        name: "Cheat",
        support: Support::NotImplemented("the cheat command runs the chat-command parser (FUN_6f7b0da0), which is not read"),
    },
];
