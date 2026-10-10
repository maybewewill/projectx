use gamedata::{Error, Slk, Value};

const SAMPLE: &str = "ID;PWXL;N;E\n\
B;X4;Y3;D0\n\
C;X1;Y1;K\"unitID\"\n\
C;X2;K\"name\"\n\
C;X3;K\"hp\"\n\
C;X4;K\"ok\"\n\
C;Y2;X1;K\"hfoo\"\n\
C;X2;K\"Footman\"\n\
C;X3;K550\n\
C;X4;KTRUE\n\
C;Y3;X1;K\"hbar\"\n\
C;X2;K\"Bar; \"\"best\"\"\"\n\
C;X3;K1.5\n\
F;P0;DG0\n\
E\n\
C;Y9;X1;K\"after-end\"\n";

#[test]
fn parses_header_keys_and_values() {
    let slk = Slk::parse(SAMPLE).unwrap();
    assert_eq!(slk.columns(), ["unitID", "name", "hp", "ok"]);
    assert_eq!(slk.len(), 2);
    assert_eq!(
        slk.get("hfoo", "name"),
        Some(&Value::Text("Footman".into()))
    );
    assert_eq!(slk.get("hfoo", "hp"), Some(&Value::Int(550)));
    assert_eq!(slk.get("hfoo", "ok"), Some(&Value::Text("TRUE".into())));
    assert_eq!(slk.get("hbar", "hp"), Some(&Value::Real(1.5)));
}

#[test]
fn quoted_values_keep_semicolons_and_unescape_quotes() {
    let slk = Slk::parse(SAMPLE).unwrap();
    assert_eq!(
        slk.get("hbar", "name").and_then(Value::as_str),
        Some("Bar; \"best\"")
    );
}

#[test]
fn empty_cells_and_unknown_names_return_none() {
    let slk = Slk::parse(SAMPLE).unwrap();
    assert_eq!(slk.get("hbar", "ok"), None);
    assert_eq!(slk.get("hfoo", "missing"), None);
    assert_eq!(slk.get("nope", "name"), None);
}

#[test]
fn stops_at_end_record() {
    let slk = Slk::parse(SAMPLE).unwrap();
    assert!(slk.row("after-end").is_none());
}

#[test]
fn iterates_rows_in_file_order() {
    let slk = Slk::parse(SAMPLE).unwrap();
    let keys: Vec<&str> = slk.rows().map(|r| r.key()).collect();
    assert_eq!(keys, ["hfoo", "hbar"]);

    let foo = slk.row("hfoo").unwrap();
    let cells: Vec<(&str, Option<&Value>)> = foo.iter().collect();
    assert_eq!(cells[0], ("unitID", Some(&Value::Text("hfoo".into()))));
    assert_eq!(cells[3].0, "ok");
    assert_eq!(foo.get("hp"), Some(&Value::Int(550)));

    let bar = slk.row("hbar").unwrap();
    assert_eq!(bar.get("ok"), None);
}

#[test]
fn crlf_line_endings_parse_the_same() {
    let crlf = SAMPLE.replace('\n', "\r\n");
    assert_eq!(Slk::parse(&crlf).unwrap(), Slk::parse(SAMPLE).unwrap());
}

#[test]
fn omitted_coordinates_inherit_previous_record() {
    // "C;K" with no X advances the column; "C;K" with no Y keeps the row.
    let src = "C;X1;Y1;K\"key\"\nC;X2;K\"a\"\nC;K\"b\"\nC;Y2;X1;K\"row\"\nC;K7\n";
    let slk = Slk::parse(src).unwrap();
    assert_eq!(slk.columns(), ["key", "a", "b"]);
    // Row 2: "C;K7" lands in column 2 (previous X + 1) of the previous row (2).
    assert_eq!(slk.get("row", "a"), Some(&Value::Int(7)));
    assert_eq!(slk.get("row", "b"), None);
}

#[test]
fn rejects_bad_coordinates() {
    let err = Slk::parse("C;X0;Y1;K1\n").unwrap_err();
    assert!(matches!(err, Error::Slk { line: 1, .. }));
    let err = Slk::parse("ID;P\nC;Xabc;Y1;K1\n").unwrap_err();
    assert!(matches!(err, Error::Slk { line: 2, .. }));
}

#[test]
fn rejects_duplicate_row_keys() {
    let src = "C;X1;Y1;K\"id\"\nC;X1;Y2;K\"same\"\nC;X1;Y3;K\"same\"\n";
    assert_eq!(
        Slk::parse(src).unwrap_err(),
        Error::DuplicateRowKey { key: "same".into() }
    );
}

#[test]
fn bytes_are_decoded_as_windows_1252() {
    // Invalid UTF-8 (0xE9 = 'é' in Windows-1252) forces the 1252 path.
    let mut bytes = b"C;X1;Y1;K\"id\"\nC;X2;K\"label\"\nC;X1;Y2;K\"cafe\"\nC;X2;K\"caf".to_vec();
    bytes.push(0xE9);
    bytes.extend_from_slice(b"\"\n");
    let slk = Slk::parse_bytes(&bytes).unwrap();
    assert_eq!(slk.columns(), ["id", "label"]);
    assert_eq!(
        slk.get("cafe", "label").and_then(Value::as_str),
        Some("caf\u{e9}")
    );
}

#[test]
fn merge_overrides_cells_and_adds_rows_and_columns() {
    let mut base = Slk::parse(SAMPLE).unwrap();
    let overlay_src = "C;X1;Y1;K\"unitID\"\nC;X2;K\"name\"\nC;X3;K\"hp\"\n\
C;X4;K\"ok\"\nC;X5;K\"armor\"\n\
C;Y2;X1;K\"hfoo\"\nC;X3;K600\nC;X5;K2\n\
C;Y3;X1;K\"hnew\"\nC;X2;K\"New one\"\n";
    let overlay = Slk::parse(overlay_src).unwrap();
    base.merge(&overlay);

    assert_eq!(base.get("hfoo", "hp"), Some(&Value::Int(600)));
    // Empty overlay cells do not erase base values.
    assert_eq!(
        base.get("hfoo", "name").and_then(Value::as_str),
        Some("Footman")
    );
    assert_eq!(base.get("hfoo", "armor"), Some(&Value::Int(2)));
    assert_eq!(base.get("hbar", "armor"), None);
    assert_eq!(
        base.get("hnew", "name").and_then(Value::as_str),
        Some("New one")
    );
    assert_eq!(base.len(), 3);
    assert_eq!(base.columns().last().map(String::as_str), Some("armor"));
}
