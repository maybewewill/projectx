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
* **Magic immunity counters, Magic Immunity (`Amim`), Anti-Magic Shell (`Aams`, `Aami`) CONFIRMED (400 each: `immunity`, `ams_effect`, `ami_effect`)**
  - Counters: add `0x6F07A3A0` / remove `0x6F077320` (ECX = unit, argument = flag) move `+0x15C` always and the **second counter `+0x160` only with the flag**;
    the enter work (side effect `0x6F2AB310`, removal of the purge debuff `Bprg`, the dispel walker `0x6F079FE0(0,1,0,0,0,1,1,0,1)`) runs when `+0x15C` becomes 1
    **or** the flag is set and `+0x160` becomes 1; the leave work (`0x6F2AC3A0`) mirrors it. Callers that pass the flag: Magic Immunity (init `0x6F110730`,
    removal `0x6F110700`), Avatar, Divine Shield; callers that do not: the Anti-Magic Shell buff (apply `0x6F110750`, removal `0x6F1107D0`) and Sanctuary.
    The sequences also drive those buff slots for real (`B` / `C` / `D` ops). **Bug fixed**: the model had one counter and dispelled only when it reached 1, so
    an Anti-Magic Shell followed by Magic Immunity (or any flagged source after a flag-0 one) missed the second enter dispel. `neutral_spells::immunity_add` /
    `immunity_remove`, `Game::magic_immunity_add` / `magic_immunity_remove`, unit field `magic_immune_flagged`; every `magic_immune` writer now goes through them.
  - Anti-Magic Shell effect `0x6F13C900` (the instant class `0x6F143040` passes the owner as target): the dispel worker `0x6F0EEF00` **always runs** on the target with
    positive = negative = `(DataC == 0)` (x87 equality, NaN false), `(a6, a7) = (0, 1)`, damage `DataA`, mana loss `DataD`, flags 64; unless the target is locked
    (`+0x5C & 0x100`) the shell follows: `DataC == 0` -> buff `Bams` (`CBuffAntiMagicShell`) carrying `DataB`, otherwise `Bam2` (`CBuffAntiMagicShellTwo`, no immunity count)
    carrying `DataC`; an existing buff of that code is refreshed through slot `0x358` (set-duration rule) instead of created through `0x354`; duration = `HeroDur` for a hero else
    `Dur + ability+0x9C`. The stock row has all `Data*` zero, `Dur` = `HeroDur` = 90, so stock Anti-Magic Shell dispels both polarities and gives `Bams`.
    **Bug fixed**: the model applied only a buff and an immunity count (no dispel at cast, no `Bam2`, restart instead of the set-duration rule, no second-counter rule);
    `unit_abilities_a::ams_plan` / `anti_magic_shell`, `BuffEffect::ShellImmunity`. Not modelled: what the stored `DataB` / `DataC` value does (the damage-absorb read).
* **Item classes that inherit a verified spell class (CONFIRMED, 400 each)**: `AIdi` (`CAbilityItemDispelAoe`, vtable `0x6F8C4F34`, Dispel Magic callback and worker
  arguments, order 852271), `AIlp` (`CAbilityLightningPurge`, vtable `0x6F8CB464`, the Purge effect reached through its own slot `0x424` = `jmp 0x6F107E10`, order 852275),
  `AIsh` (`CAbilitySpiritTroll`, vtable `0x6F92828C`, the Feral Spirit effect from slot `0x3C0`, identical in both vtables). Tests `v9_item_dispel.rs`,
  `v9_lightning_purge.rs`, `v9_troll.rs`; harness reruns in `v9_emu.py` (`itemdispel_args`, `lpurge_args`) and `v9_troll_emu.py`.
* **Creep summons `ANsg` Summon Grizzly, `ANsq` Quilbeast, `ANsw` War Eagle, `ANlm` Lava Monster, `ANwm` Watery Minion CONFIRMED (150 each)**: all five share
  the Water Elemental loop `0x6F163390` at slot `0x3C0` (and the `UnitID` getter `0x3E8`); only the code / name / order getters and the class slot `0x3E4`
  (the timed-life attach) differ. The V5 harness (`v5_summon_emu.py`) was re-run on their vtables (`v9_summon_emu.py`; `0x3E4` stubbed as for `AEst`/`AEsv`):
  count `F2I(DataA)`, duration `Dur + ability+0x9C`, flag `0x502` / `2`, creator arguments, link calls, per-unit duration all equal `orc::feral_plan`. Test `v9_summon_family.rs`.
