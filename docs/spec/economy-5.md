# Economy 5: haunted and entangled mines, ghoul and wisp lumber (S21, 2026-10-12)

Code: `crates/sim/src/production.rs` (`HarvestSpec`, `MineSpec`, `step_mines`, `step_entangles`, `take_over_mine`),
`crates/engine/src/objdata.rs` (`install_harvest`), `crates/engine/src/data.rs` (modes from the ability lists).
Method: disassembly of `Game.dll` with capstone (IDA was not reachable this session); **no Unicorn run**, so nothing
below is CONFIRMED in the Unicorn sense. Labels: DISASM = read from the machine code, LIKELY, UNSURE.

## 1. Ability data (retail 1.26a AbilityData.slk, level 1)
* `Ahar` (peon, peasant, ghoul-less gold+wood): Area 900, DataA 1 (lumber per hit), DataB 10 (lumber cap), DataC 10 (gold cap).
* `Ahrl` (ghoul): DataA 2 per hit, DataB 20 cap (no gold). `Aaha` (acolyte): Rng 200, Dur 1.
* `Awha` (wisp): Rng 900, Dur 8, DataA 5, DataC 150.
* `Abgm` (haunted mine): DataA 10, DataB 1, DataC 5, DataD 200. `Aegm` (entangled mine): DataA 10, DataB 1.
* `Aenc` (cargo of the entangled mine): DataA 5, Area 250, Rng 120. `Aent` (Tree of Life entangle): Cast 3, Rng 500, UnitID `egol`.
* Drop-off abilities in `UnitAbilities.slk`: `Argl` (halls), `Arlm` (Necropolis and **Graveyard**, lumber only), `Abgl` (Necropolis gold).
  The Graveyard has type `undead,Mechanical`, not `TownHall`, so the drop-off test is the ability, not the class: the sim now
  lets lumber trips end at any completed unit with `Argl`/`Arlm` (`TypeTech::lumber_dropoff`); gold still needs a town hall.

## 2. Harvest initialisers (DISASM)
* `0x6F0557D0` (harvest lumber init): Area -> `+0x18C` (getter `0x6F01F7D0`), DataA -> `+0x1D8` via f2i (lumber per hit),
  DataB -> `+0x198` via f2i (lumber cap); the hit timers `+0x1C8`/`+0x1D0` are copied from the unit's attack ability
  (`+0x1E8` -> fields `+0x16C`, `+0x190`; a unit without an attack gets the constant at `0x6FAAE4D0`). So the time of a
  hit comes from the **attack's damage point and backswing, not from `Dur`** (`Dur` 1.1 / 1.35 is not read here).
* `0x6F055A20` (gold + lumber harvest): lumber init, then DataC -> `+0x194` via f2i (gold cap).
* `0x6F171930` (wisp harvest): Rng -> `+0x18C`, **Dur -> `+0x194`** (seconds between payments), DataA -> `+0x1A4` via f2i
  (lumber per payment), DataC -> `+0x19C`. Getter strings checked through the global key pointers (`Rng`, `Dur`, `DataA`, `DataC`).
* So the wisp (attached to a tree, the tree is not cut) is paid DataA = 5 lumber every Dur = 8 s (DISASM of the field
  roles; the payment routine itself was not read: LIKELY).

## 3. The mine payment (DISASM, `0x6F1AE030`, `0x6F1AE6B0`, `0x6F0223C0`, `0x6F1A0AF0`)
* `0x6F1AE030` pays `[this+0x90]` (= DataA, 10 gold) per countdown end, bounded by the mine's remaining gold `[this+0x6C]`;
  when the remaining amount reaches 0 it ends the mine (event 0xDF path). Pays through the owner (calls the income path).
* `0x6F1AE6B0(arg = worker id)` runs when a worker leaves: it clears the worker's slot (`[this+0xD8]` array, count `[this+0xE0]`),
  decrements the active count `[this+0xE4]`; at 0 active workers it resets the countdown rate to 0.0 (`0x6F021560`), else it sets
  the **rate scale = soft div(I2R(count in the list), I2R(active))** through `0x6F6EF350` (I2R) and `0x6F6EFE10` (DIV), and hands it
  to `0x6F1A0AF0`, which applies it to the countdown (an agile property, `0x6F0223C0`: a value below 1e-4 -> the "tiny" path, a
  NaN/inf -> the clamp constants).
