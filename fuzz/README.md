# rwasm differential fuzzing

This fuzz target compares **rwasm** execution against **wasmtime** for the same generated module and function calls.

## What it checks

For each generated module/export invocation, the harness compares:

1. return values,
2. trap-vs-success behavior,
3. post-execution store state (exported memory/global/table views),
4. **remaining fuel** (and therefore consumed fuel) between engines.

Memory comparison is done directly on exported memory views.
For compatibility with reserved-memory implementation differences, trailing all-zero extension bytes are treated as equivalent.

Global comparison is performed on materialized global-word state in rwasm store; modules where required global words are not materialized in this view are skipped as unsupported subset.

Fuel is reset to the same `FUEL_LIMIT` **before each compared invocation** on both sides.
The harness compares per-call consumed fuel deltas.
If consumed fuel differs, the target fails.

---

## Why some binaries are excluded

Not all Wasm/module features are currently supported by rwasm compiler/runtime.

The harness handles this in two ways:

1. **Generation constraints (preferred path)**
   - no imports (`max_imports = 0`),
   - no multi-memory (`max_memories = 1`),
   - single-table subset (`max_tables = 1`),
   - no GC / exceptions / threads / SIMD / memory64 / relaxed-SIMD / custom page sizes,
   - only proposals currently enabled in the differential subset.

2. **Compiler-side unsupported filter (safety net)**
   - if `RwasmModule::compile` returns known unsupported-feature errors
     (for example unsupported extension/import/type categories, non-default memory index, missing entrypoint),
     the module is skipped from differential comparison.

3. **Store-comparison guard (fallback safety)**
   - if exported state mapping cannot be resolved on one side for a generated module,
     that module is treated as outside the currently comparable subset and skipped (instead of panic/crash).

4. **Memory grow exclusion (current subset policy)**
   - modules containing `memory.grow` are currently excluded from differential comparison in this harness subset.

This keeps fuzzing focused on the shared supported execution subset.

---

## Prerequisites

- Rust toolchain installed
- **nightly toolchain** for libFuzzer sanitizer flags:

```bash
rustup toolchain install nightly
```

- `cargo-fuzz` installed:

```bash
cargo install cargo-fuzz
```

---

## Run fuzzing

From repo root:

```bash
cd fuzz
cargo +nightly fuzz run differential
```

Run a bounded smoke session:

```bash
cd fuzz
cargo +nightly fuzz run differential -- -runs=200
```

Recommended while iterating locally:

```bash
cd fuzz
RUST_LOG=info cargo +nightly fuzz run differential -- -runs=200
```

---

## Reproduce from an artifact

Convert a crash artifact to generated `.wasm`:

```bash
cargo run --manifest-path fuzz/Cargo.toml --bin repro_artifact -- fuzz/artifacts/differential/<artifact> repro.wasm
```

Then inspect:

```bash
wasm-tools print repro.wasm
```

---

## Notes

- This harness uses `wasm-smith` generation plus explicit filtering to stay within rwasm-supported behavior.
- Fuel parity is enforced by comparing per-invocation consumed fuel deltas.
- If you broaden generator features, re-check rwasm compiler support first (see `src/compiler/*`).
