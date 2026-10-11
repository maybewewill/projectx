//! Unit classification text parsers (`docs/spec/unit-classification.md`).
//!
//! The unit type record keeps four classification inputs that `IsUnitType` and `IsUnitIdType` read:
//!
//! * the `type` column of `UnitBalance.slk` (record `+0x1C8`): a word of classification bits, parsed by the
//!   original's token matcher at `0x6F26DA70` ([`parse_classification`]);
//! * the `movetp` column of `UnitData.slk` (record `+0x1A8`): the movement type bits ([`parse_movement`], `0x6F26DB40`);
//! * the weapon `targs1` / `targs2` columns of `UnitWeapons.slk` (record `+0xA0`, `+0xA4`): target flag words
//!   ([`parse_target_flags`], `0x6F255250`);
//! * the weapon `weapTp1` / `weapTp2` columns (record `+0xF8`, `+0xFC`): the weapon kind enum
//!   ([`parse_weapon_kind`], `0x6F262D30`).
//!
//! The name tables are the original's (read from the read-only tables of Game.dll by address); names are matched
//! case-insensitively and completely (the compare is `_strnicmp` with an unbounded count, Storm ordinal 509).
//! Nothing here is a table dump of game data: the name lists are the parsers' vocabulary, written out as code.

/// Delimiters of the token splitter: space, comma, minus, tab (the string at `0x6F92C9EC`).
const DELIMS: [char; 4] = [' ', ',', '-', '\t'];

/// Longest token the splitter keeps: the destination buffer is 16 bytes, the copy stops one short of the size and
/// drops the rest of a longer token (CONFIRMED in Storm's ordinal 504: the write is skipped once `count + 1 >= size`,
/// the read pointer still advances).
const TOKEN_MAX: usize = 15;

/// Splits `text` like the original's tokenizer (Storm ordinal 504 with the delimiter set above): a token ends at the
/// next delimiter, which is consumed (so two delimiters in a row give an empty token, which matches no name and is
/// dropped here); a token longer than [`TOKEN_MAX`] characters is cut.
fn tokens(text: &str) -> Vec<String> {
    text.split(|c| DELIMS.contains(&c))
        .filter(|t| !t.is_empty())
        .map(|t| t.chars().take(TOKEN_MAX).collect())
        .collect()
}

/// Classification bits of record `+0x1C8` (unit `+0x248`). CONFIRMED (table `0x6FA70EC8`).
pub mod class_bits {
    pub const GIANT: u32 = 0x1;
    pub const UNDEAD: u32 = 0x2;
    pub const SUMMONED: u32 = 0x4;
    pub const MECHANICAL: u32 = 0x8;
    pub const PEON: u32 = 0x10;
    pub const SAPPER: u32 = 0x20;
    pub const TOWNHALL: u32 = 0x40;
    pub const ANCIENT: u32 = 0x80;
    pub const NEUTRAL: u32 = 0x100;
    pub const WARD: u32 = 0x200;
    pub const STANDON: u32 = 0x400;
    pub const TAUREN: u32 = 0x800;
}

const CLASS_TABLE: [(&str, u32); 12] = [
    ("giant", class_bits::GIANT),
    ("undead", class_bits::UNDEAD),
    ("summoned", class_bits::SUMMONED),
    ("mechanical", class_bits::MECHANICAL),
    ("peon", class_bits::PEON),
    ("sapper", class_bits::SAPPER),
    ("townhall", class_bits::TOWNHALL),
    ("ancient", class_bits::ANCIENT),
    ("neutral", class_bits::NEUTRAL),
    ("ward", class_bits::WARD),
    ("standon", class_bits::STANDON),
    ("tauren", class_bits::TAUREN),
];

/// The `type` column parser (`0x6F26DA70`). An empty text, or one starting with `_` or `-`, is 0 (checked before any
/// token is read). Otherwise every token that equals a name (case-insensitive) ORs its bit; unknown tokens are ignored
/// (the original logs them). CONFIRMED by disassembly.
pub fn parse_classification(text: &str) -> u32 {
    let first = text.chars().next();
    if matches!(first, None | Some('_') | Some('-')) {
        return 0;
    }
    let mut bits = 0;
    for t in tokens(text) {
        for (name, bit) in CLASS_TABLE {
            if t.eq_ignore_ascii_case(name) {
                bits |= bit;
            }
        }
    }
    bits
}

/// Movement type bits of record `+0x1A8` (unit `+0x1FC`). CONFIRMED (table `0x6FA70F28`).
pub mod move_bits {
    pub const FOOT: u32 = 0x1;
    pub const FLY: u32 = 0x2;
    pub const HORSE: u32 = 0x4;
    pub const HOVER: u32 = 0x8;
    pub const FLOAT: u32 = 0x10;
    pub const AMPH: u32 = 0x20;
    pub const UNBUILD: u32 = 0x40;
}

/// The `movetp` parser (`0x6F26DB40`): the whole text must equal one name (no splitting); a null text is 0, an
/// unknown text is 0 (`_`, `-`, `none` and the empty text are table entries with value 0). CONFIRMED.
pub fn parse_movement(text: &str) -> u32 {
    const TABLE: [(&str, u32); 7] = [
        ("fly", move_bits::FLY),
        ("hover", move_bits::HOVER),
        ("foot", move_bits::FOOT),
        ("horse", move_bits::HORSE),
        ("unbuild", move_bits::UNBUILD),
        ("float", move_bits::FLOAT),
        ("amph", move_bits::AMPH),
    ];
    for (name, bit) in TABLE {
        if text.eq_ignore_ascii_case(name) {
            return bit;
        }
    }
    0
}

