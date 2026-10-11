//! Training queue of `CAbilityTrain` (Game.dll 1.26a). Spec: docs/spec/economy-4.md.
//!
//! The shift is checked against vectors recorded from the original in Unicorn
//! (`tools/emu/train_queue_emu.py`); the rest checks the model's own rules by hand. The vectors are embedded,
//! so this test needs no `GAME_DLL`.

#[path = "economy/train_vectors.rs"]
mod train_vectors;

use sim::economy::{
    shift_out, PlayerEconomy, TrainError, TrainItem, TrainOutcome, TrainQueue, TrainTimer, TRAIN_QUEUE_CAPACITY,
};
use sim::timer::TimerQueue;
use softfloat::F32;

fn unit(type_id: u32, gold: i32, lumber: i32, food: i32, secs: f32) -> TrainItem {
    TrainItem { type_id, gold, lumber, food, food_exempt: false, build_seconds: secs }
}

fn rich(index: usize) -> PlayerEconomy {
    let mut p = PlayerEconomy::new(index);
    p.stock = [10_000 * 10, 10_000 * 10];
    p
}

#[test]
fn shift_matches_the_original_in_unicorn() {
    let mut bad = Vec::new();
    for &(count, index, slots, mask, count_after, slots_after, mask_after) in train_vectors::TRAIN_SHIFT_VECTORS {
        let mut s = slots;
        let mut m = mask;
        shift_out(&mut s, count as usize, &mut m, index as usize);
        // The original leaves the count alone; the vectors record that too.
        assert_eq!(count, count_after);
        if s != slots_after || m != mask_after {
            bad.push((count, index));
        }
    }
    assert!(bad.is_empty(), "{} mismatches, first {:?}", bad.len(), bad.first());
    assert!(train_vectors::TRAIN_SHIFT_VECTORS.len() >= 3000);
}

#[test]
fn shift_moves_slots_and_mask_bits_down() {
    let mut s = [1u32, 2, 3, 4, 5, 6, 7];
    let mut m = 0b1010101u32; // bits 0,2,4,6 set
    shift_out(&mut s, 5, &mut m, 1);
    // slots 2,3,4 move down over index 1; slot 4 (count-1) is cleared
    assert_eq!(s, [1, 3, 4, 5, 0, 6, 7]);
    // bit1<-bit2 (1), bit2<-bit3 (0), bit3<-bit4 (1), then bit4 (count-1) is cleared
    assert_eq!(m, 0b1001011);
}

#[test]
fn enqueue_refuses_the_eighth_unit() {
    let mut q = TrainQueue::new();
    let mut players = vec![rich(0)];
    for i in 0..TRAIN_QUEUE_CAPACITY {
        q.enqueue(&mut players, 0, unit(100 + i as u32, 10, 0, 1, 5.0), 0, 100).unwrap();
    }
    assert_eq!(q.len(), 7);
    assert_eq!(
        q.enqueue(&mut players, 0, unit(999, 10, 0, 1, 5.0), 0, 100),
        Err(TrainError::QueueFull)
    );
    assert_eq!(q.len(), 7);
}

#[test]
fn enqueue_checks_food_before_paying() {
    let mut q = TrainQueue::new();
    let mut players = vec![rich(0)];
    let gold_before = players[0].stock[0];
    // used 95 + need 10 > cap 100
    assert_eq!(q.enqueue(&mut players, 0, unit(1, 400, 0, 10, 5.0), 95, 100), Err(TrainError::FoodBlocked));
    assert_eq!(players[0].stock[0], gold_before, "a refused unit costs nothing");
    // exempt units skip the food check
    let mut ex = unit(2, 400, 0, 10, 5.0);
    ex.food_exempt = true;
    assert!(q.enqueue(&mut players, 0, ex, 95, 100).is_ok());
}

#[test]
fn enqueue_pays_gold_and_lumber_and_refuses_when_short() {
    let mut q = TrainQueue::new();
    let mut players = vec![PlayerEconomy::new(0)];
    players[0].stock = [500 * 10, 100 * 10];
    q.enqueue(&mut players, 0, unit(1, 300, 50, 1, 5.0), 0, 100).unwrap();
    assert_eq!(players[0].stock, [200 * 10, 50 * 10]);
    // 300 gold left 200: refused, nothing taken
    assert_eq!(
        q.enqueue(&mut players, 0, unit(2, 300, 0, 1, 5.0), 0, 100),
        Err(TrainError::NotEnoughResources)
    );
    assert_eq!(players[0].stock, [200 * 10, 50 * 10]);
    assert_eq!(q.len(), 1);
}

#[test]
fn start_reserves_food_and_arms_the_timer() {
    let mut q = TrainQueue::new();
    let mut players = vec![rich(0)];
    let mut timers: TimerQueue<TrainTimer> = TimerQueue::new();
    let mut food = 40;
    q.enqueue(&mut players, 0, unit(7, 100, 0, 3, 12.0), food, 100).unwrap();
    assert_eq!(q.try_start(&mut timers, &mut food, 100, 0), Some(TrainOutcome::Started { type_id: 7 }));
    assert_eq!(food, 43);
    assert!(q.is_running());
    assert_eq!(timers.len(), 1);
    assert_eq!(timers.next_expiry(), Some(12.0));
    // a running head is not started twice
    assert_eq!(q.try_start(&mut timers, &mut food, 100, 0), None);
}