* Model (`step_mines`): while n acolytes/wisps are attached, one countdown of `interval * slots / n` seconds (interval = DataB = 1 s,
  slots = DataC = 5 for the haunted mine) ends in a payment of 10 gold. One worker: 10 gold per 5 s (2 gold/s); five: 10 per second.
  Direction of the scale (n/slots speeding the countdown) is LIKELY; the exact tick arithmetic of the agile countdown is UNSURE
  (the model integrates `dt / period` in f32 per 0.1 s batch).
* Entangled mine: DataA 10, DataB 1 as above; slots from `Aenc` DataA = 5 (UNSURE which ability supplies them). Workers inside are hidden.

## 4. Model decisions (UNSURE)
* An acolyte/wisp picks its own mine of the right kind (haunted, entangled) belonging to its owner, completed, with a free slot, nearest to the
  town; attach reach 300 (`ATTACH_REACH`, stand-in for `Rng` 200 plus footprint). No free slot: the cycle ends.
* A haunted mine built on a neutral gold mine (and the entangled mine created by `Aent`) takes over the neutral mine's remaining gold and the
  neutral unit is hidden (order of the link: `Abgm` `0x6F1ADF30`, DISASM; hiding the old unit as removed is UNSURE).
* Entangle autocast: scan 1 s (stand-in), cast time 3 s (`Cast`), range 500.
* Ghoul: 2 lumber per hit, cap 20, hit time dmgpt + backSw of the ghoul's attack, drop at Necropolis or Graveyard.
* Wisp lumber: 5 per 8 s, no walking back; gold cap fields unused for wisps.

## 5. Runs (release `w3boot`, `--computer 0:orc --computer 1:human --computer 2:undead --computer 3:nightelf`, (4)TwistedMeadows, 12000 steps = 1200 s, seed 0)
0 aborted AI threads, 17,449 logged AI calls. Kills: p0 -> creeps 6, p1 -> creeps 2, p2 -> creeps 7, creeps -> p1 1; no player-versus-player kill.
| race | workers | gathered gold / lumber | army | hero | waves |
|---|---|---|---|---|---|
| orc | 15 peons | 10,020 / 2,980 | 6 grunts, 3 headhunters, 3 berserkers, 2 demolishers, shaman | blademaster L2 (AOcr1 AOmi1), tauren chieftain L1 (AOsh1) | 2 (380.8 s 2 grunts, 494.2 s 7) |
| human | 16 peasants | 10,220 / 2,840 | 8 footmen, 4 riflemen, 2 mortar teams, knight | blood mage L1 (AHfs1) | 1 (197.0 s) |
| undead | 5 acolytes, 9 ghouls | 9,970 / 3,600 | 3 crypt fiends, 4 necromancers, 2 banshees, 2 meat wagons | lich L2 (AUfn1 AUfu1), death knight L1 (AUdc1) | 2 (336.1 s, 686.3 s) |
| night elf | 10 wisps | 10,760 / 5,325 | 6 archers, 3 huntresses, 2 glaive throwers, 2 dryads, 2 mountain giants | keeper L1 (AEfn1), priestess L1 (AHfa1) | 1 (434.6 s, 8 units) |
Before S21 the undead had 0 lumber and the night elf no entangled mine (ai-natives.md 10.6).

## 6. Open
* Unicorn checks of `0x6F1AE6B0` / `0x6F1AE030` need the mine object (vtable, worker array, agile property); not built. The numbers used come from the ability
  rows (DataA/B/C) and the disassembly above, not from a trace.
* Gold per second per acolyte (2 gold/s) and wisp lumber (0.625/s) are plausible against the game but not trace-confirmed.
* Gold income of all four races is ~10k in 1200 s (travel times, harvest timings and the tree reach remain stand-ins).
* Hero levels stay at 1-2: kill XP only from creeps; no life regeneration of waves means no repeated attacks (ai-natives.md 10.7).
