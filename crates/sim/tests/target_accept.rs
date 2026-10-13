//! The weapon target acceptance against the original (`tools/emu/target_accept_emu.py`): 600 Unicorn vectors for each of
//! the expansion, the predicate, the unit flag word and the two weapon choosers. No `GAME_DLL` needed.

#[path = "target_accept/target_accept_expand.rs"]
mod expand_rows;
#[path = "target_accept/target_accept_info.rs"]
mod info_rows;
#[path = "target_accept/target_accept_pred.rs"]
mod pred_rows;
#[path = "target_accept/target_accept_uflags.rs"]
mod uflags_rows;
#[path = "target_accept/target_accept_unit.rs"]
mod unit_rows;

use sim::target_accept::*;

#[test]
fn expansion_matches() {
    for r in expand_rows::ROWS {
        assert_eq!(expand(r[0]), r[1], "{:#x}", r[0]);
    }
}

#[test]
fn predicate_matches() {
    for r in pred_rows::ROWS {
        assert_eq!(predicate(r[0], r[1]), r[2], "t={:#x} w={:#x}", r[0], r[1]);
    }
}

#[test]
fn unit_flag_word_matches() {
    for r in uflags_rows::ROWS {
        // u5c, u20, u24c, u33, u248
        let got = unit_target_flags(r[0] & 0x100 != 0, r[1] & 8 != 0, r[2], r[3] as u8, r[4]);
        assert_eq!(got, r[5], "{r:x?}");
    }
}

fn slots(a20: u32, targs: [u32; 2], atk: [u32; 2]) -> [WeaponSlot; 2] {
    [
        WeaponSlot { enabled: a20 & 0x80000 != 0, targs: targs[0], atk_type: atk[0] },
        WeaponSlot { enabled: a20 & 0x100000 != 0, targs: targs[1], atk_type: atk[1] },
    ]
}

#[test]
fn info_chooser_matches() {
    for r in info_rows::ROWS {
        // t, a20, kind0, kind1, wt0, wt1, ret, out
        let c = choose_for_info(r[0], slots(r[1], [r[4], r[5]], [0, 0]));
        assert_eq!((c.code, c.index as u32), (r[6], r[7]), "{r:x?}");
    }
}

#[test]
fn unit_chooser_matches() {
    for r in unit_rows::ROWS {
        // u5c u20 u24c u33 u248 eth mim a20 kind0 kind1 wt0 wt1 atk0 atk1 gl misc ret out
        let t = unit_target_flags(r[0] & 0x100 != 0, r[1] & 8 != 0, r[2], r[3] as u8, r[4]);
        let signed_pos = |v: u32| (v as i32) > 0;
        let checks = UnitChecks {
            magic_immune: r[14] != 0 && signed_pos(r[6]),
            ethereal: r[14] != 0 && signed_pos(r[5]),
            misc_flag: r[15] != 0,
        };
        let c = choose_for_unit(t, slots(r[7], [r[10], r[11]], [r[12], r[13]]), checks);
        assert_eq!((c.code, c.index as u32), (r[16], r[17]), "{r:x?}");
    }
}
