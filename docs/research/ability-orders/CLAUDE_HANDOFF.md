# Verified spell order ids and a tested correction patch

The original-order audit found five incorrect registrations in the Rust model.
`verified-order-fixes.patch` fixes them, updates the JASS test commands, and adds a regression test.
The patch was developed in the independent checkout `D:\rewar3\spell-order-fixes`, branch
`codex/verified-spell-orders`, based on commit `5c1e2f8ae8bf52d2bf2828ba0d8a99d8e7d6a292`.
No live Rust source files in the primary Claude checkout were changed by this work.

## Confirmed corrections

| Ability | Rawcode | Previously registered | Original cast order | Primary getter |
|---|---|---:|---:|---|
| Holy Light | AHhb | 852089 | **852092** | 0x6F0C0AB0 |
| Thunder Clap | AHtc | 852594 | **852096** | 0x6F18F870 |
| Resurrection | AHre | 852044 | **852094** | 0x6F1F4DE0 |
| Mirror Image | AOmi | 852093 | **852123** | 0x6F138DC0 |
| Feral Spirit | AOsf | 852120 | **852126** | 0x6F1F84B0 |

The cast getter is slot `0x348`, a virtual forwarder to slot `0x308` for these classes.
The primary and forwarding entries were both executed, with two different initial EAX values.
The original DLL SHA-256 matches the atlas:
`6d21bd9a0f9fbc8446f455c9e89ac994fed68174426fe608fcb9baefd4dec53c`.

These are actual conflicts, not just alternate names: 852089 is Blizzard's cast order,
852594 belongs to ANsg, and 852093 is Mass Teleport's cast order.
The new test `blizzard_order_does_not_cast_holy_light` failed on the baseline implementation,
because issuing Blizzard's order to a caster with only Holy Light caused a spell cast.
After the correction, that command neither casts Holy Light nor spends mana nor heals the target.

## Apply to the current working branch

From `C:\Users\slash\projectx`:

```powershell
git apply --check docs/research/ability-orders/verified-order-fixes.patch
git apply docs/research/ability-orders/verified-order-fixes.patch
$env:GAME_DLL = 'C:\Users\slash\re\game.dll'
cargo test -p sim --test spell_cast --test human_heroes --test orc_spells --test orc_mirror_cast
```

The patch passed `git apply --check` against the primary checkout when produced.
If the active ability branches have changed these lines, integrate the five value changes and the
regression manually. Do not replace newer ability implementations with files from the older checkout.

Validation on the isolated checkout, with `GAME_DLL` set:

- `spell_cast`: 13 passed, including the regression that failed before the fix.
- `human_heroes`: 12 passed, including tests that use the original RNG/math tables.
- `orc_spells`: 7 passed.
- `orc_mirror_cast`: 2 passed; the original-table checks executed, rather than returning early.
- Literal-registration audit after the correction: **15 matches, 0 mismatches**.
- `git diff --check`: passed.

This checks the routing change and existing behaviour tests. It is not proof that all five complete
ability implementations are bit-exact or that a full replay is synchronized.

## Reusable extraction and audit

`tools/analysis/ability_orders.py` reads the existing atlas and original DLL. It considers the
262 factory classes whose RTTI includes `CAbilitySpell`. For each it examines four order slots
(`0x308/0x318/0x348/0x368`) and three flag-return slots (`0x30C/0x31C/0x34C`).
Only direct constant leaves, direct tail jumps, and an exact virtual tail-forwarder shape are
resolved. Dynamic getters, global-variable reads, calls and unsupported bodies stay unresolved.
The flag values are numerical results only; their bit meanings were not established here.

Measured extraction: **1,834 entries, 1,326 resolved, 2,652 Unicorn executions, 0 mismatches**.
The synthetic extraction tests check class-specific virtual forwarding, dynamic-field exclusion,
forwarding-cycle termination, and literal Rust registration parsing.

```powershell
py tools/analysis/test_ability_orders.py
py tools/analysis/ability_orders.py C:\Users\slash\re\game.dll --verify
```

To audit an independent checkout:

```powershell
py tools/analysis/ability_orders.py C:\Users\slash\re\game.dll --verify --source-root D:\rewar3\spell-order-fixes --out-dir docs/research/ability-orders/after-fix
```

- `orders.json` and `getters.tsv`: original values, entry addresses, roles, unresolved statuses, forwarding paths.
- `registration-audit.tsv`: literal registration matches/mismatches and recommended cast ids.
- `after-fix/`: the same extraction plus the zero-mismatch audit of the tested checkout.

The source audit recognises local rawcode constants, literal/named order constants, insert/set_spell_order
argument pairs, and tuple pairs. It deliberately does not parse arbitrary Rust expressions or infer
dynamic registrations. A source mention or absence is not proof of support or lack of support.
Per-file SHA-256 values identify the source snapshot; rerun the audit after integrating changes.

## Important distinction: cast versus autocast/toggle

Do not replace every registered id with the primary `0x308` value. **Slow** (Aslo) returns
852076 at `0x308`, 852077 at `0x318`, and **852075 at `0x348`**. Its current manual cast id 852075
is correct. This distinction also matters for Heal, Bloodlust and other autocast families.
The shared order validator `0x6F037C00` compares all three roles; the spell message handler
`0x6F074C60` uses `0x348` to select the cast-start path.

Channel (ANcl) reads its order id from object field `+0x124`, so it is intentionally unresolved.
Divine Shield's cancel getter is state-dependent and its existing ORDER_UNDIVINE_SHIELD constant
has not been validated by this extractor. Keep that as a separate research task.

Other extracted values useful for remaining registration work: Death Pact AUdp -> 852223;
Carrion Swarm AUcs -> 852218. This does not establish that their lifecycle is fully implemented.

Next integration task: apply the tested patch, rerun the audit, and use the verified cast getter
for each new ability instead of copying an unverified order-id list. Continue using the ability atlas
for inherited methods, effect objects, timer routes and per-level data.
