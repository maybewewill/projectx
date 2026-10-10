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
