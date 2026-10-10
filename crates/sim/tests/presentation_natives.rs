//! Presentation natives through `ScriptHost` (`natives/presentation.rs`).
//!
//! The checks cover the three things the module promises: handle ids are allocated and reused like
//! the other handle objects (LIFO, interleaved with timers), the getters return what the script set,
//! and what a renderer would show is logged in order.

use jass::compile::compile;
use jass::vm::{NativeRegistry, Value};
use sim::natives::presentation::{EffectAt, PresKind, PresentationEvent, PropValue};
use sim::ScriptHost;

const PRELUDE: &str = r#"
type timer extends handle
type sound extends handle
type effect extends handle
type texttag extends handle
type location extends handle
type camerasetup extends handle
type camerafield extends handle
type multiboard extends handle
type multiboarditem extends handle
type leaderboard extends handle
type player extends handle
type lightning extends handle
type effecttype extends handle

native CreateTimer takes nothing returns timer
native DestroyTimer takes timer t returns nothing
native GetHandleId takes handle h returns integer
native I2R takes integer i returns real
native Player takes integer number returns player
native ConvertCameraField takes integer i returns camerafield
native GetLocationX takes location l returns real
native GetLocationY takes location l returns real

native SetCameraPosition takes real x, real y returns nothing
native PanCameraToTimedWithZ takes real x, real y, real zOffsetDest, real duration returns nothing
native SetCameraField takes camerafield whichField, real value, real duration returns nothing
native AdjustCameraField takes camerafield whichField, real offset, real duration returns nothing
native GetCameraField takes camerafield whichField returns real
native SetCameraBounds takes real x1, real y1, real x2, real y2, real x3, real y3, real x4, real y4 returns nothing
native GetCameraBoundMinX takes nothing returns real
native GetCameraBoundMaxY takes nothing returns real
native GetCameraTargetPositionX takes nothing returns real
native GetCameraTargetPositionY takes nothing returns real
native GetCameraTargetPositionZ takes nothing returns real
native GetCameraTargetPositionLoc takes nothing returns location
native CreateCameraSetup takes nothing returns camerasetup
native CameraSetupSetField takes camerasetup setup, camerafield whichField, real value, real duration returns nothing
native CameraSetupGetField takes camerasetup setup, camerafield whichField returns real
native CameraSetupSetDestPosition takes camerasetup setup, real x, real y, real duration returns nothing
native CameraSetupGetDestPositionX takes camerasetup setup returns real
native CameraSetupGetDestPositionLoc takes camerasetup setup returns location
native CameraSetupApply takes camerasetup setup, boolean doPan, boolean panTimed returns nothing

native CreateSound takes string fileName, boolean looping, boolean is3D, boolean stopwhenoutofrange, integer fadeInRate, integer fadeOutRate, string eaxSetting returns sound
native StartSound takes sound s returns nothing
native StopSound takes sound s, boolean fadeOut, boolean killWhenDone returns nothing
native KillSoundWhenDone takes sound s returns nothing
native SetSoundDuration takes sound s, integer duration returns nothing
native GetSoundDuration takes sound s returns integer
native GetSoundFileDuration takes string fileName returns integer
native GetSoundIsPlaying takes sound s returns boolean

native CreateTextTag takes nothing returns texttag
native DestroyTextTag takes texttag t returns nothing
native SetTextTagText takes texttag t, string s, real height returns nothing

native AddSpecialEffect takes string modelName, real x, real y returns effect
native AddSpecialEffectLoc takes string modelName, location where returns effect
native AddSpellEffectTarget takes string abilityString, effecttype t, handle target, string attachPoint returns effect
native DestroyEffect takes effect e returns nothing

