//! The shop stock list against states recorded from the original routines (`tools/emu/shop_stock_emu.py`,
//! `docs/spec/shop-stock.md`). The lines hold synthetic list states and the states after the real add / remove /
//! set-slots routines ran on them; no game bytes are involved.

use sim::shop::{Slot, StockList, TypeStock, SLOTS};
use softfloat::F32;

const VECTORS: &str = include_str!("shop_stock_vectors.txt");

fn nums(s: &str) -> Vec<i64> {
    s.split_whitespace().map(|t| t.parse().unwrap()).collect()
}

fn state(s: &str) -> StockList {
    let v = nums(s);
    assert_eq!(v.len(), 1 + 5 * SLOTS);
    let mut l = StockList { cap: v[0] as u32, ..StockList::default() };
    for i in 0..SLOTS {
        l.slots[i] = Slot {
            id: v[1 + i] as u32,
            depleted: v[1 + SLOTS + i] & 1 != 0,
            age: v[1 + 2 * SLOTS + i] as u32,
            value: F32(v[1 + 3 * SLOTS + i] as u32),
            max: F32(v[1 + 4 * SLOTS + i] as u32),
            restock: None,
        };
    }
    l
}

#[test]
fn matches_the_original_routines() {
    let (mut adds, mut removes, mut slots) = (0, 0, 0);
    for (n, line) in VECTORS.lines().enumerate() {
        let parts: Vec<&str> = line.split('|').collect();
        let head: Vec<&str> = parts[0].split_whitespace().collect();
        let mut l = state(parts[1]);
        let want = state(parts[2]);
        let tail = nums(parts[3]);
        let mut starts: Vec<(u32, usize)> = Vec::new();
        match head[0] {
            "add" => {
                let id: u32 = head[2].parse().unwrap();
                let cur: i32 = head[3].parse().unwrap();
                let mx: i32 = head[4].parse().unwrap();
                let dmax: i32 = head[5].parse().unwrap();
                let regen: u32 = head[6].parse().unwrap();
                let slot = l.add(id, cur, mx, TypeStock { stock_max: dmax, regen: F32(regen) });
                if let Some(r) = l.slots[slot].restock {
                    starts.push((r.0, slot));
                }
                adds += 1;
            }
            "remove" => {
                l.remove(head[2].parse().unwrap());
                removes += 1;
            }
            _ => {
                l.set_slots(head[2].parse::<i64>().unwrap() as i32);
                slots += 1;
            }
        }
        for s in l.slots.iter_mut() {
            s.restock = None;
        }
        assert_eq!(l, want, "line {n}: {line}");
        let n_starts = tail[1] as usize;
        assert_eq!(starts.len(), n_starts, "line {n}");
        for (i, (bits, slot)) in starts.iter().enumerate() {
            assert_eq!(*bits as i64, tail[2 + 2 * i], "line {n}");
            assert_eq!(*slot as i64, tail[3 + 2 * i], "line {n}");
        }
    }
    assert!(adds >= 1000 && removes >= 1000 && slots >= 1000, "{adds} {removes} {slots}");
}
