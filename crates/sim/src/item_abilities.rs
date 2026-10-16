//! Item abilities: how an item's ability is cast from the pack (`UnitUseItem*`), the charge and removal rules, the
//! cooldown group, and the families that matter for melee games: restoration items (`AIhe`, `AIma`, `AIre` and the area
//! forms `AIha`, `AImr`, `AIra`), tomes (`AImi`, `AImn`, `AIem`, `AIlm`, the attribute tomes `AIxm`/`AIam`/`AIim`/`AIsm`,
//! `AIaa`, `AIgo`, `AIlu`) and the persistent bonuses (`AIat`, `AIde`, `AIas`, `AIms`, `AIml`, `AImm`, `AIsi`, `AIab`).
//! Spec: `docs/spec/abilities-items.md` (labels CONFIRMED / LIKELY / UNSURE as there). Unicorn harnesses:
//! `tools/emu/item_trace_emu.py` (call tracer used for the decoding) and `tools/emu/item_cores_emu.py` (the numeric
//! cores, vectors in `crates/sim/tests/item_ability_vectors.rs`).
//!
//! ## Model
//! * An item type has up to four ability ids (`ItemData.slk` `abilList`, [`ItemSpec`]). An id is an *alias* row of
//!   `AbilityData.slk`; its `code` column is the factory base code that picks the class ([`Family`]). The level records
//!   are registered under the alias like any spell's (`Game::set_spell_levels`).
//! * Taking an item into the pack attaches its abilities to the unit and applies the persistent bonuses; leaving the
//!   pack takes them off again (`on_pack_add` / `on_pack_remove`, hooked in `natives/item.rs`).
//! * `UnitUseItem*` casts the item's first castable ability through the cast core (`cast::begin_cast`); the charge is
//!   used when the cast completes (`on_cast_end`).

use std::cell::RefCell;
use std::collections::BTreeMap;

use softfloat::{arith, conv, F32, Tables};

use crate::abilities::{code, Ability};
use crate::cast::{begin_cast, CastRec, CastTarget, SpellLevel};
use crate::heroes::{self, BonusEnv, EngineEnv, Gain};
use crate::natives::event::{Datum, Field, Firing};
use crate::natives::game::{item_flags, Game, ItemType, ObjKey};
use crate::natives::{fire_observers_in, set_life, ScriptRuntime, KEY_BASE};
use crate::timer::TimerId;

// ---- the base codes of the families ----

pub const ITEM_HEAL: u32 = code(b"AIhe");
pub const ITEM_MANA: u32 = code(b"AIma");
pub const ITEM_RESTORE: u32 = code(b"AIre");
pub const ITEM_HEAL_AOE: u32 = code(b"AIha");
pub const ITEM_MANA_AOE: u32 = code(b"AImr");
pub const ITEM_RESTORE_AOE: u32 = code(b"AIra");
pub const TOME_LIFE: u32 = code(b"AImi");
pub const TOME_MANA: u32 = code(b"AImn");
pub const TOME_XP: u32 = code(b"AIem");
pub const TOME_LEVEL: u32 = code(b"AIlm");
pub const TOME_ATTRS: u32 = code(b"AIxm");
pub const TOME_AGI: u32 = code(b"AIam");
pub const TOME_INT: u32 = code(b"AIim");
pub const TOME_STR: u32 = code(b"AIsm");
pub const TOME_ATTACK: u32 = code(b"AIaa");
pub const GIVE_GOLD: u32 = code(b"AIgo");
pub const GIVE_LUMBER: u32 = code(b"AIlu");
pub const BONUS_ATTACK: u32 = code(b"AIat");
pub const BONUS_DEFENSE: u32 = code(b"AIde");
pub const BONUS_ATTACK_SPEED: u32 = code(b"AIas");
pub const BONUS_MOVE_SPEED: u32 = code(b"AIms");
pub const BONUS_LIFE: u32 = code(b"AIml");
pub const BONUS_MANA: u32 = code(b"AImm");
pub const BONUS_SIGHT: u32 = code(b"AIsi");
pub const BONUS_ATTRS: u32 = code(b"AIab");
pub const ITEM_SPEED: u32 = code(b"AIsp");
pub const ITEM_SPEED_AOE: u32 = code(b"AIsa");
pub const TOWN_PORTAL: u32 = code(b"AItp");
pub const ITEM_REGEN: u32 = code(b"AIrg");
pub const BONUS_LIFE_REGEN: u32 = code(b"Arel");
pub const BONUS_MANA_REGEN: u32 = code(b"AIrm");
pub const BONUS_SPELL_REDUCTION: u32 = code(b"AIsr");
pub const BONUS_VAMPIRE: u32 = code(b"AIva");
pub const FIGURINE: u32 = code(b"AIfs");
pub const FIGURINE_GOLEM: u32 = code(b"AIfr");
pub const FIGURINE_GUARD: u32 = code(b"AIfu");
/// The buff of the figurine summons (the timed life special id `BFig`).
pub const BUFF_FIGURINE: u32 = code(b"BFig");
pub const ORB_FIRE: u32 = code(b"AIfb");
pub const ORB_FROST: u32 = code(b"AIob");
pub const ITEM_REINCARNATION: u32 = code(b"AIrc");
pub const ITEM_INVIS: u32 = code(b"AIvi");
pub const ITEM_INVUL: u32 = code(b"AIvu");
pub const ITEM_RAISE_DEAD: u32 = code(b"AIrd");
pub const ITEM_RESURRECTION: u32 = code(b"AIrs");
pub const ITEM_DISPEL: u32 = code(b"AIdi");
/// The buffs of the invisibility and invulnerability potions (the codes the effect bodies hand to the buff finder:
/// `0x6F10F580` returns `'Binv'`, `0x6F10F990` returns `'Bvul'`).
pub const BUFF_INVIS: u32 = code(b"Binv");
pub const BUFF_INVUL: u32 = code(b"Bvul");
/// The buff of the regeneration items (the first of `BuffID` `BIrg,BIrl,BIrm`).
pub const BUFF_REGEN: u32 = code(b"BIrg");
/// The buff of the item speed abilities (`BuffID` of `AIsp` and `AIsa`).
pub const BUFF_SPEED: u32 = code(b"Bspe");

// S45: the item abilities of `docs/spec/abilities-items.md` section 16 (class identities from the ability atlas).
/// `CAbilityItemAnimateDead` (child of `CAbilityAnimateDead`).
pub const ITEM_ANIMATE_DEAD: u32 = code(b"AIan");
/// `CAbilityItemBuild` (build scrolls).
pub const ITEM_BUILD: u32 = code(b"AIbl");
/// `CAbilityItemCureAoe`.
pub const ITEM_CURE_AOE: u32 = code(b"AIca");
/// `CAbilityCorruptionDamageBonus` (Orb of Corruption).
pub const ORB_CORRUPTION: u32 = code(b"AIcb");
/// `CAbilityItemCloakOfFlames` (a `CAbilityPermImmolation` child).
pub const ITEM_CLOAK_OF_FLAMES: u32 = code(b"AIcf");
/// `CAbilityItemCommand`.
pub const ITEM_COMMAND: u32 = code(b"AIco");
/// `CAbilityItemDefenseAoe` (Scroll of Protection).
pub const ITEM_DEFENSE_AOE: u32 = code(b"AIda");
/// `CAbilityItemDispelChain` (Wand of Neutralization).
pub const ITEM_DISPEL_CHAIN: u32 = code(b"AIdc");
/// `CAbilityItemDamageAoe`.
pub const ITEM_DAMAGE_AOE: u32 = code(b"AIdm");
/// `CAbilityItemFlare` (Flare Gun).
pub const ITEM_FLARE: u32 = code(b"AIfa");
/// The figurine subclasses (`CAbilityFigurineRedDrake`, `Furbolg`, `FelHound`) and the figurine base class.
pub const FIGURINE_DRAKE: u32 = code(b"AIfd");
pub const FIGURINE_FURBOLG: u32 = code(b"AIff");
pub const FIGURINE_BASE: u32 = code(b"AIfg");
pub const FIGURINE_HOUND: u32 = code(b"AIfh");
/// The flag classes (`CAbilityFlag < CBonusBase` and its four race children).
pub const FLAG_BASE: u32 = code(b"AIfl");
pub const FLAG_UNDEAD: u32 = code(b"AIfe");
pub const FLAG_HUMAN: u32 = code(b"AIfm");
pub const FLAG_NIGHT_ELF: u32 = code(b"AIfn");
pub const FLAG_ORC: u32 = code(b"AIfo");
/// `CAbilityGlyph` (a `CAbilityTome` child).
pub const ITEM_GLYPH: u32 = code(b"AIgl");
/// `CAbilityItemIllusion` (Wand of Illusion).
pub const ITEM_ILLUSION: u32 = code(b"AIil");
/// `CAbilityLightningDamageBonus`, `CAbilityLightningPurge`.
pub const ORB_LIGHTNING: u32 = code(b"AIlb");
/// `CAbilityItemMonsterLure` (a `CAbilityWard` child).
pub const ITEM_MONSTER_LURE: u32 = code(b"AImo");
/// `CAbilityPoisonDamageBonus` (Orb of Venom).
pub const ORB_VENOM: u32 = code(b"AIpb");
/// `CAbilityItemPermInvis`.
pub const ITEM_PERM_INVIS: u32 = code(b"AIpi");
/// `CAbilityItemPlaceMine` (Goblin Land Mine).
pub const ITEM_PLACE_MINE: u32 = code(b"AIpm");
/// `CAbilityItemRecall` (Amulet of Recall).
pub const ITEM_RECALL: u32 = code(b"AIrt");
/// `CAbilityItemRevealMap` (Potion of Omniscience).
pub const ITEM_REVEAL_MAP: u32 = code(b"AIrv");
/// `CAbilitySpiritTroll` (a `CAbilitySpiritWolf` child).
pub const ITEM_SPIRIT_TROLL: u32 = code(b"AIsh");
/// `CAbilityItemDetectAoe`, `CAbilityItemDetectByBuff`.
pub const ITEM_DETECT_AOE: u32 = code(b"AIta");
pub const ITEM_DETECT_BUFF: u32 = code(b"AItb");
/// `CAbilityItemTeleport` (the parent of `AItp`).
pub const ITEM_TELEPORT: u32 = code(b"AIte");
/// `CAbilityItemWeb`.
pub const ITEM_WEB: u32 = code(b"AIwb");
/// `CAbilityFreezeDamageBonus`.
pub const ORB_FREEZING: u32 = code(b"AIzb");
// S53 (spec section 17).
/// `CAbilityItemChangeTOD` (the Moonstone).
pub const ITEM_CHANGE_TOD: u32 = code(b"AIct");
/// `CAbilityItemVampirePotion` (the Potion of Vampirism).
pub const ITEM_VAMPIRE_POTION: u32 = code(b"AIpv");
/// `CAbilityPassiveDefense` (the Arcanite Shield; the passive form of Defend).
pub const ITEM_PASSIVE_DEFENSE: u32 = code(b"AIdd");
/// `CAbilityLightningPurge`: its unit effect (`0x424`) is a plain jump to the Purge effect `0x6F107E10`.
pub const ITEM_LIGHTNING_PURGE: u32 = code(b"AIlp");
/// `CAbilitySpellEffectBonus` (the orbs of slow, lightning and darkness): an on-hit proc of the spell named by `UnitID`.
pub const ITEM_SPELL_BONUS: u32 = code(b"AIsb");
/// The Moonstone effect object class and the vampire potion buff.
pub const BUFF_VAMPIRE_POTION: u32 = code(b"BIpv");
/// `CBuffFreeze` (the Orb of Freezing stuns with it; a `CBuffStun` child).
pub const BUFF_FREEZE: u32 = code(b"Bfre");
/// Buff codes of the S45 abilities (`BuffID` of the stock rows).
pub const BUFF_DEFENSE: u32 = code(b"Bdef");
pub const BUFF_DETECT: u32 = code(b"Bdet");
pub const BUFF_CORRUPTION: u32 = code(b"BIcb");
pub const BUFF_CLOAK: u32 = code(b"BIcf");
pub const BUFF_ILLUSION: u32 = code(b"BIil");
pub const BUFF_FLARE: u32 = code(b"Xfla");
pub const BUFF_ANIMATE_DEAD: u32 = code(b"BUan");

/// The behaviour class of an item ability base code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// `CAbilityItemHeal`: life by `DataA`.
    Heal,
    /// `CAbilityItemManaRestore`: mana by `DataA`.
    Mana,
    /// `CAbilityItemRestore`: life by `DataA`, then mana by `DataB`.
    Restore,
    HealAoe,
    ManaAoe,
    RestoreAoe,
    /// `CAbilityMaxLifeMod`: maximum life by the integer `DataA` of level 1 (life sink, mode 2).
    TomeLife,
    /// `CAbilityMaxManaMod`: the same for mana (mana sink, mode 2).
    TomeMana,
    /// `CAbilityExperienceMod`: experience by the integer `DataA` of level 1.
    TomeXp,
    /// `CAbilityLevelMod`: `DataA` levels, keeping the progress inside the level.
    TomeLevel,
    /// `CAbilityAttributeMod` and its three children: agility `DataA`, intelligence `DataB`, strength `DataC`.
    TomeAttrs,
    /// `CAbilityAttackMod`: permanent damage by the integer `DataA` of level 1.
    TomeAttack,
    Gold,
    Lumber,
    BonusAttack,
    BonusDefense,
    BonusAttackSpeed,
    BonusMoveSpeed,
    BonusLife,
    BonusMana,
    BonusSight,
    BonusAttrs,
    /// `CAbilityItemSpeed` / `CAbilityItemSpeedAoe`: the buff `Bspe` with movement `DataA` and attack speed `DataB`.
    Speed,
    SpeedAoe,
    /// `CAbilityItemTownPortal` (UNSURE model, see [`town_portal`]).
    TownPortal,
    /// `CAbilityRegeneration` (`AIrg`, a `CAbilityRejuvination` child): life `DataA` and mana `DataB` over the duration.
    Regen,
    /// `CAbilityRegenLife` (`Arel`, persistent): a life regeneration modifier of `DataA` per second while carried.
    BonusLifeRegen,
    /// `CAbilityItemRegenMana` (`AIrm`, persistent, heroes only): a mana regeneration modifier of `DataA` times the hero's
    /// base mana regeneration.
    BonusManaRegen,
    /// `CAbilityItemSpellReduction` (`AIsr`): Spells-type damage taken is scaled by `1 - DataB`.
    BonusSpellReduction,
    /// `CAbilityVampireBonus` (`AIva`): the carrier heals `DataA` times the damage its attacks deal.
    BonusVampire,
    /// `CAbilityFireDamageBonus` (`AIfb`, the Orb of Fire): `DataA` fire damage with `Area` splash after every hit.
    OrbFire,
    /// `CAbilityFrostDamageBonus` (`AIob`, the Orb of Frost): the Frost buff `Bfro` after every hit.
    OrbFrost,
    /// `CAbilityFigurine*` (`AIfs`, `AIfr`, `AIfu`; every figurine row of the item table uses the code `AIfs`): summons
    /// `f2i(DataA)` units of type `DataC` and `f2i(DataB)` of type `DataD` for `Dur` seconds.
    Figurine,
    /// `CAbilityItemReincarnation` (`AIrc`, the Ankh): a passive; the carrier that dies revives after `DataA` seconds and the
    /// item uses its charge (`0x6F2B8390`).
    ItemReincarnation,
    /// `CAbilityItemInvis` (`AIvi`): the buff `Binv` for the hero-rule duration.
    Invis,
    /// `CAbilityItemInvul` (`AIvu`): the buff `Bvul` for the hero-rule duration; the holder is invulnerable meanwhile.
    Invul,
    // ---- S45 (spec section 16) ----
    /// `CAbilityFlag` and the race flags: a capture-the-flag marker, no stat effect (LIKELY).
    Flag,
    /// `CAbilityItemPermInvis`: a permanent invisibility marker buff on the carrier (UNSURE).
    PermInvis,
    /// `CAbilityItemCloakOfFlames`: a repeating burn of `DataA` to the enemies in `Area` (UNSURE).
    CloakOfFlames,
    /// `CAbilityPoisonDamageBonus`: the poison attack buff after a hit (UNSURE duration).
    OrbVenom,
    /// `CAbilityCorruptionDamageBonus`: `-DataA` armor for the hero-rule duration after a hit (UNSURE).
    OrbCorruption,
    /// `CAbilityLightningDamageBonus`: `DataA` lightning damage to the victim after a hit (UNSURE).
    OrbLightning,
    /// `CAbilityFreezeDamageBonus`: the Frost buff after a hit (UNSURE; no stock row has a duration).
    OrbFreezing,
    /// `CAbilityItemDefenseAoe`: `+DataA` armor for the allies in `Area` (UNSURE).
    DefenseAoe,
    /// `CAbilityItemDamageAoe`: `DataA` damage to the enemies in `Area` (UNSURE; no stock row).
    DamageAoe,
    /// `CAbilityItemCureAoe`: the negative buffs of the allies in `Area` end (UNSURE; no stock row).
    CureAoe,
    /// `CAbilityItemDetectAoe` / `CAbilityItemDetectByBuff`: the marker buff `Bdet` on the enemies in `Area` (UNSURE).
    Detect,
    /// `CAbilityItemFlare`: a visibility reveal of `Area` around the target point for `Dur` (UNSURE).
    Flare,
    /// `CAbilityItemRevealMap`: the whole map is visible to the owner for `Dur` (UNSURE).
    RevealMap,
    /// `CAbilityGlyph`: the owner's tech `UnitID` goes up by `DataA` (UNSURE).
    Glyph,
    /// `CAbilityItemBuild`: a building of the owner's race (the `UnitID` list) is created at the point (UNSURE).
    BuildScroll,
    /// `CAbilityItemPlaceMine`: the `UnitID` unit is created at the point (UNSURE).
    PlaceMine,
    /// `CAbilityItemCommand`: the target changes owner when its creep level is at most `DataA` (UNSURE).
    Command,
    /// `CAbilityItemIllusion`: an illusion of the target for `Dur` (UNSURE).
    Illusion,
    /// `CAbilityItemAnimateDead`: up to `DataA` dead non-hero units of `Area` fight for the caster for `Dur` (UNSURE).
    AnimateDead,
    // ---- S53 (spec section 17) ----
    /// `CAbilityItemChangeTOD` (`AIct`): the time of day is set to `DataA + DataB / 60` hours for `Dur` seconds (the numbers
    /// are CONFIRMED, see [`moonstone_args`]; the end of the effect is UNSURE).
    Moonstone,
    /// `CAbilityItemVampirePotion` (`AIpv`): the buff `BIpv` with `DataA` bonus damage and `DataB` life steal.
    VampirePotion,
    /// `CAbilityPassiveDefense` (`AIdd`): damage of the Pierce row is scaled by `DataA`, Spells and Magic rows by `DataE`.
    PassiveDefense,
    /// `CAbilitySpellEffectBonus` (`AIsb`): after an armed hit a pseudo-random roll (stream 36) casts the spell of `UnitID`
    /// on the victim.
    SpellBonus,
}

