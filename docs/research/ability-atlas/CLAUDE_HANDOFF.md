# Ability research handoff: all 417 factory rawcodes, WC3 1.26a

Generated and checked on 2026-10-13 using the user's `C:\Users\slash\re\game.dll`.
SHA-256: `6d21bd9a0f9fbc8446f455c9e89ac994fed68174426fe608fcb9baefd4dec53c`.
IDA metadata reports the same SHA-256 and preferred image base `0x6F000000`.
All addresses below are preferred-image VAs; relocate by module-base delta in a live process.

## What is now available

- `factory.tsv`: **417/417 rawcodes**, each mapped to one primary RTTI class and vtable.
  Includes the existing factory wrapper, typed-pointer helper, inheritance parent,
  key virtual entry points, every override relative to the nearest RTTI ancestor with a primary vtable,
  IDA xrefs to that vtable, and literal mentions in `crates/sim/src`.
- `atlas.json`: **747 primary classes**: 493 `CAbility*`, 227 `CBuff*`, 27 `CEffect*`.
  Includes full vtable slot addresses and RTTI ancestor lists, also for non-factory base classes.
- `override-functions.tsv`: **4,789 distinct factory-class override targets**, indexed by rawcode and byte offset.
  This is a navigation index, not a list of 4,789 unique gameplay algorithms.
- `vtable-xrefs.json`: IDA reference metadata for all 747 vtables, with enclosing functions where IDA knows them.
- `tools/analysis/ability_atlas.py`: reproducible extractor; reuses `rtti_vtables.py`.

Validation: all **417 factory code getters were executed in Unicorn**, with zero mismatches.
Additional assertions check the known Storm Bolt identity and cast entry, Shockwave inheritance,
the absence of spell-specific labels on Bash and BHbz, and the separate XHbz effect class.
Four additional Unicorn checks execute the original Blizzard effect message dispatcher:
the three recognised messages below and one unknown message. Handler bodies are stubbed;
the selected call targets, return value, and stack cleanup are checked.
These tests confirm identity, navigation and routing. They do **not** confirm complete ability behaviour.

## Important interpretation rules

The old `ptr_ctor` column is a **typed pointer/cast helper**, not a concrete ability constructor.
It is retained as `typed_pointer_helper`. Follow the actual vtable xrefs to find functions writing
the vtable; inspect each candidate to distinguish construction, destruction, and other uses.
Some IDA references have `fn: null`, so a constructor candidate can exist outside IDA's current
function boundaries. No database boundaries or names were changed during this research.

Slot offsets are **bytes**, not ordinal numbers: slot 7 is `0x1C`, slot 32 is `0x80`.
The same numeric offset can have different meanings in unrelated class families.
Spell-specific names in `key_slots` are emitted only for RTTI descendants of `CAbilitySpell`.
For buffs and effects, use `vtable_slots`, their inheritance chain, and existing family specs.
The scanner's slot count is the contiguous executable-pointer span; it is not an independent ABI proof.

`source_mentions` only identifies files to inspect. It is **not implementation coverage**.
RTTI parent relationships justify reusing research, but do not prove behaviour equivalence.
The 417 entries include item, neutral, campaign and utility abilities; they are not 417 hero spells.
Move/attack and other fixed unit abilities still need their separate paths from `abilities.md`.

## Immediate priorities: fill the channelled-spell gap

`abilities-human-heroes.md` section 7 says Blizzard and Mass Teleport are not modelled.
`cast.rs` also explicitly lacks channelled spells. The following routes are actionable starting points.
The class identities are Unicorn-checked; deeper observations below are **static observations** only.

### Blizzard: distinguish ability, effect, and buff

| Role | Code / class | Vtable | Starting points |
|---|---|---|---|
| Ability | `AHbz`, `CAbilityBlizzard` | `0x6F910CAC` | init `0x6F197E90`; slot `0x418` -> `0x6F1C2650`; channel end `0x6F1AFBE0` |
| Effect | `XHbz`, `CEffectBlizzard` | `0x6F8FB97C` | message slot `0x0C` -> `0x6F1C2610`; setup `0x6F1B9BB0` |
| Buff | `BHbz`, `CBuffBlizzardAoe` | `0x6F905484` | parent `CBuffAoe`; inspect inherited hooks as well as the three local overrides |

The ability's slot `0x418` calls `0x6F1B9E90`, which reads level fields and calls
`0x6F1B9BB0` to initialise the separate effect object. The type lookup at `0x6F197A00`
returns **XHbz**, and `0x6F19C180` is another typed-pointer helper, not an allocator.

The effect message dispatcher `0x6F1C2610` is confirmed by disassembly and the routing checks:

