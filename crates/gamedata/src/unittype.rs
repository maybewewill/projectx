//! Unit type records: the typed data the 1.26a engine builds for each unit type from the unit SLK tables.
//!
//! Sources (`docs/spec/unit-types.md`): `Units\UnitUI.slk` (the list of unit types, key `unitUIID`), and the
//! rows of the same unit in `Units\UnitData.slk`, `Units\UnitBalance.slk` and `Units\UnitWeapons.slk`. The
//! `Units\UnitAbilities.slk` table is not read here (its columns are not part of the record).
//!
//! The field map (which column fills which record field, and how the text is read) is written from the
//! original's record builder (CONFIRMED by decompile and disassembly, see the spec). Rules implemented here:
//! - the unit must have a row in `UnitData` and in `UnitUI` (a unit without a `UnitUI` row is not valid);
//! - a missing `UnitBalance` row or `UnitWeapons` row gives defaults for that part (see [`load_unit_type`]);
//! - integer cells: text starting with an apostrophe is a four-character raw code (packed big-endian); any
//!   other text is read as C `strtol(text, NULL, 0)` (decimal, `0x` hex, leading-zero octal, optional sign);
//! - real cells: C `atof` of the text (leading white space, optional sign, digits, fraction, exponent; anything
//!   after the number is ignored; no number gives 0.0), correctly rounded to `f64` and then rounded to `f32`
//!   (LIKELY: the original calls an imported text-to-double routine, Storm ordinal 574, and stores a single
//!   precision result; Storm.dll is not available to check it. See [`crate::real::atof_f32`]);
//! - text cells: the text with one pair of surrounding quotes removed;
//! - one load-time rule: when the `Acquire` value is below 1.0 the acquire range becomes `Sight` (CONFIRMED).
//!
//! Clean-room note: this file holds our own code and the column names (game data vocabulary), and the rules above
//! are descriptions of behavior. No game data is embedded; the tests use synthetic tables.

use std::collections::BTreeMap;

use crate::real::atof_f32;
use crate::slk::Slk;

/// `Units\UnitData.slk` (the unit flags and the list of columns of the unit type).
pub const UNIT_DATA_FILE: &str = "Units\\UnitData.slk";
/// `Units\UnitBalance.slk` (the numbers: costs, speeds, hit points, armor, sight, food).
pub const UNIT_BALANCE_FILE: &str = "Units\\UnitBalance.slk";
/// `Units\UnitUI.slk` (the presentation row; its rows are the unit types).
pub const UNIT_UI_FILE: &str = "Units\\UnitUI.slk";
/// `Units\UnitWeapons.slk` (launch and impact heights, acquire range, and the two weapon slots).
pub const UNIT_WEAPONS_FILE: &str = "Units\\UnitWeapons.slk";
/// `Units\UnitAbilities.slk` (the ability list of each unit type, column `abilList`). Optional: without it the
/// lists are empty.
pub const UNIT_ABILITIES_FILE: &str = "Units\\UnitAbilities.slk";

/// The tables a unit type record is built from. A missing table is `None` (every row is then missing).
#[derive(Debug, Clone, Copy, Default)]
pub struct UnitTypeTables<'a> {
    /// `UnitData.slk`.
    pub data: Option<&'a Slk>,
    /// `UnitBalance.slk`.
    pub balance: Option<&'a Slk>,
    /// `UnitUI.slk`.
    pub ui: Option<&'a Slk>,
    /// `UnitWeapons.slk`.
    pub weapons: Option<&'a Slk>,
    /// `UnitAbilities.slk` (optional, see [`UNIT_ABILITIES_FILE`]).
    pub abilities: Option<&'a Slk>,
}

/// Why a unit type record is not built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitTypeError {
    /// The unit has no row in `UnitUI.slk` (the original reports "missing required UI data - not valid in game").
    MissingUiRow(String),
    /// The unit has no row in `UnitData.slk`.
    MissingDataRow(String),
}

