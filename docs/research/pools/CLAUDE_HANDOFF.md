# Pool selection and mutation: verified against the original

Independent work completed on 2026-10-13. Apply `docs/research/pools/pool-selection.patch`.
Scope: the weighted selection core shared by unit/item pools and bounded pool add/remove operations.
This block does not overlap the hero, unit-spell, or item-ability lanes.

## Result

The existing Rust selection and mutation algorithms matched the original on all recorded cases.
No production algorithm correction was needed. The patch supplies an original-code harness,
derived vectors, independent Rust comparisons, and corrected module verification notes.
This replaces the previous reliance on tests that computed expectations from the model's own RNG.

Verified:

- **2,000 inputs through each original selector**: 4,000 original calls total.
- **2,000 original add/remove steps**: exact entry order, ids, count and weight bits.
- Rust matched the selected type and final global RNG state for both unit and item pool objects.
- Empty pools do not call the RNG or builder. Nonempty selection makes exactly one global RNG step
  before reaching the builder, even for zero, negative, NaN or infinite weights.
- Selected id `0xFFFFFFFF` falls back to entry 0; failure to pass any prefix also falls back to entry 0.
- Explicit generated states force `U=0`, `U=0.5` and the largest representable `U<1`.
  For equal weights at `U=0.5`, the first prefix is equal to the threshold and is skipped:
  the comparison is strictly greater.
- The unit helper forwards owner in ECX, selected id in EDX, and X/Y/facing pointers on the stack.
  The item helper forwards selected id in ECX, X in EDX, and Y on the stack.

## Original functions and harness boundaries

| Function | Address | Treatment |
|---|---|---|
| Unit weighted selector | 0x6F3E17B0 | original body executed |
| Item weighted selector | 0x6F3E0280 | original body executed |
| Add or replace entry | 0x6F3EFE00 | original body executed |
| Remove and shift entries | 0x6F3EFEB0 | original body executed |
| Global RNG step | 0x6F011390 | original body executed, calls counted |
| Unit builder | 0x6F24F1F0 | stubbed after recording selection and forwarded arguments |
| Item builder | 0x6F2B9940 | stubbed after recording selection and forwarded arguments |

Soft-float arithmetic and the x87 selection comparisons execute original code. The harness uses
the existing `emu.py`, including its startup-constant initialization. The final builders are the
only replaced selection callees; their return value is a fixed marker with the original stack cleanup.
Calls must return through the sentinel, and argument forwarding/draw counts are asserted.
The emulator hooks are restricted to the two builders and RNG entry.

The mutation pool is preallocated for 64 entries; the generated sequence has at most 34 distinct ids.
The original allocation/growth routine is not called or simulated. This deliberately verifies the
mutation semantics without pretending to verify allocation behaviour.

Cases cover signed zero, subnormals, normal/extreme finite weights, negative weights, infinities,
quiet/signalling NaNs, arbitrary float bits, sentinel ids, empty pools, and longer entry lists.
The source SHA-256 guard rejects an unrelated DLL before execution.

Original Game.dll SHA-256:
`6d21bd9a0f9fbc8446f455c9e89ac994fed68174426fe608fcb9baefd4dec53c`.
Committed derived vector file SHA-256:
`b21b0f7ea956babd3401f523c55f3b76befdca2cd61227ff66b338a8a542236c`.
No game binary bytes, RNG permutation table or decompiled bodies are included in the patch.

## Integrate and verify

The isolated checkout is `D:\rewar3\pool-selection`, branch `codex/pool-selection`,
based on `abc154a50e64e2ef3867509b5749a63e01f88581`.
Primary Claude gameplay files were not modified.

From `C:\Users\slash\projectx`:

```powershell
git apply --check docs/research/pools/pool-selection.patch
git apply docs/research/pools/pool-selection.patch
$env:GAME_DLL = 'C:\Users\slash\re\game.dll'
cargo test -p sim --lib natives::pool::tests -- --include-ignored
cargo test -p sim --test pool_natives
```

Results in the isolated checkout: **4 module tests + 5 native integration tests passed**.
The selector's original-table vector test is ignored by default because it requires GAME_DLL.
The command above explicitly runs it; it fails if GAME_DLL is missing instead of silently returning.
The mutation vectors require no runtime DLL and run as a normal test.
These tests check the whole recorded sequence, not just a sample of the vector file.

Reproduce vectors:

```powershell
$env:GAME_DLL = 'C:\Users\slash\re\game.dll'
py tools/emu/pool_selection_emu.py --cases 2000 --emit crates/sim/tests/data/pools/selection.txt
cargo test -p sim --lib natives::pool::tests -- --include-ignored
```

## Remaining scope

Full pool-native lifecycle equivalence is not claimed. Pool handle creation/destruction,
invalid-player handling, native facing conversion, growth allocation, and the downstream unit/item
builders were not compared with a live original world. Builders can have additional effects or RNG
consumption beyond the selector's one step. Full replay synchronization was not tested.

Next independent work can cover the pool wrappers and constructor/handle lifecycle, or the spatial
item enumerator, using this verified selection core as the reference boundary.