impl Family {
    /// Whether the ability is cast (an order exists) rather than a passive bonus.
    pub fn castable(self) -> bool {
        !matches!(
            self,
            Family::BonusAttack
                | Family::BonusDefense
                | Family::BonusAttackSpeed
                | Family::BonusMoveSpeed
                | Family::BonusLife
                | Family::BonusMana
                | Family::BonusSight
                | Family::BonusAttrs
                | Family::BonusLifeRegen
                | Family::BonusManaRegen
                | Family::BonusSpellReduction
                | Family::BonusVampire
                | Family::OrbFire
                | Family::OrbFrost
                | Family::ItemReincarnation
                | Family::Flag
                | Family::PermInvis
                | Family::CloakOfFlames
                | Family::OrbVenom
                | Family::OrbCorruption
                | Family::OrbLightning
                | Family::OrbFreezing
                | Family::PassiveDefense
                | Family::SpellBonus
        )
    }

    /// The family of a factory base code.
    pub fn of_base(base: u32) -> Option<Family> {
        Some(match base {
            ITEM_HEAL => Family::Heal,
            ITEM_MANA => Family::Mana,
            ITEM_RESTORE => Family::Restore,
            ITEM_HEAL_AOE => Family::HealAoe,
            ITEM_MANA_AOE => Family::ManaAoe,
            ITEM_RESTORE_AOE => Family::RestoreAoe,
            TOME_LIFE => Family::TomeLife,
            TOME_MANA => Family::TomeMana,
            TOME_XP => Family::TomeXp,
            TOME_LEVEL => Family::TomeLevel,
            TOME_ATTRS | TOME_AGI | TOME_INT | TOME_STR => Family::TomeAttrs,
            TOME_ATTACK => Family::TomeAttack,
            GIVE_GOLD => Family::Gold,
            GIVE_LUMBER => Family::Lumber,
            BONUS_ATTACK => Family::BonusAttack,
            BONUS_DEFENSE => Family::BonusDefense,
            BONUS_ATTACK_SPEED => Family::BonusAttackSpeed,
            BONUS_MOVE_SPEED => Family::BonusMoveSpeed,
            BONUS_LIFE => Family::BonusLife,
            BONUS_MANA => Family::BonusMana,
            BONUS_SIGHT => Family::BonusSight,
            BONUS_ATTRS => Family::BonusAttrs,
            ITEM_SPEED => Family::Speed,
            ITEM_SPEED_AOE => Family::SpeedAoe,
            TOWN_PORTAL => Family::TownPortal,
            ITEM_REGEN => Family::Regen,
            BONUS_LIFE_REGEN => Family::BonusLifeRegen,
            BONUS_MANA_REGEN => Family::BonusManaRegen,
            BONUS_SPELL_REDUCTION => Family::BonusSpellReduction,
            BONUS_VAMPIRE => Family::BonusVampire,
            FIGURINE | FIGURINE_GOLEM | FIGURINE_GUARD | FIGURINE_DRAKE | FIGURINE_FURBOLG | FIGURINE_BASE
            | FIGURINE_HOUND => Family::Figurine,
            FLAG_BASE | FLAG_UNDEAD | FLAG_HUMAN | FLAG_NIGHT_ELF | FLAG_ORC => Family::Flag,
            ITEM_PERM_INVIS => Family::PermInvis,
            ITEM_CLOAK_OF_FLAMES => Family::CloakOfFlames,
            ORB_VENOM => Family::OrbVenom,
            ORB_CORRUPTION => Family::OrbCorruption,
            ORB_LIGHTNING => Family::OrbLightning,
            ORB_FREEZING => Family::OrbFreezing,
            ITEM_DEFENSE_AOE => Family::DefenseAoe,
            ITEM_DAMAGE_AOE => Family::DamageAoe,
            ITEM_CURE_AOE => Family::CureAoe,
            ITEM_DETECT_AOE | ITEM_DETECT_BUFF => Family::Detect,
            ITEM_FLARE => Family::Flare,
            ITEM_REVEAL_MAP => Family::RevealMap,
            ITEM_GLYPH => Family::Glyph,
            ITEM_BUILD => Family::BuildScroll,
            ITEM_PLACE_MINE => Family::PlaceMine,
            ITEM_COMMAND => Family::Command,
            ITEM_ILLUSION => Family::Illusion,
            ITEM_ANIMATE_DEAD => Family::AnimateDead,
            ITEM_CHANGE_TOD => Family::Moonstone,
            ITEM_VAMPIRE_POTION => Family::VampirePotion,
            ITEM_PASSIVE_DEFENSE => Family::PassiveDefense,
            ITEM_SPELL_BONUS => Family::SpellBonus,
            ITEM_RECALL | ITEM_TELEPORT => Family::TownPortal,
            ITEM_REINCARNATION => Family::ItemReincarnation,
            ORB_FIRE => Family::OrbFire,
            ORB_FROST => Family::OrbFrost,
            ITEM_INVIS => Family::Invis,
            ITEM_INVUL => Family::Invul,
            _ => return None,
        })
    }
}

/// What an item type needs for its abilities (`ItemData.slk`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemSpec {
    /// `abilList`: up to four alias ids, in list order.
    pub abilities: Vec<u32>,
    /// `perishable`: the item is destroyed when the last charge is used.
    pub perishable: bool,
    /// `usable`: the item can be used from the pack.
    pub usable: bool,
    /// `cooldownID`: the ability code whose cooldown the item shares (the real table names the base code, e.g. `AIhe`
    /// for every healing potion; 0 = none).
    pub cooldown_id: u32,
    /// `ignoreCD`: the item does not take part in the cooldown group.
    pub ignore_cooldown: bool,
    /// `uses`: the charges of a new item (0 = no charges).
    pub uses: i32,
    /// `powerup`: consumed when taken.
    pub powerup: bool,
    /// `sellable` and `pawnable`.
    pub sellable: bool,
    pub pawnable: bool,
    /// `level`: the item level (`GetItemLevel`).
    pub level: i32,
    /// Whether `install_item_abilities` also registers the item type (charges and flags) when it is missing.
    pub registers_type: bool,
}

/// Everything the engine loads for the item abilities: the item specs, the base code of every alias and the level
/// records of every alias.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemAbilityData {
    pub specs: BTreeMap<u32, ItemSpec>,
    pub bases: BTreeMap<u32, u32>,
    pub levels: BTreeMap<u32, Vec<SpellLevel>>,
    /// The unit type codes of `DataC` / `DataD` of the figurine rows (0 = the row has none).
    pub unit_codes: BTreeMap<u32, [u32; 2]>,
    /// The `UnitID` lists of the build scroll rows (the whole comma list, in row order).
    pub unit_lists: BTreeMap<u32, Vec<u32>>,
    /// The aura level records of the item aura rows by alias (the aura classes read `Area`, `DataA..D`, `BuffID`, `targs`).
    pub auras: BTreeMap<u32, Vec<crate::auras::AuraLevel>>,
}

/// The effect a persistent bonus applied (kept so that it is taken off exactly).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Applied {
    Armor(F32),
    AttackSpeed(F32),
    Damage(i32),
    MoveSpeed(F32),
    Life(F32),
    Mana(F32),
    Sight(F32),
    Attrs { agi: i32, int: i32, str: i32 },
    /// A regeneration modifier of the agile engine (or, without stats, the aura bonus fallback).
    Regen { mana: bool, rate: F32, modifier: Option<crate::agile::ModId> },
    /// A permanent marker buff (`AIpi`) or the Cloak of Flames buff (`AIcf`, with its burn timer record by serial).
    Buff { code: u32, cloak: bool },
}

/// State of the item abilities (owned by the spell system).
#[derive(Debug, Clone, Default)]
pub struct State {
    specs: BTreeMap<u32, ItemSpec>,
    bases: BTreeMap<u32, u32>,
    /// `alias -> cooldown group` for the aliases whose item names a group other than the alias itself.
    groups: BTreeMap<u32, u32>,
    /// The items that hold an ability on a unit.
    holders: BTreeMap<(ObjKey, u32), Vec<ObjKey>>,
    /// Persistent bonuses by `(item, alias)`.
    applied: BTreeMap<(ObjKey, u32), (ObjKey, Applied)>,
    /// Casts started by a use order and not ended yet: `(caster, alias) -> item`.
    pending: BTreeMap<(ObjKey, u32), ObjKey>,
    /// Regeneration buffs running by buff serial.
    regen: BTreeMap<u32, RegenRec>,
    /// The unit type codes of the figurine rows by alias.
    unit_codes: BTreeMap<u32, [u32; 2]>,
    /// The `UnitID` lists of the build scroll rows by alias (one unit per race, in the order of the row).
    unit_lists: BTreeMap<u32, Vec<u32>>,
    /// Units the S45 effects killed, `(killer, victim)`, waiting for the death events of the caller that owns the runtime.
    deaths: Vec<(ObjKey, ObjKey)>,
    /// Cloak of Flames burns by buff serial.
    cloaks: BTreeMap<u32, CloakRec>,
    /// The running Moonstone effect (S53): the saved time of day and the end timer.
    moonstone: Option<MoonstoneRec>,
    /// The pseudo-random counters of the Spell Effect Bonus abilities by `(unit, alias)`: one per victim kind (start at 1).
    spell_counts: BTreeMap<(ObjKey, u32), [i32; 3]>,
    /// The spell procs waiting for their timer tick: `id -> (attacker, victim, child alias)`.
    procs: BTreeMap<u32, (ObjKey, ObjKey, u32)>,
    next_proc: u32,
    /// The flat move speed bonus applied per unit by `AIms` abilities (the maximum, [`speed_flat_bonus`]).
    speed_bonus: BTreeMap<ObjKey, F32>,
    /// Visibility reveals of the flare and the potion of omniscience: id -> fog modifier.
    fogs: BTreeMap<u32, ObjKey>,
    next_fog: u32,
    /// The item ability ids that are aura classes (`AIae` -> Endurance, ...): the aura system treats them as the class.
    aura_alias: BTreeMap<u32, crate::auras::AuraKind>,
    /// Misc `IllusionsCanRestoreLife` (key CONFIRMED at `0x6F876B18`): illusions heal and carry life regeneration items.
    pub illusions_heal: bool,
    /// Misc `IllusionsCanRestoreMana` (key at `0x6F876B30`): the same for the mana regeneration item.
    pub illusions_mana: bool,
    /// Misc `IntRegenBonus` (read by `0x6F2082C0`): mana regeneration per point of hero intelligence.
    pub int_regen_bonus: F32,
}

/// The Moonstone effect: the time of day to restore, whether the map had suspended the clock, the end timer.
#[derive(Debug, Clone, Copy)]
struct MoonstoneRec {
    saved: F32,
    map_suspended: bool,
    timer: TimerId,
}

/// One Cloak of Flames: the burn per tick, the radius and the repeating timer.
#[derive(Debug, Clone, Copy)]
struct CloakRec {
    amount: F32,
    area: F32,
    timer: Option<TimerId>,
}

/// One running regeneration (`AIrg`): the amounts per 1.0 s tick and the repeating timer.
#[derive(Debug, Clone, Copy)]
struct RegenRec {
    life: F32,
    mana: F32,
    timer: Option<TimerId>,
}

impl State {
    fn base_of(&self, alias: u32) -> u32 {
        self.bases.get(&alias).copied().unwrap_or(alias)
    }

    fn family(&self, alias: u32) -> Option<Family> {
        Family::of_base(self.base_of(alias))
    }
}

// ---- registry API ----

impl Game {
    /// Installs the item specs, alias bases and level records (see [`ItemAbilityData`]).
    pub fn install_item_abilities(&mut self, data: &ItemAbilityData) {
        for (&ty, spec) in &data.specs {
            self.set_item_spec(ty, spec.clone());
            if self.item_types().get(ty).is_none() && spec.registers_type {
                let mut flags = 0;
                if spec.perishable {
                    flags |= item_flags::PERISHABLE;
                }
                if spec.powerup {
                    flags |= item_flags::POWERUP;
                }
                // Class and the random-selection flag stay 0 until the item rows are decoded fully: the type is
                // registered for creation (charges, flags), not for `ChooseRandomItem`.
                self.item_types_mut().insert(
                    ty,
                    ItemType {
                        class: 0,
                        level_ex: spec.level,
                        level_plain: spec.level,
                        flag_48: 0,
                        charges: spec.uses,
                        powerup: spec.powerup,
                        sellable: spec.sellable,
                        pawnable: spec.pawnable,
                        flags,
                    },
                );
            }
        }
        for (&alias, &base) in &data.bases {
            self.set_item_ability_base(alias, base);
        }
        for (&alias, levels) in &data.levels {
            self.set_spell_levels(alias, levels.clone());
        }
        for (&alias, codes) in &data.unit_codes {
            self.spells.item.unit_codes.insert(alias, *codes);
        }
        for (&alias, list) in &data.unit_lists {
            self.spells.item.unit_lists.insert(alias, list.clone());
        }
        for (&alias, levels) in &data.auras {
            let kind = data.bases.get(&alias).and_then(|&b| crate::auras::AuraKind::of(b));
            if let Some(kind) = kind {
                self.spells.item.aura_alias.insert(alias, kind);
                self.aura_types_mut().insert(alias, levels.clone());
            }
        }
    }

    /// Registers the abilities of item type `type_id`. The cooldown group of the item is its `cooldownID` (the key of the
    /// unit's group table, `0x6F2B8D70`): an alias of the item whose own id differs from the group is checked against
    /// the group when it is cast from a command card (the use path checks the item's group itself).
    pub fn set_item_spec(&mut self, type_id: u32, spec: ItemSpec) {
        if spec.cooldown_id != 0 {
            for &a in &spec.abilities {
                if a != spec.cooldown_id {
                    self.spells.item.groups.entry(a).or_insert(spec.cooldown_id);
                }
            }
        }
        self.spells.item.specs.insert(type_id, spec);
    }

    /// The Misc inputs of the regeneration items: `IntRegenBonus` and the two illusion switches.
    pub fn set_item_misc(&mut self, int_regen_bonus: F32, illusions_heal: bool, illusions_mana: bool) {
        let st = &mut self.spells.item;
        st.int_regen_bonus = int_regen_bonus;
        st.illusions_heal = illusions_heal;
        st.illusions_mana = illusions_mana;
    }

