//! Library tests: includes, StringLists, inheritance and typed property reads.

use std::collections::BTreeMap;

use fdf::ast::{Child, Value};
use fdf::props::{self, AlphaMode, Anchor, Point, Relative};
use fdf::{Library, LibraryError};

fn files(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.to_string()))
        .collect()
}

fn loader(map: &BTreeMap<String, String>) -> impl FnMut(&str) -> Option<String> + '_ {
    move |p: &str| map.get(&fdf::library::normalize_path(p)).cloned()
}

fn lib_from(entries: &[(&str, &str)], root: &str) -> Result<Library, LibraryError> {
    let map = files(entries);
    let mut lib = Library::new();
    lib.load(root, &mut loader(&map))?;
    Ok(lib)
}

#[test]
fn includes_load_once_and_in_order() {
    let lib = lib_from(
        &[
            (
                "main.fdf",
                "IncludeFile \"a.fdf\",\nIncludeFile \"B.fdf\",\nFrame \"FRAME\" \"Main\" { }",
            ),
            (
                "a.fdf",
                "IncludeFile \"b.fdf\",\nFrame \"FRAME\" \"FromA\" { }",
            ),
            ("b.fdf", "Frame \"FRAME\" \"FromB\" { }"),
        ],
        "main.fdf",
    )
    .unwrap();
    let names: Vec<&str> = lib.frame_names().collect();
    assert_eq!(names, vec!["froma", "fromb", "main"]);
    assert!(lib.warnings.is_empty());
}

#[test]
fn include_paths_are_matched_case_and_slash_insensitively() {
    let lib = lib_from(
        &[
            ("root.fdf", "IncludeFile \"UI/FrameDef/X.fdf\","),
            ("ui\\framedef\\x.fdf", "Frame \"FRAME\" \"X\" { }"),
        ],
        "root.fdf",
    )
    .unwrap();
    assert!(lib.frame("x").is_some());
}

