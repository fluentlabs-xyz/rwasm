# rWasm (Reduced WebAssembly)

[![codecov](https://codecov.io/gh/fluentlabs-xyz/rwasm/graph/badge.svg?token=9T2PLQQW4L)](https://codecov.io/gh/fluentlabs-xyz/rwasm)

`rwasm` is a deterministic reduced WebAssembly execution format and runtime stack used for blockchain- and proof-oriented workloads.

It provides:

- a compiler path from Wasm to rWasm module representation
- an opcode-driven VM with explicit fuel accounting
- strategy abstraction for native rWasm and optional Wasmtime execution
- integration surfaces for host imports/syscalls

## Documentation

Primary technical docs live in [`docs/`](./docs/README.md):

- [Architecture](./docs/architecture.md)
- [Compilation & Execution Pipeline](./docs/pipeline.md)
- [Module Format](./docs/module-format.md)
- [VM, Fuel, and Tracing](./docs/vm-and-fuel.md)
- [Opcode Specification](./docs/opcodes.md)
- [Contributor Guide](./docs/contributor-guide.md)

## Repository layout

- `src/` — core compiler, module model, opcode types, VM runtime, strategy layer
- `e2e/` — end-to-end test harnesses (including testsuite submodule usage)
- `snippets/` — snippet-focused fixtures/tests (nightly toolchain)
- `examples/` — sample modules/programs
- `benches/` — Criterion benchmarks
- `.github/workflows/` — CI/CD pipelines

## Local development

### Prerequisites

- Rust stable
- Rust nightly `nightly-2025-09-20`
- wasm target `wasm32-unknown-unknown` on both toolchains
- `clang`, `libclang-dev`, `pkg-config`
- initialized git submodules

### Canonical commands

```bash
make build
make clippy
make test
```

## Notes on reproducibility

- CI assumes self-hosted runner label and system packages available via apt.
- `make` targets ensure wasm targets and submodule initialization before critical build/test paths.
- Feature gates (`fpu`, `wasmtime`, `tracing`, etc.) alter executable/runtime surface; pin features in production flows.

## License

[MIT](./LICENSE)
