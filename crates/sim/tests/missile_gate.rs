//! The missile gates of `sim::missiles` against the Unicorn run of the real slot 47 and slot 45 code of Game.dll 1.26a
//! (`tools/emu/missile_gate_emu.py`: 1,500 runs per slot, 0 mismatches against its Python model). The vectors are
//! embedded, so this test needs no `GAME_DLL`.

#[path = "missile/gate_data.rs"]
mod gate_data;

use sim::missiles::{arrival, impact_check, Arrival, GateInputs, ImpactCheck};

#[test]
fn arrival_and_impact_gates_match_the_original() {
    let mut slot47 = 0usize;
    let mut slot45 = 0usize;
    let mut damage = 0usize;
    let mut blocked = 0usize;
    let mut launched = 0usize;
    for &(
        slot,
        flags,
        bound,
        resolve,
        kok,
        src,
        can,
        pred,
        dmg,
        dmg_flag,
        rec42,
        rec47,
        launch,
        flags_after,
    ) in gate_data::MISSILE_GATE_CASES
    {
        let present = bound && resolve;
        let g = GateInputs {
            flags,
            target_present: present,
            kind_ok: kok,
            source: src,
            can_target: can,
            predicate: pred,
        };
        match slot {
            47 => {
                slot47 += 1;
                assert_eq!(rec42, 1, "slot 47 always ends in slot 42 (flags {flags:#x})");
                assert_eq!(rec47, 0, "slot 47 never re-enters slot 47");
                assert_eq!(launch, 0, "slot 47 never launches");
                match arrival(g) {
                    Arrival::Damage { class_flag } => {
                        damage += 1;
                        assert_eq!(dmg, 1, "flags {flags:#x} present {present}");
                        assert_eq!(class_flag, dmg_flag == 1, "class flag of {flags:#x}");
                        assert_eq!(flags_after, flags, "slot 47 leaves the flags alone");
                    }
                    Arrival::Blocked => {
                        blocked += 1;
                        assert_eq!(dmg, 0, "blocked arrival applies no damage");
                        assert!(present, "a blocked arrival has a target");
                    }
                    Arrival::NoTarget => {
                        assert_eq!(dmg, 0, "no target, no damage");
                        assert!(!present, "NoTarget only without a target");
                    }
                }
            }
            45 => {
                slot45 += 1;
                assert_eq!(dmg, 0, "slot 45 never applies damage");
                assert_eq!(rec42, 0, "slot 45 does not call slot 42");
                match impact_check(g) {
                    ImpactCheck::Launched { flags: after } => {
                        launched += 1;
                        assert_eq!(launch, 1, "launch flag of {flags:#x}");
                        assert_eq!(after, flags_after, "flags after the launch of {flags:#x}");
                        assert_eq!(rec47, 0, "a launch does not hand over to slot 47");
                    }
                    ImpactCheck::Arrive => {
                        assert_eq!(launch, 0, "no launch without the gate ({flags:#x})");
                        assert_eq!(rec47, 1, "the check hands over to slot 47");
                        assert_eq!(flags_after, flags, "no flag change without a launch");
                    }
                }
            }
            other => panic!("unknown slot {other}"),
        }
    }
    assert_eq!(slot47, 1500, "slot 47 cases");
    assert_eq!(slot45, 1500, "slot 45 cases");
    // Every branch of the gates is exercised by the vectors.
    assert!(damage > 0 && blocked > 0, "damage {damage}, blocked {blocked}");
    assert!(launched > 0, "launch cases {launched}");
}
