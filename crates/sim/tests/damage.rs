//! Unit tests of the spec damage pieces (`sim::damage`, `docs/spec/attack-damage.md`). Synthetic tables only, except
//! the last test which reads `GAME_DLL` when it is set. No table contents are stored in the repository.

use sim::damage::{
    armor_factor, dice_sum, DamageTableError, DamageTypeTable, NegativeArmorTable, ARMOR_TYPE_COLS,
    ATTACK_TYPE_ROWS, MAX_DICE, NEG_ARMOR_LEN,
};
use sim::rng::{Rng, RngTable};
use softfloat::{arith, Tables, F32};

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

/// A PE32 image based at 0x6F000000 whose section starts at VA 0x6FAB4000 with `payload` as its file data.
fn image_with(payload: &[u8]) -> Vec<u8> {
    let mut file = vec![0u8; 0x400 + payload.len()];
    file[0..2].copy_from_slice(b"MZ");
    file[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    file[0x80..0x84].copy_from_slice(b"PE\0\0");
    file[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
    file[0x94..0x96].copy_from_slice(&0xE0u16.to_le_bytes());
    let opt = 0x98;
    file[opt..opt + 2].copy_from_slice(&0x010Bu16.to_le_bytes());
    file[opt + 28..opt + 32].copy_from_slice(&0x6F00_0000u32.to_le_bytes());
    let sec = opt + 0xE0;
    file[sec + 12..sec + 16].copy_from_slice(&0x00AB_4000u32.to_le_bytes());
    file[sec + 16..sec + 20].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    file[sec + 20..sec + 24].copy_from_slice(&0x400u32.to_le_bytes());
    file[0x400..].copy_from_slice(payload);
    file
}

/// An identity permutation: a valid table for the generator logic (the real table is never in the repository).
fn identity_table() -> RngTable {
    let mut bytes = [0u8; 256];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = i as u8;
    }
    RngTable::from_bytes(bytes).unwrap()
}

#[test]
fn negative_table_is_read_from_the_image_address() {
    let mut bits = [0u32; NEG_ARMOR_LEN];
    for (i, b) in bits.iter_mut().enumerate() {
        *b = f(0.5 + i as f32 * 0.01).to_bits();
    }
    let mut payload = vec![0u8; 0xAB_4300 - 0xAB_4000];
    payload.extend(bits.iter().flat_map(|b| b.to_le_bytes()));
    payload.resize(0x1000, 0);
    let table = NegativeArmorTable::from_game_dll(&image_with(&payload)).unwrap();
    assert_eq!(table.lookup(f(-3.7)), f(0.5 + 3.0 * 0.01));
    assert_eq!(table.lookup(f(-25.0)), f(0.5 + 20.0 * 0.01));
}

#[test]
fn all_zero_negative_table_is_refused() {
    let payload = vec![0u8; 0x1000];
    assert_eq!(
        NegativeArmorTable::from_game_dll(&image_with(&payload)),
        Err(DamageTableError::NotStored)
    );
}

#[test]
fn non_finite_negative_entry_is_refused() {
    let mut bits = [f(1.0).to_bits(); NEG_ARMOR_LEN];
    bits[5] = 0x7F80_0000; // +infinity
    assert_eq!(
        NegativeArmorTable::from_bits(bits),
        Err(DamageTableError::NotFinite)
    );
}

#[test]
fn image_without_the_address_is_an_image_error() {
    let file = image_with(&[1, 2, 3, 4]);
    assert!(matches!(
        NegativeArmorTable::from_game_dll(&file),
        Err(DamageTableError::Image(_))
    ));
}

#[test]
fn negative_armor_factor_uses_the_table_without_division() {
    let mut bits = [0u32; NEG_ARMOR_LEN];
    bits[3] = f(0.25).to_bits();
    bits[20] = f(0.9).to_bits();
    let table = NegativeArmorTable::from_bits(bits).unwrap();
    // The negative branch never divides, so no lookup tables are needed.
    let tables = Tables::default();
    // Armor -3.5 uses n = 3: M = 1 - 0.25.
    assert_eq!(
        armor_factor(f(-3.5), f(0.06), &table, &tables),
        arith::sub(F32::ONE, f(0.25))
    );
    // Armor -40 clamps to n = 20: M = 1 - 0.9.
    assert_eq!(
        armor_factor(f(-40.0), f(0.06), &table, &tables),
        arith::sub(F32::ONE, f(0.9))
    );
}

#[test]
fn damage_type_factor_clamps_out_of_range_indices() {
    let mut rows = [[F32::ONE; ARMOR_TYPE_COLS]; ATTACK_TYPE_ROWS];
    rows[6][7] = f(0.5);
    let t = DamageTypeTable(rows);
    assert_eq!(t.factor(6, 7), f(0.5));
    assert_eq!(t.factor(9, 9), f(0.5));
    assert_eq!(t.factor(0, 0), F32::ONE);
}

#[test]
fn dice_sum_draws_one_step_per_die_and_stays_in_range() {
    let table = identity_table();
    let mut a = Rng::seeded(0x1234_5678);
    let mut b = a;
    let sum = dice_sum(&mut a, &table, 5, 10).unwrap();
    let mut manual = 0u32;
    for _ in 0..5 {
        let r = u64::from(b.next_u32(&table));
        let v = ((r * 10) >> 32) as u32;
        assert!(v < 10);
        manual += v;
    }
    assert_eq!(sum, manual);
    assert_eq!(
        a, b,
        "both generators are in the same state after five dice"
    );
}

#[test]
fn dice_sum_zero_dice_draws_nothing_and_large_counts_are_refused() {
    let table = identity_table();
    let mut s = Rng::seeded(7);
    let before = s;
    assert_eq!(dice_sum(&mut s, &table, 0, 6), Some(0));
    assert_eq!(s, before);
    assert_eq!(dice_sum(&mut s, &table, MAX_DICE + 1, 6), None);
    assert_eq!(s, before, "a refused roll draws nothing");
}

/// With the 1.26a `Game.dll`: the negative table is read when the image has it (skipped otherwise), and the armor
/// factor of armor 10 with the stock K = 0.06 is 1/(1 + 0.6). The K is a test input, not the Misc value.
#[test]
fn game_dll_positive_armor_factor_is_sane() {
    let Some(path) = std::env::var_os("GAME_DLL") else {
        eprintln!("skipped: GAME_DLL not set");
        return;
    };
    let dll = std::fs::read(path).expect("read GAME_DLL");
    let table = match NegativeArmorTable::from_game_dll(&dll) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("skipped: {e}");
            return;
        }
    };
    let tables = Tables::from_game_dll(&dll).expect("lookup tables in GAME_DLL");
    let v = armor_factor(f(10.0), f(0.06), &table, &tables).to_f32();
    assert!(v > 0.0 && v <= 1.0, "factor {v}");
    assert!((v - 0.625).abs() < 1e-3, "1/(1+0.6) = 0.625, got {v}");
}
