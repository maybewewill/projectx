//! Draw-list tests and an end-to-end run over a synthetic TOC and FDF set.

use std::collections::BTreeMap;

use fdf::draw::{layer_rank, DrawItem, QuadKind};
use fdf::layout::{solve, ExternalFrames, Screen};
use fdf::props::{AlphaMode, JustifyH, JustifyV};
use fdf::{parse_toc, DrawList, Library, Scene};

/// Synthetic game-like tree: a dialog with a backdrop, a title String block,
/// a button inheriting a template, and a decorated text frame.
const TOC: &str = "// synthetic\nUI\\FrameDef\\Common.fdf,\nUI\\FrameDef\\Dialog.fdf,\n";

const COMMON: &str = r#"
StringList {
    TITLE_KEY "Synthetic Title",
    ICON_KEY "UI\Synthetic\icon.blp",
}

Frame "BUTTON" "BaseButton" {
    Width 0.12,
    Height 0.04,
    ControlStyle "AUTOTRACK",
    Frame "BACKDROP" "BaseButtonBackdrop" {
        SetAllPoints,
        BackdropBackground "UI\Synthetic\button.blp",
        BackdropBlendAll,
    }
}
"#;

const DIALOG: &str = r#"
Frame "SIMPLEFRAME" "SyntheticDialog" {
    DecorateFileNames,
    Width 0.4,
    Height 0.3,
    SetPoint CENTER, CENTER, 0, 0,
    Frame "BACKDROP" "DialogBackdrop" {
        SetAllPoints,
        BackdropBackground "UI\Synthetic\panel.blp",
        BackdropEdgeFile "UI\Synthetic\edge.blp",
        BackdropTileBackground,
    }
    Layer "ARTWORK" {
        String "DialogTitle" {
            SetPoint TOP, "SyntheticDialog", TOP, 0, -0.02,
            Font "TitleFont", 0.015,
            FontColor 1.0 0.8 0.0 1.0,
            FontJustificationH JUSTIFYCENTER,
            Text "TITLE_KEY",
        }
    }
    Layer "BACKGROUND" {
        Texture "Glow" {
            SetAllPoints,
            File "UI\Synthetic\glow.blp",
            TexCoord 0, 0.5, 0, 0.25,
            AlphaMode "ADD",
        }
    }
    Frame "BUTTON" "OkButton" INHERITS WITHCHILDREN "BaseButton" {
        SetPoint BOTTOM, "SyntheticDialog", BOTTOM, 0, 0.02,
    }
    Frame "TEXT" "Caption" {
        DecorateFileNames,
        Width 0.2, Height 0.03,
        SetPoint BOTTOMLEFT, "SyntheticDialog", BOTTOMLEFT, 0.02, 0.02,
        Text "ICON_KEY",
        FrameFont "MasterFont", 0.011, "",
        FontJustificationH JUSTIFYRIGHT,
    }
}
"#;

fn fixture_lib() -> Library {
    let mut files = BTreeMap::new();
    files.insert("ui\\framedef\\common.fdf".to_string(), COMMON.to_string());
    files.insert("ui\\framedef\\dialog.fdf".to_string(), DIALOG.to_string());
    let mut lib = Library::new();
    let toc = parse_toc(TOC);
    assert_eq!(toc.len(), 2);
    let mut loader = |p: &str| files.get(&fdf::library::normalize_path(p)).cloned();
    for path in &toc {
        lib.load(path, &mut loader).expect("load");
    }
    lib
}

#[test]
fn toc_and_inclusion_produce_all_templates() {
    let lib = fixture_lib();
    let names: Vec<&str> = lib.frame_names().collect();
    assert!(names.contains(&"syntheticdialog"));
    assert!(names.contains(&"basebutton"));
    assert_eq!(lib.string("ICON_KEY"), Some(r"UI\Synthetic\icon.blp"));
}

#[test]
fn full_pipeline_layout_and_draw() {
    let lib = fixture_lib();
    let scene = Scene::build(&lib, &["SyntheticDialog"]).expect("scene");
    let screen = Screen::for_window(1920, 1080);
    let layout = solve(&scene, &screen, &ExternalFrames::new()).expect("solve");

    let dialog = scene.find("SyntheticDialog").unwrap();
    let r = layout.rect(dialog).unwrap();
    // Centred on the window's centre x, which is the safe-area centre (0.4).
    assert!((r.left + r.right) * 0.5 - 0.4 < 1e-4 && (r.left + r.right) * 0.5 - 0.4 > -1e-4);
    assert!((r.top - r.bottom - 0.3).abs() < 1e-5);
    assert!(layout.rect(scene.find("Caption").unwrap()).is_some());

    let list = DrawList::build(&scene, &layout, &lib).expect("draw");

    // Dialog panel: a backdrop, opaque unless BackdropBlendAll is set.
    let panel = list
        .quads()
        .find(|q| q.kind == QuadKind::BackdropBackground && q.texture.ends_with("panel.blp"))
        .unwrap();
    assert_eq!(panel.alpha, AlphaMode::Opaque);
    assert!(panel.tile, "DialogBackdrop sets BackdropTileBackground");
    let edge = list
        .quads()
        .find(|q| q.kind == QuadKind::BackdropEdge)
        .unwrap();
    assert!(edge.tile);

    // Glow texture: ADD mode, TexCoord honoured.
    let glow = list
        .quads()
        .find(|q| q.texture.ends_with("glow.blp"))
        .unwrap();
    assert_eq!(glow.alpha, AlphaMode::Add);
    assert_eq!(glow.uv, [0.0, 0.5, 0.0, 0.25]);
    assert_eq!(glow.rect, r);

    // Title text decorated from the StringList, centred.
    let title = list.texts().find(|t| t.text == "Synthetic Title").unwrap();
    assert_eq!(title.justify_h, JustifyH::Center);
    assert_eq!(title.justify_v, JustifyV::Middle);
    assert_eq!(title.color, [1.0, 0.8, 0.0, 1.0]);
    assert_eq!(title.font.as_ref().unwrap().name, "TitleFont");

    // Caption: decorated path-free text, right-justified, MasterFont.
    let caption = list
        .texts()
        .find(|t| t.text == r"UI\Synthetic\icon.blp")
        .unwrap();
    assert_eq!(caption.justify_h, JustifyH::Right);
    assert_eq!(caption.font.as_ref().unwrap().size, 0.011);

    // Inherited button: its backdrop was copied through WITHCHILDREN.
    assert!(list.quads().any(|q| q.texture.ends_with("button.blp")));
    let ok = scene.find("OkButton").unwrap();
    assert!(layout.rect(ok).is_some());
}