native CreateMultiboard takes nothing returns multiboard
native MultiboardSetRowCount takes multiboard mb, integer count returns nothing
native MultiboardSetColumnCount takes multiboard mb, integer count returns nothing
native MultiboardGetRowCount takes multiboard mb returns integer
native MultiboardGetColumnCount takes multiboard mb returns integer
native MultiboardSetTitleText takes multiboard mb, string label returns nothing
native MultiboardGetTitleText takes multiboard mb returns string
native MultiboardGetItem takes multiboard mb, integer row, integer column returns multiboarditem
native MultiboardReleaseItem takes multiboarditem mbi returns nothing

native CreateLeaderboard takes nothing returns leaderboard
native LeaderboardAddItem takes leaderboard lb, string label, integer value, player p returns nothing
native LeaderboardGetItemCount takes leaderboard lb returns integer
native LeaderboardHasPlayerItem takes leaderboard lb, player p returns boolean
native LeaderboardGetPlayerIndex takes leaderboard lb, player p returns integer
native LeaderboardRemovePlayerItem takes leaderboard lb, player p returns nothing

native AddLightning takes string lightningType, boolean checkVisibility, real x1, real y1, real x2, real y2 returns lightning
native DestroyLightning takes lightning l returns boolean
native MoveLightning takes lightning l, boolean checkVisibility, real x1, real y1, real x2, real y2 returns boolean
native SetLightningColor takes lightning l, real r, real g, real b, real a returns boolean
native GetLightningColorR takes lightning l returns real
native GetLightningColorB takes lightning l returns real

native SetAllyColorFilterState takes integer state returns nothing
native GetAllyColorFilterState takes nothing returns integer
native FogEnable takes boolean enable returns nothing
native IsFogEnabled takes nothing returns boolean
native SetTerrainFog takes real a, real b, real c, real d, real e returns nothing
native EnableUserControl takes boolean b returns nothing
native DisplayTextToPlayer takes player p, real x, real y, string message returns nothing
native DisplayTimedTextToPlayer takes player p, real x, real y, real duration, string message returns nothing
native ClearTextMessages takes nothing returns nothing
native StopMusic takes boolean fadeOut returns nothing
native PlayMusic takes string musicName returns nothing
"#;

fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm")
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_integer())
        .unwrap_or_else(|| panic!("global {name} is not an integer"))
}

fn real(host: &ScriptHost, name: &str) -> f32 {
    match host.vm().global(name) {
        Some(Value::Real(r)) => r.to_f32(),
        other => panic!("global {name} is {other:?}, not a real"),
    }
}

fn boolean(host: &ScriptHost, name: &str) -> bool {
    host.vm()
        .global(name)
        .and_then(|v| v.as_boolean())
        .unwrap_or_else(|| panic!("global {name} is not a boolean"))
}

fn run(host: &mut ScriptHost) {
    host.start("Main", &[]).unwrap();
    for _ in 0..4 {
        host.step();
    }
}

#[test]
fn created_handles_are_reused_lifo_across_timers_sounds_and_effects() {
    let src = r#"
globals
    integer gTimer1 = 0
    integer gSound = 0
    integer gEffect = 0
    integer gEffect2 = 0
    integer gTimer2 = 0
    integer gTimer3 = 0
endglobals

function Main takes nothing returns nothing
    local timer t1 = CreateTimer()
    local sound s = CreateSound("a.wav", false, false, false, 10, 10, "")
    local effect e = AddSpecialEffect("x.mdl", 1.0, 2.0)
    local effect e2
    local timer t2
    local timer t3
    set gTimer1 = GetHandleId(t1)
    set gSound = GetHandleId(s)
    set gEffect = GetHandleId(e)
    call DestroyTimer(t1)
    call KillSoundWhenDone(s)
    set e2 = AddSpecialEffect("y.mdl", 0.0, 0.0)
    set gEffect2 = GetHandleId(e2)
    set t2 = CreateTimer()
    set gTimer2 = GetHandleId(t2)
    set t3 = CreateTimer()
    set gTimer3 = GetHandleId(t3)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    // Fresh ids start at 0x100001. The timer is released first, then the idle sound (both are
    // freed at once), so the next allocations take the most recently freed id first.
    assert_eq!(int(&host, "gTimer1"), 0x100001);
    assert_eq!(int(&host, "gSound"), 0x100002);
    assert_eq!(int(&host, "gEffect"), 0x100003);
    assert_eq!(int(&host, "gEffect2"), 0x100002);
    assert_eq!(int(&host, "gTimer2"), 0x100001);
    assert_eq!(int(&host, "gTimer3"), 0x100004);
}