    /// Registers the factory base code of an alias ability id.
    pub fn set_item_ability_base(&mut self, alias: u32, base: u32) {
        self.spells.item.bases.insert(alias, base);
    }

    /// The aura class of an item ability id (see [`State::aura_alias`]).
    pub(crate) fn item_aura_kind(&self, alias: u32) -> Option<crate::auras::AuraKind> {
        self.spells.item.aura_alias.get(&alias).copied()
    }

    /// The item spec of a type.
    pub fn item_spec(&self, type_id: u32) -> Option<&ItemSpec> {
        self.spells.item.specs.get(&type_id)
    }
}

/// Whether `base` (an alias or a base code) is a castable item ability that takes no target (the cast core's
/// order-class gate): every castable family does, a unit target or a point is accepted too.
pub(crate) fn takes_no_target(gm: &Game, base: u32) -> bool {
    gm.spells.item.family(base).is_some_and(Family::castable)
}

/// The spell classes an item ability can borrow unchanged: the item row's `code` names the same class as the unit
/// spell (`AEbl` Dagger of Escape, `AHds` Potion of Divinity, `AHhb`, `AHmt` Staff of Teleportation, `AHre` and its
/// item child `AIrs`, `Arai` and `AIrd`, wards, Lightning Shield, Purge, Roar, Chain Lightning, Death Pact, Inferno):
/// the effect slots are the unit spell's own (the factory atlas lists the item child without an override of them), so
/// the unit spell's effect runs with the item's level record. CONFIRMED for `AIrs` (only the code and order getters
/// of `CAbilityResurrection` are overridden) and `AIrd` (the same effect slot `0x424` = `0x6F0E6AA0` as `Arai`; the
/// child only changes the autocast gate `0x380`); LIKELY for the others (same class).
const DELEGATED: [u32; 17] = [
    crate::unit_spells::DISPEL_MAGIC,
    crate::undead_ne::BLINK,
    crate::human_heroes::DIVINE_SHIELD,
    crate::cast::HOLY_LIGHT,
    crate::human_heroes::MASS_TELEPORT,
    crate::human_heroes::RESURRECTION,
    crate::unit_spells::RAISE_DEAD,
    crate::unit_spells::SENTRY_WARD,
    crate::unit_spells::HEALING_WARD,
    crate::unit_spells::LIGHTNING_SHIELD,
    crate::unit_spells::PURGE,
    crate::unit_spells::ROAR,
    crate::orc::CHAIN_LIGHTNING,
    crate::undead_ne::DEATH_PACT,
    crate::undead_ne::INFERNO,
    // S45: `AIsh` (`CAbilitySpiritTroll < CAbilitySpiritWolf`) and `AIwb` (`CAbilityItemWeb`) run the unit spell.
    crate::orc::FERAL_SPIRIT,
    crate::unit_spells::WEB,
];

/// The unit spell class an S45 item class borrows by its atlas parent (`AIdc` -> Dispel Magic, `AIsh` -> Feral Spirit,
/// `AImo` -> the ward effect, `AIwb` -> Web); `None` for every other base code.
pub fn borrowed_class(base: u32) -> Option<u32> {
    match base {
        ITEM_DISPEL_CHAIN => Some(crate::unit_spells::DISPEL_MAGIC),
        ITEM_SPIRIT_TROLL => Some(crate::orc::FERAL_SPIRIT),
        ITEM_MONSTER_LURE => Some(crate::unit_spells::SENTRY_WARD),
        ITEM_WEB => Some(crate::unit_spells::WEB),
        // S53: the unit effect slot `0x424` of `CAbilityLightningPurge` is `jmp 0x6F107E10`, the Purge effect (CONFIRMED).
        ITEM_LIGHTNING_PURGE => Some(crate::unit_spells::PURGE),
        _ => None,
    }
}

/// The class code that runs an ability id's effect: the factory base code of the alias, with the two item classes that
/// inherit a spell class mapped to it (`AIrs` -> `AHre`, `AIrd` -> `Arai`). An id with no registered base maps to itself.
pub(crate) fn effect_base(gm: &Game, alias: u32) -> u32 {
    match gm.spells.item.base_of(alias) {
        ITEM_RESURRECTION => crate::human_heroes::RESURRECTION,
        ITEM_RAISE_DEAD => crate::unit_spells::RAISE_DEAD,
        // `AIdi` (CAbilityItemDispelAoe) overrides only the code and order getters of `CAbilityDispelMagic`; the area
        // effect slot `0x418` is `0x6F0EF320` in both.
        ITEM_DISPEL => crate::unit_spells::DISPEL_MAGIC,
        // S45 (LIKELY: the atlas parents; the effect slots `0x424` of these classes are their own, not decoded).
        b => borrowed_class(b).unwrap_or(b),
    }
}

/// Whether the ability id is an item ability that borrows a unit spell class ([`DELEGATED`]).
pub(crate) fn delegated(gm: &Game, alias: u32) -> bool {
    gm.spells.item.bases.contains_key(&alias) && DELEGATED.contains(&effect_base(gm, alias))
}

/// `rec` with the alias replaced by its class code, for the unit spell modules (they match on the class code).
pub(crate) fn resolve_rec(gm: &Game, rec: &CastRec) -> CastRec {
    if delegated(gm, rec.base) {
        CastRec {
            base: effect_base(gm, rec.base),
            ..*rec
        }
    } else {
        *rec
    }
}

/// Whether the cooldown group of `base` is cooling on `unit` (UNSURE: the group is the item's `cooldownID`).
pub(crate) fn group_cooling(gm: &Game, unit: ObjKey, base: u32) -> bool {
    gm.spells.item.groups.get(&base).is_some_and(|&g| gm.is_cooling(unit, g))
}

// ---- the pack hooks ----

fn level_of(gm: &Game, alias: u32) -> SpellLevel {
    gm.spells_level_record(alias, 0)
}

/// Taking `item` into the pack of `unit`: its abilities are attached and its persistent bonuses applied.
pub(crate) fn on_pack_add(gm: &mut Game, unit: ObjKey, item: ObjKey) {
    let ty = gm.item(item).type_id;
    let Some(spec) = gm.spells.item.specs.get(&ty).cloned() else {
        return;
    };
    for alias in spec.abilities {
        if !gm.unit(unit).abilities.contains(alias) {
            gm.unit_mut(unit).abilities.attach(Ability::new(alias, None));
        }
        gm.spells.item.holders.entry((unit, alias)).or_default().push(item);
        if let Some(fam) = gm.spells.item.family(alias) {
            if !fam.castable() {
                apply_passive(gm, unit, item, alias, fam);
            }
        }
    }
    // An aura ability among them starts its emitter (`auras::sync_auras`).
    gm.sync_auras(unit);
}

/// `item` leaves the pack of `unit` (dropped, removed or consumed): its persistent bonuses come off and the abilities
/// it alone held are removed from the unit.
pub(crate) fn on_pack_remove(gm: &mut Game, unit: ObjKey, item: ObjKey) {
    let ty = gm.item(item).type_id;
    let Some(spec) = gm.spells.item.specs.get(&ty).cloned() else {
        return;
    };
    for alias in spec.abilities {
        if let Some((u, applied)) = gm.spells.item.applied.remove(&(item, alias)) {
            undo_applied(gm, u, applied);
        }
        let empty = {
            let list = gm.spells.item.holders.entry((unit, alias)).or_default();
            list.retain(|&i| i != item);
            list.is_empty()
        };
        if empty {
            gm.spells.item.holders.remove(&(unit, alias));
            gm.unit_mut(unit).abilities.remove(alias);
        }
    }
    gm.sync_auras(unit);
}

/// A powerup taken by `unit` (consumed at once): its castable abilities run their effect on the unit with no mana,
/// cooldown or charge (UNSURE: the original adds the item abilities to the unit through `0x6F2B9600` and the
/// ability acts at once).
pub(crate) fn on_powerup(gm: &mut Game, unit: ObjKey, item: ObjKey) {
    let ty = gm.item(item).type_id;
    let Some(spec) = gm.spells.item.specs.get(&ty).cloned() else {
        return;
    };
    for alias in spec.abilities {
        let Some(fam) = gm.spells.item.family(alias) else {
            continue;
        };
        if fam.castable() {
            let level = level_of(gm, alias);
            apply_effect(gm, fam, alias, &level, unit, CastTarget::Unit(unit));
        }
    }
}

// ---- persistent bonuses ----

fn apply_passive(gm: &mut Game, unit: ObjKey, item: ObjKey, alias: u32, fam: Family) {
    let level = level_of(gm, alias);
    let a = level.data[0];
    let applied = match fam {
        Family::BonusAttack => {
            let v = damage_bonus_value(a);
            gm.add_damage(unit, v);
            Applied::Damage(v)
        }
        Family::BonusDefense => {
            gm.add_armor(unit, a);
            Applied::Armor(a)
        }
        Family::BonusAttackSpeed => {
            gm.add_attack_speed(unit, a);
            Applied::AttackSpeed(a)
        }
        // `AIms`: the unit's flat bonus is the MAXIMUM over its abilities (`0x6F077590`, Unicorn `MS`), so the sum is
        // refreshed after the record is stored (`refresh_speed_bonus`).
        Family::BonusMoveSpeed => Applied::MoveSpeed(a),
        Family::BonusLife => {
            life_sink_delta(gm, unit, a, 1);
            Applied::Life(a)
        }
        Family::BonusMana => {
            mana_sink_delta(gm, unit, a, 1);
            Applied::Mana(a)
        }
        Family::BonusSight => {
            add_sight(gm, unit, a);
            Applied::Sight(a)
        }
        Family::BonusAttrs => {
            let (agi, int, str) = attribute_values(&level.data);
            add_attributes(gm, unit, agi, int, str);
            Applied::Attrs { agi, int, str }
        }
        Family::BonusLifeRegen => {
            let illusion = gm.unit(unit).illusion;
            if !regen_item_gate(illusion, gm.spells.item.illusions_heal) {
                return;
            }
            let modifier = gm.add_life_regen_modifier(unit, a);
            if modifier.is_none() {
                gm.add_life_regen(unit, a);
            }
            Applied::Regen { mana: false, rate: a, modifier }
        }
        Family::BonusManaRegen => {
            // `0x6F0FB370`: heroes only, and the illusion gate with the mana switch.
            let Some(h) = gm.unit(unit).hero.as_ref() else {
                return;
            };
            let illusion = gm.unit(unit).illusion;
            if !regen_item_gate(illusion, gm.spells.item.illusions_mana) {
                return;
            }
            let int_total = heroes::intelligence(h.int_base, h.int_plus, h.level - 1);
            let type_regen = gm.unit_types().get(gm.unit(unit).type_id).map_or(F32::ZERO, |t| t.regen_mana);
            let term = hero_int_regen_term(gm.spells.item.int_regen_bonus, int_total);
            let rate = mana_regen_item_rate(type_regen, term, a);
            let modifier = gm.add_mana_regen_modifier(unit, rate);
            if modifier.is_none() {
                gm.add_mana_regen(unit, rate);
            }
            Applied::Regen { mana: true, rate, modifier }
        }
        Family::PermInvis => {
            let preds = crate::abilities::BuffPredicates {
                positive: true,
                magic: true,
                ..crate::abilities::buff_predicates(BUFF_INVIS)
            };
            gm.add_buff(unit, BUFF_INVIS, preds);
            gm.invisibility_note(unit);
            Applied::Buff { code: BUFF_INVIS, cloak: false }
        }
        // The orbs (`CAbilityDamageBonusBase < CAbilityAttackBonus`): the apply slot `0x80` = `0x6F0FD580` calls the
        // attack bonus apply (`0x6F0FA7F0` -> `0x6F0F4100`, the `AIat` body), so a carried orb adds `F2I(DataA)` to the
        // attack damage on top of its on-hit effect (Orb of Frost +6, Orb of Fire +5, ...).
        Family::OrbFire
        | Family::OrbFrost
        | Family::OrbCorruption
        | Family::OrbVenom
        | Family::OrbLightning
        | Family::OrbFreezing
        | Family::SpellBonus => {
            let v = damage_bonus_value(a);
            gm.add_damage(unit, v);
            Applied::Damage(v)
        }
        Family::CloakOfFlames => {
            start_cloak(gm, unit, &level);
            Applied::Buff { code: BUFF_CLOAK, cloak: true }
        }
        _ => return,
    };
    let speed = matches!(applied, Applied::MoveSpeed(_));
    gm.spells.item.applied.insert((item, alias), (unit, applied));
    if speed {
        refresh_speed_bonus(gm, unit);
    }
}

/// The flat move speed bonus of `unit` follows the maximum `DataA` of its `AIms` abilities ([`speed_flat_bonus`], CONFIRMED
/// by Unicorn `MS`: `0x6F077590` takes the maximum of the abilities' slot `0x184` values starting from 0.0, and the speed
/// computation `0x6F2019F0` adds that bonus to the base before the multiplier). The unit's speed gets the difference to the
/// bonus applied before. The Misc switch `MoveSpeedBonusesStack` (absent from the shipped MiscData, so off) is not modelled.
fn refresh_speed_bonus(gm: &mut Game, unit: ObjKey) {
    let values: Vec<F32> = gm
        .spells
        .item
        .applied
        .values()
        .filter(|(u, _)| *u == unit)
        .filter_map(|(_, a)| match a {
            Applied::MoveSpeed(v) => Some(*v),
            _ => None,
        })
        .collect();
    let bonus = speed_flat_bonus(&values);
    let old = gm.spells.item.speed_bonus.get(&unit).copied().unwrap_or(F32::ZERO);
    if bonus.to_bits() != old.to_bits() {
        add_move_speed(gm, unit, arith::sub(bonus, old));
        gm.spells.item.speed_bonus.insert(unit, bonus);
    }
}

fn undo_applied(gm: &mut Game, unit: ObjKey, applied: Applied) {
    match applied {
        Applied::Damage(v) => gm.add_damage(unit, v.wrapping_neg()),
        Applied::Armor(a) => gm.add_armor(unit, arith::neg(a)),
        Applied::AttackSpeed(a) => gm.add_attack_speed(unit, arith::neg(a)),
        Applied::MoveSpeed(_) => refresh_speed_bonus(gm, unit),
        Applied::Life(a) => life_sink_delta(gm, unit, arith::neg(a), 1),
        Applied::Mana(a) => mana_sink_delta(gm, unit, arith::neg(a), 1),
        Applied::Sight(a) => add_sight(gm, unit, arith::neg(a)),
        Applied::Attrs { agi, int, str } => {
            add_attributes(gm, unit, agi.wrapping_neg(), int.wrapping_neg(), str.wrapping_neg())
        }
        Applied::Buff { code, cloak } => {
            if cloak {
                stop_cloak(gm, unit);
            }
            gm.remove_buff_code(unit, code);
        }
        Applied::Regen { modifier: Some(m), .. } => gm.release_modifier(m),
        Applied::Regen { mana, rate, modifier: None } => {
            if mana {
                gm.add_mana_regen(unit, arith::neg(rate));
            } else {
                gm.add_life_regen(unit, arith::neg(rate));
            }
        }
    }
}

/// The move speed bonus of `AIms` (UNSURE: the sink `0x6F201D50` and the Misc switch `MoveSpeedBonusesStack` are not
/// decoded; the model adds the bonus to the unit's speed).
fn add_move_speed(gm: &mut Game, unit: ObjKey, delta: F32) {
    let u = gm.unit_mut(unit);
    u.move_speed = arith::add(u.move_speed, delta);
    gm.mover_speed_changed(unit);
}

/// The sight bonus of `AIsi` (UNSURE: the sink `0x6F27A4E0` divides by the constant `0x6FAAE50C` and writes a property;
/// the model adds the value to the day and night radii).
fn add_sight(gm: &mut Game, unit: ObjKey, delta: F32) {
    let u = gm.unit_mut(unit);
    u.sight = arith::add(u.sight, delta);
    u.night_sight = arith::add(u.night_sight, delta);
}

/// The life sink `0x6F2AC450(unit, &value, mode)` on a unit (the maximum life changes, the life follows by the mode).
fn life_sink_delta(gm: &mut Game, unit: ObjKey, value: F32, mode: u32) {
    let flagged = gm.unit(unit).dead;
    let mut cells = gm.life_cells(unit);
    let tables = gm.order_tables().cloned();
    heroes::life_sink(&mut cells, flagged, value, mode, tables.as_ref());
    gm.put_life_cells(unit, cells);
}

/// The mana sink `0x6F278F50(unit, &value, mode)`.
fn mana_sink_delta(gm: &mut Game, unit: ObjKey, value: F32, mode: u32) {
    let mut cells = gm.mana_cells(unit);
    let tables = gm.order_tables().cloned();
    heroes::mana_sink(&mut cells, value, mode, tables.as_ref());
    gm.put_mana_cells(unit, cells);
}

