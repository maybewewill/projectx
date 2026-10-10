use gamedata::decode_text;

#[test]
fn valid_utf8_is_kept_and_bom_stripped() {
    let bytes = "\u{feff}Café ✓".as_bytes();
    assert_eq!(decode_text(bytes), "Café ✓");
}

#[test]
fn invalid_utf8_falls_back_to_windows_1252() {
    // 0x80 = Euro sign, 0x93/0x94 = curly quotes, 0xE9 = e-acute.
    let bytes = [0x80, b' ', 0x93, b'x', 0x94, b' ', b'c', b'a', b'f', 0xE9];
    assert_eq!(decode_text(&bytes), "\u{20AC} \u{201C}x\u{201D} caf\u{e9}");
}
