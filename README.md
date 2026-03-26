# rWasm (Reduced WebAssembly)

[![codecov](https://codecov.io/gh/fluentlabs-xyz/rwasm/graph/badge.svg?token=9T2PLQQW4L)](https://codecov.io/gh/fluentlabs-xyz/rwasm)

`rwasm` is a deterministic reduced WebAssembly format + runtime stack for execution environments that care about *
*performance**, **predictability**, and **proof-friendliness**.

It is designed to be **ZK-friendly**: execution semantics and representation choices aim to stay efficient both for
normal execution and proving-oriented pipelines.

---

## What this repository provides

- Wasm → rWasm compilation pipeline
- rWasm opcode model and module encoding
- native rWasm VM runtime with fuel support
- strategy abstraction for native execution and optional Wasmtime backend
- host import/syscall integration surfaces

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

- Rust stable
- Rust nightly `nightly-2025-09-20`
- wasm target `wasm32-unknown-unknown` on both toolchains
- `clang`, `libclang-dev`, `pkg-config`
- initialized git submodules

### Setup

```bash
rustup target add wasm32-unknown-unknown
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

- default enables `std`, `wasmtime`, `disable-fpu`
- `fpu` exists as a feature-gated surface in code
- FPU opcodes are currently not treated as production-facing opcode surface in docs (kept mainly for testsuite/internal
  compatibility)

When integrating in production, pin exact feature set and toolchain.

---

## Repository layout

- `src/` — compiler, module model, opcode types, VM, strategy layer
- `e2e/` — end-to-end harnesses and testsuite integration
- `snippets/` — snippet fixtures/tests (nightly path)
- `examples/` — sample modules/programs
- `benches/` — Criterion benchmarks
- `.github/workflows/` — CI/CD workflows

---

## License

[Apache 2.0](./LICENSE)
