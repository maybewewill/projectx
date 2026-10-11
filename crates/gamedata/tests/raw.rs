//! The raw text of SLK values (`Slk::raw`, `Row::raw`) keeps the characters the file has, for S2R parsing.

use gamedata::Slk;

#[test]
fn raw_keeps_the_token_text_of_numbers_and_quoted_text() {
    let slk = Slk::parse(
        "ID;PWXL;N;E\n\
         C;X1;Y1;K\"unitID\"\n\
         C;X2;Y1;K\"def\"\n\
         C;X3;Y1;K\"defType\"\n\
         C;X1;Y2;K\"hfoo\"\n\
         C;X2;Y2;K1.50\n\
         C;X3;Y2;K\"medium\"\n\
         E\n",
    )
    .expect("fixture parses");
    assert_eq!(
        slk.raw("hfoo", "def"),
        Some("1.50"),
        "the value is 1.5, the text keeps the zero"
    );
    assert_eq!(slk.get("hfoo", "def").and_then(|v| v.as_real()), Some(1.5));
    assert_eq!(slk.raw("hfoo", "defType"), Some("\"medium\""));
    assert_eq!(slk.row("hfoo").and_then(|r| r.raw("def")), Some("1.50"));
    assert_eq!(slk.raw("hfoo", "missing"), None);
}
