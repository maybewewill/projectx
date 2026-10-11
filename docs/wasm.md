# wasm32 build

The simulation stack builds for `wasm32-unknown-unknown`. The headless entry crate `crates/web` exposes
the boot and the step to JavaScript. It has no rendering and no file system access: the caller passes bytes.

## Checked crates

`tools/check_wasm.ps1` builds these for `wasm32-unknown-unknown`:

mpq, w3g, jass, softfloat, sim, gamedata, w3map, blp, mdx, fdf, engine, and the release build of web.

No crate needed source changes. The library code has no `std::fs`, threads or system time. The remaining
`std::fs` and `std::env` uses are in `#[cfg(test)]` code (they read `GAME_DLL` and vector files) and in the
native `w3boot` binary, which is not built for wasm.

## Commands

From the repository root:

```sh
rustup target add wasm32-unknown-unknown
cargo build -j 2 -p engine --target wasm32-unknown-unknown
cargo build -j 2 -p web --target wasm32-unknown-unknown --release
```

Or run all of them with the script (PowerShell 5.1 or later):

```powershell
powershell -ExecutionPolicy Bypass -File tools/check_wasm.ps1
```

The release artifact is `target/wasm32-unknown-unknown/release/web.wasm` (about 4.0 MB unoptimised,
built with the default release profile).

## JS glue (optional)

The glue is generated with `wasm-bindgen-cli`, which must match the library version in `Cargo.lock`
(0.2.129):

```sh
cargo install wasm-bindgen-cli --version 0.2.129 --locked
wasm-bindgen --target web --out-dir target/web-js target/wasm32-unknown-unknown/release/web.wasm
```

`tools/check_wasm.ps1` runs this step only when `wasm-bindgen` is on `PATH`. The output is in
`target/web-js` and is not committed.

## JavaScript API (`crates/web`)

| Function | Returns |
|---|---|
| `boot(map: Uint8Array, mpqs: Uint8Array[], gameDll?: Uint8Array)` | the boot report (same text as `w3boot`) |
| `step(n: number)` | one line: batches, ticks, timers fired, game time. Each batch is 20 game ticks |
| `report()` | the full report of the booted engine |

`mpqs` are the game archives in load order (`War3.mpq`, `War3x.mpq`, ...). A patch archive goes last, because
later archives override earlier ones. Without `gameDll` the soft-float tables and the RNG table are missing,
as with `w3boot` without `--dll`.

Only one map is booted at a time. A second `boot` replaces the first engine.

## Status

- Build: all listed crates and `web` pass for wasm32 (checked with the script).
- Run: not run in a browser or in Node. The exports were checked with Node's `WebAssembly.Module` only.
  Without the bindgen CLI the exported names carry a hash suffix. The generated glue gives the plain names.
- Size: `web.wasm` is 4,020,482 bytes without `wasm-opt`.
