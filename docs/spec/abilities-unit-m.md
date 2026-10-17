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
| `Aspl` | SpiritLink (SimpleSpell) | order 852499; chain of `F2I(DataB)` units (stock 4): the target, then each time the nearest eligible friendly unit within `Area` of the LAST unit; every unit gets `Bspl` for `Dur`; a hit on a linked unit is scaled to `amount * DataA` (stock 0.5) for the struck unit and the rest `amount - own` is divided by the number of linked units and dealt to EACH linked unit (the struck one too) as a separate record of class `0x1000000` (flags `(f & ~4) | 0x42`); nothing happens for a zero amount, a record with flag `0x10` or class `0x1000000` | CONFIRMED roles (`DataA` fraction, `DataB` count) and split arithmetic: handler `0x6F1E9760` + share routine `0x6F1EFED0` run in Unicorn, 400 cases (126 active), 0 mismatches (`tools/emu/v2_spiritlink_emu.py`, `tests/v2_vectors.rs`); bugs fixed: the model used `DataA` as the count (stock 0.5 linked nobody) and split `amount / n` with no fraction. UNSURE: the chain pick (`0x6F1DEDC0`, nearest to the last unit, tie rules), the 120 s default, the struck unit's two records are summed |
| `Amil`, `Amic` | Militia, MilitiaConvert (CallToArms) | town hall: peasants with `Amic` within `Area` morph to `UnitID` (default `hmil`) for `Dur` (45 s); `Amic` converts the caster; `Bmil` end restores (Polymorph morph path) | UNSURE (no order id in the getter table; hosts call it by registering an id) |

Queries only (rules belong to S44 `invisibility.rs`): `Apiv` (`m_perm_invisible`), `Ashm` (`m_shadow_meld_active`,
night and stationary, caller supplies "stationary"), `Atru` (`m_true_sight_reveals`, radius `Area`), `Ault`.

Listed only (class and parent recorded, no behaviour): the other 61 codes (S52 below moved 14 of them to Modelled). Cast order ids that exist are in
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

## S52: melee-relevant Listed codes moved to Modelled

