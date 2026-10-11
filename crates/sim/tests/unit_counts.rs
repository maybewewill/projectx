//! Player unit counters (`unit_counts.rs`, `natives/player.rs`, the registration in `natives/unit.rs`).
//!
//! `vectors/unit_counts_seed1.tsv` is written by `tools/emu/unit_counts_emu.py` from the original Game.dll
//! (Unicorn): 400 random counter operations and 800 typed walks, each with the four totals after the
//! operation. The vector replay needs no DLL. The native tests run JASS scripts through `ScriptHost`.

use std::collections::BTreeMap;

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::UnitType;
use sim::unit_counts::{PlayerCounts, TypeLinks, CLASS_OTHER, CLASS_STRUCTURE, CLASS_UNIT};
use sim::ScriptHost;

/// Raw codes of the synthetic types.
const FOOTMAN: u32 = 0x6866_6F6F; // 'hfoo'
const MAGE: u32 = 0x6868_6D61; // 'hhma'
const TOWER: u32 = 0x6574_6F31; // 'eto1' (a structure)

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type unitcode extends handle

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native SetUnitOwner takes unit whichUnit, player whichPlayer, boolean changeColor returns nothing
native RemoveUnit takes unit whichUnit returns nothing
native GetPlayerUnitCount takes player whichPlayer, boolean includeIncomplete returns integer
native GetPlayerStructureCount takes player whichPlayer, boolean includeIncomplete returns integer
native GetPlayerTypedUnitCount takes player whichPlayer, string unitName, boolean includeIncomplete, boolean includeUpgrades returns integer
"#;

fn unit_type(name: &str) -> UnitType {
    UnitType {
        name: name.to_string(),
        ..Default::default()
    }
}

/// A host with the synthetic types: footman (unit), mage (unit, child of footman's list), tower (structure).
fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse_with(&full, None).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        let types = g.unit_types_mut();
        types.insert(FOOTMAN, unit_type("Footman"));
        types.insert(MAGE, unit_type("Mage"));
        types.insert(TOWER, unit_type("Tower1"));
        types.set_building(TOWER, true);
        // Acyclic on purpose: the walk recurses through both lists without a visited set.
        types.set_count_links(FOOTMAN, vec![MAGE], vec![]);
        types.set_count_links(MAGE, vec![], vec![TOWER]);
    }
    host
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_integer())
        .unwrap_or_else(|| panic!("global {name} is not an integer"))
}

fn run(host: &mut ScriptHost) {
    host.start("Main", &[]).unwrap();
    host.step();
}

// --- Vector replay -------------------------------------------------------------------------------

/// The model of the vector script: types 1..6 with these classes and links (see the emulator script).
fn vector_classes() -> BTreeMap<u32, u32> {
    [(1, CLASS_UNIT), (2, CLASS_UNIT), (3, CLASS_STRUCTURE), (4, CLASS_OTHER), (5, CLASS_UNIT), (6, CLASS_STRUCTURE)]
        .into_iter()
        .collect()
}

#[derive(Default)]
struct VectorLinks {
    children: BTreeMap<u32, Vec<u32>>,
    upgrades: BTreeMap<u32, Vec<u32>>,
}

impl TypeLinks for VectorLinks {
    fn children(&self, t: u32) -> &[u32] {
        self.children.get(&t).map(Vec::as_slice).unwrap_or(&[])
    }
    fn upgrades(&self, t: u32) -> &[u32] {
        self.upgrades.get(&t).map(Vec::as_slice).unwrap_or(&[])
    }
}

fn vector_links() -> VectorLinks {
    let mut l = VectorLinks::default();
    l.children.insert(1, vec![2, 3]);
    l.children.insert(2, vec![5]);
    l.upgrades.insert(1, vec![5]);
    l.upgrades.insert(3, vec![6]);
    l.upgrades.insert(4, vec![1]);
    l.upgrades.insert(6, vec![2]);
    l
}

#[test]
fn counters_match_the_original_on_the_vector_sequences() {
    let text = include_str!("vectors/unit_counts_seed1.tsv");
    let classes = vector_classes();
    let links = vector_links();
    let mut c = PlayerCounts::new();
    let mut ops = 0;
    let mut walks = 0;
    for line in text.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        match f[0] {
            "C" => {
                let op = f[1];
                let t: u32 = f[2].parse().unwrap();
                let p3 = f[3] == "1";
                let class = classes[&t];
                match op {
                    "add" => c.add_completed(t, class, p3),
                    "remove_inprog" => c.remove_in_progress(t, class, p3),
                    "finish" => c.finish(t, class, p3),
                    "remove_completed" => c.remove_completed(t, class),
                    "begin" => c.begin_in_progress(t, class, p3),
                    other => panic!("unknown op {other}"),
                }
                let want: [i32; 4] = [
                    f[4].parse().unwrap(),
                    f[5].parse().unwrap(),
                    f[6].parse().unwrap(),
                    f[7].parse().unwrap(),
                ];
                let got = [
                    c.structures_in_progress,
                    c.structures_completed,
                    c.units_in_progress,
                    c.units_completed,
                ];
                assert_eq!(got, want, "totals after {line}");
                ops += 1;
            }
            "W" => {
                let t: u32 = f[1].parse().unwrap();
                let flags: u32 = f[2].parse().unwrap();
                let want: u32 = f[3].parse().unwrap();
                let got = c.typed_walk(&links, t, flags) as u32;
                assert_eq!(got, want, "walk type {t} flags {flags:#x}: {line}");
                walks += 1;
            }
            other => panic!("unknown line kind {other}"),
        }
    }
    assert_eq!((ops, walks), (400, 800));
}

