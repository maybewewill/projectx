//! Button labels: `NormalText "<String template>" "<StringList key>"` draws a centred text run in the
//! template's font and colour, with the key decorated from the StringList.

use fdf::layout::{solve, ExternalFrames, Screen};
use fdf::props::{JustifyH, JustifyV};
use fdf::{DrawList, Library, Scene};

#[test]
fn button_label_uses_the_template_font_and_the_string_key() {
    let mut lib = Library::new();
    let mut none = |_: &str| None;
    lib.load_source(
        "t",
        r#"
StringList { KEY_QUESTS "Quests", }
String "LabelTemplate" {
    Font "MasterFont", 0.01,
    FontColor 1.0 0.83 0.07,
}
Frame "SIMPLEBUTTON" "QuestsButton" {
    SetAllPoints,
    NormalText "LabelTemplate" "KEY_QUESTS",
}
Frame "SIMPLEBUTTON" "PlainButton" {
    SetAllPoints,
    NormalText "LabelTemplate" "NO_SUCH_KEY",
}
"#,
        &mut none,
    )
    .unwrap();
    let scene = Scene::build(&lib, &["QuestsButton", "PlainButton"]).unwrap();
    let layout = solve(&scene, &Screen::safe_area(), &ExternalFrames::new()).unwrap();
    let list = DrawList::build(&scene, &layout, &lib).unwrap();

    let quests = list
        .texts()
        .find(|t| t.text == "Quests")
        .expect("label decorated from the StringList");
    assert_eq!(quests.justify_h, JustifyH::Center);
    assert_eq!(quests.justify_v, JustifyV::Middle);
    assert_eq!(quests.color, [1.0, 0.83, 0.07, 1.0]);
    assert_eq!(quests.font.as_ref().map(|f| f.name.as_str()), Some("MasterFont"));
    // A key without a StringList entry is shown as written.
    assert!(list.texts().any(|t| t.text == "NO_SUCH_KEY"));
}