- `0xD01B0` -> `0x6F1B6C00`.
- `0xD01B1` -> `0x6F1B9DB0`.
- `0xD01C1` -> `0x6F1A1000`.

`0x6F1B9BB0` clamps one integer argument to at least 1 and another to 1..32, stores them at
effect offsets `+0x78/+0x84` and `+0x98`, immediately calls `0x6F1B6C00`, and arms `0xD01B1`.
Determine which SLK fields feed these arguments before naming them as wave/shard counts.
`0x6F1B6C00` decrements `+0x78`, re-arms `0xD01B0` while positive using `+0x80`, and calls
`0x6F1AF6E0`. That worker calls `0x6F2853D0` with **stream argument 4**, populates coordinate
arrays, and calls `0x6F1A1000`. Determine which draws affect simulation versus presentation
before omitting them or moving them to a separate RNG.

For the damage path, start at **`0x6F1B9DB0` -> `0x6F2C31D0`**, with callback
**`0x6F1B6CB0` -> `0x6F1AF950`**. The callback checks target state, branches through the
hero predicate, and forwards to effect vtable offsets `0x80/0x84/0x8C`. The former two
resolve to `0x6F1CAD10/0x6F1CADD0` and attach or update buffs. Inspect the AoE worker and
the buff update chain to establish damage, target enumeration order, building modifiers,
and stacking. The inspected callback alone does not establish the final damage formula.

Avoid treating `CAbilityBlizzard`'s `0xD01BF/0xD01C1` handlers (`0x6F14CEF0/0x6F14CFC0`)
as the effect's wave dispatcher. They belong to a different object and message path.

### Mass Teleport

`AHmt` -> `CAbilityMassTeleport`, vtable `0x6F88BEFC`.
Slot `0x418` is the forwarding/target-resolving entry `0x6F032CB0`;
slot `0x424` is **`0x6F0565D0`**. That entry changes ability flags, removes several ability
entries from the caster, changes caster/target state, and arms `0xD01C1` with the runtime
1.0 constant. Group selection and final placement remain unverified.
Follow message handler **`0x6F075320`**, channel hooks `0x6F056530/0x6F056AC0`, and
`0x6F069D60`. Check interruption, target removal, which units join, and their placement order.

### Channel and Death and Decay

- `ANcl` -> `CAbilityChannel`, vtable `0x6F90EA1C`: init `0x6F1AC760`;
  `0x384` -> `0x6F1AC800`, `0x388` -> `0x6F1AC8A0`, `0x418` -> `0x6F1AC880`,
  unit/no-target hooks `0x6F1AC890/0x6F1AC870`. Init stores an order id at `+0x124`
  and uses fallback **852600** if lookup fails. Do not generalise this init to all spells.
- `AUdd` -> `CAbilityDeathAndDecay`, vtable `0x6F8A3574`: `0x418` -> `0x6F0A98F0`,
  message `0x6F1562C0`, channel end `0x6F09FC60`. Use this as another channelled AoE
  research target after the shared channel lifecycle is established.

## Reduce repeated research using real inheritance

Examples extracted from RTTI:

- `AOsh` Shockwave -> `CAbilityCarrionSwarm`; compare to `AUcs` before writing another wave engine.
- `AHbh` Bash -> `CAbilityCriticalStrike`; compare its actual overrides and proc-state logic.
- The factory includes 262 descendants of `CAbilitySpell`; 120 classes have the immediate
  parent `CAbilitySimpleSpell`, 32 `CAbilityPassive`, 17 `CAbilityAutoTargetSpell`, 14 `CAbilityAura`.

For each family, establish the shared lifecycle once, then analyse class overrides plus per-level data.
Do not ignore inherited effects just because an override list is short. In particular, BHbz inherits
almost all of its mechanics from `CBuffAoe` and `CBuff`.

## Reproduce and query

From `C:\Users\slash\projectx` in PowerShell:

```powershell
py tools/analysis/ability_atlas.py C:\Users\slash\re\game.dll --verify --xref-json docs/research/ability-atlas/vtable-xrefs.json
Import-Csv docs/research/ability-atlas/factory.tsv -Delimiter "`t" | Where-Object rawcode -eq AHbz | Format-List
```

The xref snapshot is tied to the recorded DLL/IDA database. If the binary changes, regenerate
the xrefs; do not feed the old snapshot into a new binary's atlas.

Suggested next task: implement and differentially validate the shared channel lifecycle plus
Blizzard using the routes above. Verify event order, interruption, timer cadence, mana/cooldown,
RNG consumption, enumeration order, hero durations, building modifiers, and stacking independently.
Then apply the same research to Mass Teleport and other channelled abilities.

No Rust gameplay files or existing specs were changed. The working tree had concurrent changes
and unmerged paths during research; resolve those in the normal Claude workflow before integration.
