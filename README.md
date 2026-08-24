# rWasm (Reduced WebAssembly)

[![codecov](https://codecov.io/gh/fluentlabs-xyz/rwasm/graph/badge.svg?token=9T2PLQQW4L)](https://codecov.io/gh/fluentlabs-xyz/rwasm)

rWasm is a reduced WebAssembly format and runtime stack for execution environments that care about **performance**,
**predictability**, and **proof-friendliness**.

Given identical rWasm bytecode, inputs, and runtime limits, the VM core executes deterministically when host imports are
deterministic. It is designed to be **ZK-friendly**: execution semantics and representation choices aim to remain
efficient for both ordinary execution and proving-oriented pipelines.

---

## What this repository provides

- Wasm → rWasm compilation pipeline
- rWasm opcode model and module encoding
- built-in rWasm interpreter with fuel support
- execution strategy abstraction for the rWasm VM and Wasmtime backend (`wasmtime` is enabled by default)
- interfaces for host imports and syscalls

---

## Trust boundary

Serialized rWasm modules are trusted artifacts of the compilation pipeline. The VM assumes rWasm bytecode was produced
from validated Wasm by a trusted rWasm compiler using the expected feature set, import linker, fuel policy, and codegen
identity.

Do not treat arbitrary serialized rWasm bytes received from users or network peers as trusted input. Decoding validates
only the binary encoding, not the module structure, and the codegen identity is not embedded in the wire format. For
untrusted programs, validate and compile the original Wasm input locally. If a compiled rWasm module is distributed, the
host must verify its integrity and provenance and check its accompanying codegen identity before execution.

---

## Documentation

Start with [`docs/README.md`](./docs/README.md).

Core docs:

- [Architecture](./docs/architecture.md)
- [Compilation & Execution Pipeline](./docs/pipeline.md)
- [Module Format](./docs/module-format.md)
- [VM, Fuel, and Tracing](./docs/vm-and-fuel.md)
- [Opcode Specification](./docs/opcodes.md)
- [Security Considerations](./docs/security-considerations.md)
- [Contributor Guide](./docs/contributor-guide.md)

---

## Quick start (local)

### Prerequisites

- `rustup` with the `1.93` and `nightly-2025-09-20` toolchains installed
- the `wasm32-unknown-unknown` target installed for both toolchains
- a POSIX-compatible environment with `make`
- `clang`, `libclang`, and `pkg-config` (package names vary by platform; on Debian and Ubuntu, the `libclang`
  development package is named `libclang-dev`)
- Git

### Setup

```bash
rustup +1.93 target add wasm32-unknown-unknown
rustup +nightly-2025-09-20 target add wasm32-unknown-unknown
git submodule update --init --recursive
```

### Canonical commands

```bash
make build
make clippy
make test
```

---

## Feature notes

`Cargo.toml` defines the runtime surface via features. Important points:

- The default feature set enables `std` and `wasmtime`; `StrategyDefinition::new` therefore selects Wasmtime by default.
- Consensus-sensitive integrations that may execute with either backend must use a strategy-compatible configuration,
  such as `CompilationConfig::default_strategy_compatible()`, so fuel accounting does not depend on the selected
  strategy.
- `fpu` is a test/fuzz-only feature that enables compiler emission and execution of floating-point opcodes. It is
  disabled by default and must not be enabled in production. Enabling it can change emitted rWasm bytecode and module
  hashes.

When integrating in production, pin the exact Cargo feature set, toolchain, compilation configuration, and codegen
identity.

---

## Repository layout

- `src/` — compiler, module model, opcode types, VM, strategy layer, and Wasmtime integration
- `tests/` — integration and regression tests
- `e2e/` — end-to-end harnesses and WebAssembly testsuite integration
- `snippets/` — snippet fixtures and tests using the pinned nightly toolchain
- `fuzz/` — fuzzing targets
- `examples/` — sample modules and programs
- `benches/` — Criterion benchmarks
- `docs/` — technical documentation
- `audits/` — historical audit reports
- `.github/workflows/` — CI and publishing workflows

---

## License

[Apache 2.0](./LICENSE)