/// The hero attribute adders `0x6F20A1E0` (agility), `0x6F20A260` (intelligence), `0x6F20A160` (strength) with the
/// show flag 0: the base counter grows and the derived values are refreshed (`natives::hero::change_hero_attributes`).
fn add_attributes(gm: &mut Game, unit: ObjKey, agi: i32, int: i32, str: i32) {
    if gm.unit(unit).hero.is_none() {
        return;
    }
    crate::natives::hero::change_hero_attributes(gm, unit, false, |h| {
        h.agi_base = h.agi_base.wrapping_add(agi);
        h.int_base = h.int_base.wrapping_add(int);
        h.str_base = h.str_base.wrapping_add(str);
    });
}

// ---- the pure cores (Unicorn-checked against the original, `tools/emu/item_cores_emu.py`) ----

/// The illusion gate of the regeneration items (`0x6F0F4840` for `Arel`, `0x6F0F4AE0` for `AIrm`, Unicorn `RL` rows): the
/// modifier is made unless the carrier is an illusion and the Misc switch (`IllusionsCanRestoreLife` /
/// `IllusionsCanRestoreMana`) is off.
pub fn regen_item_gate(illusion: bool, misc: bool) -> bool {
    !illusion || misc
}

/// The damage handler of `AIsr` (`0x6F0F5730`, Unicorn `SR` rows): the factor `1 - DataB` (soft subtract from `1.0`) that
/// `0x6F255800` applies to the record, or `None` when the handler leaves the record alone: no record, an illusion
/// carrier, record flag `0x10`, or a record type row other than 0 (Spells).
pub fn spell_reduction_factor(has_record: bool, carrier_illusion: bool, record_flags: u32, record_row: u32, data_b: F32) -> Option<F32> {
    if !has_record || carrier_illusion || record_flags & 0x10 != 0 || record_row != 0 {
        None
    } else {
        Some(arith::sub(F32::ONE, data_b))
    }
}

/// The record scale `0x6F255800` (Unicorn `RS` rows): the two amounts of the record times the factor.
pub fn scale_record(a: F32, b: F32, factor: F32) -> (F32, F32) {
    (arith::mul(a, factor), arith::mul(b, factor))
}

/// The inputs of the life steal handler of `AIva` (`0x6F0F53A0`, Unicorn `VA` rows).
#[derive(Debug, Clone, Copy, Default)]
pub struct LifeStealInput {
    /// The pre-damage handler armed the ability (flag `0x80`).
    pub armed: bool,
    /// The target filter (`0x6F26D590`) refused the victim.
    pub filter_refused: bool,
    /// The victim answers the immunity query (`0x6F28D750`).
    pub immune: bool,
    /// The victim resolved (`0x6F282F60`).
    pub victim: bool,
    pub owner_illusion: bool,
    pub victim_illusion: bool,
    /// Misc `IllusionsCanRestoreLife`.
    pub misc: bool,
}

/// The life steal handler of `AIva`: `(effect shown, heal)`. The heal is `DataA * damage` (soft multiply) for a damage
/// above 0.0, unless an illusion on either side blocks it while the Misc switch is off; the effect routine
/// (`0x6F053040`) is called whenever the gates up to the damage compare pass.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn life_steal_outcome(i: &LifeStealInput, damage: F32, data_a: F32) -> (bool, Option<F32>) {
    // The compare is an x87 `fcompp` + `test ah, 1`: C0 is set for 0.0 < damage and for an unordered pair, so a NaN
    // damage passes the gate (Unicorn `VA` rows).
    let positive = !(0.0f32 >= f32::from_bits(damage.to_bits()));
    if !i.armed || i.filter_refused || (i.victim && i.immune) || !positive {
        return (false, None);
    }
    let heal = arith::mul(data_a, damage);
    let blocked = !i.misc && (i.owner_illusion || (i.victim && i.victim_illusion));
    (true, (!blocked).then_some(heal))
}

/// The duration the Orb of Frost hands to the Frost buff (`0x6F108FC0`, Unicorn `FR` rows): `HeroDur` for a hero victim,
/// else `Dur` as it is (no extra term), plus the carrier's illusion bit.
pub fn orb_frost_duration(level: &SpellLevel, hero: bool) -> F32 {
    if hero {
        level.hero_dur
    } else {
        level.dur
    }
}

/// The damage type index of the orb damage: the record class `0x4000000` that the slot `0x324` of the damage bonus
/// classes returns (`0x6F100880` for `AIfb`) is `1 << 26`.
pub const ORB_DAMAGE_TYPE: u32 = 26;

// ---- S53 cores (Unicorn rows `item_ability_vectors3.txt`, harness `tools/emu/item_cores3_emu.py`) ----

/// The record class word `0x4000000` of the orb damage (`CAbilityDamageBonusBase` slot `0x324`).
pub const ORB_CLASS_WORD: u32 = 0x0400_0000;

/// The arguments the Moonstone hands to the effect start (`CAbilityItemChangeTOD` slot `0x3C0` = `0x6F109EB0`, Unicorn `CT`
/// rows, 350): the time of day `DataA + DataB / 60.0` (hours and minutes; soft divide, soft add) and the duration `extra +
/// Dur` (no hero rule). The start `0x6F104570` runs only when the duration is above 0.0 (x87 compare: a NaN does not).
/// `div` is the soft-float divide of the host.
pub fn moonstone_args(level: &SpellLevel, extra: F32, div: &dyn Fn(F32, F32) -> F32) -> Option<(F32, F32)> {
    let minutes = div(level.data[1], F32::from_bits(0x4270_0000));
    let tod = arith::add(level.data[0], minutes);
    let dur = arith::add(extra, level.dur);
    (0.0f32 < f32::from_bits(dur.to_bits())).then_some((tod, dur))
}

/// The arguments of the Potion of Vampirism's buff apply (`CAbilityItemVampirePotion` slot `0x428` = `0x6F109F70`, Unicorn
/// `PV` rows, 350): the duration by the hero rule, then `DataB` (the life steal fraction, holder `+0xCC`/`+0xD0`), then
/// `DataA` (the bonus damage, holder `+0xD4`/`+0xD8`).
pub fn vampire_potion_args(level: &SpellLevel, hero: bool, extra: F32) -> (F32, F32, F32) {
    (crate::cast::effect_duration(level, hero, extra), level.data[1], level.data[0])
}

/// The gates of the vampire buff's damage handler (`CBuffItemVampirePotion` slot `0x170` = `0x6F0FB7D0`, Unicorn `VP`).
#[derive(Debug, Clone, Copy, Default)]
pub struct VampireHitInput {
    /// The victim is mechanical (`+0x248` bit 3).
    pub victim_mechanical: bool,
    /// The relation test `0x6F285B20` answered yes (an enemy).
    pub relation: bool,
    /// The Misc switch at `0x6F8B4A5C`: with it the victim must not answer the vtable `0x11C` query or the immunity query.
    pub misc_immunity_gate: bool,
    pub victim_slot_11c: bool,
    pub victim_immune: bool,
    /// Misc `IllusionsCanRestoreLife`.
    pub misc_illusions_heal: bool,
    /// The buff gate `0x6F0F57A0`: the owner is an illusion and the Misc key `0x6F8B288C` is off.
    pub owner_gate: bool,
    pub victim_illusion: bool,
}

/// The heal of the vampire buff: `(effect shown, heal)`. The heal is `ratio * damage` for a damage above 0.0 (a NaN
/// passes, x87 compare) unless the owner gate or an illusion victim with the Misc switch off blocks it.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn vampire_potion_heal(i: &VampireHitInput, ratio: F32, damage: F32) -> (bool, Option<F32>) {
    if i.victim_mechanical || !i.relation {
        return (false, None);
    }
    if i.misc_immunity_gate && (i.victim_slot_11c || i.victim_immune) {
        return (false, None);
    }
    if !(0.0f32 >= f32::from_bits(damage.to_bits())) {
        let heal = arith::mul(ratio, damage);
        let blocked = i.owner_gate || (!i.misc_illusions_heal && i.victim_illusion);
        return (true, (!blocked).then_some(heal));
    }
    (false, None)
}

/// What the Orb of Fire hands to the area body after an armed hit (`CAbilityFireDamageBonus` slot `0x154` = `0x6F0F2CD0`,
/// Unicorn `FB` rows, 350): one shape `{Area, value}` (the value is the ability's `+0x88`, `DataA`), the class word
/// `0x4000000`, the target word `targs | 0x10000` (enemies), the victim excluded by the callback `0x6F0DC570`, and the same
/// amount delivered to the victim with the record's flag `0x100`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrbFirePlan {
    pub area: F32,
    pub value: F32,
    pub flags: u32,
    pub class: u32,
    /// The flag the victim delivery gets (`record flags & 0x100`).
    pub deliver_flag: u32,
}

/// The plan of [`OrbFirePlan`]; `None` when the pre-damage handler did not arm the ability (flag `0x100`).
pub fn orb_fire_plan(armed: bool, area: F32, value: F32, targs: u32, record_flags: u32) -> Option<OrbFirePlan> {
    armed.then_some(OrbFirePlan {
        area,
        value,
        flags: targs | 0x10000,
        class: ORB_CLASS_WORD,
        deliver_flag: record_flags & 0x100,
    })
}

/// The Orb of Freezing apply (`CAbilityFreezeDamageBonus` slot `0x154` = `0x6F108E20`, Unicorn `ZB` rows, 350): the stun
/// buff `Bfre` (`CBuffFreeze < CBuffStun`) for the hero-rule duration (`HeroDur` for a hero victim, else `Dur`, no extra term
/// and no positivity check) with the carrier's illusion bit; `None` when not armed.
pub fn orb_freeze_plan(level: &SpellLevel, armed: bool, hero: bool, owner_illusion: bool) -> Option<(F32, bool)> {
    armed.then(|| (orb_frost_duration(level, hero), owner_illusion))
}

/// The Orb of Corruption apply (`CAbilityCorruptionDamageBonus` slot `0x154` = `0x6F109B40`, Unicorn `CR` rows, 350): the
/// buff `BIcb` for the hero-rule duration with the amount `DataB` (`0.0` when the carrier is an illusion); the buff takes
/// the amount off the victim's armor unless the victim is an illusion (`0x6F0F3A90`). `None` when not armed.
pub fn orb_corruption_plan(level: &SpellLevel, armed: bool, hero: bool, owner_illusion: bool) -> Option<(F32, F32)> {
    armed.then(|| (orb_frost_duration(level, hero), if owner_illusion { F32::ZERO } else { level.data[1] }))
}

/// Whether the Orb of Lightning (old) fires the carrier's `AIlp` ability on the victim (`CAbilityLightningDamageBonus` slot
/// `0x154` = `0x6F0F2F70`, Unicorn `LB` rows, 350): armed, the victim resolves, the carrier is not an illusion, the carrier
/// has an `AIlp` ability and its blocked query (slot `0x42C`) answers no.
pub fn orb_lightning_fires(armed: bool, victim: bool, owner_illusion: bool, ability_found: bool, blocked: bool) -> bool {
    armed && victim && !owner_illusion && ability_found && !blocked
}

/// The buff arguments the Wand of Illusion hands to the illusion (`CAbilityItemIllusion` slot `0x424` = `0x6F13DFD0`, Unicorn
/// `IL` rows, 350): the duration by the hero rule of the TARGET (`HeroDur` for a hero, else `Dur` plus the extra term, soft
/// add), then `DataA` (the damage the illusion deals, factor) and `DataB` (the damage it takes, factor), then the caster.
pub fn illusion_args(level: &SpellLevel, target_hero: bool, extra: F32) -> (F32, F32, F32) {
    (crate::cast::effect_duration(level, target_hero, extra), level.data[0], level.data[1])
}

/// The victim class the Spell Effect Bonus picks its chance by (`0x6F0E5080`, Unicorn `SB`): 1 for a hero type (the first
/// character of the type code is an upper-case letter), else 2 when the classification word has bit 2 (summoned), else 0.
pub fn spell_bonus_kind(type_first_byte: u8, class_word: u32) -> usize {
    if type_first_byte.wrapping_sub(0x41) <= 0x19 {
        1
    } else {
        ((class_word & 4) >> 1) as usize
    }
}

/// The chance in percent of a kind (`0x6F0E50B0`): `DataB` for kind 0, `DataC` for the heroes, `DataD` for the summoned.
pub fn spell_bonus_chance(level: &SpellLevel, kind: usize) -> F32 {
    match kind {
        1 => level.data[2],
        2 => level.data[3],
        _ => level.data[1],
    }
}

/// The proc roll of the Spell Effect Bonus (`CAbilitySpellEffectBonus` slot `0x124` = `0x6F0F3DD0`, Unicorn `SB` rows, 350):
/// the pseudo-random distribution of Bash (`human_heroes::prd_effective_chance`: the table constant of the chance times the
/// count of rolls since the last proc, one count per kind) against a draw of RNG stream 36 (`0x6F2853D0`, `edx = 0x24`);
/// the proc is `draw <= effective` (`0x6F01FD60`). A proc sets the armed flag `0x400` and resets the kind's count to 1; a
/// miss adds 1. Returns `(proc, new count)`.
pub fn spell_bonus_roll(level: &SpellLevel, kind: usize, count: i32, draw: F32, tables: &Tables) -> (bool, i32) {
    spell_bonus_outcome(spell_bonus_effective(level, kind, count, tables), count, draw)
}

/// The effective chance of a roll (the first half of [`spell_bonus_roll`]).
pub fn spell_bonus_effective(level: &SpellLevel, kind: usize, count: i32, tables: &Tables) -> F32 {
    crate::human_heroes::prd_effective_chance(spell_bonus_chance(level, kind), count, tables)
}

/// The decision of a roll against its effective chance (the second half of [`spell_bonus_roll`]).
pub fn spell_bonus_outcome(effective: F32, count: i32, draw: F32) -> (bool, i32) {
    let hit = arith::le(draw, effective);
    (hit, if hit { 1 } else { count.wrapping_add(1) })
}

/// The flat move speed bonus of a unit's abilities (`0x6F077590`, Unicorn `MS` rows, 350): the maximum of the slot `0x184`
/// values (`AIms`: `DataA`) starting from 0.0, so boots do **not** stack and a negative bonus counts as none. The x87
/// compare replaces the current value when it is below the new one or the pair is unordered (a NaN).
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn speed_flat_bonus(values: &[F32]) -> F32 {
    let mut cur = F32::ZERO;
    for &v in values {
        if !(f32::from_bits(cur.to_bits()) >= f32::from_bits(v.to_bits())) {
            cur = v;
        }
    }
    cur
}

/// The factor the passive defense handler scales both record amounts by (`CAbilityPassiveDefense` slot `0xFC` = `0x6F098F90`,
/// Unicorn `DD` rows, 350): for the Pierce row (2) `DataA` (`DataG` when armed), for the Spells and Magic rows (0 and 4)
/// `DataE` (`DataH` when armed); a value that is exactly 1.0 falls back to the plain one and a final 1.0 changes nothing.
/// `None` when no multiply happens. The armed flag (`0x200`, set by the slot `0x160` handler) is cleared by the handler.
pub fn passive_defense_factor(row: u32, armed: bool, a: F32, e: F32, g: F32, h: F32) -> Option<F32> {
    let one = 1.0f32;
    let mut v = one;
    if armed {
        v = match row {
            0 | 4 => f32::from_bits(h.to_bits()),
            2 => f32::from_bits(g.to_bits()),
            _ => one,
        };
    }
    let mut bits = v.to_bits();
    if v == one {
        bits = match row {
            0 | 4 => e.to_bits(),
            2 => a.to_bits(),
            _ => bits,
        };
    }
    (f32::from_bits(bits) != one || f32::from_bits(bits).is_nan()).then_some(F32::from_bits(bits))
}

/// The summon batches of a figurine (`0x6F0B8A00`, Unicorn `FG` rows): the unit type of `DataC` times `f2i(DataA)` and
/// the unit type of `DataD` times `f2i(DataB)` (a batch whose type is the empty marker `0x5F5F5F5F` is skipped), each
/// with the lifetime `Dur + extra` (soft add) and the `Area`.
pub fn figurine_batches(level: &SpellLevel, codes: [u32; 2], extra: F32) -> Vec<(u32, i32, F32, F32)> {
    let dur = arith::add(level.dur, extra);
    let mut out = Vec::new();
    for (code, count) in [(codes[0], conv::f2i(level.data[0])), (codes[1], conv::f2i(level.data[1]))] {
        if code != 0x5F5F_5F5F && code != 0 {
            out.push((code, count, dur, level.area));
        }
    }
    out
}

