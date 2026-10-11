use gamedata::{Error, Profile};

const SAMPLE: &str = "// hand-written sample\n\
[hfoo]\n\
Name=Footman          // trailing comment\n\
Abilities=Aatk,Aho,\"Ab, cd\"\n\
Tip=\"Train, then // not a comment\"\n\
Empty=\n\
[hbar]\n\
Name = Bar\n\
Values = 1 , 2 ,3\n";

#[test]
fn reads_sections_and_raw_values() {
    let p = Profile::parse(SAMPLE).unwrap();
    assert_eq!(p.get("hfoo", "Name"), Some("Footman"));
    assert_eq!(p.get("hbar", "Name"), Some("Bar"));
    assert_eq!(
        p.get("hfoo", "Tip"),
        Some("\"Train, then // not a comment\"")
    );
    assert_eq!(p.get("hfoo", "Empty"), Some(""));
    assert_eq!(p.get("hfoo", "Missing"), None);
    assert_eq!(p.get("nosection", "Name"), None);
    assert_eq!(p.sections().collect::<Vec<_>>(), ["hbar", "hfoo"]);
}

#[test]
fn lists_respect_quotes_and_trim_items() {
    let p = Profile::parse(SAMPLE).unwrap();
    assert_eq!(p.get_list("hfoo", "Abilities"), ["Aatk", "Aho", "Ab, cd"]);
    assert_eq!(p.get_list("hbar", "Values"), ["1", "2", "3"]);
    assert_eq!(p.get_list("hfoo", "Tip"), ["Train, then // not a comment"]);
    assert!(p.get_list("hfoo", "Empty").is_empty());
    assert!(p.get_list("hfoo", "Missing").is_empty());
}

#[test]
fn keys_before_any_section_use_empty_section_name() {
    let p = Profile::parse("Global=1\n[s]\nk=2\n").unwrap();
    assert_eq!(p.get("", "Global"), Some("1"));
    assert_eq!(p.get("s", "k"), Some("2"));
}

#[test]
fn later_duplicate_key_in_same_file_wins() {
    let p = Profile::parse("[s]\nk=1\nk=2\n").unwrap();
    assert_eq!(p.get("s", "k"), Some("2"));
}

#[test]
fn crlf_and_bom_are_accepted() {
    let crlf = format!("\u{feff}{}", SAMPLE.replace('\n', "\r\n"));
    assert_eq!(
        Profile::parse(&crlf).unwrap(),
        Profile::parse(SAMPLE).unwrap()
    );
}

#[test]
fn rejects_malformed_lines_with_line_number() {
    let err = Profile::parse("[s]\njust text\n").unwrap_err();
    assert!(matches!(err, Error::Profile { line: 2, .. }));
    let err = Profile::parse("[unclosed\n").unwrap_err();
    assert!(matches!(err, Error::Profile { line: 1, .. }));
    let err = Profile::parse("=value\n").unwrap_err();
    assert!(matches!(err, Error::Profile { line: 1, .. }));
}

#[test]
fn merge_overrides_keys_and_adds_sections() {
    let mut base = Profile::parse("[hfoo]\nName=Footman\nArmor=2\n").unwrap();
    let overlay = Profile::parse("[hfoo]\nArmor=3\n[hnew]\nName=New\n").unwrap();
    base.merge(&overlay);
    assert_eq!(base.get("hfoo", "Name"), Some("Footman"));
    assert_eq!(base.get("hfoo", "Armor"), Some("3"));
    assert_eq!(base.get("hnew", "Name"), Some("New"));
}

#[test]
fn parse_bytes_decodes_windows_1252() {
    let mut bytes = b"[hfoo]\nName=\"Caf".to_vec();
    bytes.push(0xE9);
    bytes.extend_from_slice(b"\"\n");
    let p = Profile::parse_bytes(&bytes).unwrap();
    assert_eq!(p.get_list("hfoo", "Name"), ["Café"]);
}

#[test]
fn lenient_parse_skips_stray_lines_and_keeps_the_rest() {
    let p = Profile::parse_lenient("[a]
Name=Footman
stray text
/ Stuffed Penguin
[broken
Tipx=1
=oops
art=x.blp
");
    assert_eq!(p.get("a", "Name"), Some("Footman"));
    assert_eq!(p.get("a", "art"), Some("x.blp"));
    assert_eq!(p.get_ci("a", "ART"), Some("x.blp"));
    assert_eq!(p.get_list_ci("a", "NAME"), vec!["Footman".to_string()]);
}
