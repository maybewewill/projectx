# V9 verification wave (Unicorn, `tools/emu/v9_emu.py`, vectors `crates/sim/tests/data/v9/`, test `crates/sim/tests/v9_vectors.rs`)

Method as V4 / V8: the real routines of Game.dll run on fake objects, world-dependent callees are stubs that read per-case
switches or record their arguments, one check per process, 400 random cases (NaN / infinity / denormal bit patterns included),
rows dumped as hex words and re-computed by the Rust model. A row is added to `confirmed-abilities.tsv` only after a 0-mismatch run.

* **Frenzy (`Afzy`, `CAbilityFrenzy < CAbilityBloodlust`) CONFIRMED (400)**: the self cast is the no-target effect slot `0x428` (`0x6F1CB1D0`);
  its unit form (slot `0x424`) is Bloodlust's own body `0x6F1CB080`. The creator `0x6F1CB150` builds a `CBuffFrenzy` (vtable `0x6F90363C`,
  code getter `0x6F198130`) whose apply slot `0x35C` is the Bloodlust buff's `0x6F1C2870`. Result: duration = `HeroDur` for a hero, else
  `Dur + ability+0x9C`; movement sink gets `DataB`, attack speed sink `DataA` (`unit_abilities_a::frenzy_values`, same function as
  Bloodlust). The model already agreed (no bug); the numbers are now a tested function.
