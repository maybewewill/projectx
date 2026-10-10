//! `BucketMap` against the vectors recorded from the original `Game.dll` 1.26a hashtable map
//! (`tools/emu/hashmap_vectors.py`). Every vector is a sequence of adds and removes that the original
//! ran without growing the table; the destroy order of the survivors must match exactly.

use sim::hashmap::{BucketMap, Insert};

#[path = "hashmap/data.rs"]
mod data;

#[test]
fn recorded_destroy_orders_match_the_original() {
    assert!(!data::VECTORS.is_empty());
    for (case, (ops, expected)) in data::VECTORS.iter().enumerate() {
        let mut map = BucketMap::new();
        let mut adds: Vec<(u32, bool)> = Vec::new(); // (key, still linked) per add index
        for &(kind, value) in ops.iter() {
            if kind == 0 {
                let key = value as u32;
                let idx = adds.len() as u32;
                assert_eq!(
                    map.insert_new(key, idx),
                    Insert::Placed,
                    "case {case}: an original vector must not grow"
                );
                adds.push((key, true));
            } else {
                let idx = value as usize;
                if idx < adds.len() && adds[idx].1 {
                    let key = adds[idx].0;
                    let removed = map.remove_where(key, |&v| v == idx as u32);
                    assert_eq!(removed, Some(idx as u32), "case {case}: remove {idx}");
                    adds[idx].1 = false;
                }
            }
        }
        assert_eq!(map.destroy_order(), *expected, "case {case}: destroy order");
    }
}

#[test]
fn vectors_cover_removes_and_collisions() {
    let with_remove = data::VECTORS
        .iter()
        .filter(|(ops, _)| ops.iter().any(|&(k, _)| k == 1))
        .count();
    assert!(with_remove > 100, "only {with_remove} vectors with removes");
    let collide = data::VECTORS
        .iter()
        .filter(|(ops, _)| ops.iter().filter(|&&(k, v)| k == 0 && v % 4 == 0).count() >= 6)
        .count();
    assert!(collide > 100, "only {collide} vectors with collisions");
}
