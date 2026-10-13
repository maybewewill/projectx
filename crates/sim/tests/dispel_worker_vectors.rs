//! The per-unit dispel worker `0x6F0EEF00` (Dispel Magic, Purge, Abolish Magic) against the original: 1,600 random cases
//! (`tools/emu/dispel_worker_emu.py`, four seeds) in which the real worker runs in Unicorn with every callee replaced by a
//! recording stub. Each row lists the calls the original made; `sim::unit_spells::dispel_worker` must make the same calls
//! in the same order with the same arguments and return the same value. Numbers only; no game data.

use sim::unit_spells::{dispel_worker, WorkerIn, WorkerStep};

const TARGET: u32 = 0x0020_0800;

fn render(steps: &[WorkerStep]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for s in steps {
        match s {
            WorkerStep::Rel(p) => parts.push(format!("rel:{p:x}")),
            WorkerStep::FindBams => parts.push("find:42616d73:0,0,1,1".to_string()),
            WorkerStep::Slot11c => parts.push("v11c".to_string()),
            WorkerStep::Blocked(a, b) => parts.push(format!("legal:{a:x},{b:x}")),
            WorkerStep::Misc(d) => parts.push(format!("misc:Misc.InvulnSummonDispelDamage:{d:x}")),
            WorkerStep::Damage(amount, class, src, flags) => {
                parts.push(format!("rec:{amount:08x},{class:x},{src:x},{flags:x},0"));
                parts.push("apply".to_string());
            }
            WorkerStep::Buffs(a) => {
                let list: Vec<String> = a.iter().map(|x| format!("{x:x}")).collect();
                parts.push(format!("buffs:{}", list.join(",")));
            }
            WorkerStep::Mana(bits) => parts.push(format!("mana:{bits:08x},0")),
            WorkerStep::Notify(a) => parts.push(format!("notify:{a:x}:{TARGET:x}")),
        }
    }
    parts.join(";")
}

#[test]
fn dispel_worker_matches_the_original_on_1600_vectors() {
    let text = include_str!("vectors/dispel_worker.txt");
    let (mut n, mut with_damage, mut with_buffs, mut with_mana, mut early) = (0, 0, 0, 0, 0);
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let (head, calls) = line.split_once(" | ").expect("row");
        let v: Vec<u32> = head.split_whitespace().map(|x| u32::from_str_radix(x, 16).unwrap()).collect();
        assert_eq!(v.len(), 20, "{line}");
        let (p, c, flag20, t248, t5c) = (v[0], v[1], v[2], v[3], v[4]);
        let i = WorkerIn {
            player: p,
            rel: v[5],
            bams: v[6],
            summoned: t248 & 4 != 0,
            illusion: t5c & 0x4000_0000 != 0,
            v11c: v[7],
            blocked: v[8],
            misc: v[9],
            a2: v[10],
            ability: v[11],
            positive: v[12],
            negative: v[13],
            a6: v[14],
            a7: v[15],
            damage_bits: v[16],
            mana_bits: v[17],
            flags: v[18],
            target_flag8: flag20 & 8 != 0,
            source: c,
        };
        let (steps, ret) = dispel_worker(&i);
        assert_eq!(render(&steps), calls, "row {n}: {line}");
        assert_eq!(ret, v[19], "return value of row {n}");
        n += 1;
        with_damage += usize::from(calls.contains("apply"));
        with_buffs += usize::from(calls.contains("buffs"));
        with_mana += usize::from(calls.contains("mana"));
        early += usize::from(v[19] == 1);
    }
    assert_eq!(n, 1600);
    assert!(with_damage > 300 && with_buffs > 200 && with_mana > 50 && early > 100, "coverage {with_damage} {with_buffs} {with_mana} {early}");
}
