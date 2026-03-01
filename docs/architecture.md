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

## Feature gates (important)

- `wasmtime`: enables wasmtime-backed execution paths
- `fpu`: enables floating-point opcode surface
- `serde`: serialization for selected types
- `tracing`: runtime tracing/event capture

Treat feature combinations as part of the runtime surface: always test the exact feature set you ship.
