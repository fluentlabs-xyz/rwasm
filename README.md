<p align="center">
  <img src="assets/logo.png" alt="rwasm" width="440">
</p>

<p align="center">
  <strong>A reduced WebAssembly format and runtime for deterministic, metered, proof-friendly execution.</strong>
</p>

<p align="center">
  <a href="https://crates.io/crates/rwasm"><img src="https://img.shields.io/crates/v/rwasm.svg" alt="crates.io"></a>
  <a href="https://docs.rs/rwasm"><img src="https://img.shields.io/docsrs/rwasm" alt="docs.rs"></a>
  <a href="https://github.com/fluentlabs-xyz/rwasm/actions/workflows/ci.yml"><img src="https://github.com/fluentlabs-xyz/rwasm/actions/workflows/ci.yml/badge.svg?branch=devel" alt="CI"></a>
  <a href="https://codecov.io/gh/fluentlabs-xyz/rwasm"><img src="https://codecov.io/gh/fluentlabs-xyz/rwasm/graph/badge.svg?token=9T2PLQQW4L" alt="codecov"></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License: Apache-2.0"></a>
</p>

`rwasm` validates a standard WebAssembly binary and translates it into **rWasm**: a reduced, flat instruction
format that is cheaper to interpret, to hash, and to prove than the original. The resulting module runs on one of
two interchangeable backends: a built-in `no_std` interpreter, or native code through a Fluent-maintained
Wasmtime fork.

It is built for environments where every run must produce exactly the same result on every machine, such as
blockchains and zero-knowledge provers. Given the same rWasm bytecode, inputs, and runtime limits, execution is
deterministic whenever the host imports are.