#[test]
fn texttag_values_are_pool_slots_with_lifo_reuse_and_do_not_use_handle_ids() {
    let src = r#"
globals
    texttag gA = null
    texttag gB = null
    texttag gC = null
    texttag gD = null
    integer gTimerId = 0
endglobals

function Main takes nothing returns nothing
    local timer t
    set gA = CreateTextTag()
    set gB = CreateTextTag()
    set gC = CreateTextTag()
    call DestroyTextTag(gB)
    set gD = CreateTextTag()
    set t = CreateTimer()
    set gTimerId = GetHandleId(t)
    call SetTextTagText(gD, "hello", 0.02)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    // The first texttag is slot 1 (0 is null); the freed slot 2 comes back first. The timer is
    // not affected by texttag slots and takes the first handle id.
    assert_eq!(int(&host, "gTimerId"), 0x100001);
    let game = host.game();
    let events = game.presentation().log.events();
    let created: Vec<u32> = events
        .iter()
        .filter_map(|e| match e {
            PresentationEvent::Created {
                kind: PresKind::TextTag,
                handle,
            } => Some(*handle),
            _ => None,
        })
        .collect();
    assert_eq!(created, vec![1, 2, 3, 2]);
    assert!(events.iter().any(|e| matches!(
        e,
        PresentationEvent::Destroyed {
            kind: PresKind::TextTag,
            handle: 2
        }
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        PresentationEvent::Set { kind: PresKind::TextTag, handle: 2, prop: "text", value: PropValue::Text(t) }
            if t == "hello"
    )));
}

#[test]
fn camera_getters_return_what_the_script_set() {
    let src = r#"
globals
    real gX = 0.0
    real gY = 0.0
    real gZ = 0.0
    real gDist = 0.0
    real gDistAdjusted = 0.0
    real gFarz = 0.0
    real gBoundMinX = 0.0
    real gBoundMaxY = 0.0
    real gSetupFarz = 0.0
    real gSetupDestX = 0.0
    real gLocX = 0.0
    real gTargetAfterApply = 0.0
    real gFarzAfterApply = 0.0
endglobals

function Main takes nothing returns nothing
    local camerasetup cs = CreateCameraSetup()
    call SetCameraPosition(100.0, 200.0)
    call PanCameraToTimedWithZ(300.0, 400.0, 50.0, 2.0)
    set gX = GetCameraTargetPositionX()
    set gY = GetCameraTargetPositionY()
    set gZ = GetCameraTargetPositionZ()
    call SetCameraField(ConvertCameraField(0), 900.0, 0.0)
    call AdjustCameraField(ConvertCameraField(0), 100.0, 0.0)
    set gDist = GetCameraField(ConvertCameraField(0))
    call SetCameraBounds(-10.0, -20.0, 30.0, -20.0, 30.0, 40.0, -10.0, 40.0)
    set gBoundMinX = GetCameraBoundMinX()
    set gBoundMaxY = GetCameraBoundMaxY()
    call CameraSetupSetField(cs, ConvertCameraField(1), 6000.0, 0.0)
    call CameraSetupSetDestPosition(cs, 11.0, 22.0, 0.0)
    set gSetupFarz = CameraSetupGetField(cs, ConvertCameraField(1))
    set gSetupDestX = CameraSetupGetDestPositionX(cs)
    set gLocX = GetLocationX(CameraSetupGetDestPositionLoc(cs))
    call CameraSetupApply(cs, true, false)
    set gTargetAfterApply = GetCameraTargetPositionX()
    set gFarzAfterApply = GetCameraField(ConvertCameraField(1))
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    // The pan (after the position) sets the target to its destination, height included.
    assert_eq!(real(&host, "gX"), 300.0);
    assert_eq!(real(&host, "gY"), 400.0);
    assert_eq!(real(&host, "gZ"), 50.0);
    assert_eq!(real(&host, "gDist"), 1000.0);
    assert_eq!(real(&host, "gBoundMinX"), -10.0);
    assert_eq!(real(&host, "gBoundMaxY"), 40.0);
    assert_eq!(real(&host, "gSetupFarz"), 6000.0);
    assert_eq!(real(&host, "gSetupDestX"), 11.0);
    assert_eq!(real(&host, "gLocX"), 11.0);
    assert_eq!(real(&host, "gTargetAfterApply"), 11.0);
    assert_eq!(real(&host, "gFarzAfterApply"), 6000.0);
}