/// The hero term of the mana regeneration item (`0x6F2082C0`, Unicorn `RH` rows): `Misc IntRegenBonus * I2R(int)`.
pub fn hero_int_regen_term(misc: F32, int_total: i32) -> F32 {
    arith::mul(misc, conv::i2r(int_total))
}

/// The rate of the mana regeneration item (`0x6F0E64E0`, Unicorn `RM` rows): `(type regenMana + hero term) * DataA`.
pub fn mana_regen_item_rate(type_regen: F32, hero_term: F32, data_a: F32) -> F32 {
    arith::mul(arith::add(type_regen, hero_term), data_a)
}

/// Where a restoration amount goes: the life adder `0x6F277C20` or the mana adder `0x6F279140`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sink {
    Life,
    Mana,
}

/// The sink calls of a restoration effect body (`0x6F0E3990`, `0x6F119F10`, `0x6F1DBB10`): the amounts are the level
/// data as they are, in this order (Unicorn: no ethereal factor, the class slot `0x42C` answers 0 for all three).
pub fn restore_plan(fam: Family, d: &[F32; 9]) -> Vec<(Sink, F32)> {
    match fam {
        Family::Heal | Family::HealAoe => vec![(Sink::Life, d[0])],
        Family::Mana | Family::ManaAoe => vec![(Sink::Mana, d[0])],
        Family::Restore | Family::RestoreAoe => vec![(Sink::Life, d[0]), (Sink::Mana, d[1])],
        _ => Vec::new(),
    }
}

/// The area callback `0x6F0F0470`: the candidate is skipped when it is immune, or when it is an illusion and the Misc
/// switch is off; otherwise the life adder gets `DataA` (the callback always returns 1).
pub fn area_callback_amount(illusion: bool, misc: bool, immune: bool, a: F32) -> Option<F32> {
    if immune || (!misc && illusion) {
        None
    } else {
        Some(a)
    }
}

/// The Rune of Mana area callback `0x6F1269E0` (`AImr`): a candidate immune to the ability's order (`0x6F28D750`) or whose
/// maximum mana is below `2e-7` in absolute value (x87 compare against `0x6FAAE614`: NaN passes) is skipped, an
/// illusion is skipped unless Misc `IllusionsCanRestoreMana` is set, otherwise the mana adder gets `DataA`. Rows `MT`.
pub fn mana_area_callback(illusion: bool, misc_mana: bool, immune: bool, max_mana: F32, a: F32) -> Option<F32> {
    if immune {
        return None;
    }
    let mag = f32::from_bits(arith::sub(max_mana, F32::ZERO).to_bits() & 0x7FFF_FFFF);
    if f32::from_bits(0x3456_BF95) > mag {
        return None;
    }
    if !misc_mana && illusion {
        return None;
    }
    Some(a)
}

/// The Restoration area callback `0x6F1E6130` (`AIra`): an immune candidate is skipped; an illusion is skipped entirely
/// when `DataA` is not zero and Misc `IllusionsCanRestoreLife` is off, or when `DataB` is not zero and
/// `IllusionsCanRestoreMana` is off (a NaN amount counts as not zero); otherwise the life adder gets `DataA` and the
/// mana adder `DataB`. Rows `RT`.
pub fn restore_area_callback(
    illusion: bool,
    misc_life: bool,
    misc_mana: bool,
    immune: bool,
    a: F32,
    b: F32,
) -> Option<(F32, F32)> {
    if immune {
        return None;
    }
    let nonzero = |v: F32| f32::from_bits(v.to_bits()) != 0.0;
    if illusion && !misc_life && nonzero(a) {
        return None;
    }
    if illusion && !misc_mana && nonzero(b) {
        return None;
    }
    Some((a, b))
}

/// The Regeneration area callback `0x6F108800` (`AIrg` scrolls): the same candidate rules as [`restore_area_callback`]
/// (the immunity answer is slot `0x3F0` of the ability; the life amount is `DataA`, the mana amount `DataB`), then the
/// regeneration apply `0x6F108620` is called with the duration the context carries for the candidate: `HeroDur` for a hero
/// (`0x6F1D2660`), else `Dur`. Returns that duration, or `None` when the candidate is skipped. Rows `RG`.
#[allow(clippy::too_many_arguments)]
pub fn regen_area_callback(
    illusion: bool,
    misc_life: bool,
    misc_mana: bool,
    immune: bool,
    hero: bool,
    dur: F32,
    hero_dur: F32,
    a: F32,
    b: F32,
) -> Option<F32> {
    restore_area_callback(illusion, misc_life, misc_mana, immune, a, b).map(|_| if hero { hero_dur } else { dur })
}

/// The three attribute values the attribute tome and the attribute bonus read (`0x6F0E5D50`, `0x6F0E57E0`): the
/// integer parts (`F2I`) of `DataA`, `DataB` and `DataC`, handed to the agility, intelligence and strength adders.
pub fn attribute_values(d: &[F32; 9]) -> (i32, i32, i32) {
    (conv::f2i(d[0]), conv::f2i(d[1]), conv::f2i(d[2]))
}

/// The flat damage bonus of `AIat` (`F2I(DataA)`; the removal is the negative).
pub fn damage_bonus_value(a: F32) -> i32 {
    conv::f2i(a)
}

/// The outcome of the charge routine `0x6F2B8390` for an item: `None` when it returns at once, else `(new charges, the
/// holder is told, the item is destroyed)`. `holder_live` is the answer of the key resolver for the holder key.
pub fn charge_outcome(flags: u32, charges: i32, holder_live: bool) -> Option<(i32, bool, bool)> {
    let (_, new) = charge_step(flags, charges);
    let new = new?;
    Some((new, holder_live, destroys(flags, new)))
}

// ---- damage hooks ----

/// The levels of the passive item abilities of family `fam` that `unit` holds.
fn passive_levels(gm: &Game, unit: ObjKey, fam: Family) -> Vec<SpellLevel> {
    gm.unit(unit)
        .abilities
        .iter()
        .filter(|a| !a.is_removed() && gm.spells.item.family(a.base) == Some(fam))
        .map(|a| level_of(gm, a.base))
        .collect()
}

/// `AIsr` on a damage record about to be delivered to `target` (the handler runs in the target side before armor,
/// `0x6F0F5730`): the raw amount of a Spells-type record is scaled by `1 - DataB` for every bracer carried.
pub(crate) fn spell_reduction(gm: &Game, target: ObjKey, row: u32, flags: u32, raw: F32) -> F32 {
    let illusion = gm.unit(target).illusion;
    let mut amount = raw;
    for level in passive_levels(gm, target, Family::BonusSpellReduction) {
        if let Some(factor) = spell_reduction_factor(true, illusion, flags, row, level.data[1]) {
            amount = scale_record(amount, F32::ZERO, factor).0;
        }
    }
    target_defense(gm, target, row, amount)
}

/// `AIdd` abilities of `target` on a record of table row `row` and amount `raw` (the handler `0xFC` of the target side's
/// first pass, `0x6F098F90`, [`passive_defense_factor`], CONFIRMED by Unicorn `DD`): the Pierce row is scaled by `DataA`
/// (0.7 for the Arcanite Shield), the Spells and Magic rows by `DataE`. The "armed" branch (`DataG` / `DataH`, set by the
/// slot `0x160` handler) is not reachable in the model: every stock row has them at 1.0.
pub(crate) fn target_defense(gm: &Game, target: ObjKey, row: u32, raw: F32) -> F32 {
    let mut amount = raw;
    for level in passive_levels(gm, target, Family::PassiveDefense) {
        let d = &level.data;
        if let Some(v) = passive_defense_factor(row, false, d[0], d[4], d[6], d[7]) {
            amount = arith::mul(amount, v);
        }
    }
    amount
}

/// The on-hit hook of a basic attack that dealt `damage` to `victim` (called by the attack arrival after the damage):
/// the life steal of `AIva` and the orbs. The pre-damage arming of the original (`0x6F0F54A0` / `0x6F0F2B40`: the
/// carrier is enabled and not paused, not an illusion for the orbs, the target filter accepts the victim) is modelled
/// by the enemy test. Returns the units the orb damage killed, in order.
pub fn on_hit(gm: &mut Game, attacker: ObjKey, victim: ObjKey, damage: F32, victim_alive: bool) -> Vec<ObjKey> {
    let mut died = Vec::new();
    if !gm.alive(attacker) {
        return died;
    }
    let abilities: Vec<u32> = gm
        .unit(attacker)
        .abilities
        .iter()
        .filter(|a| !a.is_removed())
        .map(|a| a.base)
        .collect();
    let (a_owner, v_owner) = (gm.unit(attacker).owner, gm.unit(victim).owner);
    let enemy = gm.is_enemy(a_owner, v_owner);
    // The vampire potion is a buff, not an ability of the carrier (S53).
    vampire_hit(gm, attacker, victim, damage);
    for alias in abilities {
        let level = level_of(gm, alias);
        match gm.spells.item.family(alias) {
            Some(Family::BonusVampire) => {
                let input = LifeStealInput {
                    armed: true,
                    filter_refused: !enemy,
                    immune: false,
                    victim: true,
                    owner_illusion: gm.unit(attacker).illusion,
                    victim_illusion: gm.unit(victim).illusion,
                    misc: gm.spells.item.illusions_heal,
                };
                if let (_, Some(heal)) = life_steal_outcome(&input, damage, level.data[0]) {
                    add_life(gm, attacker, heal);
                }
            }
            Some(Family::VampirePotion) | Some(Family::PassiveDefense) => {}
            Some(Family::SpellBonus) if enemy && !gm.unit(attacker).illusion => {
                spell_bonus_hit(gm, attacker, victim, alias, &level, victim_alive);
            }
            Some(Family::OrbFire) if enemy && !gm.unit(attacker).illusion => {
                orb_fire(gm, attacker, victim, &level, &mut died);
            }
            Some(Family::OrbFrost) if enemy && victim_alive && !gm.unit(attacker).illusion => {
                gm.frost_apply(victim, &level);
            }
            Some(fam @ (Family::OrbCorruption | Family::OrbVenom | Family::OrbLightning | Family::OrbFreezing))
                if enemy && victim_alive && !gm.unit(attacker).illusion =>
            {
                orb_effects(gm, attacker, victim, fam, &level, &mut died);
            }
            _ => {}
        }
    }
    died
}

/// The Spell Effect Bonus after a hit by `attacker` on `victim` (`CAbilitySpellEffectBonus` slot `0x124`, CONFIRMED by
/// Unicorn `SB`): the pre-damage roll is made for every armed hit (it consumes stream 36 even when the hit kills); on a proc
/// the spell of the row's `UnitID` is cast on the victim as a free cast (`0x6F0FA3C0` sets the child's target and runs its
/// end-cast). The model runs the child's effect on the next timer tick (the cast machinery needs the script runtime), UNSURE
/// by that one tick; the child's own range, mana and cooldown are not checked.
fn spell_bonus_hit(gm: &mut Game, attacker: ObjKey, victim: ObjKey, alias: u32, level: &SpellLevel, victim_alive: bool) {
    let (first, summoned) = {
        let u = gm.unit(victim);
        ((u.type_id >> 24) as u8, u.summoned)
    };
    let kind = spell_bonus_kind(first, if summoned { 4 } else { 0 });
    let count = gm.spells.item.spell_counts.get(&(attacker, alias)).map_or(1, |c| c[kind]);
    let Some(effective) = gm.order_tables().map(|t| spell_bonus_effective(level, kind, count, t)) else {
        return;
    };
    let Some((table, streams)) = gm.rng_parts() else {
        return;
    };
    let u = streams.stream_mut(SPELL_BONUS_STREAM).random_fraction(table);
    // `0x6F2853D0`: the real is `add(0.0, mul(1.0, U))`, i.e. U itself.
    let draw = arith::add(F32::ZERO, arith::mul(F32::ONE, u));
    let (hit, new) = spell_bonus_outcome(effective, count, draw);
    gm.spells.item.spell_counts.entry((attacker, alias)).or_insert([1; 3])[kind] = new;
    if hit && victim_alive && level.unit_id != 0 {
        let st = &mut gm.spells.item;
        st.next_proc += 1;
        let id = st.next_proc;
        st.procs.insert(id, (attacker, victim, level.unit_id));
        gm.timers.start(
            0.0,
            false,
            crate::natives::game::Payload::Spell(crate::cast::SpellMsg::Item { kind: TICK_PROC, unit: attacker, aux: id }),
        );
    }
}

/// The RNG stream of the Spell Effect Bonus roll (`0x6F2853D0` with `EDX = 0x24`).
pub const SPELL_BONUS_STREAM: usize = 0x24;

/// The Orb of Fire after a hit (`0x6F0F2CD0`, CONFIRMED by Unicorn `FB`, [`orb_fire_plan`]): the area body `0x6F2C31D0` damages
/// every enemy within `Area` of the victim EXCEPT the victim (the callback `0x6F0DC570` skips it) with `DataA`, class
/// `0x4000000`; the call that follows on the victim (`[victim]+0x120`) re-delivers the INCOMING attack record, it is the normal
/// hit (S41 and S45 read it as a second `DataA` for the victim, which the argument capture disproves). The victim's `+DataA`
/// comes from the persistent attack bonus of the orb (`apply_passive`). The splash goes through `unit_damage_target` as
/// Spells-row damage (armor does not apply).
fn orb_fire(gm: &mut Game, attacker: ObjKey, victim: ObjKey, level: &SpellLevel, died: &mut Vec<ObjKey>) {
    let amount = level.data[0];
    if arith::lt(F32::ZERO, level.area) {
        let (vx, vy) = (gm.unit(victim).x, gm.unit(victim).y);
        let owner = gm.unit(attacker).owner;
        for k in crate::natives::circle_candidates(gm, vx, vy, level.area) {
            if k == victim || !gm.alive(k) {
                continue;
            }
            if !gm.is_enemy(owner, gm.unit(k).owner) {
                continue;
            }
            if let Some(d) = gm.unit_damage_target(attacker, k, amount, false, false, 0, ORB_DAMAGE_TYPE) {
                died.push(d);
            }
        }
    }
}

/// The Ankh of Reincarnation when its carrier dies (`0x6F1F1D50`, the death handler of `CAbilityItemReincarnation`,
/// structure CONFIRMED by reading): the carrier must not be an illusion; the effect starts with the delay `DataA` (the
/// same revival timer as `AOre`), the item uses a charge (`0x6F2B8390`, which destroys the perishable Ankh) and the
/// unit gets flag `0x1000`. UNSURE: the other conditions of the handler (`0x6F1DB690`, the pathing probe at the death
/// point, `0x6F1D2F20`), and the roles of `DataB` (500, clamped to at least 1.0) and `DataC` (-1, a negative value
/// selects a unit stat): the model revives with full life, like Reincarnation.
pub(crate) fn on_unit_died(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey) {
    let (item, delay) = {
        let gm = game.borrow();
        if gm.unit(unit).illusion {
            return;
        }
        let found = gm.unit(unit).abilities.iter().find_map(|a| {
            (!a.is_removed() && gm.spells.item.family(a.base) == Some(Family::ItemReincarnation)).then_some(a.base)
        });
        let Some(alias) = found else {
            return;
        };
        let Some(&item) = gm.spells.item.holders.get(&(unit, alias)).and_then(|v| v.first()) else {
            return;
        };
        (item, level_of(&gm, alias).data[0].to_f32())
    };
    game.borrow_mut().start_reincarnation(unit, delay);
    use_charge(game, rt, item);
}

// ---- the effects ----

/// The level-1 record of an alias: the tomes and attribute bonuses read `DataA..C` at level 1 whatever the level
/// (the profile reader is called with the constant level 1, `0x6F0E6580`, `0x6F0E5D50`, `0x6F0E57E0`).
fn level_one(gm: &Game, alias: u32) -> SpellLevel {
    gm.spells_level_record(alias, 0)
}

/// The life adder `0x6F277C20` (no healing modifiers in the model) on `target`: add and clamp to the maximum.
fn add_life(gm: &mut Game, target: ObjKey, amount: F32) {
    let (life, max) = {
        let u = gm.unit(target);
        (u.life, u.max_life)
    };
    let sum = arith::add(amount, life);
    let next = if arith::lt(max, sum) { max } else { sum };
    set_life(gm, target, next);
}

/// The mana adder `0x6F279140` on `target` (UNSURE clamp: the property setter limits the value to the maximum).
fn add_mana(gm: &mut Game, target: ObjKey, amount: F32) {
    let (mana, max) = {
        let u = gm.unit(target);
        (u.mana, u.max_mana)
    };
    let sum = arith::add(mana, amount);
    let next = if arith::lt(max, sum) { max } else { sum };
    gm.write_mana(target, next);
}

fn run_plan(gm: &mut Game, t: ObjKey, plan: &[(Sink, F32)]) {
    for &(sink, amount) in plan {
        match sink {
            Sink::Life => add_life(gm, t, amount),
            Sink::Mana => add_mana(gm, t, amount),
        }
    }
}

