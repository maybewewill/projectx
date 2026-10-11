//! Parser tests: syntax, raw properties, comments, escapes, errors.

use fdf::ast::{BlockKind, Child, TopItem, Value};
use fdf::{parse, parse_toc};

fn frame_of(src: &str) -> fdf::Frame {
    let doc = parse(src).expect("parse");
    match &doc.items[0] {
        TopItem::Frame(f) => f.clone(),
        other => panic!("expected a frame, got {other:?}"),
    }
}

fn prop<'a>(f: &'a fdf::Frame, key: &str) -> &'a fdf::Property {
    f.props
        .iter()
        .find(|p| p.key == key)
        .unwrap_or_else(|| panic!("no property {key}"))
}

fn num(v: &Value) -> f32 {
    match v {
        Value::Num(n) => *n,
        other => panic!("not a number: {other:?}"),
    }
}

#[test]
fn comments_line_and_block_are_skipped() {
    let src = r#"
// leading line comment
/* block
   comment */ Frame "BACKDROP" "A" { // trailing
    Width 0.1, /* inline */ Height 0.2,
    // Width 9,
}
"#;
    let f = frame_of(src);
    assert_eq!(f.name, "A");
    assert_eq!(f.props.len(), 2);
    assert_eq!(num(&prop(&f, "Width").values[0]), 0.1);
}

#[test]
fn unterminated_block_comment_is_an_error() {
    let err = parse("/* never ends").unwrap_err();
    assert_eq!((err.line, err.col), (1, 1));
    assert!(err.message.contains("comment"));
}

