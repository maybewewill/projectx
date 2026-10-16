# Unit abilities, batch M (S47)

Module `crates/sim/src/unit_abilities_m.rs`, tests `crates/sim/tests/unit_abilities_m.rs`. Labels: CONFIRMED (machine
code), LIKELY (RTTI parent / editor data), UNSURE (stand-in). **Nothing in this batch is CONFIRMED or Unicorn-run**:
no new arithmetic constants were read; the effects reuse already-checked pieces (owner change of Charm, the poison
tick of Envenomed Spears, the morph of Polymorph, the buff framework). Order ids come only from
`docs/research/ability-orders/getters.tsv` (`tools/analysis/ability_orders.py`).

## Coverage (79 codes)

All 79 requested rawcodes are in `CODES` (class, RTTI parent, status), taken from the atlas `factory.tsv`.

Modelled (behaviour + test):

| Code | Class (parent) | Behaviour | Label |
|---|---|---|---|
| `Apoi`, `Apo2` | PoisonAttack (Passive), PoisonDamage (PoisonAttack) | on-hit poison `Bpoi`: `DataA` dmg/s (class 23), `Dur`/`HeroDur`, same engine as `Aven` | LIKELY (parent of `Aven`); stacking `DataD` not modelled |
| `Apos`, `Aps2` | Possession (SimpleSpell), PossessionTwo (Possession) | order 852196; target changes owner (Charm path), caster removed; targs air,ground,nonhero,enemy,neutral,organic | LIKELY / targs UNSURE |
| `Amls` | MagicLeash (SimpleSpell) | order 852480 `magicleash` = Control Magic: enemy summoned non-hero changes owner | UNSURE (mana-per-hp rule not modelled) |
| `Asps` | SpellSteal (AutoTargetSpell) | order 852483; one dispellable buff of the relation's polarity moves to the caster as a marker | UNSURE (the original lets the player choose the receiver; effects not copied) |
| `Aspl` | SpiritLink (SimpleSpell) | order 852499; target + up to `DataA-1` friends in `Area` get `Bspl` for `Dur` (120 s default); a hit is split equally among live members (`Game::m_link_share`, after the Mana Shield absorb) | LIKELY; death dispatch of link victims left to the host (`m_take_link_deaths`) |
| `Amil`, `Amic` | Militia, MilitiaConvert (CallToArms) | town hall: peasants with `Amic` within `Area` morph to `UnitID` (default `hmil`) for `Dur` (45 s); `Amic` converts the caster; `Bmil` end restores (Polymorph morph path) | UNSURE (no order id in the getter table; hosts call it by registering an id) |

Queries only (rules belong to S44 `invisibility.rs`): `Apiv` (`m_perm_invisible`), `Ashm` (`m_shadow_meld_active`,
night and stationary, caller supplies "stationary"), `Atru` (`m_true_sight_reveals`, radius `Area`), `Ault`.

Listed only (class and parent recorded, no behaviour): the other 61 codes. Cast order ids that exist are in
`ORDER_IDS_LISTED`. The ones with no cast id (passives, morph toggles `Amou`/`Aphx`/`Arav`/`Astn` use primary ids only,
cargo, tornado, tank and spawn abilities) need their class slots read first. Not attempted because they need
systems outside this module: repair family `Arep/Aren/Arst` (construction + economy), `Artn` return (the harvest
model lives in `production.rs`), `Aroo` root/uproot (mover state), `Apit` (shop purchase, see `shop.rs`),
transport/cargo (`Amtc/Amel/Amed/Astd/Atlp/Atdp/Atpi/Attu`), replenish (`Arpb/Arpl/Arpm`), `Auns` unsummon, `Atau` taunt.

## Shared-file edits

`lib.rs` (module), `cast.rs` (state field, order registration, no-target list), `neutral_spells.rs` (the two fall-through
arms call this module), `unit_spells.rs` (`m_on_hit` after `neutral_on_hit`; `morph_unit` / `restore_morph` made
`pub(crate)`), `combat.rs` (`m_link_share` after both `neutral_absorb` calls).

## Open

Unicorn runs for the numbers (poison, Spirit Link share fraction, Control Magic mana rule, militia duration), the
`Amil` order id, autocast scans for `Asps`, and the item/neutral alias rows for these codes.