#[test]
fn blocked_head_waits_for_food_and_starts_on_retry() {
    let mut q = TrainQueue::new();
    let mut players = vec![rich(0)];
    let mut timers: TimerQueue<TrainTimer> = TimerQueue::new();
    let mut food = 99;
    // queued while food was free
    q.enqueue(&mut players, 0, unit(8, 100, 0, 5, 9.0), 0, 100).unwrap();
    assert_eq!(q.try_start(&mut timers, &mut food, 100, 0), Some(TrainOutcome::Blocked { type_id: 8 }));
    assert!(!q.is_running());
    assert_eq!(food, 99, "a blocked head reserves nothing");
    assert!(timers.is_empty());
    // a cap raise (or a unit dying elsewhere) makes room
    assert_eq!(q.retry(&mut timers, &mut food, 110, 0), Some(TrainOutcome::Started { type_id: 8 }));
    assert_eq!(food, 104);
    assert_eq!(timers.len(), 1);
}

#[test]
fn completion_finishes_the_head_and_starts_the_next() {
    let mut q = TrainQueue::new();
    let mut players = vec![rich(0)];
    let mut timers: TimerQueue<TrainTimer> = TimerQueue::new();
    let mut food = 0;
    q.enqueue(&mut players, 0, unit(1, 100, 0, 2, 3.0), 0, 100).unwrap();
    q.enqueue(&mut players, 0, unit(2, 100, 0, 4, 5.0), 0, 100).unwrap();
    q.try_start(&mut timers, &mut food, 100, 0);
    assert_eq!(food, 2);
    // the timer fires at 3.0 s
    let fired = timers.pop_due(3.0).expect("head timer due");
    assert_eq!(fired.payload, TrainTimer { building: 0 });
    let out = q.complete(&mut timers, &mut food, 100, 0);
    assert_eq!(out, vec![TrainOutcome::Finished { type_id: 1 }, TrainOutcome::Started { type_id: 2 }]);
    assert_eq!(food, 6, "the finished unit keeps its food, the next one reserves its own");
    assert_eq!(q.len(), 1);
    assert_eq!(timers.next_expiry(), Some(8.0));
}

#[test]
fn completion_without_a_running_head_is_ignored() {
    let mut q = TrainQueue::new();
    let mut players = vec![rich(0)];
    let mut timers: TimerQueue<TrainTimer> = TimerQueue::new();
    let mut food = 0;
    q.enqueue(&mut players, 0, unit(1, 100, 0, 1, 3.0), 0, 100).unwrap();
    assert!(q.complete(&mut timers, &mut food, 100, 0).is_empty());
    assert_eq!(q.len(), 1);
}

#[test]
fn cancel_head_refunds_gives_back_food_and_starts_next() {
    let mut q = TrainQueue::new();
    let mut players = vec![rich(0)];
    let mut timers: TimerQueue<TrainTimer> = TimerQueue::new();
    let mut food = 0;
    let before = players[0].stock;
    q.enqueue(&mut players, 0, unit(1, 400, 200, 2, 3.0), 0, 100).unwrap();
    q.enqueue(&mut players, 0, unit(2, 100, 0, 3, 4.0), 0, 100).unwrap();
    q.try_start(&mut timers, &mut food, 100, 0);
    assert_eq!(food, 2);
    let ((g, l), next) = q.cancel(&mut timers, &mut players, 0, &mut food, 100, 0, 0, F32(0x3F00_0000));
    // rate 0.5: 400 -> 200 gold, 200 -> 100 lumber (F2I(MUL(I2R(c), 0.5)))
    assert_eq!((g, l), (200, 100));
    assert_eq!(next, Some(TrainOutcome::Started { type_id: 2 }));
    assert_eq!(food, 3, "the cancelled head's food is given back, the next head reserves its own");
    // paid 400 + 100 gold and 200 lumber, refunded 200 gold and 100 lumber
    assert_eq!(players[0].stock, [before[0] - 500 * 10 + 200 * 10, before[1] - 200 * 10 + 100 * 10]);
    assert_eq!(timers.len(), 1, "only the new head's timer is left");
}

#[test]
fn cancel_middle_slot_keeps_the_head_running() {
    let mut q = TrainQueue::new();
    let mut players = vec![rich(0)];
    let mut timers: TimerQueue<TrainTimer> = TimerQueue::new();
    let mut food = 0;
    for i in 1..=3 {
        q.enqueue(&mut players, 0, unit(i, 100, 0, 1, 5.0), 0, 100).unwrap();
    }
    q.try_start(&mut timers, &mut food, 100, 0);
    let ((g, _), next) = q.cancel(&mut timers, &mut players, 0, &mut food, 100, 0, 1, F32(0x3F00_0000));
    assert_eq!(g, 50);
    assert_eq!(next, None);
    assert!(q.is_running());
    assert_eq!(q.items().iter().map(|i| i.type_id).collect::<Vec<_>>(), vec![1, 3]);
    assert_eq!(food, 1);
}

#[test]
fn refund_bit_suppresses_the_refund() {
    let mut q = TrainQueue::new();
    let mut players = vec![rich(0)];
    let mut timers: TimerQueue<TrainTimer> = TimerQueue::new();
    let mut food = 0;
    q.enqueue(&mut players, 0, unit(1, 400, 0, 1, 5.0), 0, 100).unwrap();
    q.refund_mask = 1;
    let before = players[0].stock;
    let ((g, l), _) = q.cancel(&mut timers, &mut players, 0, &mut food, 100, 0, 0, F32(0x3F00_0000));
    assert_eq!((g, l), (0, 0));
    assert_eq!(players[0].stock, before);
    assert!(q.is_empty());
}
