//! V7: the Ethereal ability counter against the original (`tools/emu/v7_ethereal_emu.py`, `tests/data/v7/ethereal.txt`).

use sim::unit_abilities_a::ethereal_step;

#[test]
fn ethereal_counter_matches_the_original() {
    let (mut n, mut enters, mut leaves, mut negative) = (0, 0, 0, 0);
    for line in include_str!("data/v7/ethereal.txt").lines() {
        let (head, tail) = line.split_once('|').unwrap();
        let h: Vec<&str> = head.split_whitespace().collect();
        let t: Vec<&str> = tail.split_whitespace().collect();
        let mut count = u32::from_str_radix(h[0], 16).unwrap() as i32;
        let (mut en, mut le) = (0, 0);
        for op in h[1].chars() {
            let (c, entered, left) = ethereal_step(count, op == 'I');
            count = c;
            en += entered as u32;
            le += left as u32;
        }
        assert_eq!(u32::from_str_radix(t[0], 16).unwrap(), count as u32, "counter, row {line}");
        // refresh 0x6F284830 runs once per enter / leave; the enter effects (0x6F2AB310) once per enter, the leave ones per leave
        assert_eq!(t[2].parse::<u32>().unwrap(), en + le, "refresh calls, row {line}");
        assert_eq!(t[3].parse::<u32>().unwrap(), en, "enter effects, row {line}");
        assert_eq!(t[4].parse::<u32>().unwrap(), le, "leave effects, row {line}");
        // the attack-block counter +0x1C0 is +1 per enter and -1 per leave (0x6F0767F0)
        assert_eq!(u32::from_str_radix(t[1], 16).unwrap(), (en as i32 - le as i32) as u32, "+0x1C0, row {line}");
        n += 1;
        enters += en;
        leaves += le;
        negative += (count < 0) as u32;
    }
    assert!(n >= 400 && enters > 150 && leaves > 100 && negative > 5, "{n} {enters} {leaves} {negative}");
}
