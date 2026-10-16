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

#[test]
fn s50_rows_resolve_and_their_code_lists_are_read() {
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
    for (alias, base) in [("ANs1", "ANsy"), ("ANs3", "ANsy"), ("ANc2", "ANcs"), ("ANg1", "ANrg")] {
        assert_eq!(text.spell_aliases.get(&code(alias)), Some(&code(base)), "{alias} -> {base}");
    }
    let l = &text.neutral_lists;
    assert_eq!(l.get(&(code("ANef"), 0, 0)), Some(&vec![code("npn1"), code("npn2"), code("npn3")]));
    assert_eq!(l.get(&(code("ANsy"), 0, 1)), Some(&vec![code("ncgb")]));
    assert_eq!(l.get(&(code("ANs1"), 0, 1)), Some(&vec![code("ncg1")]));
    assert_eq!(l.get(&(code("ANeg"), 0, 2)), Some(&vec![code("ANsy"), code("ANs1")]));
    assert_eq!(l.get(&(code("ANeg"), 1, 4)), Some(&vec![code("ANg1"), code("ANg2")]));
}