rwasm is developed by [Fluent Labs](https://www.fluent.xyz) and is the WebAssembly execution core of the Fluent
network.

## Why rwasm

- **Flat, reduced instruction set.** Structured Wasm control flow (`block`, `loop`, `if`) is compiled away into
  relative branches, and every opcode carries its immediates inline. The interpreter loop stays small, and the
  semantics stay easy to model in a proving circuit. See the [opcode specification](./docs/opcodes.md).
- **Deterministic by construction.** Floating point compiles to a trap in production builds, malformed input
  fails validation, and invalid execution traps instead of corrupting state.
- **One fuel policy for both backends.** The interpreter and the Wasmtime backend share the
  [`rwasm-fuel-policy`](https://crates.io/crates/rwasm-fuel-policy) schedule and charge it eagerly per
  straight-line region. A module compiled with a strategy-compatible configuration burns the same fuel on
  either engine, and a trap leaves the same counter behind on both.
- **Two backends, one API.** Run the interpreter for portability and `no_std`, or Wasmtime for native speed.
  The choice is a Cargo feature, not a code change.
- **Compact, hashable modules.** A serialized rWasm module is self-contained, and its bytes are fully determined
  by the Wasm input and a 32-byte codegen identity.
- **Spec-tested, fuzzed, audited.** The end-to-end suite runs the WebAssembly spec test suite, the fuzzer
  runs differential checks against Wasmtime, and external security reviews are published in
  [`audits/`](./audits).

## Quick start

```toml
[dependencies]
rwasm = "0.5"
```

The example runs the `fib` module from [`examples/fib`](./examples/fib), which exports `main: (i32) -> i32`.
Build it and copy the output next to your source file, or substitute any Wasm module of your own:

```bash
cargo build --release --target wasm32-unknown-unknown --manifest-path examples/fib/Cargo.toml
cp examples/fib/target/wasm32-unknown-unknown/release/fib.wasm .
```

```rust
use rwasm::{
    always_failing_syscall_handler, CompilationConfig, ImportLinker, StoreTr, StrategyDefinition,
    Value,
};
use std::sync::Arc;

fn main() {
    // `fib.wasm` built above, or any Wasm module that exports `main: (i32) -> i32`.
    let wasm: &[u8] = include_bytes!("fib.wasm");

    // 1. Compile Wasm -> rWasm.
    //    `default_strategy_compatible` charges identical fuel on both execution
    //    backends, so use it whenever a module may run on either one.
    let config = CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_consume_fuel(true);
    let strategy = StrategyDefinition::new(config, wasm, None).expect("compilation failed");

    // 2. Instantiate: no host imports, an empty host context, and 1M units of fuel.
    let mut executor = strategy
        .create_executor(
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            Some(1_000_000), // fuel limit; `None` runs unmetered
            None,            // max memory pages; `None` keeps the default limit
        )
        .expect("instantiation failed");

    // 3. Run.
    let mut result = [Value::I32(0)];
    executor
        .execute("main", &[Value::I32(10)], &mut result)
        .expect("execution trapped");

    assert_eq!(result[0].i32(), Some(55));
    println!("fuel remaining: {:?}", executor.remaining_fuel());
}
```

For a `no_std` build that ships only the interpreter, disable the default features:

```toml
[dependencies]
rwasm = { version = "0.5", default-features = false }
```

## How it works

```text
Wasm binary
   │  parse + validate      wasmparser, rwasm feature set
   ▼
rWasm compiler             flat opcodes, fuel injection, entrypoint and syscall mapping
   │
   ▼
RwasmModule                compact, bincode-encoded, ready to instantiate
   │
   ├──▶ rwasm VM           built-in interpreter, no_std + alloc
   └──▶ Wasmtime backend   native code via wasmtime-rwasm (default)
```

`StrategyDefinition` is the entry point. It compiles a binary for whichever backend the crate was built with and
hands out a `StrategyExecutor` that exposes the same `execute`, `resume`, fuel, and memory API on both. Host
functions are declared through an `ImportLinker` and dispatched to your syscall handler, so the host side is
written once and runs on either engine.

The full walk-through lives in [pipeline.md](./docs/pipeline.md) and [architecture.md](./docs/architecture.md).

## Execution backends

|               | rwasm VM                                     | Wasmtime backend                   |
| ------------- | -------------------------------------------- | ---------------------------------- |
| Cargo feature | always available                             | `wasmtime` (enabled by default)    |
| Environment   | `no_std` + `alloc`                           | `std`                              |
| Execution     | interpreter                                  | native code                        |
| Best for      | portability, proving pipelines, minimal deps | throughput                         |

`StrategyDefinition::new` selects Wasmtime when the feature is enabled and the interpreter otherwise;
`new_as_rwasm` and `new_as_wasmtime` pick a backend explicitly.

Anything consensus-critical that may run on either backend must be compiled with a strategy-compatible
configuration such as `CompilationConfig::default_strategy_compatible()`. The plain `default()` enables two fuel
injections that only the interpreter implements, so the same module would burn different fuel on the two engines.
Details are in [vm-and-fuel.md](./docs/vm-and-fuel.md#engine-alignment).

## Fuel

Fuel bounds execution so that untrusted code terminates deterministically. Pass `Some(limit)` when creating an
executor and execution traps with `OutOfFuel` once the budget is exhausted; `None` runs unmetered.
`remaining_fuel()` and `reset_fuel()` on the executor let a host meter several calls against one budget.

Metering is eager and region-based: every straight-line region is charged in full before its first instruction
runs, and a charge that would exceed the remaining fuel is never applied. Both backends agree on when fuel is
charged and checked, which is what makes a trap reproducible across engines.

## Cargo features

| Feature                    | Default | What it does                                                                                                       |
| -------------------------- | :-----: | ------------------------------------------------------------------------------------------------------------------ |
| `std`                      |   yes   | Standard-library support. Disable for `no_std` builds (interpreter only).                                          |
| `wasmtime`                 |   yes   | Wasmtime execution backend; makes `StrategyDefinition::new` select it.                                             |
| `cache-compiled-artifacts` |         | On-disk cache for compiled Wasmtime artifacts. Implies `wasmtime`.                                                 |
| `serde`                    |         | `serde` implementations for selected types.                                                                        |
| `tracing`                  |         | Execution tracing. Implies `serde`.                                                                                |
| `debug-print`              |         | Debug printing from the VM.                                                                                        |
| `pooling-allocator`        |         | Pooling allocator hooks.                                                                                           |
| `full-wasm-mode`           |         | Full Wasm mode in the Wasmtime backend. Execution equivalence between backends is not guaranteed.                  |
| `fpu`                      |         | **Test and fuzz only.** Emits and executes float opcodes, which changes emitted bytecode and module hashes.        |
| `e2e`                      |         | **Test only.** Relaxations required by the spec test suite.                                                        |

Feature combinations are part of the runtime surface: test the exact set you ship, and never enable `fpu` or
`e2e` in a production build.

## Trust boundary

Serialized rWasm modules are trusted artifacts of the compilation pipeline. The VM assumes the bytecode was
produced from validated Wasm by a trusted compiler using the expected feature set, import linker, fuel policy, and
codegen identity.

Never execute arbitrary serialized rWasm received from users or network peers. Decoding checks only the binary
encoding, not the module structure, and the codegen identity is not part of the wire format. For untrusted
programs, validate and compile the original Wasm locally. If compiled modules are distributed, verify their
integrity and provenance and check the accompanying codegen identity before execution.

When integrating in production, pin the exact Cargo feature set, toolchain, compilation configuration, and codegen
identity. [security-considerations.md](./docs/security-considerations.md) covers the threat model in more depth.

## Documentation

| Document                                                        | Covers                                                          |
| --------------------------------------------------------------- | --------------------------------------------------------------- |
| [Architecture](./docs/architecture.md)                          | Subsystems, design goals, feature gates                         |
| [Compilation & Execution Pipeline](./docs/pipeline.md)          | Input to output, step by step                                   |
| [Module Format](./docs/module-format.md)                        | Binary header, section layout, codegen determinism              |
| [VM, Fuel, and Tracing](./docs/vm-and-fuel.md)                  | Runtime objects, fuel model, traps, resumable execution         |
| [Opcode Specification](./docs/opcodes.md)                       | The complete opcode catalog                                     |
| [Security Considerations](./docs/security-considerations.md)    | Validation, runtime safety, host boundary, DoS                  |
| [Contributor Guide](./docs/contributor-guide.md)                | Setup, canonical commands, change policy, PR checklist          |

API reference: [docs.rs/rwasm](https://docs.rs/rwasm).

## Development

Prerequisites:

- `rustup` with the `1.93` and `nightly-2025-09-20` toolchains
- the `wasm32-unknown-unknown` target for both toolchains
- `clang`, `libclang`, and `pkg-config` (on Debian and Ubuntu the package is `libclang-dev`)
- a POSIX environment with `make`

One-time setup:

```bash
rustup +1.93 target add wasm32-unknown-unknown
rustup +nightly-2025-09-20 target add wasm32-unknown-unknown
git submodule update --init --recursive
```

Canonical commands:

```bash
make build    # check the crate, build the examples and snippets
make clippy   # lint the crate, e2e, and snippets
make test     # unit, integration, e2e, snippet, and release nitro-verifier tests
```

`make test` runs the full WebAssembly spec suite and takes a while; `cargo test` alone covers the crate's own unit
and integration tests. Benchmarks live in [`benches/`](./benches) and run with `cargo bench`.

## Repository layout

| Path                  | Contents                                                                    |
| --------------------- | --------------------------------------------------------------------------- |
| `src/`                | compiler, module model, opcode types, VM, strategy layer, Wasmtime backend  |
| `tests/`              | integration and regression tests                                            |
| `e2e/`                | end-to-end harnesses and the WebAssembly spec test suite                    |
| `snippets/`           | snippet fixtures and tests built with the pinned nightly toolchain          |
| `fuzz/`               | fuzzing targets                                                             |
| `examples/`           | sample modules and programs                                                 |
| `benches/`            | Criterion benchmarks                                                        |
| `docs/`               | technical documentation                                                     |
| `audits/`             | external security audit reports                                             |
| `.github/workflows/`  | CI, clippy, coverage, benchmark, and publish workflows                      |

## Security audits

External reviews and their follow-up reports are kept in [`audits/`](./audits):

- [Cantina, 2026-02-23](./audits/Cantina_2026_02_23.pdf)
- [Audit report, 2026-05-01](./audits/2026-05-01-rwasm-audit.md)
- [Audit report, 2026-06-18](./audits/2026-06-18-rwasm-audit.md)
- [Audit report, 2026-08-07](./audits/2026-08-07-rwasm-audit.md)

## License

Licensed under the [Apache License, Version 2.0](./LICENSE).