/// Target flag names and bits (`0x6FA6F478`, 51 entries). Bit 1 is `none`, 2 ground, 4 air, 8 structure.
const TARGET_TABLE: [(&str, u32); 51] = [
    ("air", 0x4),
    ("aliv", 0x0100_0000),
    ("alive", 0x0100_0000),
    ("alli", 0x4000),
    ("allies", 0x4000),
    ("ally", 0x4000),
    ("ancient", 0x8000_0000),
    ("bridge", 0x400),
    ("dead", 0x0200_0000),
    ("deco", 0x200),
    ("decoration", 0x200),
    ("debr", 0x100),
    ("debris", 0x100),
    ("enem", 0x1_0000),
    ("enemies", 0x1_0000),
    ("enemy", 0x1_0000),
    ("grou", 0x2),
    ("ground", 0x2),
    ("frie", 0x6000),
    ("friend", 0x6000),
    ("hero", 0x40_0000),
    ("invu", 0x20_0000),
    ("invulnerable", 0x20_0000),
    ("item", 0x20),
    ("mech", 0x0800_0000),
    ("mechanical", 0x0800_0000),
    ("neut", 0x8000),
    ("neutral", 0x8000),
    ("none", 0x1),
    ("nonancient", 0x4000_0000),
    ("nonh", 0x80_0000),
    ("nonhero", 0x80_0000),
    ("nonsapper", 0x1000_0000),
    ("nots", 0x1_E000),
    ("notself", 0x1_E000),
    ("orga", 0x0400_0000),
    ("organic", 0x0400_0000),
    ("play", 0x2000),
    ("player", 0x2000),
    ("sapper", 0x2000_0000),
    ("self", 0x1000),
    ("stru", 0x8),
    ("structure", 0x8),
    ("tote", 0x10),
    ("totem", 0x10),
    ("tree", 0x40),
    ("vuln", 0x10_0000),
    ("vulnerable", 0x10_0000),
    ("wall", 0x80),
    ("ward", 0x10),
    // The 51st entry: `_` ORs this odd word (0x6FA6F60C, constant in the file and at run time). CONFIRMED by Unicorn.
    ("_", 4_258_394_126),
];

/// The target flag parser (`0x6F255250`): every token that equals a name ORs its bits; unknown tokens are ignored.
/// There is no early `_` check: the text `_` matches the 51st table entry and gives its odd word, which sets many bits
/// including ground and air (so a disabled weapon slot with `targs` `_` would read as attacking both). CONFIRMED.
pub fn parse_target_flags(text: &str) -> u32 {
    let mut bits = 0;
    for t in tokens(text) {
        for (name, bit) in TARGET_TABLE {
            if t.eq_ignore_ascii_case(name) {
                bits |= bit;
                break;
            }
        }
    }
    bits
}

/// Target flag bits that `IsUnitType` reads from a weapon.
pub mod target_bits {
    pub const GROUND: u32 = 0x2;
    pub const AIR: u32 = 0x4;
}

/// The weapon kind enum of `weapTp` (`0x6F262D30`): `_`/empty/unknown 0, `normal` 1 (the melee kind), `missile` 2,
/// `artillery` 3, `instant` 4, `msplash` 5, `mbounce` 6, `mline` 7, `aline` 8. Kinds 2 to 8 are the ranged kinds.
/// CONFIRMED by decompile. The compare is Storm ordinal 508 (`strncmp`, case-sensitive), unlike the other parsers.
pub fn parse_weapon_kind(text: &str) -> u32 {
    if text.is_empty() || text == "_" {
        return 0;
    }
    const TABLE: [(&str, u32); 8] = [
        ("normal", 1),
        ("missile", 2),
        ("instant", 4),
        ("artillery", 3),
        ("msplash", 5),
        ("mbounce", 6),
        ("mline", 7),
        ("aline", 8),
    ];
    for (name, v) in TABLE {
        if text == name {
            return v;
        }
    }
    0
}

/// The "special target set" test shared by the melee and ranged attack tests (`0x6F0C5F80`, `0x6F088FD0`,
/// `0x6F0C9710`): a weapon with these target flag words does not count. CONFIRMED.
pub fn targs_excluded(flags: u32) -> bool {
    matches!(flags, 0x1 | 0x40 | 0x80 | 0x100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_tokens() {
        assert_eq!(parse_classification("_"), 0);
        assert_eq!(parse_classification(""), 0);
        assert_eq!(parse_classification("-giant"), 0);
        assert_eq!(parse_classification("undead,peon"), 0x12);
        assert_eq!(parse_classification("Mechanical, ANCIENT"), 0x88);
        assert_eq!(parse_classification("townhall tauren bogus"), 0x840);
        assert_eq!(parse_classification("giant-summoned"), 0x5);
    }

    #[test]
    fn movement_and_kind() {
        assert_eq!(parse_movement("foot"), 1);
        assert_eq!(parse_movement("fly"), 2);
        assert_eq!(parse_movement("none"), 0);
        assert_eq!(parse_movement("walk"), 0);
        assert_eq!(parse_weapon_kind("normal"), 1);
        assert_eq!(parse_weapon_kind("aline"), 8);
        assert_eq!(parse_weapon_kind("_"), 0);
    }

    #[test]
    fn targets() {
        assert_eq!(
            parse_target_flags("ground,structure,debris,tree,wall,ward"),
            2 | 8 | 0x100 | 0x40 | 0x80 | 0x10
        );
        assert_eq!(parse_target_flags("air,ground"), 6);
        assert_eq!(parse_target_flags("_"), 4_258_394_126);
        assert!(targs_excluded(0x100) && !targs_excluded(2));
    }
}