Tests: `crates/sim/tests/unit_abilities_m_s52.rs`. **No Unicorn run**: the numbers below are the editor rows of
`Units\AbilityData.slk` (War3Patch.mpq, the user's 1.26a files), and none of the effects reads a constant that was
found in `Game.dll`. Every row is therefore LIKELY (class, targs and order ids read from the original) or UNSURE
(the effect arithmetic is a model). Order ids: cast getters of `docs/research/ability-orders/getters.tsv` except
`Artn`, which has no cast getter (`returnresources` 852020 comes from the original's order-name initializer,
`orders/names.rs`).

| Code | Class | Order | What the model does | Label |
|---|---|---|---|---|
| `Arav` Raven, `Astn` Stone, `Amou` Mount | `CAbilityMorph` children | 852155/6, 852206/7, 852469/70 | rows of `unit_abilities_a::MORPH_ORDERS`: the level's `UnitID` after `Dur`, the alternate order returns | LIKELY (same class as Bear Form) |
| `Aphx` Phoenix morph | `CAbilityMorph` child | 852482 (alternate getter unhandled) | primary order only | LIKELY, no way back |
| `Arep` Repair, `Aren` Renew, `Arst` Restoration | `CAbilityRepair` (+children) | 852024 / 852161 / 852202 | target order; the builder restores `max life / (bldtm * DataB)` per second in 0.5 s ticks and pays `DataA * cost * repaired / max life` of gold and lumber (x10 tenths, plain changer) as it goes; ends when whole, when the owner cannot pay, when the builder moves or is stunned, or when the target dies. targs from the level rows (`mechanical,friend,nonancient,...`; Renew has no `mechanical`); the target must be hurt | structure and the editor meaning of `DataA` (0.35) / `DataB` (1.5) LIKELY; the 0.5 s tick, paying as it goes, `bldtm` as the base time (UnitBalance has `goldRep`/`lumberRep` but no repair-time column), power build (`DataC/D`) UNSURE |
| `Artn` Return | `CAbility` | 852020 (immediate) | `production::Game::force_return`: a carrying worker walks to the nearest hall and deposits; a lumber cutter mid trip brings the whole hits made so far (`floor(elapsed / hit seconds)`); a worker in a mine or without a load is left alone | LIKELY (effect), UNSURE (partial-load rule) |
| `Arpb` Replenish, `Arpl` Replenish Life, `Arpm` Replenish Mana | `CAbilityReplenish*` (Moon Well) | 852542 / 852545 / 852548 | **V10 CONFIRMED (Unicorn): an immediate no-target pulse, not a channel**; see `abilities-v10.md` (`replenish_plan`) | CONFIRMED |
| `Auns` Unsummon | `CAbilitySimpleSpell` | 852210 | removes an own structure (`structure,player`) and refunds `DataA` (0.5) of its gold and lumber cost | order id and targs LIKELY, refund rule UNSURE (`DataB` 50 unused) |
| `Atau` Taunt | `CAbilitySimpleSpell` | 852520 (immediate) | every enemy within `Area` (450) is given an attack order on the caster (`air,ground,enemy,vuln,invu`) | UNSURE (the original probably holds the order for `DataA` = 10 s) |
| `Aoar` Regeneration aura | `CAbilityAuraRegenLife < CAbilityAura` | passive | joins batch A's aura machinery: buff `Boar` (CONFIRMED: `CBuffAuraRegenLife` code getter `0x6F022D40`; `Bpig` of `CBuffPandaImmolation` `0x6F10E930` read the same way, not applied), `DataA` (0.02) of max life per second when `DataB != 0`, `Area` 500, targs `friend,neutral,...` | LIKELY (shape of `Aabr`) |
| `Apig` Panda Immolation | `CAbilityPandaImmolation < CAbilityPermImmolation` | passive | a periodic timer (`Dur`) while the ability exists; enemies within `Area` (220) take `DataA` (10), damage type 9 like the toggled Immolation | LIKELY, damage type UNSURE |

### Shared-file edits (kept to a few lines)

`unit_abilities_a.rs` (rows of `MORPH_ORDERS`, `Aoar` in the aura lists, the timer fall-through to this module and
one call of `sync_unit_abilities_m`), `cast.rs` (one condition: `cast_field_is_duration`), `production.rs`
(`force_return`).

### Checked against the data, not modelled (no behaviour added)

* `Ambt` Mana Battery: `Rng` 99999, `Area` 400, `DataA..E` = 2, 0.5, 10, 30, 1: the fields cannot be assigned to an effect.
* `Amfl` Mana Flare: `Cast` 0.75, `Dur` 30, `Cool` 20, `Cost` 50, `Area` 750, `DataA..F` = 3, 1, 90, 50, 12, 1; buffs `Bmfl,Bmfa`.
* `Asds` Self Destruct (Sapper): `DataA..E` = 100, 250, 250, 100, 3; the damage split is unknown.
* `Amin` Land Mine (`Rng` 200, `DataA` 10, `DataB` 2), `Atdg`/`Atsp`/`Atwa` tornado, `Awrp` warp (400/400), `Awha` Wisp harvest
  (5, 5, 150), `Aste` Steal (`DataA` 50), `Assk` Stone Skin (100, 3, 12, 1, 1), `Amdf` Magic Defense (0.33), `Asal` Salvage (0.5, 1).
* `Apit` Purchase Item, the shop and sell abilities: the stock lists (`shop.rs`) have no restock timer, no purchase and no
  item cost table in the model; not started.
* `Aroo` Root / Uproot (orders 852165 / 852166): the planted and uprooted forms are two unit types and the pairing is in
  no table the model loads.
* `Amgl` Moon Glaive: a bounce attack whose count and loss come from the weapon row, not the ability.

## V4: Taunt (`Atau`) corrected and CONFIRMED (Unicorn, `tools/emu/v4_spells_emu.py`)

The earlier model ordered EVERY enemy in `Area` to attack. The original (`0x6F17B5A0`): the collector `0x6F17B490` fills 20-byte
records `{unit, key1, key2, ally flag 0x6F285B80, not-ally flag 0x6F285B00}` for every candidate that passes the target
legality slot `0x3EC` within `Area`; with `n = F2I(DataA)` nonzero they are sorted by the CRT `qsort` (`0x6F4C6210`, MSVC 2005
layout, ported as `crt_qsort`) with `0x6F160310` (`DataB == 0` and `DataC == 0`: key1 then key2 by soft subtraction, `+1` for a
positive difference and `-1` for a negative or NaN one, then `B.id - A.id`), `0x6F16E390` (`DataB == 0`, `DataC != 0`: `B.f3 - A.f3`
first) or `0x6F16E3B0` (`DataB != 0`: `B.f4 - A.f4` first); `n` is limited to the number of records (0 or too large: all of them,
unsorted); each of the first `n` gets the effect slot `0x42C` with `HeroDur` for a hero, else `Dur + ability +0x9C`. Model:
`taunt_plan` (400 cases with the real sort and comparators, up to 10 records), `taunt_compare` (400). UNSURE: key1 (the distance
from the caster in cell units; the model uses the squared world distance, same order), key2 (nonzero for a unit that already carries
the Taunt buff; the model has no such buff: 0.0) and what slot `0x42C` does (the Taunt buff that forces the attack; modelled as an
attack order).