// --- Natives ------------------------------------------------------------------------------------

#[test]
fn created_units_count_for_their_owner_and_removal_takes_them_back() {
    let src = r#"
globals
    integer gA = -1
    integer gB = -1
    integer gC = -1
    integer gD = -1
    integer gE = -1
    integer gS = -1
endglobals

function Main takes nothing returns nothing
    local unit u1 = CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
    local unit u2 = CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
    local unit t1 = CreateUnit(Player(0), 'eto1', 0.0, 0.0, 0.0)
    set gA = GetPlayerUnitCount(Player(0), false)
    set gS = GetPlayerStructureCount(Player(0), false)
    set gB = GetPlayerUnitCount(Player(1), false)
    call SetUnitOwner(u2, Player(1), true)
    set gC = GetPlayerUnitCount(Player(0), false)
    set gD = GetPlayerUnitCount(Player(1), false)
    call RemoveUnit(u1)
    set gE = GetPlayerUnitCount(Player(0), false)
endfunction
"#;
    // 'eto1' is a structure: it is in the structure pair, not the unit pair.
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(int(&host, "gA"), 2, "two footmen and no structure in the unit count");
    assert_eq!(int(&host, "gS"), 1, "one tower in the structure count");
    assert_eq!(int(&host, "gB"), 0);
    assert_eq!(int(&host, "gC"), 1, "u2 left player 0");
    assert_eq!(int(&host, "gD"), 1, "u2 joined player 1");
    assert_eq!(int(&host, "gE"), 0, "u1 removed");
}

#[test]
fn typed_count_follows_the_registry_links_and_the_flags() {
    let src = r#"
globals
    integer gFoot = -1
    integer gFootUp = -1
    integer gMage = -1
    integer gByName = -1
    integer gNone = -1
endglobals

function Main takes nothing returns nothing
    local unit f1 = CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
    local unit f2 = CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
    local unit m1 = CreateUnit(Player(0), 'hhma', 0.0, 0.0, 0.0)
    set gFoot = GetPlayerTypedUnitCount(Player(0), "hfoo", false, false)
    set gFootUp = GetPlayerTypedUnitCount(Player(0), "hfoo", false, true)
    set gMage = GetPlayerTypedUnitCount(Player(0), "hhma", false, false)
    set gByName = GetPlayerTypedUnitCount(Player(0), "Footman", false, false)
    set gNone = GetPlayerTypedUnitCount(Player(0), "zzzz", false, false)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    // Footman's first list holds the mage, so the mage is counted with the footmen (the list is always walked).
    assert_eq!(int(&host, "gFoot"), 3);
    // Footman has no upgrade list in this registry, so the upgrades flag adds nothing.
    assert_eq!(int(&host, "gFootUp"), 3);
    // Mage's upgrade list (the tower) is walked only with the upgrades flag, and it is not set here: 1 mage.
    assert_eq!(int(&host, "gMage"), 1);
    assert_eq!(int(&host, "gByName"), 3, "a name resolves to the same type as the raw code");
    assert_eq!(int(&host, "gNone"), 0, "an unknown type counts 0");
}

#[test]
fn an_unknown_player_or_missing_type_counts_zero() {
    let src = r#"
globals
    integer gA = -1
endglobals

function Main takes nothing returns nothing
    set gA = GetPlayerUnitCount(null, false) + GetPlayerStructureCount(null, true)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(int(&host, "gA"), 0);
}

#[test]
fn an_unregistered_type_creates_no_count() {
    let src = r#"
globals
    integer gA = -1
endglobals

function Main takes nothing returns nothing
    local unit u = CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
    set gA = GetPlayerUnitCount(Player(0), false)
endfunction
"#;
    // The registry is empty here, so the creation of the unit fails (no type). The count stays 0.
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse_with(&full, None).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    run(&mut host);
    assert_eq!(int(&host, "gA"), 0);
    assert!(host.game_mut().unit_types().get(FOOTMAN).is_none());
}