/// One of the two weapon slots of a unit type (columns with the suffix `1` or `2` in `UnitWeapons.slk`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WeaponSlot {
    /// `dmgUp` (integer; damage upgrade column).
    pub dmg_up: i32,
    /// `dice` (number of damage dice).
    pub dice: i32,
    /// `sides` (sides per die).
    pub sides: i32,
    /// `dmgplus` (flat damage added to the dice).
    pub dmg_plus: i32,
    /// `targs` (target flags text; the flags are parsed by the original's flag reader, not here).
    pub targs: String,
    /// `targCount` (number of targets).
    pub targ_count: i32,
    /// `damageLoss` (real).
    pub damage_loss: f32,
    /// `spillDist` (real).
    pub spill_dist: f32,
    /// `spillRadius` (real).
    pub spill_radius: f32,
    /// `atkType` (attack type name, as text; the original maps it to an enum).
    pub atk_type: String,
    /// `weapType` (weapon type name, as text).
    pub weap_type: String,
    /// `weapTp` (weapon sound/type name, as text).
    pub weap_tp: String,
    /// `rangeN` (attack range, real).
    pub range: f32,
    /// `RngBuff` (range buff, real).
    pub rng_buff: f32,
    /// `cool` (cooldown in seconds, real).
    pub cool: f32,
    /// `dmgpt` (damage point in seconds, real).
    pub dmg_pt: f32,
    /// `backSw` (backswing in seconds, real).
    pub back_swing: f32,
    /// `splashTargs` (splash target flags text).
    pub splash_targs: String,
    /// `Farea` (real).
    pub farea: f32,
    /// `Harea` (real).
    pub harea: f32,
    /// `Qarea` (real).
    pub qarea: f32,
    /// `Hfact` (real).
    pub hfact: f32,
    /// `Qfact` (real).
    pub qfact: f32,
    /// `mindmg` (real).
    pub min_dmg: f32,
    /// `maxdmg` (real).
    pub max_dmg: f32,
    /// `showUI` (integer flag).
    pub show_ui: i32,
}

