# VM, Fuel, and Tracing

This page describes runtime execution behavior in `rwasm`.

## Core runtime objects

- **`RwasmStore<T>`** (`src/vm/store.rs`)
  - linear memory
  - globals/tables
  - host context `T`
  - import linker + syscall handler
  - fuel accounting state
  - optional resumable context

- **`RwasmExecutor`** (`src/vm/executor.rs`)
  - drives opcode dispatch loop (`step`)
  - manages value stack + call stack transitions
  - performs trap handling and return-value extraction

- **`ImportLinker`** (`src/vm/import_linker.rs`)
  - resolves import names and system function indices
  - validates/records expected function signatures

## Fuel model

Fuel is consumed through `StoreTr::try_consume_fuel` and can be bounded by `fuel_limit`.

Behavior summary:

- `fuel_limit: None` => unbounded execution
- `fuel_limit: Some(x)` => execution traps with `OutOfFuel` if consumed fuel exceeds limit
- `remaining_fuel()` returns `None` for unbounded mode, else remaining units
- `reset_fuel(new_limit)` resets consumed counter to zero and applies new limit

Fuel is part of runtime policy and can be consumed by:

- explicit fuel opcodes (`ConsumeFuel`, `ConsumeFuelStack`)
- host/syscall operations through runtime wrappers/policies

## Traps and errors

Typical trap categories include:

- out-of-fuel
- memory/table bounds violations
- invalid indirect calls/signature mismatch
- explicit `Trap` opcode
- host syscall failures mapped into trap codes

Runtime clears/normalizes state for non-interruption trap paths before returning error.

## Resumable execution

`RwasmStore` can carry resumable context (`ReusableContext`) for interruption-style flows.
This enables host-driven pause/resume patterns where supported by caller logic.

## Tracing (`tracing` feature)

When enabled, tracer captures instruction/memory/table events and metadata.

Primary types live in `src/vm/tracer/**`:

- `Tracer`
- `TracerInstrState`
- memory access records/events

Use tracing for:

- execution debugging
- differential analysis
- instrumentation pipelines

## Operational recommendations

- Pin feature set (`wasmtime`, `fpu`, `tracing`) per environment.
- Treat host syscall determinism as part of consensus safety.
- For reproducible tests/CI, ensure wasm targets + submodules are initialized before run.