#[test]
fn sound_state_round_trips_and_a_finished_sound_frees_its_handle() {
    let src = r#"
globals
    sound gS = null
    integer gId = 0
    integer gDuration = 0
    boolean gPlayingAfterStart = false
    boolean gPlayingAfterStop = true
    integer gFileDuration = 7
endglobals

function Main takes nothing returns nothing
    set gS = CreateSound("music.wav", false, false, false, 0, 0, "")
    set gId = GetHandleId(gS)
    call SetSoundDuration(gS, 1500)
    set gDuration = GetSoundDuration(gS)
    call StartSound(gS)
    set gPlayingAfterStart = GetSoundIsPlaying(gS)
    call StopSound(gS, false, false)
    set gPlayingAfterStop = GetSoundIsPlaying(gS)
    set gFileDuration = GetSoundFileDuration("unknown.wav")
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(int(&host, "gId"), 0x100001);
    assert_eq!(int(&host, "gDuration"), 1500);
    assert!(boolean(&host, "gPlayingAfterStart"));
    assert!(!boolean(&host, "gPlayingAfterStop"));
    // Unknown file lengths read as 0 (the host fills them in).
    assert_eq!(int(&host, "gFileDuration"), 0);

    // A sound that is playing when it is killed keeps its handle until the host reports it ended.
    let src2 = r#"
globals
    sound gS = null
endglobals

function Main takes nothing returns nothing
    set gS = CreateSound("loop.wav", true, false, false, 0, 0, "")
    call StartSound(gS)
    call KillSoundWhenDone(gS)
endfunction
"#;
    let mut host2 = host_for(src2);
    run(&mut host2);
    let id = {
        let game = host2.game();
        let sounds: Vec<u32> = game
            .presentation()
            .log
            .events()
            .iter()
            .filter_map(|e| match e {
                PresentationEvent::Created {
                    kind: PresKind::Sound,
                    handle,
                } => Some(*handle),
                _ => None,
            })
            .collect();
        assert_eq!(sounds, vec![0x100001]);
        sounds[0]
    };
    assert!(
        host2.game().handle_object(id).is_some(),
        "still alive while it plays"
    );
    assert!(host2.game_mut().finish_sound(id));
    assert!(
        host2.game().handle_object(id).is_none(),
        "released when it ends"
    );
    let game = host2.game();
    let events = game.presentation().log.events();
    assert!(events
        .iter()
        .any(|e| matches!(e, PresentationEvent::SoundStarted { handle } if *handle == id)));
    assert!(events
        .iter()
        .any(|e| matches!(e, PresentationEvent::SoundFinished { handle } if *handle == id)));
}