/// The record of one unit type, with the fields the engine reads. Names follow the SLK columns.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UnitTypeData {
    /// The unit code as written in the tables (for example `hfoo`).
    pub code: String,
    /// The raw code (four characters packed big-endian), when the code has four characters.
    pub rawcode: Option<u32>,
    /// The `name` column of `UnitUI.slk` (text; empty when the cell is missing). UNSURE: which name the original's
    /// name lookup uses (`CreateUnitByName`); this column is the editor's display name.
    pub name: String,
    /// Whether the unit has a row in `UnitBalance.slk`. Without it the balance fields are zero (see
    /// [`load_unit_type`]).
    pub has_balance_row: bool,
    /// Whether the unit has a row in `UnitWeapons.slk`. Without it there is no weapon.
    pub has_weapons_row: bool,

    // UnitData.slk
    /// `canSleep` (integer).
    pub can_sleep: i32,
    /// `canFlee` (integer).
    pub can_flee: i32,
    /// `cargoSize` (integer).
    pub cargo_size: i32,
    /// `orientInterp` (integer).
    pub orient_interp: i32,
    /// `formation` (integer).
    pub formation: i32,
    /// `fatLOS` (integer).
    pub fat_los: i32,
    /// `death` (real).
    pub death: f32,
    /// `turnRate` (real; the per-type turn value, unit of the original's turn formula not decoded).
    pub turn_rate: f32,
    /// `propWin` (real).
    pub prop_win: f32,
    /// `moveHeight` (real).
    pub move_height: f32,
    /// `moveFloor` (real).
    pub move_floor: f32,
    /// `prio` (real; movement priority class input).
    pub prio: f32,
    /// `points` (integer).
    pub points: i32,
    /// `deathType` (integer).
    pub death_type: i32,
    /// `nameCount` (integer).
    pub name_count: i32,
    /// `isBuildOn` (integer).
    pub is_build_on: i32,
    /// `canBuildOn` (integer).
    pub can_build_on: i32,
    /// `requireWaterRadius` (real).
    pub require_water_radius: f32,
    /// `buffRadius` (real).
    pub buff_radius: f32,
    /// `buffType` (text, the buff type name).
    pub buff_type: String,
    /// `race` (text, the race name of `UnitData`).
    pub race: String,
    /// The race enum of the record (offset `+0x1C0`): [`race_enum`] of the `race` cell. CONFIRMED table and call
    /// site; a missing cell gives 0 (UNSURE: the original may give 7 for a missing text, see [`RACE_NULL_TEXT`]).
    /// Creeps are 8, neutral (`other`) buildings are 7 (docs/spec/natives-pending.md section 3.1).
    pub race_id: i32,
    /// `movetp` (text, the movement type name).
    pub move_type: String,

    // UnitBalance.slk
    /// `level` (integer).
    pub level: i32,
    /// `bldtm` (build time, integer as written by the table).
    pub build_time: i32,
    /// `reptm` (repair time, integer).
    pub repair_time: i32,
    /// `goldcost` (integer).
    pub gold_cost: i32,
    /// `lumbercost` (integer).
    pub lumber_cost: i32,
    /// `goldRep` (integer, refund rate column).
    pub gold_rep: i32,
    /// `lumberRep` (integer).
    pub lumber_rep: i32,
    /// `fmade` (food made, integer).
    pub food_made: i32,
    /// `fused` (food used, integer).
    pub food_used: i32,
    /// `bountydice`, `bountysides`, `bountyplus` (gold bounty).
    pub bounty: [i32; 3],
    /// `lumberbountydice`, `lumberbountysides`, `lumberbountyplus` (lumber bounty).
    pub lumber_bounty: [i32; 3],
    /// `stockMax` (integer).
    pub stock_max: i32,
    /// `stockRegen` (real).
    pub stock_regen: f32,
    /// `stockStart` (real).
    pub stock_start: f32,
    /// `HP` (hit points, real).
    pub hp: f32,
    /// `regenHP` (real).
    pub regen_hp: f32,
    /// `regenType` (text).
    pub regen_type: String,
    /// `mana0` (initial mana, real).
    pub mana0: f32,
    /// `manaN` (maximum mana, real).
    pub mana_max: f32,
    /// `regenMana` (real).
    pub regen_mana: f32,
    /// `spd` (movement speed, real, units per second as the original stores it).
    pub speed: f32,
    /// `minSpd` (real).
    pub min_speed: f32,
    /// `maxSpd` (real).
    pub max_speed: f32,
    /// `def` (armor, real).
    pub armor: f32,
    /// `defUp` (armor upgrade, real).
    pub armor_up: f32,
    /// `defType` (armor type name, text).
    pub armor_type: String,
    /// `collision` (real; the collision size).
    pub collision: f32,
    /// `sight` (real).
    pub sight: f32,
    /// `nsight` (night sight, real).
    pub night_sight: f32,
    /// `STR`, `AGI`, `INT` (integers).
    pub attributes: [i32; 3],
    /// `STRplus`, `AGIplus`, `INTplus` (reals).
    pub attribute_plus: [f32; 3],
    /// `Primary` (text, primary attribute name).
    pub primary: String,
    /// `isbldg` (integer).
    pub is_building: i32,
    /// `tilesets` (text: the tileset letters where the type may appear, or `*` for all). The table it is read from
    /// is UNSURE (UnitBalance per docs/spec/unit-types.md section 4.1); the random pick walks test it with
    /// [`tileset_accepts`].
    pub tilesets: String,
    /// `nbrandom` (integer, random name count).
    pub nb_random: i32,
    /// `repulse`, `repulseParam`, `repulseGroup`, `repulsePrio` (integers).
    pub repulse: [i32; 4],

    // UnitUI.slk
    /// `fogRad` (real).
    pub fog_radius: f32,
    /// `scaleBull` (integer as read by the original).
    pub scale_bull: i32,
    /// `blend` (real).
    pub blend: f32,
    /// `walk` (real, walk speed of the animation).
    pub walk: f32,
    /// `run` (real, run speed of the animation).
    pub run: f32,
    /// `occH` (real, occluded height).
    pub occ_h: f32,
    /// `special` (integer).
    pub special: i32,
    /// `hostilePal` (integer).
    pub hostile_pal: i32,

    // UnitWeapons.slk (unit level)
    /// `weapsOn` (integer; the unit has weapons).
    pub weapons_on: i32,
    /// `abilList` of `UnitAbilities.slk`: the ability rawcodes of the type, in file order (the comma-separated
    /// cell split and trimmed; empty entries dropped). Empty when the table or the cell is missing.
    pub abil_list: Vec<String>,
    /// `acquire` (real, after the load-time rule: below 1.0 becomes `sight`).
    pub acquire: f32,
    /// `minRange` (real).
    pub min_range: f32,
    /// `castpt` (cast point, real).
    pub cast_point: f32,
    /// `castbsw` (cast backswing, real).
    pub cast_backswing: f32,
    /// `launchX`, `launchY`, `launchZ` (the missile launch offset, real).
    pub launch: [f32; 3],
    /// `launchSwimZ` (real).
    pub launch_swim_z: f32,
    /// `impactZ` (real; the impact height of the weapon. This is the value at record offset `+0x210`).
    pub impact_z: f32,
    /// `impactSwimZ` (real).
    pub impact_swim_z: f32,
    /// The two weapon slots (`<name>1` and `<name>2`).
    pub weapons: [WeaponSlot; 2],
}

