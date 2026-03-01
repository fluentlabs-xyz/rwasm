# Compilation & Execution Pipeline

This is the end-to-end flow from input Wasm to runtime execution.

## 1) Input

- A `.wasm` module is loaded by compiler entrypoints.
- Validation/parsing uses wasmparser-based infrastructure.

## 2) Compilation/translation

Core components live in `src/compiler/**`.

Key responsibilities:

- map wasm control flow to rWASM branch model
- normalize stack behavior
- emit compact opcode stream (`Opcode` enum)
- emit metadata needed for runtime (signatures, globals, segments)

## 3) Module construction

`src/module/**` materializes `RwasmModule` / builder outputs:

- function bodies
- imports/exports
- table/data/element sections
- execution metadata required by the VM

## 4) Executor creation

Via strategy layer (`src/strategy/**`):

- native rWASM VM executor, or
- wasmtime-backed executor (feature-gated)

Executors are created with:

- import linker
- host context/state
- optional fuel limit
- optional tracer

## 5) Runtime execution

VM (`src/vm/**`) runs instruction stream:

- value stack + call stack transitions
- memory/table/global operations
- control flow branch handling
- host/syscall boundaries via import linker

## 6) Output

Execution returns:

- completion/return values, or
- trap/error code

Optionally with tracing data if tracing is enabled.

## Build/test pipeline prerequisites

Before `make build`, `make clippy`, or `make test`:

- ensure wasm target on stable + pinned nightly
- initialize submodules (`e2e/testsuite`)

This is encoded in Makefile helper targets (`ensure-wasm-targets`, `ensure-submodules`).

## Determinism notes

Determinism is guaranteed by VM semantics + host behavior together.
If host imports are nondeterministic, total execution is nondeterministic regardless of VM core.
