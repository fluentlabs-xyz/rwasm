# Architecture

`rwasm` is a deterministic reduced execution model for WebAssembly workloads.

At a high level:

```text
Wasm binary
  -> parser + validator
  -> rWASM compiler/translator
  -> compact rWASM module
  -> execution strategy (native rWASM VM or Wasmtime compatibility)
```

## Major subsystems

- **Compiler** (`src/compiler/**`)
    - parses/validates wasm
    - rewrites to rWASM-oriented instruction stream
    - builds module metadata (func/table/data/global sections)

- **Module model** (`src/module/**`)
    - serialized representation consumed by the runtime
    - contains compiled funcs, signatures, globals, data/element segments, etc.

- **Instruction set** (`src/types/opcode.rs`)
    - defines opcode enum and instruction categories
    - includes optional FPU opcodes behind `fpu` feature

- **Runtime VM** (`src/vm/**`)
    - stack machine execution engine
    - memory/table/global handling
    - trap model + host import linkage
    - optional tracing support

- **Execution strategy abstraction** (`src/strategy/**`)
    - `Rwasm` strategy (native engine)
    - optional `Wasmtime` strategy for compatibility/comparison

## Design goals

- deterministic execution semantics
- compact executable representation
- predictable fuel accounting hooks
- host embedding via explicit syscall/import boundaries
- ZK-friendly layout and execution choices for proving-aware pipelines

## Feature gates (important)

Current cargo features (`Cargo.toml`):

- `default = ["std", "wasmtime"]`
- `std`: std support for crate/runtime dependencies
- `wasmtime`: enables wasmtime-backed strategy/execution
- `fpu`: enables floating-point opcode/runtime surface in rWASM VM
- `serde`: serde support for selected types
- `tracing`: tracing-related model support (depends on `serde`)
- `debug-print`: debug print surface
- `cache-compiled-artifacts`: enables artifact cache helpers (depends on `wasmtime`)
- `pooling-allocator`: optional allocator mode hooks
- `e2e`: e2e feature flag surface

Treat feature combinations as part of the runtime surface: always test the exact feature set you ship.