/// Strips one pair of surrounding double quotes and the spaces around them.
pub fn unquote(raw: &str) -> &str {
    let t = raw.trim();
    t.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(t)
        .trim()
}

/// The C library's `strtol(text, NULL, 0)` on a 32-bit `long`, returned as the 32-bit pattern of the result.
///
/// Whitespace is skipped, then an optional sign, then `0x`/`0X` (hex), a leading `0` (octal) or decimal digits.
/// Parsing stops at the first character that is not a digit of the base. Overflow gives `LONG_MAX` (positive) or
/// `LONG_MIN` (negative). No digits give 0.
pub fn strtol_base0(text: &str) -> u32 {
    let b = text.as_bytes();
    let n = b.len();
    let mut i = 0;
    while i < n && matches!(b[i], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < n && (b[i] == b'+' || b[i] == b'-') {
        neg = b[i] == b'-';
        i += 1;
    }
    let mut base: u64 = 10;
    if i + 2 < n
        && b[i] == b'0'
        && (b[i + 1] == b'x' || b[i + 1] == b'X')
        && b[i + 2].is_ascii_hexdigit()
    {
        base = 16;
        i += 2;
    } else if i < n && b[i] == b'0' {
        base = 8;
    }
    let start = i;
    let mut val: u64 = 0;
    let mut overflow = false;
    while i < n {
        let d = match (b[i] as char).to_digit(16) {
            Some(d) if u64::from(d) < base => u64::from(d),
            _ => break,
        };
        val = val.saturating_mul(base).saturating_add(d);
        if val > (1 << 40) {
            overflow = true;
        }
        i += 1;
    }
    if i == start {
        return 0;
    }
    let mut value: i64 = if overflow { i64::MAX } else { val as i64 };
    if neg {
        value = -value;
    }
    let clamped = if value > i64::from(i32::MAX) {
        i64::from(i32::MAX)
    } else if value < i64::from(i32::MIN) {
        i64::from(i32::MIN)
    } else {
        value
    };
    (clamped as i32) as u32
}

/// The integer value of an integer cell (see the module documentation). Returns the 32-bit pattern.
pub fn cell_int_bits(text: &str) -> u32 {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix('\'') {
        // four-character raw code: up to four characters, the closing apostrophe ends the code
        let mut v: u32 = 0;
        let mut count = 4;
        for c in rest.bytes() {
            count -= 1;
            if c == 0 {
                return v;
            }
            if c == b'\'' {
                return v;
            }
            v = (v << 8) | u32::from(c);
            if count == 0 {
                return v;
            }
        }
        return v;
    }
    strtol_base0(t)
}

/// The integer value of an integer cell as a signed number (see [`cell_int_bits`]).
pub fn cell_int(text: &str) -> i32 {
    cell_int_bits(text) as i32
}

/// The real value of a real cell: C `atof` of the text, rounded to `f32` (see [`crate::real::atof_f32`], LIKELY).
/// Text without a number prefix gives `0.0`, as `atof` does.
pub fn cell_real(text: &str) -> f32 {
    atof_f32(text).unwrap_or(0.0)
}

/// The raw text of a cell with its quotes removed, or `None` when the cell is empty or missing.
fn cell_text<'a>(slk: Option<&'a Slk>, key: &str, column: &str) -> Option<&'a str> {
    let raw = slk?.raw(key, column)?;
    let t = unquote(raw);
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

/// A row of one table, read with the typed cell rules. A missing cell gives the zero of the type.
#[derive(Clone, Copy)]
struct Cells<'a> {
    slk: Option<&'a Slk>,
    key: &'a str,
}

