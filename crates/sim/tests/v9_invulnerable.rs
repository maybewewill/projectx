//! The invulnerability counter and the Invulnerable ability (`Avul`) against the original (`tools/emu/v9_emu.py` check `invuln`, 400 sequences,
//! docs/spec/abilities-v9.md): `0x6F0796F0` / `0x6F079730` on a fake unit, driven through the `CAbilityInvulnerable` init slot `0x6F10FAE0` and
//! removal slot `0x6F10FB10` (`I` / `R`, notify 1) and directly (`i` / `a`, notify 0). The enter / leave notices and the two property refreshes are
//! counting stubs.

use sim::abilities::{invuln_add, invuln_remove};

#[test]
fn invulnerability_counter_matches_the_original() {
    // start count, start flags, ops | count, flags | enter notices, leave notices, refresh 0x6F26FE20, refresh 0x6F26FE10
    let (mut n, mut enters, mut leaves, mut negative, mut unbalanced) = (0, 0, 0, 0, 0);
    for line in include_str!("data/v9/invuln.txt").lines().filter(|l| !l.trim().is_empty()) {
        let w: Vec<&str> = line.split_whitespace().collect();
        let h = |i: usize| u32::from_str_radix(w[i], 16).unwrap();
        let mut state = (h(0) as i32, h(1));
        let (mut en, mut le, mut refreshed) = (0u32, 0u32, 0u32);
        for op in w[2].chars() {
            let (c, f, notice, refresh) = match op {
                'I' => invuln_add(state, true),
                'i' => invuln_add(state, false),
                'R' => invuln_remove(state, true),
                _ => invuln_remove(state, false),
            };
            if op == 'I' || op == 'i' {
                en += u32::from(notice);
            } else {
                le += u32::from(notice);
            }
            refreshed += u32::from(refresh);
            state = (c, f);
        }
        assert_eq!((h(3), h(4)), (state.0 as u32, state.1), "counter and flags, row {line}");
        assert_eq!((h(5), h(6)), (en, le), "notices, row {line}");
        assert_eq!((h(7), h(8)), (refreshed, refreshed), "refreshes, row {line}");
        enters += en;
        leaves += le;
        negative += u32::from(state.0 < 0);
        unbalanced += u32::from(h(0) as i32 <= 0 && w[2].starts_with(['R', 'a']));
        n += 1;
    }
    assert!(n >= 400 && enters > 60 && leaves > 60 && negative > 20 && unbalanced > 10, "{n} {enters} {leaves} {negative} {unbalanced}");
}