/// The target of an effect: the ability's unit target, else its owner (`[ability+0x30]`, `0x6F472890`).
fn effect_target(caster: ObjKey, target: CastTarget) -> ObjKey {
    match target {
        CastTarget::Unit(t) => t,
        _ => caster,
    }
}

/// The center of an area effect: the position of the effect target (a unit target or the caster).
fn area_center(gm: &Game, caster: ObjKey, target: CastTarget) -> (F32, F32) {
    let u = gm.unit(effect_target(caster, target));
    (u.x, u.y)
}

/// The area callbacks (`0x6F0F0470` and the mana and restore twins): immune units are skipped (not modelled), an
/// illusion is skipped unless the Misc switch allows it (CONFIRMED structure, UNSURE key), the amount is `DataA`.
fn area_units(gm: &Game, caster: ObjKey, target: CastTarget, level: &SpellLevel) -> Vec<ObjKey> {
    area_friends(gm, caster, target, level)
        .into_iter()
        .filter(|&k| area_callback_amount(gm.unit(k).illusion, gm.spells.item.illusions_heal, false, F32::ZERO).is_some())
        .collect()
}

/// The friendly candidates of an area effect around its center (UNSURE: the group builder's relation flags `0x1D` / `0x1F`
/// are not decoded; friends and the caster). `AIda` uses these as they are: its callback handles illusions itself.
fn area_friends(gm: &Game, caster: ObjKey, target: CastTarget, level: &SpellLevel) -> Vec<ObjKey> {
    let (cx, cy) = area_center(gm, caster, target);
    let owner = gm.unit(caster).owner;
    crate::natives::circle_candidates(gm, cx, cy, level.area)
        .into_iter()
        .filter(|&k| {
            if !gm.alive(k) || gm.unit(k).hidden {
                return false;
            }
            let u = gm.unit(k);
            k == caster || u.owner == owner || !gm.is_enemy(owner, u.owner)
        })
        .collect()
}

/// Runs the effect of `fam` for `caster` (state only: no events are needed, so the powerup path shares it).
fn apply_effect(gm: &mut Game, fam: Family, alias: u32, level: &SpellLevel, caster: ObjKey, target: CastTarget) {
    let d = &level.data;
    match fam {
        Family::Heal | Family::Mana | Family::Restore => {
            let t = effect_target(caster, target);
            if gm.alive(t) {
                run_plan(gm, t, &restore_plan(fam, d));
            }
        }
        Family::HealAoe | Family::ManaAoe | Family::RestoreAoe => {
            // One callback per family (`0x6F0F0470`, `0x6F1269E0`, `0x6F1E6130`): each has its own illusion rule.
            let (misc_life, misc_mana) = (gm.spells.item.illusions_heal, gm.spells.item.illusions_mana);
            for k in area_friends(gm, caster, target, level) {
                let (illusion, max_mana) = (gm.unit(k).illusion, gm.unit(k).max_mana);
                let plan = match fam {
                    Family::HealAoe => area_callback_amount(illusion, misc_life, false, d[0]).map(|a| vec![(Sink::Life, a)]),
                    Family::ManaAoe => mana_area_callback(illusion, misc_mana, false, max_mana, d[0]).map(|a| vec![(Sink::Mana, a)]),
                    _ => restore_area_callback(illusion, misc_life, misc_mana, false, d[0], d[1])
                        .map(|(a, b)| vec![(Sink::Life, a), (Sink::Mana, b)]),
                };
                if let Some(plan) = plan {
                    run_plan(gm, k, &plan);
                }
            }
        }
        Family::TomeLife => {
            let t = effect_target(caster, target);
            life_sink_delta(gm, t, conv::i2r(conv::f2i(d[0])), 2);
        }
        Family::TomeMana => {
            let t = effect_target(caster, target);
            mana_sink_delta(gm, t, conv::i2r(conv::f2i(d[0])), 2);
        }
        Family::TomeXp => {
            let t = effect_target(caster, target);
            add_hero_xp(gm, t, conv::i2r(conv::f2i(d[0])), true);
        }
        Family::TomeLevel => {
            let t = effect_target(caster, target);
            let n = conv::f2i(d[0]);
            if let Some(amount) = level_mod_amount(gm, t, n) {
                add_hero_xp(gm, t, amount, false);
            }
        }
        Family::TomeAttrs => {
            let t = effect_target(caster, target);
            let (agi, int, str) = attribute_values(d);
            add_attributes(gm, t, agi, int, str);
        }
        Family::TomeAttack => {
            let t = effect_target(caster, target);
            // `0x6F0DE190`: only a unit with an attack ability; the value goes into both weapons' base bonuses.
            gm.add_damage(t, conv::f2i(d[0]));
        }
        Family::Speed => {
            let t = effect_target(caster, target);
            apply_speed(gm, t, level);
        }
        Family::SpeedAoe => {
            for k in area_units(gm, caster, target, level) {
                apply_speed(gm, k, level);
            }
        }
        Family::TownPortal => town_portal(gm, caster, target, level),
        Family::Figurine => figurine(gm, caster, alias, level),
        Family::Invis | Family::Invul => {
            let t = effect_target(caster, target);
            apply_potion_buff(gm, fam, t, level);
        }
        Family::Regen => {
            // The scroll forms (`Area` above 0) act on the units around the target or owner (`0x6F1088D0`), the potions on
            // the target or the owner (`0x6F108AC0`).
            let units = if arith::lt(F32::ZERO, level.area) {
                let (misc_life, misc_mana) = (gm.spells.item.illusions_heal, gm.spells.item.illusions_mana);
                area_friends(gm, caster, target, level)
                    .into_iter()
                    .filter(|&k| {
                        let illusion = gm.unit(k).illusion;
                        let hero = gm.target_hero(k);
                        regen_area_callback(illusion, misc_life, misc_mana, false, hero, level.dur, level.hero_dur, d[0], d[1])
                            .is_some()
                    })
                    .collect()
            } else {
                vec![effect_target(caster, target)]
            };
            for k in units {
                apply_regen(gm, caster, k, level);
            }
        }
        Family::DefenseAoe => {
            for k in area_friends(gm, caster, target, level) {
                apply_defense(gm, k, level);
            }
        }
        Family::DamageAoe => damage_aoe(gm, caster, target, level),
        Family::CureAoe => {
            let select = crate::abilities::BuffSelect {
                remove_negative: true,
                dispellable: true,
                ..Default::default()
            };
            for k in area_units(gm, caster, target, level) {
                gm.remove_buffs(k, select);
            }
        }
        Family::Detect => detect(gm, caster, target, level),
        Family::Flare => {
            let (x, y) = match target {
                CastTarget::Point(x, y) => (x, y),
                _ => area_center(gm, caster, target),
            };
            reveal(gm, caster, x, y, level.area, level.dur);
        }
        Family::RevealMap => {
            // The whole playable map: a circle larger than any map around the caster.
            let (x, y) = area_center(gm, caster, target);
            reveal(gm, caster, x, y, F32::from_bits(0x471C_4000), level.dur);
        }
        Family::Glyph => {
            let owner = gm.unit(caster).owner;
            if level.unit_id != 0 {
                crate::natives::misc::add_tech(gm, owner, level.unit_id, conv::f2i(level.data[0]));
            }
        }
        Family::BuildScroll => build_scroll(gm, caster, alias, target),
        Family::PlaceMine => {
            let (x, y) = match target {
                CastTarget::Point(x, y) => (x, y),
                _ => area_center(gm, caster, target),
            };
            let (owner, facing) = (gm.unit(caster).owner, gm.unit(caster).facing);
            if level.unit_id != 0 {
                let _ = crate::natives::unit::build(gm, owner, level.unit_id, x, y, facing, true);
            }
        }
        Family::Illusion => illusion(gm, caster, target, level),
        Family::AnimateDead => animate_dead(gm, caster, target, level),
        Family::Moonstone => moonstone(gm, level),
        Family::VampirePotion => vampire_potion(gm, caster, level),
        Family::Gold | Family::Lumber => {
            let t = effect_target(caster, target);
            let owner = gm.unit(t).owner;
            let res = if fam == Family::Gold {
                crate::economy::Res::Gold
            } else {
                crate::economy::Res::Lumber
            };
            gm.credit_resource(owner, res, conv::f2i(d[0]), crate::economy::Flags::NONE);
        }
        _ => {}
    }
}


// ---- S45 effects (spec section 16; UNSURE models unless the spec says otherwise) ----

/// What the Scroll of Protection (`AIda`) area callback `0x6F0B61D0` does to one candidate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DefenseOutcome {
    /// The amount handed to the life adder `0x6F277C20` (`DataB`), when the heal block runs.
    pub life: Option<F32>,
    /// The amount handed to the mana adder `0x6F279140` (`DataC`).
    pub mana: Option<F32>,
    /// The buff `Bdef` apply (`0x6F0B60F0`): `(armor = DataA, duration)`.
    pub buff: Option<(F32, F32)>,
}

/// The `AIda` callback (Unicorn `DA` rows): a candidate that is not an illusion (`+0x5C & 0x40000000`) gets the life adder
/// with `DataB` and the mana adder with `DataC` when `0 < DataB` or `0 < DataC` (x87 compares, so NaN fails them; the
/// ethereal branch is dead: slot `0x42C` of the class answers 0); a candidate that is not immune to the ability's order
/// (`0x6F28D750`) gets `Bdef` with `DataA` and the hero-rule duration ([`potion_buff_duration`]). Illusions get the buff.
pub fn defense_callback(illusion: bool, immune: bool, hero: bool, level: &SpellLevel, extra: F32) -> DefenseOutcome {
    let (b, c) = (level.data[1], level.data[2]);
    let positive = |v: F32| f32::from_bits(v.to_bits()) > 0.0;
    let heals = !illusion && (positive(b) || positive(c));
    DefenseOutcome {
        life: heals.then_some(b),
        mana: heals.then_some(c),
        buff: (!immune).then(|| (level.data[0], potion_buff_duration(level, hero, extra))),
    }
}

/// `AIda` on one unit ([`defense_callback`]). The immunity answer of `0x6F28D750` is not modelled (false). The buff is
/// skipped for a duration that is not above zero (a stand-in: the original hands any duration to the apply).
fn apply_defense(gm: &mut Game, k: ObjKey, level: &SpellLevel) {
    if !gm.alive(k) {
        return;
    }
    let hero = gm.target_hero(k);
    let out = defense_callback(gm.unit(k).illusion, false, hero, level, F32::ZERO);
    if let (Some(l), Some(m)) = (out.life, out.mana) {
        run_plan(gm, k, &[(Sink::Life, l), (Sink::Mana, m)]);
    }
    let Some((armor, dur)) = out.buff else { return };
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let preds = crate::abilities::BuffPredicates {
        positive: true,
        magic: true,
        dispellable: true,
        ..crate::abilities::buff_predicates(BUFF_DEFENSE)
    };
    let mods = crate::unit_spells::UnitMods {
        armor,
        ..Default::default()
    };
    gm.apply_mods_buff(k, BUFF_DEFENSE, preds, mods, dur);
}

fn enemies_in_area(gm: &Game, caster: ObjKey, cx: F32, cy: F32, area: F32) -> Vec<ObjKey> {
    let owner = gm.unit(caster).owner;
    crate::natives::circle_candidates(gm, cx, cy, area)
        .into_iter()
        .filter(|&k| gm.alive(k) && !gm.unit(k).hidden && gm.is_enemy(owner, gm.unit(k).owner))
        .collect()
}

/// `AIdm`: `DataA` Spells-row damage to every enemy within `Area` of the target or the caster.
fn damage_aoe(gm: &mut Game, caster: ObjKey, target: CastTarget, level: &SpellLevel) {
    let (cx, cy) = area_center(gm, caster, target);
    for k in enemies_in_area(gm, caster, cx, cy, level.area) {
        if let Some(d) = gm.unit_damage_target(caster, k, level.data[0], false, false, 0, ORB_DAMAGE_TYPE) {
            gm.spells.item.deaths.push((caster, d));
        }
    }
}

/// `AIta` / `AItb`: the marker buff `Bdet` on the enemy units around the target (or the caster) for the hero-rule duration.
fn detect(gm: &mut Game, caster: ObjKey, target: CastTarget, level: &SpellLevel) {
    let (cx, cy) = area_center(gm, caster, target);
    for k in enemies_in_area(gm, caster, cx, cy, level.area) {
        let hero = gm.target_hero(k);
        let dur = potion_buff_duration(level, hero, F32::ZERO);
        if !arith::lt(F32::ZERO, dur) {
            continue;
        }
        let preds = crate::abilities::BuffPredicates {
            negative: true,
            ..crate::abilities::buff_predicates(BUFF_DETECT)
        };
        gm.apply_mods_buff(k, BUFF_DETECT, preds, Default::default(), dur);
    }
}

/// `AIfa` / `AIrv`: a visible-state fog modifier over a circle for the caster's owner, stopped by a one-shot timer after
/// `dur` (the original's reveal is an effect object, not decoded).
pub(crate) fn reveal(gm: &mut Game, caster: ObjKey, x: F32, y: F32, area: F32, dur: F32) {
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let owner = gm.unit(caster).owner;
    let cells = gm.fog_grid().circle_shape(x, y, area);
    let handle = gm.fog_create(owner, crate::natives::fog::FOG_VISIBLE, cells, false, false);
    let Some(key) = gm.fog_modifier_of_handle(handle) else {
        return;
    };
    gm.fog_start(key);
    let st = &mut gm.spells.item;
    st.next_fog += 1;
    let id = st.next_fog;
    st.fogs.insert(id, key);
    gm.timers.start(
        dur.to_f32(),
        false,
        crate::natives::game::Payload::Spell(crate::cast::SpellMsg::Item { kind: TICK_FOG, unit: caster, aux: id }),
    );
}

/// `AIbl`: the unit of the row's `UnitID` list that matches the owner's race (human 1, orc 2, undead 3, night elf 4, in the
/// order of the stock rows `hcas,ofrt,unp2,etoe`) is created at the target point, finished at once (UNSURE: `Dur` is the
/// construction time in the data and is not modelled).
fn build_scroll(gm: &mut Game, caster: ObjKey, alias: u32, target: CastTarget) {
    let (x, y) = match target {
        CastTarget::Point(x, y) => (x, y),
        _ => area_center(gm, caster, target),
    };
    let (owner, facing) = (gm.unit(caster).owner, gm.unit(caster).facing);
    let race = gm.player(owner).race.clamp(1, 4) as usize;
    let Some(&code) = gm.spells.item.unit_lists.get(&alias).and_then(|l| l.get(race - 1).or(l.first())) else {
        return;
    };
    let _ = crate::natives::unit::build(gm, owner, code, x, y, facing, true);
}

