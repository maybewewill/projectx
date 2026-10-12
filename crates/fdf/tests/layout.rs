//! Layout tests: anchors, sizes, screen mapping, cycles and warnings.

use fdf::layout::{solve, ExternalFrames, LayoutError, Rect, Screen, Warning};
use fdf::{Library, Scene};

fn scene_of(src: &str, roots: &[&str]) -> Scene {
    let mut lib = Library::new();
    let mut none = |_: &str| None;
    lib.load_source("layout-test", src, &mut none)
        .expect("load");
    Scene::build(&lib, roots).expect("scene")
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

fn assert_rect(r: Option<Rect>, left: f32, bottom: f32, right: f32, top: f32) {
    let r = r.unwrap_or_else(|| panic!("expected a rect"));
    assert!(
        near(r.left, left) && near(r.bottom, bottom) && near(r.right, right) && near(r.top, top),
        "got {r:?}, want ({left}, {bottom}, {right}, {top})"
    );
}

fn node(scene: &Scene, name: &str) -> usize {
    scene.find(name).unwrap_or_else(|| panic!("no node {name}"))
}

fn run(src: &str, roots: &[&str]) -> (Scene, fdf::Layout) {
    let scene = scene_of(src, roots);
    let layout = solve(&scene, &Screen::safe_area(), &ExternalFrames::new()).expect("solve");
    (scene, layout)
}

#[test]
fn screen_mapping_pillarbox_rule() {
    let s43 = Screen::for_window(1024, 768);
    assert_eq!(s43.window, s43.safe);
    assert!(near(s43.window.left, 0.0) && near(s43.window.right, 0.8));

    let s169 = Screen::for_window(1920, 1080);
    assert!(near(s169.window.left, 0.4 - 0.3 * 1920.0 / 1080.0));
    assert!(near(s169.window.right, 0.4 + 0.3 * 1920.0 / 1080.0));
    assert!(near(s169.window.left, -0.1333333));
    assert!(near(s169.window.right, 0.9333333));
    assert_eq!(s169.safe, Rect::new(0.0, 0.0, 0.8, 0.6));

    // The safe area is centred: its centre is the window centre.
    let mid = (s169.window.left + s169.window.right) * 0.5;
    assert!(near(mid, 0.4));

    let s54 = Screen::for_window(1280, 1024);
    assert!(s54.window.width() < 0.8);

    let zero = Screen::for_window(0, 0);
    assert_eq!(zero.window, zero.safe);
}

#[test]
fn parent_relative_offsets_and_sizes() {
    let (scene, layout) = run(
        r#"
Frame "FRAME" "Root" {
    SetAllPoints,
    Frame "BACKDROP" "Child" {
        Width 0.2, Height 0.1,
        SetPoint TOPLEFT, TOPLEFT, 0.05, -0.1,
    }
}
"#,
        &["Root"],
    );
    assert_rect(layout.rect(node(&scene, "Root")), 0.0, 0.0, 0.8, 0.6);
    // Top-left 0.05 right and 0.1 down from the parent's top-left (0, 0.6).
    assert_rect(layout.rect(node(&scene, "Child")), 0.05, 0.4, 0.25, 0.5);
}

#[test]
fn bottomright_anchor_with_size_derives_left_and_bottom() {
    let (scene, layout) = run(
        r#"
Frame "FRAME" "Root" {
    SetAllPoints,
    Frame "BACKDROP" "Corner" {
        Width 0.1, Height 0.05,
        SetPoint BOTTOMRIGHT, BOTTOMRIGHT, -0.01, 0.01,
    }
}
"#,
        &["Root"],
    );
    assert_rect(layout.rect(node(&scene, "Corner")), 0.69, 0.01, 0.79, 0.06);
}

#[test]
fn two_points_define_size_and_width_is_ignored() {
    let (scene, layout) = run(
        r#"
Frame "FRAME" "Root" {
    SetAllPoints,
    Frame "BACKDROP" "Span" {
        Width 0.9,
        SetPoint TOPLEFT, TOPLEFT, 0.1, -0.1,
        SetPoint BOTTOMRIGHT, BOTTOMRIGHT, -0.3, 0.2,
    }
}
"#,
        &["Root"],
    );
    assert_rect(layout.rect(node(&scene, "Span")), 0.1, 0.2, 0.5, 0.5);
}

#[test]
fn setallpoints_on_child_matches_parent() {
    let (scene, layout) = run(
        r#"
Frame "FRAME" "Root" {
    SetPoint CENTER, CENTER, 0, 0,
    Width 0.4, Height 0.2,
    Frame "BACKDROP" "Fill" { SetAllPoints, }
}
"#,
        &["Root"],
    );
    assert_rect(layout.rect(node(&scene, "Root")), 0.2, 0.2, 0.6, 0.4);
    assert_rect(layout.rect(node(&scene, "Fill")), 0.2, 0.2, 0.6, 0.4);
}

#[test]
fn center_anchor_with_size() {
    let (scene, layout) = run(
        r#"Frame "FRAME" "C" { SetPoint CENTER, CENTER, 0, 0, Width 0.2, Height 0.2, }"#,
        &["C"],
    );
    assert_rect(layout.rect(node(&scene, "C")), 0.3, 0.2, 0.5, 0.4);
}

#[test]
fn one_point_without_size_is_zero_sized() {
    let (scene, layout) = run(
        r#"Frame "FRAME" "Z" { SetPoint BOTTOMLEFT, BOTTOMLEFT, 0.1, 0.2, }"#,
        &["Z"],
    );
    assert_rect(layout.rect(node(&scene, "Z")), 0.1, 0.2, 0.1, 0.2);
}

#[test]
fn named_sibling_declared_later_is_solved_first() {
    let (scene, layout) = run(
        r#"
Frame "FRAME" "Root" {
    SetAllPoints,
    Frame "BACKDROP" "First" {
        Width 0.1, Height 0.1,
        SetPoint TOPLEFT, "Second", TOPRIGHT, 0.01, 0,
    }
    Frame "BACKDROP" "Second" {
        Width 0.2, Height 0.2,
        SetPoint TOPLEFT, "Root", TOPLEFT, 0.1, -0.1,
    }
}
"#,
        &["Root"],
    );
    // Second: top-left (0.1, 0.5) from the root, so (0.1, 0.3)-(0.3, 0.5).
    // First: top-left at Second's top-right plus (0.01, 0), so (0.31, 0.4)-(0.41, 0.5).
    assert_rect(layout.rect(node(&scene, "Second")), 0.1, 0.3, 0.3, 0.5);
    assert_rect(layout.rect(node(&scene, "First")), 0.31, 0.4, 0.41, 0.5);
    let pos = |i: usize| layout.order.iter().position(|&x| x == i).unwrap();
    assert!(pos(node(&scene, "Second")) < pos(node(&scene, "First")));
}

#[test]
fn external_frame_as_relative() {
    let scene = scene_of(
        r#"Frame "FRAME" "Ext" { SetPoint TOPLEFT, "ConsoleUIBackdrop", BOTTOMLEFT, 0, 0, Width 0.1, Height 0.1, }"#,
        &["Ext"],
    );
    let mut externals = ExternalFrames::new();
    externals.insert("ConsoleUIBackdrop".into(), Rect::new(0.0, 0.0, 1.0, 0.6));
    let layout = solve(&scene, &Screen::safe_area(), &externals).unwrap();
    // Top-left at the external's bottom-left (0, 0), so the frame hangs below it.
    assert_rect(layout.rect(node(&scene, "Ext")), 0.0, -0.1, 0.1, 0.0);
}

#[test]
fn simple_roots_use_the_window_and_frames_use_the_safe_area() {
    let scene = scene_of(
        r#"
Frame "SIMPLEFRAME" "Wide" { SetAllPoints, }
Frame "FRAME" "Narrow" { SetAllPoints, }
"#,
        &["Wide", "Narrow"],
    );
    let screen = Screen::for_window(1920, 1080);
    let layout = solve(&scene, &screen, &ExternalFrames::new()).unwrap();
    assert_eq!(layout.rect(node(&scene, "Wide")), Some(screen.window));
    assert_eq!(layout.rect(node(&scene, "Narrow")), Some(screen.safe));
    // The Frame-group root is exactly the safe area: no warning for it.
    assert!(!layout
        .warnings
        .iter()
        .any(|w| matches!(w, Warning::OutsideSafeArea { node } if node == "Narrow")));
}

#[test]
fn frame_outside_safe_area_warns_but_simple_does_not() {
    let scene = scene_of(
        r#"
Frame "FRAME" "Big" { SetPoint TOPLEFT, TOPLEFT, 0.7, 0, Width 0.3, Height 0.1, }
Frame "SIMPLEFRAME" "BigSimple" { SetPoint TOPLEFT, TOPLEFT, 0.7, 0, Width 0.3, Height 0.1, }
"#,
        &["Big", "BigSimple"],
    );
    let layout = solve(&scene, &Screen::safe_area(), &ExternalFrames::new()).unwrap();
    let outside: Vec<&Warning> = layout
        .warnings
        .iter()
        .filter(|w| matches!(w, Warning::OutsideSafeArea { .. }))
        .collect();
    assert_eq!(outside.len(), 1);
    assert_eq!(outside[0], &Warning::OutsideSafeArea { node: "Big".into() });
}

#[test]
fn unpositioned_nodes_warn_and_have_no_rect() {
    let scene = scene_of(
        r#"Frame "FRAME" "P" { SetAllPoints, Frame "FRAME" "Lost" { Width 0.1, } }"#,
        &["P"],
    );
    let layout = solve(&scene, &Screen::safe_area(), &ExternalFrames::new()).unwrap();
    assert_eq!(layout.rect(node(&scene, "Lost")), None);
    assert_eq!(
        layout.warnings,
        vec![Warning::Unpositioned {
            node: "Lost".into()
        }]
    );
}

#[test]
fn root_without_anchors_fills_its_screen_rect() {
    // ConsoleUI (a SIMPLEFRAME root with no SetPoint) fills the window; a
    // FRAME root fills the 4:3 safe area. Its children anchor to it.
    let scene = scene_of(
        r#"
Frame "SIMPLEFRAME" "Console" {
    Frame "FRAME" "Bar" { SetAllPoints, }
}
Frame "FRAME" "Menu" {
    Frame "FRAME" "Inner" { SetAllPoints, }
}
"#,
        &["Console", "Menu"],
    );
    let screen = Screen::for_window(1920, 1080);
    let layout = solve(&scene, &screen, &ExternalFrames::new()).unwrap();
    assert_rect(
        layout.rect(node(&scene, "Console")),
        screen.window.left,
        0.0,
        screen.window.right,
        0.6,
    );
    assert_rect(
        layout.rect(node(&scene, "Bar")),
        screen.window.left,
        0.0,
        screen.window.right,
        0.6,
    );
    assert_rect(layout.rect(node(&scene, "Menu")), 0.0, 0.0, 0.8, 0.6);
    assert_rect(layout.rect(node(&scene, "Inner")), 0.0, 0.0, 0.8, 0.6);
    // Bar is a FRAME outside the 4:3 safe area, the existing warning; no
    // node is unpositioned.
    assert!(!layout
        .warnings
        .iter()
        .any(|w| matches!(w, Warning::Unpositioned { .. })));
}

#[test]
fn child_of_unpositioned_parent_is_unresolved() {
    let scene = scene_of(
        r#"Frame "FRAME" "Root" { SetAllPoints, Frame "FRAME" "P" { Frame "FRAME" "K" { SetAllPoints, } } }"#,
        &["Root"],
    );
    let layout = solve(&scene, &Screen::safe_area(), &ExternalFrames::new()).unwrap();
    assert_eq!(layout.rect(node(&scene, "P")), None);
    assert_eq!(layout.rect(node(&scene, "K")), None);
}

#[test]
fn anchor_cycle_is_detected_with_names() {
    let scene = scene_of(
        r#"
Frame "FRAME" "A" { SetPoint TOPLEFT, "B", TOPLEFT, 0, 0, Width 0.1, Height 0.1, }
Frame "FRAME" "B" { SetPoint TOPLEFT, "A", TOPLEFT, 0, 0, Width 0.1, Height 0.1, }
"#,
        &["A", "B"],
    );
    match solve(&scene, &Screen::safe_area(), &ExternalFrames::new()).unwrap_err() {
        LayoutError::Cycle(names) => {
            assert!(names.len() >= 2);
            assert!(names.iter().any(|n| n == "A") && names.iter().any(|n| n == "B"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn self_anchor_is_a_cycle() {
    let scene = scene_of(
        r#"Frame "FRAME" "Me" { SetPoint TOPLEFT, "Me", TOPLEFT, 0, 0, }"#,
        &["Me"],
    );
    assert!(matches!(
        solve(&scene, &Screen::safe_area(), &ExternalFrames::new()),
        Err(LayoutError::Cycle(_))
    ));
}

#[test]
fn unknown_relative_is_an_error() {
    let scene = scene_of(
        r#"Frame "FRAME" "U" { SetPoint TOPLEFT, "Nowhere", TOPLEFT, 0, 0, }"#,
        &["U"],
    );
    match solve(&scene, &Screen::safe_area(), &ExternalFrames::new()).unwrap_err() {
        LayoutError::UnknownRelative { node, name } => {
            assert_eq!(node, "U");
            assert_eq!(name, "Nowhere");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn bad_width_is_a_property_error_with_the_node_name() {
    let scene = scene_of(
        r#"Frame "FRAME" "Bad" { Width "wide", SetAllPoints, }"#,
        &["Bad"],
    );
    match solve(&scene, &Screen::safe_area(), &ExternalFrames::new()).unwrap_err() {
        LayoutError::Property { node, error } => {
            assert_eq!(node, "Bad");
            assert_eq!(error.key, "Width");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn names_are_case_insensitive_for_anchors() {
    let (scene, layout) = run(
        r#"
Frame "FRAME" "Root" {
    SetAllPoints,
    Frame "BACKDROP" "Target" { Width 0.1, Height 0.1, SetPoint CENTER, CENTER, 0, 0, }
    Frame "BACKDROP" "Follower" { Width 0.1, Height 0.1, SetPoint LEFT, "TARGET", RIGHT, 0, 0, }
}
"#,
        &["Root"],
    );
    assert_rect(layout.rect(node(&scene, "Target")), 0.35, 0.25, 0.45, 0.35);
    assert_rect(
        layout.rect(node(&scene, "Follower")),
        0.45,
        0.25,
        0.55,
        0.35,
    );
}

#[test]
fn offsets_are_right_and_up_positive() {
    let (scene, layout) = run(
        r#"Frame "FRAME" "O" { SetPoint BOTTOMLEFT, BOTTOMLEFT, 0.25, 0.1, Width 0.1, Height 0.1, }"#,
        &["O"],
    );
    assert_rect(layout.rect(node(&scene, "O")), 0.25, 0.1, 0.35, 0.2);
}

#[test]
fn screen_root_for_16_9_window_allows_simple_frames_outside_4_3() {
    let scene = scene_of(
        r#"Frame "SIMPLEFRAME" "Full" { Width 0.2, Height 0.6, SetPoint TOPRIGHT, TOPRIGHT, 0, 0, }"#,
        &["Full"],
    );
    let screen = Screen::for_window(1920, 1080);
    let layout = solve(&scene, &screen, &ExternalFrames::new()).unwrap();
    let r = layout.rect(node(&scene, "Full")).unwrap();
    assert!(near(r.right, screen.window.right));
    assert!(!screen.safe.contains(&r));
    assert!(layout.warnings.is_empty());
}

#[test]
fn rect_helpers() {
    let outer = Rect::new(0.0, 0.0, 0.8, 0.6);
    assert!(outer.contains(&Rect::new(0.1, 0.1, 0.8, 0.6)));
    assert!(!outer.contains(&Rect::new(0.1, 0.1, 0.81, 0.6)));
    assert!(near(Rect::new(0.1, 0.2, 0.4, 0.7).width(), 0.3));
    assert!(near(Rect::new(0.1, 0.2, 0.4, 0.7).height(), 0.5));
}

#[test]
fn placed_root_takes_the_host_rectangle_and_its_children_follow() {
    let src = r#"
Frame "SIMPLEFRAME" "ResourceBarFrame" {
    Width 0.4,
    Height 0.02,
    String "GoldText" {
        Anchor TOPRIGHT, -0.1, 0,
        Width 0.1,
        Height 0.02,
    }
}
"#;
    let scene = scene_of(src, &["ResourceBarFrame"]);
    let mut placed = ExternalFrames::new();
    // Host placement: the frame's top-right corner at (0.8, 0.6).
    placed.insert("resourcebarframe".to_string(), Rect::new(0.4, 0.58, 0.8, 0.6));
    let layout = fdf::layout::solve_placed(&scene, &Screen::safe_area(), &ExternalFrames::new(), &placed)
        .expect("solve");
    assert_rect(layout.rects[node(&scene, "ResourceBarFrame")], 0.4, 0.58, 0.8, 0.6);
    assert_rect(layout.rects[node(&scene, "GoldText")], 0.6, 0.58, 0.7, 0.6);
    // Without the placement the root fills the screen rectangle.
    let (scene2, plain) = run(src, &["ResourceBarFrame"]);
    assert_rect(plain.rects[node(&scene2, "ResourceBarFrame")], 0.0, 0.0, 0.8, 0.6);
}
