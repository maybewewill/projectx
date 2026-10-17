# V9 verification wave (Unicorn, `tools/emu/v9_emu.py`, vectors `crates/sim/tests/data/v9/`, test `crates/sim/tests/v9_vectors.rs`)

Method as V4 / V8: the real routines of Game.dll run on fake objects, world-dependent callees are stubs that read per-case
switches or record their arguments, one check per process, 400 random cases (NaN / infinity / denormal bit patterns included),
rows dumped as hex words and re-computed by the Rust model. A row is added to `confirmed-abilities.tsv` only after a 0-mismatch run.

* **Frenzy (`Afzy`, `CAbilityFrenzy < CAbilityBloodlust`) CONFIRMED (400)**: the self cast is the no-target effect slot `0x428` (`0x6F1CB1D0`);
  its unit form (slot `0x424`) is Bloodlust's own body `0x6F1CB080`. The creator `0x6F1CB150` builds a `CBuffFrenzy` (vtable `0x6F90363C`,
  code getter `0x6F198130`) whose apply slot `0x35C` is the Bloodlust buff's `0x6F1C2870`. Result: duration = `HeroDur` for a hero, else
  `Dur + ability+0x9C`; movement sink gets `DataB`, attack speed sink `DataA` (`unit_abilities_a::frenzy_values`, same function as
  Bloodlust). The model already agreed (no bug); the numbers are now a tested function.
* **Neutral Heal (`Anhe`, `CAbilityNeutralHeal` vtable `0x6F8C9AEC`) CONFIRMED (3 x 400)**: its vtable differs from `CAbilityHeal`'s only in the dtor, code,
  name and order-id getters, so effect `0x6F108280`, target rule `0x6F0F0290` and autocast filter `0x6F0F0370` are Heal's. The three V4 Heal harnesses were
  re-run on the neutral vtable (`v9_emu.py` checks `nheal_effect|target|filter`, test `v9_neutral_heal.rs`); the model aliases `Ahea` (`neutral_spells.rs`).
* **Disenchant (`Adch`, `CAbilityDisenchant` vtable `0x6F8C3E5C`) CONFIRMED (400)**: differs from Dispel Magic only in dtor / code / name / order-id getters, so the
  per-unit callback `0x6F0EF210` -> worker arguments (positive / negative bits from `DataA`/`DataB`, damage `DataB`, mana `DataA`, flags `0x40`) are Dispel's;
  the V4 harness was re-run on the Adch vtable (`v9_emu.py` check `disenchant_args`; order id `0xD020F` = 852495 matches the model). Test `v9_disenchant.rs`.
* **Creep Thunder Clap (`ACtc`, vtable `0x6F8F7044`) CONFIRMED (400 + 400)**: differs from `CAbilityThunderClap` only in dtor / code / name / order id and slot `0x430`
  (`0x6F18AA90` instead of `0x6F18A930`; same body with the creep buff factory `0x6F188C00`). Per-unit body (shared with War Stomp) run with the creep vtable
  (`v9_creep_clap_unit.py`) and the slow slot run for real (`v9_emu.py` check `creep_clap_slow`: hero-rule duration, `-DataC` movement, `-DataD` attack speed;
  the hero walk `0x6F0766A0` stubbed to "no" as in V1). The model aliases `AHtc` (`neutral_spells.rs`). Test `v9_creep_clap.rs`.
