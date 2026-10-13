//! The creep / neutral alias table read from the user's own `AbilityData.slk` (`engine::objdata`, spec
//! `docs/spec/abilities-neutral.md`). Needs the git-ignored archives in `tools/`; without them the test does nothing.

fn code(s: &str) -> u32 {
    u32::from_be_bytes(s.as_bytes().try_into().unwrap())
}

#[test]
fn creep_rows_resolve_to_the_modelled_abilities() {
    let dir = std::env::var("W3_ARCHIVE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools"));
    let mut blobs = Vec::new();
    for name in ["war3.mpq", "War3x.mpq", "War3xLocal.mpq", "War3Patch.mpq"] {
        match std::fs::read(dir.join(name)) {
            Ok(b) => blobs.push(b),
            Err(_) => return,
        }
    }
    let refs: Vec<&[u8]> = blobs.iter().map(Vec::as_slice).collect();
    let text = engine::objdata::load(&refs).expect("object data");
    let al = &text.spell_aliases;
    for (alias, base) in [
        ("ACcl", "AOcl"),
        ("ACbl", "Ablo"),
        ("ACsw", "Aslo"),
        ("ACpv", "Awar"),
        ("ACvs", "Aven"),
        ("ACmi", "Amim"),
        ("ACtb", "AHtb"),
        ("ACtc", "AHtc"),
        ("ACt2", "AHtc"),
        ("ACfb", "AHtb"),
        ("ANfb", "AHtb"),
        ("ANcf", "AUcs"),
        ("ACbc", "AUcs"),
        ("ACbf", "AUcs"),
        ("Anhe", "Ahea"),
        ("Anh1", "Ahea"),
        ("Arsw", "AOsw"),
    ] {
        assert_eq!(al.get(&code(alias)), Some(&code(base)), "{alias} -> {base}");
        assert!(text.spell_levels.contains_key(&code(alias)), "{alias} has level records");
    }
    // Hero rows are no aliases of themselves, and item rows are left to the item ability system.
    assert!(!al.contains_key(&code("AOcl")));
    assert!(!al.contains_key(&code("AIcl")));
    assert!(text.spell_levels.contains_key(&code("Ahea")), "the modelled row is loaded too");
}
