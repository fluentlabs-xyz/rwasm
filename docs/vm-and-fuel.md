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

### Engine alignment

The rwasm VM and the Wasmtime strategy (`wasmtime-rwasm`) implement one fuel policy; the
per-operator schedule lives in the shared `rwasm-fuel-policy` crate. Both engines also agree on
*when* fuel is charged and checked:

- Metering is eager and region based. Each straight-line region (function entry, loop header,
  `if` arm, `else` arm, the code after any `end`, the fall-through after `br_if`) is charged in
  full before its first instruction runs. rwasm does this with one `ConsumeFuel` per region;
  Wasmtime emits the same charge in native code. A trap therefore leaves the same counter behind
  on both engines.
- A charge that exceeds the remaining fuel is never applied: execution traps with `OutOfFuel` and
  the remaining fuel stays what it was before the region.
- `fuel_limit: None` is unbounded on both engines and `remaining_fuel()` returns `None`.
- `LinearFuelParams::param_index` and `QuadraticFuelParams::local_depth` address the imported
  function's parameters by position counted from the last parameter, independent of the stack
  slots those parameters occupy.

Only `consume_fuel_for_bulk_ops` and `consume_fuel_for_params_and_locals` remain rwasm-only; use
`CompilationConfig::default_strategy_compatible` for modules that may run on either engine. The
strategy-agnostic constructors (`StrategyDefinition::new`, `StrategyDefinition::new_as_wasmtime`,
`for_each_strategy`) reject a config that enables either flag with
`CompilationError::StrategyIncompatibleConfig`; only `StrategyDefinition::new_as_rwasm` and
`RwasmModule::compile` accept it, because the rwasm VM is the one engine that implements the
injections. `tests/fuel_alignment.rs` pins these invariants differentially.

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