#[test]
fn multiboard_and_leaderboard_keep_their_counts_items_and_titles() {
    let src = r#"
globals
    integer gRows = 0
    integer gCols = 0
    boolean gTitleOk = false
    boolean gSameItem = false
    integer gItemCount = 0
    integer gIndexP1 = 0
    boolean gHasP0 = true
    integer gItemCountAfterRemove = 0
endglobals

function Main takes nothing returns nothing
    local multiboard mb = CreateMultiboard()
    local multiboarditem a
    local multiboarditem b
    local leaderboard lb = CreateLeaderboard()
    call MultiboardSetRowCount(mb, 3)
    call MultiboardSetColumnCount(mb, 2)
    set gRows = MultiboardGetRowCount(mb)
    set gCols = MultiboardGetColumnCount(mb)
    call MultiboardSetTitleText(mb, "Scores")
    set gTitleOk = MultiboardGetTitleText(mb) == "Scores"
    set a = MultiboardGetItem(mb, 1, 1)
    set b = MultiboardGetItem(mb, 1, 1)
    set gSameItem = GetHandleId(a) == GetHandleId(b)
    call MultiboardReleaseItem(a)
    call LeaderboardAddItem(lb, "Gold", 5, Player(0))
    call LeaderboardAddItem(lb, "Lumber", 7, Player(1))
    call LeaderboardAddItem(lb, "Gold again", 9, Player(0))
    set gItemCount = LeaderboardGetItemCount(lb)
    set gIndexP1 = LeaderboardGetPlayerIndex(lb, Player(1))
    call LeaderboardRemovePlayerItem(lb, Player(0))
    set gHasP0 = LeaderboardHasPlayerItem(lb, Player(0))
    set gItemCountAfterRemove = LeaderboardGetItemCount(lb)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(int(&host, "gRows"), 3);
    assert_eq!(int(&host, "gCols"), 2);
    assert!(boolean(&host, "gTitleOk"));
    assert!(boolean(&host, "gSameItem"));
    // Player 0 had an item and got a second one: the item is replaced, so there are 2 items.
    assert_eq!(int(&host, "gItemCount"), 2);
    assert_eq!(int(&host, "gIndexP1"), 1);
    assert!(!boolean(&host, "gHasP0"));
    assert_eq!(int(&host, "gItemCountAfterRemove"), 1);
}

#[test]
fn lightning_color_is_stored_as_bytes_and_destroy_reports_once() {
    let src = r#"
globals
    lightning gL = null
    real gR = 0.0
    real gB = 0.0
    boolean gMoved = false
    boolean gSet = false
    boolean gFirstDestroy = false
    boolean gSecondDestroy = true
    real gRAfterFree = 1.0
endglobals

function Main takes nothing returns nothing
    set gL = AddLightning("CLSB", false, 1.0, 2.0, 3.0, 4.0)
    set gSet = SetLightningColor(gL, 1.0, 0.0, 0.5, 1.0)
    set gR = GetLightningColorR(gL)
    set gB = GetLightningColorB(gL)
    set gMoved = MoveLightning(gL, false, 5.0, 6.0, 7.0, 8.0)
    set gFirstDestroy = DestroyLightning(gL)
    set gSecondDestroy = DestroyLightning(gL)
    set gRAfterFree = GetLightningColorR(gL)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert!(boolean(&host, "gSet"));
    assert_eq!(real(&host, "gR"), 1.0);
    // 0.5 * 255 rounds to 128 (not 127), so the stored byte reads back as 128 / 255.
    assert_eq!(real(&host, "gB"), 128.0 / 255.0);
    assert!(boolean(&host, "gMoved"));
    assert!(boolean(&host, "gFirstDestroy"));
    assert!(!boolean(&host, "gSecondDestroy"));
    assert_eq!(real(&host, "gRAfterFree"), 0.0);
}