#[test]
fn missing_include_is_an_error_naming_the_file() {
    let err = lib_from(&[("main.fdf", "IncludeFile \"gone.fdf\",")], "main.fdf").unwrap_err();
    match err {
        LibraryError::MissingInclude { path, from } => {
            assert_eq!(path, "gone.fdf");
            assert_eq!(from, "main.fdf");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn circular_includes_terminate() {
    let lib = lib_from(
        &[
            ("a.fdf", "IncludeFile \"b.fdf\", Frame \"FRAME\" \"A\" { }"),
            ("b.fdf", "IncludeFile \"a.fdf\", Frame \"FRAME\" \"B\" { }"),
        ],
        "a.fdf",
    )
    .unwrap();
    assert!(lib.frame("a").is_some() && lib.frame("b").is_some());
}

#[test]
fn parse_error_in_an_include_names_the_file() {
    let err = lib_from(
        &[
            ("main.fdf", "IncludeFile \"bad.fdf\",\n"),
            ("bad.fdf", "junk"),
        ],
        "main.fdf",
    )
    .unwrap_err();
    match err {
        LibraryError::Parse { path, error } => {
            assert_eq!(path, "bad.fdf");
            assert_eq!((error.line, error.col), (1, 1));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn string_lists_decorate_only_when_flagged() {
    let mut lib = Library::new();
    let mut none = |_: &str| None;
    lib.load_source(
        "s",
        r#"StringList { MYLABEL "My Label", Icon "UI\Icons\a.blp", }
           Frame "TEXT" "Plain" { Text "MYLABEL", }
           Frame "TEXT" "Deco" { DecorateFileNames, Text "mylabel", }"#,
        &mut none,
    )
    .unwrap();
    assert_eq!(lib.string("MyLabel"), Some("My Label"));
    assert_eq!(lib.decorate(false, "MYLABEL"), "MYLABEL");
    assert_eq!(lib.decorate(true, "mylabel"), "My Label");
    assert_eq!(lib.decorate(true, "unknown"), "unknown");
    assert_eq!(lib.decorate(true, "Icon"), r"UI\Icons\a.blp");
}

#[test]
fn duplicate_string_and_frame_warn_and_last_wins() {
    let mut lib = Library::new();
    let mut none = |_: &str| None;
    lib.load_source(
        "d",
        r#"StringList { K "one", } StringList { K "two", }
           Frame "FRAME" "F" { Width 0.1, } Frame "FRAME" "F" { Width 0.2, }"#,
        &mut none,
    )
    .unwrap();
    assert_eq!(lib.string("k"), Some("two"));
    assert_eq!(lib.warnings.len(), 2);
    let f = lib.frame("f").unwrap();
    assert_eq!(props::number(&f.props, "Width").unwrap(), Some(0.2));
}

fn resolved(src: &str, name: &str) -> fdf::Frame {
    let mut lib = Library::new();
    let mut none = |_: &str| None;
    lib.load_source("t", src, &mut none).unwrap();
    lib.resolve(name).unwrap()
}

#[test]
fn inheritance_copies_properties_and_child_overrides_same_key() {
    let f = resolved(
        r#"
Frame "BACKDROP" "Base" {
    Width 0.2,
    Height 0.1,
    BackdropBackground "base.blp",
    SetPoint TOPLEFT, "Parent", TOPLEFT, 0.01, -0.01,
}
Frame "BACKDROP" "Child" INHERITS "Base" {
    Width 0.5,
    SetPoint TOPLEFT, "Parent", TOPLEFT, 0.1, -0.1,
    SetPoint BOTTOMRIGHT, "Parent", BOTTOMRIGHT, 0, 0,
}
"#,
        "Child",
    );
    assert!(f.inherits.is_none());
    assert_eq!(props::number(&f.props, "Width").unwrap(), Some(0.5));
    assert_eq!(props::number(&f.props, "Height").unwrap(), Some(0.1));
    assert_eq!(
        props::text(&f.props, "BackdropBackground").as_deref(),
        Some("base.blp")
    );
    // Same own point is replaced, the other point is added.
    let anchors = props::anchors(&f.props).unwrap();
    assert_eq!(anchors.len(), 2);
    let tl = anchors.iter().find(|a| a.own == Point::TopLeft).unwrap();
    assert_eq!((tl.x, tl.y), (0.1, -0.1));
}

#[test]
fn inherited_setallpoints_is_kept_until_child_sets_a_corner() {
    let f = resolved(
        r#"
Frame "BACKDROP" "Base" { SetAllPoints, }
Frame "BACKDROP" "Child" INHERITS "Base" { SetPoint TOPLEFT, "Parent", TOPLEFT, 0.05, 0, }
"#,
        "Child",
    );
    let anchors = props::anchors(&f.props).unwrap();
    let tl = anchors.iter().find(|a| a.own == Point::TopLeft).unwrap();
    let br = anchors
        .iter()
        .find(|a| a.own == Point::BottomRight)
        .unwrap();
    assert_eq!(tl.x, 0.05);
    assert_eq!(br.x, 0.0);
    assert_eq!(br.relative, Relative::Parent);
}

#[test]
fn withchildren_copies_children_and_plain_inherit_does_not() {
    let src = r#"
Frame "FRAME" "Tpl" {
    Frame "TEXT" "TplChild" { Text "a", }
    Frame "TEXT" "TplChild2" { Text "b", }
}
Frame "FRAME" "With" INHERITS WITHCHILDREN "Tpl" { }
Frame "FRAME" "Without" INHERITS "Tpl" { }
Frame "FRAME" "Own" INHERITS WITHCHILDREN "Tpl" {
    Frame "TEXT" "TplChild" { Text "replaced", }
    Frame "TEXT" "Extra" { }
}
"#;
    let with = resolved(src, "With");
    assert_eq!(with.children.len(), 2);
    let without = resolved(src, "Without");
    assert!(without.children.is_empty());
    let own = resolved(src, "Own");
    let names: Vec<&str> = own.children.iter().filter_map(Child::name).collect();
    assert_eq!(names, vec!["TplChild", "TplChild2", "Extra"]);
    match &own.children[0] {
        Child::Frame(f) => assert_eq!(props::text(&f.props, "Text").as_deref(), Some("replaced")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn inheritance_chains_through_several_templates() {
    let f = resolved(
        r#"
Frame "BACKDROP" "A" { Width 0.1, Height 0.2, }
Frame "BACKDROP" "B" INHERITS "A" { Height 0.3, }
Frame "BACKDROP" "C" INHERITS "B" { }
"#,
        "C",
    );
    assert_eq!(props::number(&f.props, "Width").unwrap(), Some(0.1));
    assert_eq!(props::number(&f.props, "Height").unwrap(), Some(0.3));
}

#[test]
fn nested_child_inheritance_is_resolved() {
    let f = resolved(
        r#"
Frame "TEXT" "Tpl" { Width 0.4, }
Frame "FRAME" "Root" {
    Frame "TEXT" "Kid" INHERITS "Tpl" { }
}
"#,
        "Root",
    );
    match &f.children[0] {
        Child::Frame(k) => {
            assert!(k.inherits.is_none());
            assert_eq!(props::number(&k.props, "Width").unwrap(), Some(0.4));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn inheritance_cycle_and_unknown_parent_are_errors() {
    let mut lib = Library::new();
    let mut none = |_: &str| None;
    lib.load_source(
        "c",
        "Frame \"F\" \"A\" INHERITS \"B\" { } Frame \"F\" \"B\" INHERITS \"A\" { }",
        &mut none,
    )
    .unwrap();
    match lib.resolve("A").unwrap_err() {
        LibraryError::InheritCycle(path) => assert!(path.len() >= 3),
        other => panic!("{other:?}"),
    }
    let mut lib2 = Library::new();
    lib2.load_source("u", "Frame \"F\" \"X\" INHERITS \"Nope\" { }", &mut none)
        .unwrap();
    assert!(matches!(
        lib2.resolve("X"),
        Err(LibraryError::UnknownParent { .. })
    ));
    assert!(matches!(
        lib2.resolve("Missing"),
        Err(LibraryError::UnknownFrame(_))
    ));
}

#[test]
fn self_inheritance_is_a_cycle() {
    let mut lib = Library::new();
    let mut none = |_: &str| None;
    lib.load_source("s", "Frame \"F\" \"Self\" INHERITS \"Self\" { }", &mut none)
        .unwrap();
    assert!(matches!(
        lib.resolve("Self"),
        Err(LibraryError::InheritCycle(_))
    ));
}

#[test]
fn child_inheriting_its_ancestor_is_a_cycle() {
    let mut lib = Library::new();
    let mut none = |_: &str| None;
    lib.load_source(
        "n",
        "Frame \"FRAME\" \"Tpl\" { Frame \"FRAME\" \"Kid\" INHERITS \"Tpl\" { } }",
        &mut none,
    )
    .unwrap();
    assert!(matches!(
        lib.resolve("Tpl"),
        Err(LibraryError::InheritCycle(_))
    ));
}

#[test]
fn anchors_read_all_forms() {
    let f = resolved(
        r#"Frame "FRAME" "P" {
            SetPoint TOPLEFT, "Other", BOTTOMRIGHT, 0.1, -0.2,
            SetPoint TOP, BOTTOM, 0.3, 0.4,
            Anchor BOTTOMLEFT, 0.5, 0.6,
            SetPoint RIGHT, "Named",
        }"#,
        "P",
    );
    let a = props::anchors(&f.props).unwrap();
    assert_eq!(a.len(), 4);
    assert_eq!(
        a[0],
        Anchor {
            own: Point::TopLeft,
            relative: Relative::Named("Other".into()),
            rel_point: Point::BottomRight,
            x: 0.1,
            y: -0.2
        }
    );
    // Parent relative with an explicit relative point.
    assert_eq!(a[1].relative, Relative::Parent);
    assert_eq!(a[1].rel_point, Point::Bottom);
    assert_eq!((a[1].x, a[1].y), (0.3, 0.4));
    // Anchor shorthand: parent, same point.
    assert_eq!(
        a[2],
        Anchor {
            own: Point::BottomLeft,
            relative: Relative::Parent,
            rel_point: Point::BottomLeft,
            x: 0.5,
            y: 0.6
        }
    );
    // Named relative without offsets.
    assert_eq!(a[3].rel_point, Point::Right);
    assert_eq!((a[3].x, a[3].y), (0.0, 0.0));
}

#[test]
fn bad_anchor_points_are_property_errors() {
    let f = resolved(
        "Frame \"FRAME\" \"Q\" { SetPoint NOWHERE, \"X\", TOPLEFT, 0, 0, }",
        "Q",
    );
    let err = props::anchors(&f.props).unwrap_err();
    assert_eq!(err.key, "SetPoint");
    let f2 = resolved("Frame \"FRAME\" \"R\" { SetPoint TOPLEFT, 0, 0, 0, }", "R");
    assert!(props::anchors(&f2.props).is_err());
}

#[test]
fn number_and_numbers_reject_wrong_shapes() {
    let f = resolved(
        "Frame \"FRAME\" \"W\" { Width 0.1, 0.2, Height \"x\", TexCoord 0, 1, 0, }",
        "W",
    );
    assert!(props::number(&f.props, "Width").is_err());
    assert!(props::number(&f.props, "Height").is_err());
    assert!(props::numbers(&f.props, "TexCoord", 4).is_err());
    assert_eq!(props::number(&f.props, "Missing").unwrap(), None);
}

#[test]
fn typed_readers_have_documented_defaults() {
    let f = resolved(
        r#"Frame "TEXT" "Typed" {
            FontColor 1.0 0.8 0.0,
            FontJustificationH JUSTIFYCENTER,
            FontJustificationV JUSTIFYTOP,
            AlphaMode "ADD",
            FrameFont "MasterFont", 0.011, "",
        }"#,
        "Typed",
    );
    assert_eq!(
        props::color(&f.props, "FontColor").unwrap(),
        Some([1.0, 0.8, 0.0, 1.0])
    );
    assert_eq!(props::justify_h(&f.props).unwrap(), props::JustifyH::Center);
    assert_eq!(props::justify_v(&f.props).unwrap(), props::JustifyV::Top);
    assert_eq!(props::alpha_mode(&f.props).unwrap(), AlphaMode::Add);
    let font = props::font(&f.props, "FrameFont").unwrap().unwrap();
    assert_eq!((font.name.as_str(), font.size), ("MasterFont", 0.011));
    assert_eq!(props::tex_coord(&f.props).unwrap(), [0.0, 1.0, 0.0, 1.0]);

    let g = resolved(r#"Frame "TEXT" "Def" { }"#, "Def");
    assert_eq!(props::justify_h(&g.props).unwrap(), props::JustifyH::Left);
    assert_eq!(props::justify_v(&g.props).unwrap(), props::JustifyV::Middle);
    assert_eq!(props::alpha_mode(&g.props).unwrap(), AlphaMode::Blend);
    assert_eq!(props::color(&g.props, "FontColor").unwrap(), None);
}

#[test]
fn unknown_enum_values_are_errors() {
    let f = resolved(
        "Frame \"TEXT\" \"E\" { AlphaMode \"WOBBLE\", FontJustificationH JUSTIFYDIAGONAL, }",
        "E",
    );
    assert!(props::alpha_mode(&f.props).is_err());
    assert!(props::justify_h(&f.props).is_err());
}

#[test]
fn raw_values_are_still_reachable() {
    let f = resolved(
        "Frame \"BACKDROP\" \"Raw\" { LayerStyle \"NOSHADING|IGNORETRACKEVENTS\", }",
        "Raw",
    );
    let p = props::last(&f.props, "layerstyle").unwrap();
    assert_eq!(
        p.values,
        vec![Value::Str("NOSHADING|IGNORETRACKEVENTS".into())]
    );
}

#[test]
fn top_level_texture_and_string_templates_load_and_inherit() {
    // Game files define Texture and String templates at top level, and a
    // String template may INHERITS another one (the H3 load failure).
    let lib = lib_from(
        &[(
            "t.fdf",
            r#"
Texture "BaseTex" {
    File "a.blp",
    TexCoord 0.0, 1.0, 0.0, 1.0,
}
String "BaseText" {
    Font "MasterFont",0.01,
    FontColor 1.0 1.0 1.0,
}
String "DisabledText" INHERITS "BaseText" {
    FontColor 0.4 0.4 0.4,
}
Texture "NoName" { File "b.blp", }
"#,
        )],
        "t.fdf",
    )
    .unwrap();
    assert!(lib.warnings.is_empty(), "{:?}", lib.warnings);
    let d = lib.block("disabledtext").unwrap();
    assert_eq!(d.inherits, None);
    let keys: Vec<&str> = d.props.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(keys, vec!["Font", "FontColor"]);
    assert_eq!(lib.block("BaseTex").unwrap().props.len(), 2);
    assert!(lib.block_names().any(|n| n == "noname"));
    assert!(matches!(
        lib.block("missing"),
        Err(LibraryError::UnknownBlock(_))
    ));
}

#[test]
fn nested_string_inheriting_a_template_is_resolved_in_frames() {
    let lib = lib_from(
        &[(
            "t.fdf",
            r#"
String "Base" { FontColor 1.0 1.0 1.0, Font "F",0.01, }
Frame "SIMPLEFRAME" "Host" {
    String "Child" INHERITS "Base" {
        FontColor 0.0 0.0 0.0,
    }
}
"#,
        )],
        "t.fdf",
    )
    .unwrap();
    let flat = lib.resolve("Host").unwrap();
    let Child::Block(b) = &flat.children[0] else {
        panic!("expected a block child");
    };
    assert_eq!(b.name.as_deref(), Some("Child"));
    let keys: Vec<&str> = b.props.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["Font", "FontColor"],
        "inherited Font, own FontColor"
    );
}

#[test]
fn block_inheritance_errors_are_reported() {
    let lib = lib_from(
        &[(
            "t.fdf",
            r#"
String "A" INHERITS "B" { }
String "B" INHERITS "A" { }
String "C" INHERITS "Nowhere" { }
"#,
        )],
        "t.fdf",
    )
    .unwrap();
    assert!(matches!(lib.block("A"), Err(LibraryError::InheritCycle(_))));
    assert!(matches!(
        lib.block("C"),
        Err(LibraryError::UnknownParent { .. })
    ));
}
