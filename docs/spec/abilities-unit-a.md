# Unit abilities, batch A (S46)

Model: `crates/sim/src/unit_abilities_a.rs` (registry `ABILITIES`, hooks in `cast.rs`, `auras.rs`, `unit_spells.rs`).
Tests: `crates/sim/tests/unit_abilities_a.rs` (17). Class and parent names are from the ability atlas
(`docs/research/ability-atlas/factory.tsv`); order ids are the Unicorn-verified getters
(`docs/research/ability-orders/getters.tsv`); data labels from `WorldEditStrings.txt` (`WESTRING_AEVAL_*`) and the
1.26a `AbilityData.slk` first-level rows (read from the user's archives, not committed).
Labels: CONFIRMED = machine code, LIKELY = class parent / editor meaning, UNSURE = stand-in. No Unicorn number runs
were done in this batch (no new bit-exact numeric core was found); everything below is LIKELY or UNSURE except where noted.

## Modelled (32 codes)

| Code | Class (parent) | Model | Label |
|---|---|---|---|
| Aetl | CAbilityEthereal (Passive) | init slot 0x80 -> `0x6F07A340` adds 1 to unit+0x158 (ethereal counter), removal `0x6F0796A0` subtracts; the first count also toggles other state (`0x6F079FE0`, `0x6F0754E0`: not decoded) | CONFIRMED counter |
| Aabr, Aarm | CAbilityAuraBlightRegen / AuraRegenMana (Aura) | emitter: 0.5 s scan, buff 4 s; buffs Babr / Barm (code getters CONFIRMED); life / mana regen DataA, a fraction of the target's maximum when DataB != 0 (the ability apply `0x6F06F430` passes the absolute flag as the DataB == 0 compare; the buff skips illusions, `0x6F043F60`) | LIKELY |
| Aasl | CAbilityAuraSlow (Aura; buff < CBuffAuraEndurance) | Basl: movement DataA, attack speed DataB, enemies per targs | LIKELY |
| Aakb | CAbilityAuraKotoBeast (Aura; buff < CBuffAuraCommand) | Bakb: damage bonus DataA applied as a percentage (the original adds round(DataA * base damage) to the dice bonus) | UNSURE |
| Acar Abun Aenc Advc Achd | CAbilityCargoHold family | capacity = DataA per hold ability; holder list, `loaded` flag, upkeep timer 0.5 s; holder death: Achd kills the cargo, a plain hold unloads it | LIKELY / UNSURE (Achd) |
| Aloa Adro Adri | CargoLoad / CargoDrop / CargoDropInstant | load order 852046 (ground friend), unload order 852048 (unit or point), unload all 852049; unloaded units go on a ring 96 units out | LIKELY, ring UNSURE |
| Adev | CAbilityDevour (< CargoLoad) | order 852104 loads an enemy into the Advc stomach; Bdvv / Bdig; 1 s tick damage = Advc DataB; death releases it | UNSURE |
| Adet Adts Agyv Abdt | CAbilityDetector family | within Rng of a living detector, a unit is detected by the detector's player (`Game::unit_detected_by`, 0.5 s rescan) | LIKELY |
| Agho Aeth | CAbilityGhost (< PermInvis) / CAbilityGhostVisible | hook only (S44 owns invisibility): `is_ghost`, `sees_ghosts`, `ghost_visible_to` | UNSURE, hook |
| Aabs | CAbilityAbsorb | order 852529: moves up to DataB (max mana absorbed, ABS2) mana from an own unit; DataA (life) not modelled | LIKELY |
| Adch Adtn | CAbilityDisenchant / Detonate (< DispelMagic) | run the Dispel Magic effect on the row (DataA mana loss, DataB summoned damage); Detonate kills the caster afterwards; orders 852495 / 852145 | LIKELY |
| Afzy | CAbilityFrenzy (< Bloodlust) | order 852561, self only, Bfzy: attack speed DataA, movement DataB, Dur / HeroDur | LIKELY |
| Absk | CAbilityBerserkerRage | order 852100 (self): BSK1 movement DataA, BSK2 attack speed DataB, Bbsk for Dur; BSK3 damage taken has no sink | LIKELY, partial |
| Aams | CAbilityAntiMagicShell | order 852186: one magic immunity count for Dur (re-cast restarts the timer); data fields unused | UNSURE |
| Acmg | CAbilityControlMagic | order 852474: summoned non-hero target, caster pays DataB * life mana (CMG2 label), owner changes | UNSURE |
| Aave Abrf Acpf Aetf Abur | CAbilityMorph family | immediate primary / alternate orders (852531/2, 852138/9, 852493/4, 852496/7, 852533/4); after Dur the unit takes the UnitID type (life / mana fractions kept, abilities rebuilt from the type); alternate returns to the recorded form (`Game::set_morph_alt` for units placed transformed). Tested through Abur | LIKELY |

Engine hookup (not done here, `crates/engine` is outside this lane): `objdata` loads `spell_levels` for a fixed code list;
the codes above need `AbilityData.slk` rows (area, rng, dur, data, unit_id) added to it, and the alternate-form code
(DataA text such as ucry) needs to feed `set_morph_alt`.

## Not done (no model; reason)

- Passive attack effects needing the on-hit hook and data decoding: Abof, Acor, Afak, Aflk, Afrb, Afrz, Afsh, Aimp, Agyb, Aliq, Alit.
- Marker or neutral classes without a behaviour I could prove: Aall, Aawa, Alea, Aamk (hero attribute path), Aapl (CBuffLifeDoT numbers 120/1/10 and plague ward spawn not decoded).
- Handled by other modules: Agld, Ahar, Ahrl, Aaha, Abgm, Aegm (economy.rs, production.rs, natives/economy.rs).
- Tree and mine targets (cast infrastructure takes unit and point targets only): Aeat, Agra, Aent, Aesn.
- Unit merge / split and unit conversion: Acoa, Acoh, Acoi, Adec, Abtl, Acha, Achl, Alam, Aexh, Agyd.
- Area / reveal effects: Aclf, Afla, Adta, Advm, Adda, Aabr's Statue graphics, Ablp, Abli (blight grid), Aast (corpse revive), Acan (corpse channel), Abar (modal), Adef (toggle), Ahid (S44).

## V7: verification against the original (Unicorn, 0 mismatches)

Harnesses `tools/emu/v7_*_emu.py`, vectors `crates/sim/tests/data/v7/`, tests `crates/sim/tests/v7_*_vectors.rs`; rows in
`confirmed-abilities.tsv`. The level getters are mapped by `tools/emu/v7_getters.py` (`0x6F02E6F0` Cast, `E760` Dur, `E7D0` HeroDur,
`E890` Cool, `E900` Area, `E9E0..EBA0` DataA..DataE). The name-based getters `0x6F01F7D0 / 01FA50 / 01FAD0 / 01FB50` read "Area1",
"DataA1", "DataB1", "DataC1" from the ability's data row (`0x6F262760`).

* **Aabr / Aoar / Aarm CONFIRMED (300 each):** the ability apply (`0x6F06F430`, `0x6F06F310`, `0x6F06F3A0`) hands the buff the flag
  `0.0 == DataB` (x87, so `-0.0` true, NaN false); the buff apply (`0x6F043FA0` Babr, `0x6F043A20` Boar, `0x6F043CE0` Barm) treats the
  rate as a fraction of the owner's max life / mana (soft `mul(strength, max)`, that operand order) exactly when the flag is CLEAR, else
  as an absolute amount; an illusion owner is skipped unless the Misc key is on. `unit_abilities_a::regen_aura_*`. The scan, targs and
  4.0 s duration were checked in earlier lanes (`buffs.md`).
* **Aasl CONFIRMED (600), model bug fixed:** `CBuffAuraSlow` slot `0x370` (`0x6F1AD2F0`): nothing happens (not even the duration) when
  BOTH new strengths are weaker than the stored ones by the tolerance; otherwise each component whose stored strength exceeds the new one
  by the tolerance is undone (negated sink, only if applied), replaced and re-applied, and the other component keeps its value. The model
  used to replace both on every refresh. `unit_abilities_a::slow_aura_update`.
* **Aetl CONFIRMED (400):** init `0x6F07A340` / removal `0x6F0796A0` count unit `+0x158`; the first count and reaching zero run the enter /
  leave side effects (`+0x1C0` attack-block counter via `0x6F0767F0`, refresh `0x6F284830`); an unbalanced removal goes negative and runs
  nothing. `ethereal_step`. The ability-cancel walk `0x6F079FE0` ran on an empty list only.
* **Found, not modelled:** the cargo family reads its capacity per unit "cargo size" (`0x6F079520` -> type record `+100`, 0 -> -1) into the
  holder's used count (`+0x74`), and the hold's `Cool` field drives an attack-speed effect (`0x6F166090`); load range uses
  `Rng`^2 against the pair distance (`0x6F173BB0`). Detector init reads `Rng1` (`0x6F01F9D0`). The morph timer is
  `add(add(ability+0x9C, Dur), flag 0x40000 ? DataC : 0)` (message `0xD01C4`, `0x6F0FD3D0`), the type swap is `0x6F0F9CD0`. Not run: cargo,
  detectors, absorb (the `Mabs` missile), morph family.