#[test]
fn string_escapes_and_paths() {
    let src =
        r#"Frame "TEXT" "T" { Text "say \"hi\" C:\\x", BackdropBackground "UI\FrameDef\a.blp", }"#;
    let f = frame_of(src);
    assert_eq!(
        prop(&f, "Text").values[0],
        Value::Str(r#"say "hi" C:\x"#.into())
    );
    // Single backslashes are kept as written.
    assert_eq!(
        prop(&f, "BackdropBackground").values[0],
        Value::Str(r"UI\FrameDef\a.blp".into())
    );
}

#[test]
fn numbers_signs_and_exponents() {
    let f = frame_of("Frame \"BACKDROP\" \"N\" { A -0.01, B .5, C +2, D 1e-3, E 2.5E+1, F 0.8, }");
    let v = |k: &str| num(&prop(&f, k).values[0]);
    assert_eq!(v("A"), -0.01);
    assert_eq!(v("B"), 0.5);
    assert_eq!(v("C"), 2.0);
    assert_eq!(v("D"), 0.001);
    assert_eq!(v("E"), 25.0);
    assert_eq!(v("F"), 0.8);
}

#[test]
fn number_with_unit_suffix_is_rejected() {
    let err = parse("Frame \"BACKDROP\" \"U\" {\n  Width 0.2px,\n}").unwrap_err();
    assert_eq!((err.line, err.col), (2, 9));
    assert!(err.message.contains("unit suffix"), "{err}");
}

#[test]
fn unterminated_string_reports_its_start() {
    let err = parse("Frame \"BACKDROP\" \"A\" {\n  Text \"oops\n}").unwrap_err();
    assert_eq!((err.line, err.col), (2, 8));
    assert!(err.message.contains("unterminated string"));
}

#[test]
fn unexpected_character_reports_position() {
    let err = parse("Frame \"BACKDROP\" \"A\" {\n    @\n}").unwrap_err();
    assert_eq!((err.line, err.col), (2, 5));
}

#[test]
fn missing_closing_brace_is_an_error_at_eof() {
    let err = parse("Frame \"BACKDROP\" \"A\" {\n Width 0.1,\n").unwrap_err();
    assert!(err.message.contains("end of file"), "{err}");
}

#[test]
fn top_level_junk_is_rejected_with_position() {
    let err = parse("\n  Width 0.1,").unwrap_err();
    assert_eq!((err.line, err.col), (2, 3));
}

#[test]
fn unknown_keys_are_kept_raw() {
    let f = frame_of(r#"Frame "BACKDROP" "R" { FooBar 1, "x", BAR, TOPLEFT, 2, }"#);
    let p = prop(&f, "FooBar");
    assert_eq!(
        p.values,
        vec![
            Value::Num(1.0),
            Value::Str("x".into()),
            Value::Ident("BAR".into()),
            Value::Ident("TOPLEFT".into()),
            Value::Num(2.0)
        ]
    );
}

#[test]
fn inheritance_header_parses() {
    let f = frame_of(r#"Frame "BUTTON" "B" INHERITS WITHCHILDREN "Template" { }"#);
    let inh = f.inherits.expect("inherits");
    assert_eq!(inh.parent, "Template");
    assert!(inh.with_children);
    let g = frame_of(r#"Frame "TEXT" "T" INHERITS "Other" { }"#);
    assert!(!g.inherits.unwrap().with_children);
}

#[test]
fn nested_frames_layers_textures_and_strings() {
    let src = r#"
Frame "SIMPLEFRAME" "Outer" {
    Layer "BACKGROUND" {
        Texture "Bg" {
            File "UI\a.blp",
            TexCoord 0, 0.5, 0, 1,
        }
    }
    Layer "ARTWORK" {
        String "Label" {
            Font "Font", 0.01,
            Text "hello",
        }
        Frame "TEXT" "Inner" {
            Text "x",
        }
    }
}
"#;
    let f = frame_of(src);
    assert_eq!(f.children.len(), 3);
    match &f.children[0] {
        Child::Block(b) => {
            assert_eq!(b.kind, BlockKind::Texture);
            assert_eq!(b.name.as_deref(), Some("Bg"));
            assert_eq!(b.layer.as_deref(), Some("BACKGROUND"));
            assert_eq!(b.props.len(), 2);
        }
        other => panic!("{other:?}"),
    }
    match &f.children[1] {
        Child::Block(b) => {
            assert_eq!(b.kind, BlockKind::String);
            assert_eq!(b.layer.as_deref(), Some("ARTWORK"));
        }
        other => panic!("{other:?}"),
    }
    match &f.children[2] {
        Child::Frame(c) => {
            assert_eq!(c.name, "Inner");
            assert_eq!(c.layer.as_deref(), Some("ARTWORK"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn properties_separated_by_lines_without_commas() {
    let f = frame_of("Frame \"BACKDROP\" \"L\" {\n  Width 0.2\n  Height 0.1\n  SetAllPoints\n}");
    assert_eq!(num(&prop(&f, "Width").values[0]), 0.2);
    assert_eq!(num(&prop(&f, "Height").values[0]), 0.1);
    assert!(prop(&f, "SetAllPoints").values.is_empty());
}

#[test]
fn multi_line_setpoint_with_commas() {
    let src = "Frame \"TEXT\" \"S\" {\n  SetPoint TOPLEFT,\n    \"Parent\", BOTTOMRIGHT,\n    0.01, -0.02,\n}";
    let f = frame_of(src);
    let p = prop(&f, "SetPoint");
    assert_eq!(p.values.len(), 5);
    assert_eq!(p.values[1], Value::Str("Parent".into()));
    assert_eq!(num(&p.values[3]), 0.01);
    assert_eq!(num(&p.values[4]), -0.02);
}

#[test]
fn space_separated_values_and_bare_words() {
    let f = frame_of("Frame \"TEXT\" \"C\" {\n  FontColor 1.0 0.8 0.0 1.0,\n  BackdropBackgroundInsets right top bottom left,\n}");
    assert_eq!(prop(&f, "FontColor").values.len(), 4);
    assert_eq!(prop(&f, "BackdropBackgroundInsets").values.len(), 4);
}

#[test]
fn string_list_and_include_at_top_level() {
    let src = r#"
IncludeFile "UI\FrameDef\UI\EscMenuTemplates.fdf",
StringList {
    MYLABEL "My Label Text",
    MYVALUE "Value",
    NUMBERED 42,
}
"#;
    let doc = parse(src).unwrap();
    match &doc.items[0] {
        TopItem::Include { path, .. } => assert_eq!(path, r"UI\FrameDef\UI\EscMenuTemplates.fdf"),
        other => panic!("{other:?}"),
    }
    match &doc.items[1] {
        TopItem::StringList(list) => {
            assert_eq!(list.entries.len(), 3);
            assert_eq!(list.entries[0].key, "MYLABEL");
            assert_eq!(list.entries[0].value, "My Label Text");
            assert_eq!(list.entries[2].value, "42");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn toc_lists_paths_with_comments_and_quotes() {
    let toc = "// comment\nUI\\FrameDef\\A.fdf,\n\"UI\\FrameDef\\B.fdf\"\n\n  UI\\FrameDef\\C.fdf // tail\n";
    assert_eq!(
        parse_toc(toc),
        vec![
            r"UI\FrameDef\A.fdf",
            r"UI\FrameDef\B.fdf",
            r"UI\FrameDef\C.fdf"
        ]
    );
}

#[test]
fn frame_types_are_upper_cased_and_names_kept() {
    let f = frame_of(r#"Frame "simpleframe" "MixedCase" { }"#);
    assert_eq!(f.kind, "SIMPLEFRAME");
    assert_eq!(f.name, "MixedCase");
}

#[test]
fn a_trailing_float_suffix_is_accepted() {
    // The shipped Glue\CampaignMenu.fdf writes `Height 0.090f,`.
    let doc = parse("Frame \"BACKDROP\" \"U\" {
  Height 0.090f,
  Width 2F,
}").unwrap();
    assert_eq!(doc.items.len(), 1);
    let err = parse("Frame \"BACKDROP\" \"U\" {
  Width 0.2fx,
}").unwrap_err();
    assert!(err.message.contains("unit suffix"), "{err}");
}