impl Cells<'_> {
    fn int(self, column: &str) -> i32 {
        cell_text(self.slk, self.key, column).map_or(0, cell_int)
    }

    fn real(self, column: &str) -> f32 {
        cell_text(self.slk, self.key, column).map_or(0.0, cell_real)
    }

    fn text(self, column: &str) -> String {
        cell_text(self.slk, self.key, column)
            .unwrap_or("")
            .to_owned()
    }

    fn weapon(self, n: u8) -> WeaponSlot {
        let col = |name: &str| format!("{name}{n}");
        WeaponSlot {
            dmg_up: self.int(&col("dmgUp")),
            dice: self.int(&col("dice")),
            sides: self.int(&col("sides")),
            dmg_plus: self.int(&col("dmgplus")),
            targs: self.text(&col("targs")),
            targ_count: self.int(&col("targCount")),
            damage_loss: self.real(&col("damageLoss")),
            spill_dist: self.real(&col("spillDist")),
            spill_radius: self.real(&col("spillRadius")),
            atk_type: self.text(&col("atkType")),
            weap_type: self.text(&col("weapType")),
            weap_tp: self.text(&col("weapTp")),
            range: self.real(&col("rangeN")),
            rng_buff: self.real(&col("RngBuff")),
            cool: self.real(&col("cool")),
            dmg_pt: self.real(&col("dmgpt")),
            back_swing: self.real(&col("backSw")),
            splash_targs: self.text(&col("splashTargs")),
            farea: self.real(&col("Farea")),
            harea: self.real(&col("Harea")),
            qarea: self.real(&col("Qarea")),
            hfact: self.real(&col("Hfact")),
            qfact: self.real(&col("Qfact")),
            min_dmg: self.real(&col("mindmg")),
            max_dmg: self.real(&col("maxdmg")),
            show_ui: self.int(&col("showUI")),
        }
    }
}

/// The row key that a table holds for `code`. The rows are matched by their key, as the original does.
fn has_row(slk: Option<&Slk>, code: &str) -> bool {
    slk.is_some_and(|s| s.row(code).is_some())
}

