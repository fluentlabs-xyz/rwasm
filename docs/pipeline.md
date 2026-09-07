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

`CompilationConfig::max_allowed_function_types` limits the declared type-section entry count to
4,096 by default, including duplicate signatures. `ModuleParser` checks this before wasmparser
allocates type storage and before rWasm's signature deduplication scans earlier entries. Oversized
sections return `CompilationError::TooManyFunctionTypes { count, limit }`; exactly the limit is
accepted. The default bounds deduplication to at most 8,386,560 signature comparisons per module.

Hosts can set an explicit limit with `with_max_allowed_function_types(...)`; raising it requires a
matching compilation resource budget. The rule applies to all rWasm compilation and export-parsing
entrypoints, independently of runtime fuel metering. Accepted modules retain their generated
bytecode and codegen identity: this is an input-validation rule, not a code-generation change.
The Wasmtime adapter still requires callers to validate untrusted input with rWasm's compilation
rules first, as documented on `StrategyDefinition::new_as_wasmtime`.

Integrations adopting this compiler must coordinate the new default as an input-compatibility
change. On-chain compilers must be rebuilt and upgraded to activate it; already compiled rWasm
artifacts are unchanged.

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