/// `AIil`: a copy of the target unit for the caster's player, an illusion for the hero-rule duration; the damage factors are
/// `DataA` dealt and `DataB` taken ([`illusion_args`], CONFIRMED by Unicorn `IL`; the stock Wand of Illusion has `DataA` 0 and
/// `DataB` 2). UNSURE: the spawn spot (`0x6F1D26B0`) and the buff flags.
fn illusion(gm: &mut Game, caster: ObjKey, target: CastTarget, level: &SpellLevel) {
    let t = effect_target(caster, target);
    if !gm.alive(t) {
        return;
    }
    // The duration follows the hero rule of the target (`0x6F13DFD0`, CONFIRMED by Unicorn `IL`).
    let hero = gm.target_hero(t);
    let (dur, dealt, taken) = illusion_args(level, hero, F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let (ty, x, y, facing) = {
        let u = gm.unit(t);
        (u.type_id, u.x, u.y, u.facing)
    };
    let owner = gm.unit(caster).owner;
    if let Some((k, _)) = crate::natives::unit::build(gm, owner, ty, x, y, facing, true) {
        gm.unit_mut(k).illusion = true;
        gm.register_illusion(k, dealt, taken);
        gm.apply_timed_life(k, BUFF_ILLUSION, dur);
    }
}

/// `AIan`: up to `DataA` dead non-hero units within `Area` come back under the caster's player with full life for `Dur`
/// (timed life `BUan`). UNSURE: the corpse choice (query order), the placement (where they died), the hero case.
fn animate_dead(gm: &mut Game, caster: ObjKey, target: CastTarget, level: &SpellLevel) {
    let (cx, cy) = area_center(gm, caster, target);
    let owner = gm.unit(caster).owner;
    let mut left = conv::f2i(level.data[0]);
    for k in crate::natives::circle_candidates(gm, cx, cy, level.area) {
        if left <= 0 {
            break;
        }
        let u = gm.unit(k);
        if !u.dead || u.removed || u.hero.is_some() || gm.unit_types().is_building(u.type_id) {
            continue;
        }
        gm.resurrect_unit(k);
        if gm.unit(k).owner != owner {
            crate::natives::unit::change_owner(gm, k, owner);
        }
        gm.unit_mut(k).summoned = true;
        gm.apply_timed_life(k, BUFF_ANIMATE_DEAD, level.dur);
        left -= 1;
    }
}

/// `AIcf` (Cloak of Flames): the permanent buff `BIcf` and a repeating tick every `Dur` seconds (1.0 when the row has
/// none) that burns the enemies within `Area` of the carrier for `DataA` (UNSURE: the tick period and amount reading).
fn start_cloak(gm: &mut Game, unit: ObjKey, level: &SpellLevel) {
    let period = if arith::lt(F32::ZERO, level.dur) { level.dur.to_f32() } else { 1.0 };
    let preds = crate::abilities::buff_predicates(BUFF_CLOAK);
    let serial = gm.add_buff(unit, BUFF_CLOAK, preds);
    let timer = gm.timers.start(
        period,
        true,
        crate::natives::game::Payload::Spell(crate::cast::SpellMsg::Item { kind: TICK_CLOAK, unit, aux: serial }),
    );
    gm.spells.item.cloaks.insert(serial, CloakRec { amount: level.data[0], area: level.area, timer: Some(timer) });
}

fn stop_cloak(gm: &mut Game, unit: ObjKey) {
    let serials: Vec<u32> = gm.unit(unit).buffs.iter().filter(|b| b.base == BUFF_CLOAK).map(|b| b.serial).collect();
    for s in serials {
        if let Some(rec) = gm.spells.item.cloaks.remove(&s) {
            if let Some(t) = rec.timer {
                gm.timers.cancel(t);
            }
        }
    }
}

/// The timer kinds of S45.
pub const TICK_CLOAK: u8 = 2;
pub const TICK_FOG: u8 = 3;
/// The end of the Moonstone effect (S53).
pub const TICK_TOD: u8 = 4;
/// A spell proc of the Spell Effect Bonus waiting to be cast (S53).
pub const TICK_PROC: u8 = 5;

/// `AIcb` / `AIpb` / `AIlb` / `AIzb` after a basic hit (UNSURE models, see the family docs).
fn orb_effects(gm: &mut Game, attacker: ObjKey, victim: ObjKey, fam: Family, level: &SpellLevel, died: &mut Vec<ObjKey>) {
    match fam {
        Family::OrbCorruption => {
            // `0x6F109B40` (CONFIRMED, Unicorn `CR`): the hero-rule duration and the amount `DataB`; the buff takes the amount
            // off the victim's armor unless the victim is an illusion (`0x6F0F3A90`). Carriers that are illusions never arm.
            let hero = gm.target_hero(victim);
            let Some((dur, amount)) = orb_corruption_plan(level, true, hero, gm.unit(attacker).illusion) else {
                return;
            };
            if !arith::lt(F32::ZERO, dur) {
                return;
            }
            let preds = crate::abilities::BuffPredicates {
                negative: true,
                magic: true,
                dispellable: true,
                ..crate::abilities::buff_predicates(BUFF_CORRUPTION)
            };
            let armor = if gm.unit(victim).illusion { F32::ZERO } else { arith::neg(amount) };
            let mods = crate::unit_spells::UnitMods {
                armor,
                ..Default::default()
            };
            gm.apply_mods_buff(victim, BUFF_CORRUPTION, preds, mods, dur);
        }
        // `AIpb` (`0x154` = `0x6F0F3170`) only registers a damage callback (`0x6F0F3020` through `0x6F076700`); the poison of the
        // Orb of Venom is the item's second ability `Apo2` (`CAbilityPoisonDamage`, `Dur` 6, `DataA` 9, buff `BIpb`), which
        // the poison attack module of `unit_abilities_m` / `neutral_spells` already applies. The S45 stand-in (a 5 s `Bpoi`
        // poison from `DataA`) is removed.
        Family::OrbVenom => {}
        Family::OrbLightning => {
            // `0x6F0F2F70` (CONFIRMED, Unicorn `LB`): the orb has no damage of its own; it runs the unit effect of the
            // carrier's `AIlp` ability (the Purge effect) on the victim.
            let lp = gm
                .unit(attacker)
                .abilities
                .iter()
                .filter(|a| !a.is_removed())
                .map(|a| a.base)
                .find(|&a| gm.spells.item.base_of(a) == ITEM_LIGHTNING_PURGE);
            if let Some(alias) = lp {
                let rec = CastRec {
                    id: 0,
                    caster: attacker,
                    base: crate::unit_spells::PURGE,
                    target: CastTarget::Unit(victim),
                    level: level_of(gm, alias),
                };
                if let Some(d) = crate::unit_spells::purge_body(gm, &rec, victim) {
                    died.push(d);
                }
            }
        }
        Family::OrbFreezing => {
            // `0x6F108E20` (CONFIRMED, Unicorn `ZB`): the stun buff `Bfre` (`CBuffFreeze < CBuffStun`) for the hero-rule duration.
            let hero = gm.target_hero(victim);
            if let Some((dur, _)) = orb_freeze_plan(level, true, hero, gm.unit(attacker).illusion) {
                if arith::lt(F32::ZERO, dur) {
                    gm.apply_stun_as(BUFF_FREEZE, victim, dur);
                }
            }
        }
        _ => {}
    }
}

/// `AIco`: the target (a non-hero enemy or neutral unit whose creep level is at most `DataA`) changes owner to the
/// caster's player and stops (the Charm body). UNSURE: the target rules of `0x6F1CF8B0`.
fn command(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let CastTarget::Unit(t) = rec.target else {
        return;
    };
    let changed = {
        let mut gm = game.borrow_mut();
        if !gm.alive(t) || gm.unit(t).hero.is_some() {
            return;
        }
        let owner = gm.unit(rec.caster).owner;
        if !gm.is_enemy(owner, gm.unit(t).owner) {
            return;
        }
        let max = conv::f2i(rec.level.data[0]);
        let lvl = gm.unit_types().get(gm.unit(t).type_id).map_or(0, |ty| ty.level);
        if lvl > max {
            return;
        }
        let from = crate::natives::unit::change_owner(&mut gm, t, owner);
        if from.is_some() {
            let stop = crate::orders::Order {
                id: crate::orders::ORDER_STOP,
                class: crate::orders::OrderClass::Immediate,
                point: None,
                target: 0,
            };
            let _ = gm.issue_order(t, stop, true);
        }
        from.map(|f| (f, owner))
    };
    if let Some((from, to)) = changed {
        crate::natives::event::fire_owner_change(game, rt, t, from, to);
    }
}

/// A tick of the S45 timers: the Cloak of Flames burn and the end of a reveal.
fn s45_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, aux: u32) {
    match kind {
        TICK_CLOAK => {
            {
                let mut gm = game.borrow_mut();
                let Some(rec) = gm.spells.item.cloaks.get(&aux).copied() else {
                    return;
                };
                let live = gm.alive(unit) && gm.unit(unit).buffs.iter().any(|b| b.serial == aux);
                if !live {
                    if let Some(t) = rec.timer {
                        gm.timers.cancel(t);
                    }
                    gm.spells.item.cloaks.remove(&aux);
                    return;
                }
                let (cx, cy) = (gm.unit(unit).x, gm.unit(unit).y);
                for k in enemies_in_area(&gm, unit, cx, cy, rec.area) {
                    if let Some(d) = gm.unit_damage_target(unit, k, rec.amount, false, false, 0, ORB_DAMAGE_TYPE) {
                        gm.spells.item.deaths.push((unit, d));
                    }
                }
            }
            flush_deaths(game, rt);
        }
        TICK_FOG => {
            let mut gm = game.borrow_mut();
            if let Some(key) = gm.spells.item.fogs.remove(&aux) {
                gm.fog_stop(key);
            }
        }
        TICK_PROC => {
            let rec = {
                let mut gm = game.borrow_mut();
                let Some((attacker, victim, child)) = gm.spells.item.procs.remove(&aux) else {
                    return;
                };
                if !gm.alive(attacker) || !gm.alive(victim) {
                    return;
                }
                CastRec {
                    id: 0,
                    caster: attacker,
                    base: gm.spells.item.base_of(child),
                    target: CastTarget::Unit(victim),
                    level: level_of(&gm, child),
                }
            };
            crate::cast::run_effect(game, rt, &rec);
            flush_deaths(game, rt);
        }
        TICK_TOD => {
            let mut gm = game.borrow_mut();
            if let Some(rec) = gm.spells.item.moonstone.take() {
                gm.set_time_of_day(rec.saved);
                if !rec.map_suspended {
                    gm.suspend_time_of_day(false);
                }
            }
        }
        _ => {}
    }
}

/// Dispatches the death events of the units the S45 effects killed.
fn flush_deaths(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime) {
    let deaths = std::mem::take(&mut game.borrow_mut().spells.item.deaths);
    for (killer, victim) in deaths {
        crate::natives::dispatch_unit_death_by(game, rt, victim, Some(killer));
    }
}

/// The duration the potion buff effects hand to the buff apply (`0x6F13C180` for `AIvi`, `0x6F13C270` for `AIvu`,
/// Unicorn `VI` rows): `HeroDur` for a hero target, else `Dur` plus the ability's extra term (`+0x9C`, a soft add).
pub fn potion_buff_duration(level: &SpellLevel, hero: bool, extra: F32) -> F32 {
    crate::cast::effect_duration(level, hero, extra)
}