/// Builds the record of one unit type.
///
/// Missing parts (CONFIRMED from the builder's branches):
/// - no `UnitBalance` row: the balance fields stay zero and `acquire` becomes `1.0`;
/// - no `UnitWeapons` row: the weapon fields and the unit weapon fields are zero, `acquire` becomes zero before the
///   load-time rule (so it ends as `sight`), and both weapon slots get `dmg_pt = 1.0` and `show_ui = 1`.
///
/// Errors: no `UnitData` row, or no `UnitUI` row.
pub fn load_unit_type(
    tables: &UnitTypeTables<'_>,
    code: &str,
) -> Result<UnitTypeData, UnitTypeError> {
    if !has_row(tables.ui, code) {
        return Err(UnitTypeError::MissingUiRow(code.to_owned()));
    }
    if !has_row(tables.data, code) {
        return Err(UnitTypeError::MissingDataRow(code.to_owned()));
    }
    let data = Cells {
        slk: tables.data,
        key: code,
    };
    let balance_ok = has_row(tables.balance, code);
    let weapons_ok = has_row(tables.weapons, code);
    let abil = Cells {
        slk: tables.abilities,
        key: code,
    };
    let bal = Cells {
        slk: tables.balance,
        key: code,
    };
    let ui = Cells {
        slk: tables.ui,
        key: code,
    };
    let wpn = Cells {
        slk: tables.weapons,
        key: code,
    };

    let mut out = UnitTypeData {
        code: code.to_owned(),
        rawcode: rawcode_of(code),
        name: ui.text("name"),
        has_balance_row: balance_ok,
        has_weapons_row: weapons_ok,
        can_sleep: data.int("canSleep"),
        can_flee: data.int("canFlee"),
        cargo_size: data.int("cargoSize"),
        orient_interp: data.int("orientInterp"),
        formation: data.int("formation"),
        fat_los: data.int("fatLOS"),
        death: data.real("death"),
        turn_rate: data.real("turnRate"),
        prop_win: data.real("propWin"),
        move_height: data.real("moveHeight"),
        move_floor: data.real("moveFloor"),
        prio: data.real("prio"),
        points: data.int("points"),
        death_type: data.int("deathType"),
        name_count: data.int("nameCount"),
        is_build_on: data.int("isBuildOn"),
        can_build_on: data.int("canBuildOn"),
        require_water_radius: data.real("requireWaterRadius"),
        buff_radius: data.real("buffRadius"),
        buff_type: data.text("buffType"),
        move_type: data.text("movetp"),
        race: data.text("race"),
        race_id: race_enum(&data.text("race")),
        fog_radius: ui.real("fogRad"),
        scale_bull: ui.int("scaleBull"),
        blend: ui.real("blend"),
        walk: ui.real("walk"),
        run: ui.real("run"),
        occ_h: ui.real("occH"),
        special: ui.int("special"),
        hostile_pal: ui.int("hostilePal"),
        ..UnitTypeData::default()
    };

    if balance_ok {
        out.level = bal.int("level");
        out.build_time = bal.int("bldtm");
        out.repair_time = bal.int("reptm");
        out.gold_cost = bal.int("goldcost");
        out.lumber_cost = bal.int("lumbercost");
        out.gold_rep = bal.int("goldRep");
        out.lumber_rep = bal.int("lumberRep");
        out.food_made = bal.int("fmade");
        out.food_used = bal.int("fused");
        out.bounty = [
            bal.int("bountydice"),
            bal.int("bountysides"),
            bal.int("bountyplus"),
        ];
        out.lumber_bounty = [
            bal.int("lumberbountydice"),
            bal.int("lumberbountysides"),
            bal.int("lumberbountyplus"),
        ];
        out.stock_max = bal.int("stockMax");
        out.stock_regen = bal.real("stockRegen");
        out.stock_start = bal.real("stockStart");
        out.hp = bal.real("HP");
        out.regen_hp = bal.real("regenHP");
        out.regen_type = bal.text("regenType");
        out.mana0 = bal.real("mana0");
        out.mana_max = bal.real("manaN");
        out.regen_mana = bal.real("regenMana");
        out.speed = bal.real("spd");
        out.min_speed = bal.real("minSpd");
        out.max_speed = bal.real("maxSpd");
        out.armor = bal.real("def");
        out.armor_up = bal.real("defUp");
        out.armor_type = bal.text("defType");
        out.collision = bal.real("collision");
        out.sight = bal.real("sight");
        out.night_sight = bal.real("nsight");
        out.attributes = [bal.int("STR"), bal.int("AGI"), bal.int("INT")];
        out.attribute_plus = [
            bal.real("STRplus"),
            bal.real("AGIplus"),
            bal.real("INTplus"),
        ];
        out.primary = bal.text("Primary");
        out.is_building = bal.int("isbldg");
        out.nb_random = bal.int("nbrandom");
        out.tilesets = bal.text("tilesets");
        out.repulse = [
            bal.int("repulse"),
            bal.int("repulseParam"),
            bal.int("repulseGroup"),
            bal.int("repulsePrio"),
        ];
    }

    out.abil_list = abil
        .text("abilList")
        .split(',')
        .map(|t| unquote(t).to_owned())
        .filter(|t| !t.is_empty())
        .collect();

    if weapons_ok {
        out.weapons_on = wpn.int("weapsOn");
        out.acquire = wpn.real("acquire");
        out.min_range = wpn.real("minRange");
        out.cast_point = wpn.real("castpt");
        out.cast_backswing = wpn.real("castbsw");
        out.launch = [
            wpn.real("launchX"),
            wpn.real("launchY"),
            wpn.real("launchZ"),
        ];
        out.launch_swim_z = wpn.real("launchSwimZ");
        out.impact_z = wpn.real("impactZ");
        out.impact_swim_z = wpn.real("impactSwimZ");
        out.weapons = [wpn.weapon(1), wpn.weapon(2)];
    } else {
        out.weapons = [
            WeaponSlot {
                dmg_pt: 1.0,
                show_ui: 1,
                ..WeaponSlot::default()
            },
            WeaponSlot {
                dmg_pt: 1.0,
                show_ui: 1,
                ..WeaponSlot::default()
            },
        ];
    }

    // Load-time rules. Balance missing: acquire is 1.0 (the builder stores it after the weapon read).
    if !balance_ok {
        out.acquire = 1.0;
    }
    // Acquire below 1.0 falls back to sight (CONFIRMED: compare with the constant 1.0 at 0x6FAAE4F4).
    if out.acquire < 1.0 {
        out.acquire = out.sight;
    }
    Ok(out)
}

/// Builds the records of all unit types: one per row of `UnitUI.slk` whose record is valid
/// (see [`load_unit_type`]). The map is ordered by code, so the result does not depend on hash order.
pub fn load_all(tables: &UnitTypeTables<'_>) -> BTreeMap<String, UnitTypeData> {
    load_all_ordered(tables).into_iter().collect()
}

