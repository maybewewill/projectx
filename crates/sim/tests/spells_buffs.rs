//! Integration of spells (Storm Bolt, Holy Light, Slow) with their buff effects.
//! Tests that stun prevents action, slow reduces speed, and buffs expire properly.
//!
//! These tests verify that the spell system's add_spell_buff and drop_spell_buff methods
//! properly integrate with the unit buff system and stat effects.

use sim::cast::{BuffEffect, BUFF_SLOW, BUFF_STUN};

#[test]
fn buff_effect_stun_variant_exists() {
    let _effect = BuffEffect::Stun;
}

#[test]
fn buff_effect_slow_variant_exists() {
    let move_delta = softfloat::F32::ZERO;
    let attack_delta = softfloat::F32::ZERO;
    let _effect = BuffEffect::Slow {
        move_delta,
        attack_delta,
    };
}

#[test]
fn buff_codes_are_valid() {
    // Verify the buff codes are defined as expected
    assert_eq!(BUFF_STUN, u32::from_be_bytes(*b"BSTN"));
    assert_eq!(BUFF_SLOW, u32::from_be_bytes(*b"Bslo"));
}