#[test]
fn paint_order_follows_layers_then_children() {
    let lib = fixture_lib();
    let scene = Scene::build(&lib, &["SyntheticDialog"]).unwrap();
    let layout = solve(
        &scene,
        &Screen::for_window(1920, 1080),
        &ExternalFrames::new(),
    )
    .unwrap();
    let list = DrawList::build(&scene, &layout, &lib).unwrap();

    let pos_of = |pred: &dyn Fn(&DrawItem) -> bool| list.items.iter().position(pred).unwrap();
    let glow = pos_of(&|i| matches!(i, DrawItem::Quad(q) if q.texture.ends_with("glow.blp")));
    let title = pos_of(&|i| matches!(i, DrawItem::Text(t) if t.text == "Synthetic Title"));
    let panel = pos_of(&|i| matches!(i, DrawItem::Quad(q) if q.texture.ends_with("panel.blp")));
    let button = pos_of(&|i| matches!(i, DrawItem::Quad(q) if q.texture.ends_with("button.blp")));
    // The dialog's own blocks come first, by layer (BACKGROUND before ARTWORK).
    // Its child frames (the panel backdrop, then the inherited button) come after.
    assert!(glow < title);
    assert!(title < panel);
    assert!(panel < button);
}

#[test]
fn layer_ranks() {
    assert!(layer_rank(Some("BACKGROUND")) < layer_rank(Some("BORDER")));
    assert!(layer_rank(Some("BORDER")) < layer_rank(Some("ARTWORK")));
    assert!(layer_rank(Some("ARTWORK")) < layer_rank(Some("artwork_overlay")));
    assert_eq!(layer_rank(None), layer_rank(Some("ARTWORK")));
}

#[test]
fn unpositioned_frames_produce_no_items() {
    let mut lib = Library::new();
    let mut none = |_: &str| None;
    lib.load_source(
        "u",
        r#"Frame "FRAME" "Root" { SetAllPoints,
            Frame "BACKDROP" "Floating" { BackdropBackground "x.blp", Width 0.1, }
        }"#,
        &mut none,
    )
    .unwrap();
    let scene = Scene::build(&lib, &["Root"]).unwrap();
    let layout = solve(&scene, &Screen::safe_area(), &ExternalFrames::new()).unwrap();
    let list = DrawList::build(&scene, &layout, &lib).unwrap();
    assert!(list.items.is_empty());
}

#[test]
fn texture_without_file_and_text_without_font_are_still_handled() {
    let mut lib = Library::new();
    let mut none = |_: &str| None;
    lib.load_source(
        "t",
        r#"Frame "FRAME" "Root" { SetAllPoints,
            Texture "Empty" { SetAllPoints, }
            Frame "TEXT" "Plain" { SetAllPoints, Text "hi", }
        }"#,
        &mut none,
    )
    .unwrap();
    let scene = Scene::build(&lib, &["Root"]).unwrap();
    let layout = solve(&scene, &Screen::safe_area(), &ExternalFrames::new()).unwrap();
    let list = DrawList::build(&scene, &layout, &lib).unwrap();
    assert_eq!(list.items.len(), 1);
    let t = list.texts().next().unwrap();
    assert_eq!(t.text, "hi");
    assert!(t.font.is_none());
    assert_eq!(t.color, [1.0; 4]);
}

#[test]
fn bad_property_in_draw_is_reported_with_node_name() {
    let mut lib = Library::new();
    let mut none = |_: &str| None;
    lib.load_source(
        "b",
        r#"Frame "FRAME" "Root" { SetAllPoints,
            Frame "TEXT" "Bad" { SetAllPoints, Text "x", FontJustificationV JUSTIFYSIDEWAYS, }
        }"#,
        &mut none,
    )
    .unwrap();
    let scene = Scene::build(&lib, &["Root"]).unwrap();
    let layout = solve(&scene, &Screen::safe_area(), &ExternalFrames::new()).unwrap();
    let err = DrawList::build(&scene, &layout, &lib).unwrap_err();
    assert_eq!(err.node, "Bad");
    assert_eq!(err.error.key, "FontJustificationV");
}

#[test]
fn scene_rejects_duplicate_names() {
    let mut lib = Library::new();
    let mut none = |_: &str| None;
    lib.load_source(
        "d",
        r#"Frame "FRAME" "Root" { Frame "FRAME" "Same" { } Frame "FRAME" "same" { } }"#,
        &mut none,
    )
    .unwrap();
    let err = Scene::build(&lib, &["Root"]).unwrap_err();
    assert!(err.to_string().contains("used twice"), "{err}");
}

#[test]
fn unknown_root_is_reported() {
    let lib = Library::new();
    assert!(Scene::build(&lib, &["Nope"]).is_err());
}