/// The same records in the order the original inserts them into its global unit type list: the row order of
/// `UnitUI.slk` (the builder loop `0x6F2A7C30` walks the rows from 0 and the list order is the insertion order,
/// CONFIRMED by `tools/emu/unit_order_growth.py` across bucket growths). The random creep and neutral building picks
/// index into that order.
pub fn load_all_ordered(tables: &UnitTypeTables<'_>) -> Vec<(String, UnitTypeData)> {
    let mut out: Vec<(String, UnitTypeData)> = Vec::new();
    if let Some(ui) = tables.ui {
        for row in ui.rows() {
            let code = row.key();
            if let Ok(unit) = load_unit_type(tables, code) {
                out.push((code.to_owned(), unit));
            }
        }
    }
    out
}

/// The raw code of a four-character unit code (characters packed big-endian), or `None` for other lengths.
fn rawcode_of(code: &str) -> Option<u32> {
    let b = code.as_bytes();
    if b.len() != 4 || !b.is_ascii() {
        return None;
    }
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// The race names of the builder's name table (CONFIRMED: 8-byte entries `{name, value}` at `0x6FA70FE8`, read by
/// `0x6F26E210`; the value of the last entry is 0 and it stands for `unknown`).
pub const RACE_TABLE: &[(&str, i32)] = &[
    ("nightelf", 4),
    ("demon", 5),
    ("human", 1),
    ("orc", 2),
    ("undead", 3),
    ("creeps", 8),
    ("other", 7),
    ("critters", 10),
    ("commoner", 9),
    ("naga", 11),
    ("unknown", 0),
];

/// The value of a NULL race text in `0x6F26E210` (CONFIRMED: `lea eax,[ecx+7]`). The builder passes a text pointer;
/// whether a missing cell gives NULL is UNSURE, so [`load_unit_type`] uses [`race_enum`] of the empty text (0).
pub const RACE_NULL_TEXT: i32 = 7;

/// The race enum of a race name (`0x6F26E210`). The first entry of [`RACE_TABLE`] whose name matches gives its value;
/// no match gives 0. The match is ASCII case-insensitive (LIKELY: the compare is a Storm import, ordinal 508, that is
/// not checked against `Storm.dll`).
pub fn race_enum(text: &str) -> i32 {
    RACE_TABLE
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(text))
        .map_or(0, |&(_, value)| value)
}

/// The tileset test of the random pick walks (`0x6F6EB690`, called with `*` and then with the map's tileset char).
/// A type is accepted when its `tilesets` text contains `*` or the map's tileset char. A map tileset char of 0 skips
/// the test. LIKELY: the import (Storm ordinal 571, called as `(text, char)`) is a character search; not checked.
pub fn tileset_accepts(tilesets: &str, map_tileset: u8) -> bool {
    if map_tileset == 0 {
        return true;
    }
    tilesets.as_bytes().contains(&b'*') || tilesets.as_bytes().contains(&map_tileset)
}

/// Whether a unit type is a candidate of the creep walk `0x6F29FD90` (`ChooseRandomCreep`). CONFIRMED rules: race 8,
/// not a building, `special` 0, the first byte of the code not an uppercase letter, the level equal to `level` (or
/// `level` is -1), and the tileset test. The movement-type gate of the walk is not here: the native passes 1 for
/// all three of its flags, so that gate is always open. The candidate order is NOT confirmed (natives stay pending).
pub fn is_creep_candidate(ty: &UnitTypeData, level: i32, map_tileset: u8) -> bool {
    let first = ty.code.as_bytes().first().copied().unwrap_or(0);
    if first.is_ascii_uppercase() {
        return false;
    }
    (level == -1 || ty.level == level)
        && ty.race_id == 8
        && ty.is_building == 0
        && ty.special == 0
        && tileset_accepts(&ty.tilesets, map_tileset)
}

/// Whether a unit type is a candidate of the neutral building walk `0x6F294740` (`ChooseRandomNPBuilding`). CONFIRMED
/// rules: race 7, a building (`isbldg` non-zero), `hostilePal` 0, `special` 0, `nbrandom` non-zero, and the tileset
/// test. No level and no code test. The candidate order is NOT confirmed (natives stay pending).
pub fn is_neutral_building_candidate(ty: &UnitTypeData, map_tileset: u8) -> bool {
    ty.race_id == 7
        && ty.is_building != 0
        && ty.hostile_pal == 0
        && ty.special == 0
        && ty.nb_random != 0
        && tileset_accepts(&ty.tilesets, map_tileset)
}
