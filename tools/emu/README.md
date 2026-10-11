# Differential tests against the original Game.dll (Unicorn)

`emu.py` maps the user's own `Game.dll` (1.26a) into a Unicorn x86 emulator and calls
individual functions. The `test_*.py` scripts compare those results with Python models of
the same behaviour (the models are what the Rust code is then written from).

```bash
pip install --target tools/emu/pylib unicorn pefile
GAME_DLL=/path/to/Game.dll python3 tools/emu/test_natives.py
```

Status (2026-10-10): RNG seeding, step, GetRandomInt, SetRandomSeed + 45 streams: 0 mismatches.
GetRandomReal: off by up to 1.2e-4 (the original's soft-float helpers are not modelled yet).

`verify_strings.py` (2026-10-10): R2I (`0x6f6eec70`, 21,626 cases) and the Game.dll MPQ-style
string hash (`0x6f68cc10` with the crypt table from `0x6f68cb00`, 33 cases) match the Python
transcriptions with 0 mismatches. The pairs it prints are embedded in `crates/jass/tests/natives_pure.rs`.
Storm.dll (StringHash's import) and msvcr80.dll (atoi) are not in the upload, so those were not run.

`text_vectors.py` records the text conversions from the original: R2S / R2SW (`0x6f6f0360`),
S2R (`0x6f6f0520`) and the lexer's real literal (`0x6f466690`). The helpers call MSVCR80
`isdigit`, `sprintf` (`%d`, `%*d`, `%0*d`) and `memset`; those IAT slots are pointed at Python
implementations (the loader does not bind imports). The Rust side is `crates/softfloat/src/text.rs`
and `crates/softfloat/tests/text.rs` (`SOFTFLOAT_TEXT_VECTORS=<json>`, `GAME_DLL`). The
`--embed-rs` output is `crates/softfloat/tests/text_embedded.rs`.

`handle_vectors.py` (2026-10-10): random create / add_ref / release sequences run through the
handle allocator harness (`handle_alloc_emu.py`). It records the returned ids, counts, table and
free-stack lengths and writes them into the generated block of `crates/sim/tests/handles.rs`:

```bash
GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn, pefile> \
  python3 tools/emu/handle_vectors.py --seed 1 --ops 800 --rs crates/sim/tests/handles.rs
```

Two sequences are written: `EMU_OPS` (mostly live ids) and `EMU_EDGE_OPS` (any issued id, so freed
ids are touched too; this produces duplicate free-stack entries). Object death is not in the sequence.

`pyramid_vectors.py` (2026-10-10): the pathing pyramid lookup `0x6F498A60` with its labeller `0x6F498A00`,
list append `0x6F4984E0` and copy `0x6F498010`. Random pyramids (`make_case`) with requests that bump the
generation or not, and random lookups at all four levels. After every lookup the return value, the
generation, the list count and capacity, every record (stamp, word) and every list entry are compared with
the Python model. The allocator `0x6F4B39C0` (Storm.dll, not in the upload) is replaced by a hook that
returns 1 and moves the list base. Writes the (seed, digest) pairs into
`crates/sim/tests/pathing/pyramid_data.rs`:

```bash
GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn, pefile> \
  python3 tools/emu/pyramid_vectors.py --cases 2000 --rs crates/sim/tests/pathing/pyramid_data.rs
```

`search_vectors.py` (2026-10-10): the region search. Each case is a random pyramid with a flag table and one
to three requests, driven through the request setup `0x6F49A640` (world points are passed as cell floats,
which the setup floors and truncates) and, when it returns 2, the start node and the pop loop
`0x6F49A240` / `0x6F49A170`. After every request the emulated node list, heap, closed list, pops, ids,
best distance and pyramid are compared with the model. The path spot values are recorded from the parent
links. Writes `crates/sim/tests/pathing/search_data.rs` (the emulator calls are not capped at 200,000
instructions, since a 400-pop search needs more):

```bash
GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn, pefile> \
  python3 tools/emu/search_vectors.py --cases 400 --rs crates/sim/tests/pathing/search_data.rs
```

`request_vectors.py` (2026-10-10): whole path requests through the dispatcher `0x6F49A800` (direct move
`0x6F49A5E0`, region search, waypoint output `0x6F49A470`, cell centre `0x6F497D30`). It extends the
region search model of `search_vectors.py` with the two-cell footprint (mode 2, size class 1): the footprint
probes, the mode-2 branches of the edge scanners and corners, and the block clamp. Starts are inside the
pyramid, fractional (eighths); goals may be outside. After every request the return code, the path object
(count and points, bit for bit), `S+0x90`, `S+0xA0` and the region search state are compared. The start-up
constants are set by running the image's own stubs (`0x6F84DD90`, `0x6F84DDF0`, `0x6F84DE00`,
`0x6F84DE80`) and by writing the marker value `0x6FAB7748` (S2R of the text "-128000.0", see the script).
The path object's allocator `0x6F4B39C0` is the same hook as in `pyramid_vectors.py`. Writes
`crates/sim/tests/pathing/request_data.rs`:

```bash
GAME_DLL=/path/to/Game.dll PYTHONPATH=<dir with unicorn, pefile> \
  python3 tools/emu/request_vectors.py --cases 600 --rs crates/sim/tests/pathing/request_data.rs
```

All scripts exit with status 1 on any mismatch and print the coverage counters of the run.

`pyramid_build_vectors.py` (2026-10-11): the pyramid builder from the cell grid. Runs the rect rebuild
`0x6F488330` (level A from 2x2 cells, levels B to D from the level below) on random grids, including odd
sizes, and compares every record word of levels A to D with the model of `docs/spec/pathing-build.md`.
It does not use `emu.call` (its 200,000-instruction cap stops a 40 x 40 rebuild early). Writes
`crates/sim/tests/pathing/build_data.rs`:

```bash
GAME_DLL=/path/to/Game.dll EMU_PYLIB=<dir with unicorn, pefile> \
  python3 tools/emu/pyramid_build_vectors.py --cases 300 --rs crates/sim/tests/pathing/build_data.rs
```

`EMU_PYLIB` is added to `sys.path` before the emulator is imported (use it instead of `PYTHONPATH`).