/// `AIvi` / `AIvu` on one unit: the buff `Binv` (invisibility) or `Bvul` (invulnerability) for [`potion_buff_duration`].
/// The effect bodies are CONFIRMED (they find or create the buff by its code and call its apply slot `0x350` with the
/// duration); the buff classes are not decoded: invisibility has no model beyond the buff (no detection in the sim),
/// invulnerability adds one count (the same flag the `Avul` ability sets); both are positive and magic (UNSURE).
fn apply_potion_buff(gm: &mut Game, fam: Family, target: ObjKey, level: &SpellLevel) {
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let dur = potion_buff_duration(level, hero, F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let (code, invuln) = if fam == Family::Invul { (BUFF_INVUL, true) } else { (BUFF_INVIS, false) };
    let preds = crate::abilities::BuffPredicates {
        positive: true,
        magic: true,
        dispellable: true,
        ..crate::abilities::buff_predicates(code)
    };
    let mods = crate::unit_spells::UnitMods {
        invuln,
        ..Default::default()
    };
    gm.apply_mods_buff(target, code, preds, mods, dur);
    if code == BUFF_INVIS {
        gm.invisibility_note(target);
    }
}

/// A figurine (`0x6F0B8A00`, structure CONFIRMED by Unicorn `FG` rows): the summon batches of [`figurine_batches`], each
/// unit a summoned unit with the figurine timed life `BFig` (the summon loop `0x6F0B8900` creates the units near the
/// owner, marks them summoned (`+0x5C` bit `0x2000`) and starts the buff with the batch's duration). UNSURE: the spot (the
/// original asks the owner for a placement within `Area`; the model uses the owner's position) and the hero duration.
fn figurine(gm: &mut Game, caster: ObjKey, alias: u32, level: &SpellLevel) {
    if !gm.alive(caster) {
        return;
    }
    let codes = gm.spells.item.unit_codes.get(&alias).copied().unwrap_or([0, 0]);
    let (x, y, owner, facing) = {
        let c = gm.unit(caster);
        (c.x, c.y, c.owner, c.facing)
    };
    for (code, count, dur, _area) in figurine_batches(level, codes, F32::ZERO) {
        for _ in 0..count.max(0) {
            if let Some((k, _)) = crate::natives::unit::build(gm, owner, code, x, y, facing, true) {
                gm.unit_mut(k).summoned = true;
                gm.apply_timed_life(k, BUFF_FIGURINE, dur);
            }
        }
    }
}

/// The per-tick amounts of a regeneration: the totals `DataA` (life) and `DataB` (mana) spread over the duration (soft
/// division, like Rejuvenation `0x6F0F0650`).
pub fn regen_tick_amounts(level: &SpellLevel, dur: F32, t: Option<&Tables>) -> (F32, F32) {
    let div = |total: F32| match t {
        Some(t) => arith::div(total, dur, t),
        None => F32::from_bits((total.to_f32() / dur.to_f32()).to_bits()),
    };
    (div(level.data[0]), div(level.data[1]))
}

/// `AIrg` on one unit: the hero rule duration, the buff `BIrg` for the duration plus 0.01 s and a repeating 1.0 s tick that
/// adds the per-tick life and mana (LIKELY: the class reuses the Rejuvenation tick `0x6F0E3A20`; a refresh replaces the
/// amounts and keeps the running timer).
fn apply_regen(gm: &mut Game, caster: ObjKey, target: ObjKey, level: &SpellLevel) {
    let _ = caster;
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let dur = crate::cast::effect_duration(level, hero, F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let tables = gm.order_tables().cloned();
    let (life, mana) = regen_tick_amounts(level, dur, tables.as_ref());
    let preds = crate::abilities::BuffPredicates {
        positive: true,
        magic: true,
        dispellable: true,
        ..crate::abilities::buff_predicates(BUFF_REGEN)
    };
    let buff_dur = arith::add(dur, F32::from_bits(0x3C23_D70A));
    let serial = gm.apply_mods_buff(target, BUFF_REGEN, preds, crate::unit_spells::UnitMods::default(), buff_dur);
    if let Some(rec) = gm.spells.item.regen.get_mut(&serial) {
        rec.life = life;
        rec.mana = mana;
        return;
    }
    let timer = gm.timers.start(
        1.0,
        true,
        crate::natives::game::Payload::Spell(crate::cast::SpellMsg::Item { kind: TICK_REGEN, unit: target, aux: serial }),
    );
    gm.spells.item.regen.insert(serial, RegenRec { life, mana, timer: Some(timer) });
}

/// The timer kind of a regeneration tick.
pub const TICK_REGEN: u8 = 1;

/// A timer of [`crate::cast::SpellMsg::Item`]: one regeneration tick while the buff lives.
pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, serial: u32) {
    if kind != TICK_REGEN {
        s45_timer(game, rt, kind, unit, serial);
        return;
    }
    let mut gm = game.borrow_mut();
    let Some(rec) = gm.spells.item.regen.get(&serial).copied() else {
        return;
    };
    let alive = gm.alive(unit) && gm.unit(unit).buffs.iter().any(|b| b.serial == serial);
    if !alive {
        if let Some(t) = rec.timer {
            gm.timers.cancel(t);
        }
        gm.spells.item.regen.remove(&serial);
        return;
    }
    if rec.life.to_bits() & 0x7FFF_FFFF != 0 {
        add_life(&mut gm, unit, rec.life);
    }
    if rec.mana.to_bits() & 0x7FFF_FFFF != 0 {
        add_mana(&mut gm, unit, rec.mana);
    }
}

/// The speed buff arguments of `CAbilityItemSpeed::vf424` (`0x6F1FC4C0`): the duration by the hero rule (`HeroDur` for a
/// hero target, else `Dur` plus the ability's extra term, a soft add), then `DataA` and `DataB`. The buff's apply slot
/// (`CBuffSpeed`, `0x6F0D6500`) gives `DataA` to the movement sink (`0x6F202FF0`, holder `+0xCC`) and `DataB` to the attack
/// speed sink (`0x6F0C9090`, holder `+0xD4`). Unicorn: `item_cores_emu.py` rows `SP`.
pub fn speed_buff_args(level: &SpellLevel, hero: bool, extra: F32) -> (F32, F32, F32) {
    (crate::cast::effect_duration(level, hero, extra), level.data[0], level.data[1])
}

fn apply_speed(gm: &mut Game, target: ObjKey, level: &SpellLevel) {
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let (dur, mv, atk) = speed_buff_args(level, hero, F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let preds = crate::abilities::BuffPredicates {
        positive: true,
        magic: true,
        dispellable: true,
        ..crate::abilities::buff_predicates(BUFF_SPEED)
    };
    let mods = crate::unit_spells::UnitMods {
        move_delta: mv,
        attack_delta: atk,
        ..Default::default()
    };
    gm.apply_mods_buff(target, BUFF_SPEED, preds, mods, dur);
}

/// The Moonstone (`AIct`): the arguments are CONFIRMED ([`moonstone_args`], Unicorn `CT`): the time of day is set to
/// `DataA + DataB / 60` hours and a timer of `Dur` seconds starts (`0x6F104570` finds the effect object `XIct` and calls
/// `0x6F0F5690`: `SetFloatGameState(TIME_OF_DAY, x)` then the one-shot timer `0xD01C1`). At the expiry the handler
/// `0x6F0F56D0` sets the time of day to the value saved in the effect (`+0x28`) and calls `0x6F429920(.., 2)`, which the
/// model reads as the resume of the time of day. UNSURE: where `+0x28` is saved (the model keeps the time of day of the
/// first cast) and that the effect suspends the clock while it lasts (the restore-and-resume pair suggests it).
fn moonstone(gm: &mut Game, level: &SpellLevel) {
    let Some((tod, dur)) = moonstone_args(level, F32::ZERO, &|a, b| gm.soft_div(a, b)) else {
        return;
    };
    let active = gm.spells.item.moonstone.take();
    let rec = match active {
        Some(rec) => {
            gm.timers.cancel(rec.timer);
            rec
        }
        None => MoonstoneRec {
            saved: gm.time_of_day(),
            map_suspended: gm.game_state.time_of_day_suspended,
            timer: TimerId(0),
        },
    };
    if !rec.map_suspended {
        gm.suspend_time_of_day(true);
    }
    gm.set_time_of_day(tod);
    let timer = gm.timers.start(
        dur.to_f32(),
        false,
        crate::natives::game::Payload::Spell(crate::cast::SpellMsg::Item { kind: TICK_TOD, unit: ObjKey(0), aux: 0 }),
    );
    gm.spells.item.moonstone = Some(MoonstoneRec { timer, ..rec });
}

/// The Potion of Vampirism (`AIpv`, slot `0x428` on the carrier): the buff `BIpv` for the hero-rule duration with
/// `F2I(DataA)` bonus damage and the life steal fraction `DataB` ([`vampire_potion_args`], CONFIRMED by Unicorn `PV`; the
/// holders `+0xD8` / `+0xD0` read by the buff's damage getters `0x6F0FB6C0` and its hit handler `0x6F0FB7D0`, `VP`).
/// The buff flags (positive, magic, dispellable) are UNSURE.
fn vampire_potion(gm: &mut Game, caster: ObjKey, level: &SpellLevel) {
    if !gm.alive(caster) {
        return;
    }
    let hero = gm.target_hero(caster);
    let (dur, ratio, bonus) = vampire_potion_args(level, hero, F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let preds = crate::abilities::BuffPredicates {
        positive: true,
        magic: true,
        dispellable: true,
        ..crate::abilities::buff_predicates(BUFF_VAMPIRE_POTION)
    };
    let mods = crate::unit_spells::UnitMods {
        damage_int: conv::f2i(bonus),
        life_steal: ratio,
        ..Default::default()
    };
    gm.apply_mods_buff(caster, BUFF_VAMPIRE_POTION, preds, mods, dur);
}

/// The life steal of the vampire buff after a hit that dealt `damage` (the buff handler `0x6F0FB7D0`, [`vampire_potion_heal`]).
fn vampire_hit(gm: &mut Game, attacker: ObjKey, victim: ObjKey, damage: F32) {
    let ratio = gm.unit(attacker).buffs.iter().find_map(|b| match &b.effect {
        Some(crate::auras::BuffEffect::Mods(m)) if b.base == BUFF_VAMPIRE_POTION => Some(m.life_steal),
        _ => None,
    });
    let Some(ratio) = ratio else {
        return;
    };
    let (a_owner, v_owner) = (gm.unit(attacker).owner, gm.unit(victim).owner);
    let mechanical = {
        let u = gm.unit(victim);
        crate::natives::unit_attr::is_unit_type(gm, u, crate::unitclass::ty::MECHANICAL)
    };
    let input = VampireHitInput {
        victim_mechanical: mechanical,
        relation: gm.is_enemy(a_owner, v_owner),
        // The Misc keys at `0x6F8B4A5C` and `0x6F8B288C` are not in the shipped MiscData (UNSURE: read as off).
        misc_immunity_gate: false,
        victim_slot_11c: false,
        victim_immune: false,
        misc_illusions_heal: gm.spells.item.illusions_heal,
        owner_gate: gm.unit(attacker).illusion,
        victim_illusion: gm.unit(victim).illusion,
    };
    if let (_, Some(heal)) = vampire_potion_heal(&input, ratio, damage) {
        add_life(gm, attacker, heal);
    }
}

/// Town portal (`AItp`, `CAbilityItemTownPortal`): UNSURE model. Decoded: a target structure (`targs`), the cast time
/// `Cast` (5 s) is the cast core's delay, `Area` 1100 around the caster, the class slot `0x418` runs the area group
/// builder with the callback `0x6F0264B0` (not decoded). The model reads `DataA` as the count of teleported units and
/// `DataB` as the clustering flag (the Mass Teleport fields: `human_heroes::teleport_select`): the caster and the chosen
/// own or allied non-structure units in the area go to the target's position, keeping their offsets when clustering.
fn town_portal(gm: &mut Game, caster: ObjKey, target: CastTarget, level: &SpellLevel) {
    let CastTarget::Unit(dest) = target else {
        return;
    };
    if !gm.alive(caster) || !gm.alive(dest) {
        return;
    }
    let (cx, cy, owner) = {
        let c = gm.unit(caster);
        (c.x, c.y, c.owner)
    };
    let (dx, dy) = {
        let d = gm.unit(dest);
        (d.x, d.y)
    };
    let mut recs = Vec::new();
    for k in crate::natives::circle_candidates(gm, cx, cy, level.area) {
        if k == caster || !gm.alive(k) {
            continue;
        }
        let u = gm.unit(k);
        if gm.is_enemy(owner, u.owner) || gm.unit_types().is_building(u.type_id) {
            continue;
        }
        let (ex, ey) = (arith::sub(u.x, cx), arith::sub(u.y, cy));
        let d2 = arith::add(arith::add(arith::mul(ex, ex), arith::mul(ey, ey)), F32::ZERO);
        let hero = u.hero.as_ref().map(|h| h.level);
        recs.push((
            k,
            crate::human_heroes::TeleportRec {
                dist: conv::f2i(d2),
                key: hero.unwrap_or(0),
                hero: i32::from(hero.is_some() && !u.illusion),
                own: i32::from(u.owner == owner),
                id: k.0 as i32,
            },
        ));
    }
    let chosen = crate::human_heroes::teleport_select(recs, conv::f2i(level.data[0]));
    let cluster = level.data[1].to_bits() & 0x7FFF_FFFF != 0;
    let (px, py) = gm.clamp_unit_move(dx, dy);
    gm.store_unit_position(caster, px, py);
    for (k, _) in chosen {
        let (ux, uy) = {
            let u = gm.unit(k);
            (u.x, u.y)
        };
        let (tx, ty) = if cluster {
            (arith::add(dx, arith::sub(ux, cx)), arith::add(dy, arith::sub(uy, cy)))
        } else {
            (dx, dy)
        };
        let (px, py) = gm.clamp_unit_move(tx, ty);
        gm.store_unit_position(k, px, py);
    }
}

/// The experience a `CAbilityLevelMod` of `n` levels adds (`0x6F0DE200`): the hero ends `n` levels higher with the same
/// fraction of progress inside the level. `None` for a unit that is not a hero or when the tables are missing.
pub fn level_mod_amount(gm: &Game, unit: ObjKey, n: i32) -> Option<F32> {
    let h = gm.unit(unit).hero.as_ref()?;
    let tables = gm.order_tables()?;
    let xp = gm.hero_xp();
    Some(level_mod_value(h.level, h.xp, n, &|l| xp.need_xp(l), tables))
}

/// The arithmetic of [`level_mod_amount`] on plain numbers (Unicorn-checked against `0x6F0DE200`).
pub fn level_mod_value(level: i32, xp: i32, n: i32, need: &dyn Fn(i32) -> i32, t: &Tables) -> F32 {
    let esi = level.wrapping_sub(1);
    let top = n.wrapping_add(esi); // N + L - 1
    let target = top.wrapping_add(1); // N + L
    let a = if esi > 0 { need(esi) } else { 0 };
    let b = need(top);
    let c = need(target);
    let r1 = conv::i2r(b.wrapping_sub(a));
    let r2 = conv::i2r(xp.wrapping_sub(a));
    let f = arith::div(r2, r1, t);
    let r3 = conv::i2r(c.wrapping_sub(b));
    let prod = arith::mul(f, r3);
    let whole = conv::f2i(prod);
    conv::i2r(whole.wrapping_add(b.wrapping_sub(xp)))
}

/// Gives experience through the experience engine `0x6F20B080` (the tome of experience passes the range flag, which
/// applies the upgrade bonus; the model has no such upgrade, so it is a plain gain).
fn add_hero_xp(gm: &mut Game, unit: ObjKey, value: F32, range: bool) {
    let Some(tables) = gm.order_tables().cloned() else {
        return;
    };
    let xp = gm.hero_xp().clone();
    let owner = gm.unit(unit).owner;
    let handicap = gm.misc.players[(owner as usize).min(15)].handicap_xp;
    let caps = crate::kill_xp::caps_of(gm.unit(unit));
    let no_levels = |_: u32| 0i32;
    let no_value = |_: u32, _: i32| F32::ZERO;
    let env = EngineEnv {
        handicap,
        bonus: BonusEnv {
            b1: 0,
            slots: 0,
            codes: [0; 9],
            levels: &no_levels,
            value: &no_value,
        },
        caps,
    };
    if let Some(h) = gm.unit_mut(unit).hero.as_mut() {
        heroes::experience_engine(&xp, &tables, h, value, Gain { handicap: false, walk: false, range }, &env);
    }
}

/// The class effect of a cast item ability (called from `cast::run_effect`). Returns whether `rec.base` is an item
/// ability handled here.
pub(crate) fn run_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    let fam = {
        let gm = game.borrow();
        let Some(fam) = gm.spells.item.family(rec.base) else {
            return false;
        };
        if !fam.castable() {
            return true;
        }
        fam
    };
    if fam == Family::Command {
        command(game, rt, rec);
        return true;
    }
    let mut gm = game.borrow_mut();
    let level = match fam {
        // The tomes read level 1 whatever the ability level.
        Family::TomeLife
        | Family::TomeMana
        | Family::TomeXp
        | Family::TomeLevel
        | Family::TomeAttrs
        | Family::TomeAttack
        | Family::Glyph => level_one(&gm, rec.base),
        _ => rec.level,
    };
    apply_effect(&mut gm, fam, rec.base, &level, rec.caster, rec.target);
    drop(gm);
    flush_deaths(game, rt);
    true
}

// ---- the use path ----

/// The first castable ability of an item (`0x6F2B6CC0`: the first ability whose class answers as usable).
fn usable_alias(gm: &Game, item: ObjKey) -> Option<Option<u32>> {
    let spec = gm.spells.item.specs.get(&gm.item(item).type_id)?;
    Some(spec.abilities.iter().copied().find(|&a| gm.spells.item.family(a).is_some_and(Family::castable) || delegated(gm, a)))
}

/// Starts the use of `item` by `unit`: the cast of the item's first castable ability on `target`. `None` when the item
/// type has no registered item abilities (the caller keeps the plain recorded order). `Some(false)` when the item has
/// no castable ability or the cast does not start (the validator or the readiness refuses).
pub fn start_use(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    unit: ObjKey,
    item: ObjKey,
    target: CastTarget,
) -> Option<bool> {
    let alias = {
        let gm = game.borrow();
        match usable_alias(&gm, item)? {
            Some(a) => a,
            None => return Some(false),
        }
    };
    {
        let mut gm = game.borrow_mut();
        if !gm.alive(unit) || gm.item(item).removed || gm.item(item).holder != Some(unit) {
            return Some(false);
        }
        // The item cools while its group cools on the holder (`0x6F2B6CC0`: the item's cooling flag `0x200` yields no order).
        let g = item_group(&gm, item);
        if g != 0 && gm.is_cooling(unit, g) {
            return Some(false);
        }
        gm.spells.item.pending.insert((unit, alias), item);
    }
    let ok = begin_cast(game, rt, unit, alias, target).is_ok();
    if !ok {
        game.borrow_mut().spells.item.pending.remove(&(unit, alias));
    }
    Some(ok)
}

/// The group cooldown a finished item cast starts (`0x6F05C840`, the item cast end, CONFIRMED structure, Unicorn `GR`
/// rows): the ability's `Cool` is handed to the unit's group routine (`0x6F0E3F30`) for the item named by the ability's
/// item key, unless the item type's `ignoreCD` flag (`0x6F2B8DA0`) zeroes it, and only when it is above 0.0 (the soft
/// compare `0x6F01FD40`).
pub fn group_cooldown_start(cool: F32, ignore: bool) -> Option<F32> {
    if ignore || !arith::lt(F32::ZERO, cool) {
        None
    } else {
        Some(cool)
    }
}

/// The cooldown group of an item: its type's `cooldownID` (the key the unit's group table compares, `0x6F2B8D70`).
fn item_group(gm: &Game, item: ObjKey) -> u32 {
    gm.spells.item.specs.get(&gm.item(item).type_id).map_or(0, |s| s.cooldown_id)
}

/// The end of a cast (`0x6F050950`): a cast started by a use order uses one charge when it completed (UNSURE for a
/// cast that was interrupted: the original marks the ability with flag `0x4000` and uses the charge at the end).
pub(crate) fn on_cast_end(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec, completed: bool) {
    let item = {
        let mut gm = game.borrow_mut();
        let Some(item) = gm.spells.item.pending.remove(&(rec.caster, rec.base)) else {
            return;
        };
        if !completed {
            return;
        }
        // The cooldown group (CONFIRMED, see [`group_cooldown_start`]): keyed by the item type's `cooldownID`, started with
        // the ability's `Cool` for every item of the group the unit carries.
        let g = item_group(&gm, item);
        let ignore = gm.spells.item.specs.get(&gm.item(item).type_id).is_some_and(|s| s.ignore_cooldown);
        if g != 0 && g != rec.base {
            if let Some(cool) = group_cooldown_start(rec.level.cool, ignore) {
                gm.start_cooldown(rec.caster, g, cool);
            }
        }
        item
    };
    use_charge(game, rt, item);
}

/// The charge routine `0x6F2B8390` (CONFIRMED structure by disassembly, Unicorn `item_cores_emu.py`): an item with the
/// no-charges flag keeps its count; otherwise a count at or below 0 does nothing and any other count drops by one.
/// Then the holder is told (`0x6F299750`, event `0x80257`: the use-item events), and a perishable item whose count is
/// 0 is destroyed.
pub fn use_charge(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, item: ObjKey) {
    let holder;
    {
        let mut gm = game.borrow_mut();
        let it = gm.item_mut(item);
        if it.removed {
            return;
        }
        let Some((charges, _, _)) = charge_outcome(it.flags, it.charges, true) else {
            return;
        };
        it.charges = charges;
        holder = it.holder;
    }
    if let Some(unit) = holder {
        fire_use_events(game, rt, unit, item);
    }
    // The destroy test reads the flags and the count again, after the event handlers ran.
    let mut gm = game.borrow_mut();
    let it = gm.item(item);
    if destroys(it.flags, it.charges) && !it.removed {
        crate::natives::item::remove_item(&mut gm, item);
    }
}

/// The decrement of the charge routine: `(flags, new charges)`, or `None` when the routine returns early (charges at or
/// below 0 without the no-charges flag).
pub fn charge_step(flags: u32, charges: i32) -> (u32, Option<i32>) {
    if flags & item_flags::NO_CHARGES != 0 {
        return (flags, Some(charges));
    }
    if charges <= 0 {
        return (flags, None);
    }
    (flags, Some(charges - 1))
}

/// The destroy rule after a use: no no-charges flag, the perishable flag and no charge left.
pub fn destroys(flags: u32, charges: i32) -> bool {
    flags & item_flags::NO_CHARGES == 0 && flags & item_flags::PERISHABLE != 0 && charges == 0
}

/// Fires the use-item events: player-unit `0x32`, then unit `0x57` (the key `0x80257` is the unit one), with the unit
/// as manipulating unit and the item as manipulated item.
fn fire_use_events(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey, item: ObjKey) {
    let owner = game.borrow().unit(unit).owner;
    for (agent, id) in [(ObjKey(owner), 0x32u32), (unit, 0x57u32)] {
        let firing = {
            let mut gm = game.borrow_mut();
            gm.firings.push(Firing {
                id,
                data: vec![
                    (Field::Manipulator, Datum::Object(unit)),
                    (Field::Manipulated, Datum::Object(item)),
                ],
            });
            (gm.firings.len() - 1) as u32
        };
        fire_observers_in(game, rt, agent, KEY_BASE + id, unit, Some(firing));
    }
}

// ---- helpers on Game used above ----

impl Game {
    /// The level record `index` (0 = level 1) of ability `alias` (an empty record when none is installed).
    pub(crate) fn spells_level_record(&self, alias: u32, index: usize) -> SpellLevel {
        self.spell_levels_of(alias).get(index).copied().unwrap_or_default()
    }
}
