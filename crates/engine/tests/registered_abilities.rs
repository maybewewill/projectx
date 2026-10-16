//! Every ability code the simulation registers gets its level records from the user's own `AbilityData.slk`
//! (`engine::objdata`, derived from `sim::ability_registry`). Needs the git-ignored archives in `tools/` (or
//! `W3_ARCHIVE_DIR`); without them the tests do nothing.

use mpq::FileBytes;

fn archives() -> Option<Vec<FileBytes>> {
    let dir = std::env::var("W3_ARCHIVE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools"));
    let mut out = Vec::new();
    for name in ["war3.mpq", "War3x.mpq", "War3xLocal.mpq", "War3Patch.mpq"] {
        out.push(FileBytes::open(dir.join(name)).ok()?);
    }
    Some(out)
}

fn name(c: u32) -> String {
    String::from_utf8_lossy(&c.to_be_bytes()).into_owned()
}

#[test]
fn every_registered_ability_has_level_data() {
    let Some(blobs) = archives() else { return };
    let refs: Vec<&[u8]> = blobs.iter().map(|b| &b[..]).collect();
    let text = engine::objdata::load(&refs).expect("object data");
    let codes = sim::ability_registry::registered_codes();
    assert!(codes.len() > 200, "the registries list many codes, got {}", codes.len());
    let mut empty = Vec::new();
    for &c in &codes {
        let Some(levels) = text.spell_levels.get(&c) else { continue };
        let any = levels.iter().any(|l| {
            l.cast.to_bits() != 0
                || l.dur.to_bits() != 0
                || l.hero_dur.to_bits() != 0
                || l.cost != 0
                || l.cool.to_bits() != 0
                || l.area.to_bits() != 0
                || l.rng.to_bits() != 0
                || l.unit_id != 0
                || l.data.iter().any(|d| d.to_bits() != 0)
        });
        if !any {
            empty.push(name(c));
        }
    }
    let missing: Vec<String> = codes.iter().filter(|c| !text.spell_levels.contains_key(c)).map(|&c| name(c)).collect();
    let no_row: Vec<String> = text.missing_ability_rows.iter().map(|&c| name(c)).collect();
    eprintln!("registered {} codes; no level record: {missing:?}; no AbilityData row: {no_row:?}; all-zero: {empty:?}", codes.len());
    // Structural hooks that 1.26a has no `AbilityData.slk` row for at all (neither as a row nor as a `code` target).
    let rowless = ["Aetl", "Amou", "Apit", "Arem", "Atpi"];
    let unexpected: Vec<&String> = missing.iter().filter(|m| !rowless.contains(&m.as_str())).collect();
    assert!(unexpected.is_empty(), "registered codes without level data: {unexpected:?}");
    // The cast spells (an order id is registered) must carry real numbers; passives may have all-default levels.
    let spells: std::collections::BTreeSet<u32> = sim::cast::SpellSystem::new().order_bases().collect();
    let empty_spells: Vec<String> = empty.iter().filter(|n| spells.contains(&u32::from_be_bytes(n.as_bytes().try_into().unwrap()))).cloned().collect();
    assert!(empty_spells.is_empty(), "cast spells whose level data is all default: {empty_spells:?}");
    for known in ["AHtb", "AOws", "Abur", "Aabr", "Anhe", "ACrn"] {
        let c = u32::from_be_bytes(known.as_bytes().try_into().unwrap());
        assert!(codes.contains(&c) && text.spell_levels.contains_key(&c), "{known}");
    }
}

#[test]
fn morph_forms_come_from_the_ability_unit_id() {
    let Some(blobs) = archives() else { return };
    let refs: Vec<&[u8]> = blobs.iter().map(|b| &b[..]).collect();
    let text = engine::objdata::load(&refs).expect("object data");
    let abur = u32::from_be_bytes(*b"Abur");
    let form = text.spell_levels[&abur][0].unit_id;
    assert_ne!(form, 0, "Burrow names the burrowed form");
    let orig = u32::from_be_bytes(*b"ucry");
    let alts = engine::objdata::morph_alternates(&text, &[(orig, vec![abur])]);
    assert_eq!(alts, vec![(form, orig)]);
}