#[test]
fn spell_targets_return_null_and_null_locations_give_no_effect() {
    let src = r#"
globals
    integer gTargetId = 1
    integer gLocEffectId = 1
endglobals

function Main takes nothing returns nothing
    local effect e = AddSpellEffectTarget("Abil", null, Player(0), "origin")
    local effect f = AddSpecialEffectLoc("x.mdl", null)
    set gTargetId = GetHandleId(e)
    set gLocEffectId = GetHandleId(f)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(int(&host, "gTargetId"), 0);
    assert_eq!(int(&host, "gLocEffectId"), 0);
}

#[test]
fn switches_and_text_are_logged_in_order_and_fog_terrain_is_a_no_op() {
    let src = r#"
globals
    boolean gFogBefore = false
    boolean gFogAfter = true
    integer gFilter = 0
endglobals

function Main takes nothing returns nothing
    set gFogBefore = IsFogEnabled()
    call FogEnable(false)
    set gFogAfter = IsFogEnabled()
    call SetAllyColorFilterState(3)
    set gFilter = GetAllyColorFilterState()
    call SetTerrainFog(1.0, 2.0, 3.0, 4.0, 5.0)
    call EnableUserControl(false)
    call DisplayTextToPlayer(Player(0), 0.1, 0.2, "hi")
    call DisplayTimedTextToPlayer(Player(1), 0.0, 0.0, 5.0, "later")
    call ClearTextMessages()
    call PlayMusic("theme.mp3")
    call StopMusic(true)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert!(boolean(&host, "gFogBefore"));
    assert!(!boolean(&host, "gFogAfter"));
    assert_eq!(int(&host, "gFilter"), 3);

    let mut game = host.game_mut();
    let events = game.presentation_mut().log.drain();
    assert!(game.presentation().log.is_empty(), "drain empties the log");
    // SetTerrainFog does nothing, so it adds no event; the rest are in call order.
    let names: Vec<&'static str> = events
        .iter()
        .map(|e| match e {
            PresentationEvent::Command { name, .. } => *name,
            PresentationEvent::Text { .. } => "Text",
            PresentationEvent::TextMessagesCleared => "Clear",
            PresentationEvent::MusicPlayed { .. } => "Play",
            PresentationEvent::MusicStopped { .. } => "Stop",
            _ => "other",
        })
        .collect();
    assert_eq!(
        names,
        vec![
            "FogEnable",
            "AllyColorFilterState",
            "UserControl",
            "Text",
            "Text",
            "Clear",
            "Play",
            "Stop"
        ]
    );
    assert_eq!(
        events[3],
        PresentationEvent::Text {
            player: 0,
            x: 0.1,
            y: 0.2,
            duration: None,
            text: "hi".to_string(),
        }
    );
    assert_eq!(
        events[4],
        PresentationEvent::Text {
            player: 1,
            x: 0.0,
            y: 0.0,
            duration: Some(5.0),
            text: "later".to_string(),
        }
    );
}

#[test]
fn spawned_effects_log_their_placement() {
    let src = r#"
globals
    integer gId = 0
endglobals

function Main takes nothing returns nothing
    local effect e = AddSpecialEffect("Heal.mdl", 10.0, -4.0)
    set gId = GetHandleId(e)
    call DestroyEffect(e)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(int(&host, "gId"), 0x100001);
    let game = host.game();
    let spawned = game
        .presentation()
        .log
        .events()
        .iter()
        .find_map(|e| match e {
            PresentationEvent::EffectSpawned { handle, model, at } => {
                Some((*handle, model.clone(), at.clone()))
            }
            _ => None,
        })
        .expect("an effect was spawned");
    assert_eq!(spawned.0, 0x100001);
    assert_eq!(spawned.1, "Heal.mdl");
    assert_eq!(spawned.2, EffectAt::Point { x: 10.0, y: -4.0 });
    assert!(game.presentation().log.events().iter().any(|e| matches!(
        e,
        PresentationEvent::Destroyed {
            kind: PresKind::Effect,
            handle: 0x100001
        }
    )));
}
