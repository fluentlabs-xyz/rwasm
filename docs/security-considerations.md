# Security Considerations

This document summarizes the current security posture and threat boundaries of `rwasm`.

## Validation-first execution

`rwasm` execution starts from validated module input and structured translation paths.
Validation prevents malformed module structures from being accepted into runtime execution.

In practice, this means malformed or invalid wasm should fail validation/translation rather than reach arbitrary execution paths.

## Runtime safety properties

The runtime model is designed to fail safely through explicit traps and bounds checks.

Examples:

- invalid control-flow transitions trap
- stack misuse conditions are checked and trapped (no silent stack underflow/overflow corruption)
- memory/table operations are bounds-checked and trap on invalid access
- fuel-limited execution can terminate deterministically with out-of-fuel

So malicious inputs are expected to produce controlled failures (validation errors/traps), not undefined behavior execution.

## Panic-free entry points

A node embedding `rwasm` should not need a `catch_unwind` net around the guest-facing API. The
following entry points report every failure on guest-controlled input as a `Result`:

- compilation: `RwasmModule::compile`, `ModuleParser::parse`, `StrategyDefinition::new`,
  `new_as_rwasm`, `new_as_wasmtime`, `compile_wasmtime_module`, `compile_wasmtime_module_cached`
- decoding of serialized rWasm: `RwasmModule::new_checked`, `RwasmModule::new_checked_exact`
- instantiation: `StrategyDefinition::create_executor`, `ImportLinker::instantiate`,
  `WasmtimeExecutor::new`, `WasmtimeExecutor::try_new`
- execution: `ExecutionEngine::execute`, `ExecutionEngine::resume` (an error, not a panic, when
  there is no interrupted execution to resume), `StrategyExecutor::execute`,
  `StrategyExecutor::resume`, and the `StoreTr` memory and fuel accessors

The remaining panics are deliberate fail-fast checks on host-supplied configuration, documented on
each function: `RwasmModule::new` and `RwasmModule::serialize` (a trusted rWasm artifact that does
not decode or encode), `ImportLinker::insert_*` (duplicate import names or syscall indices in the
host's own linker), `GlobalMemory::new` and `ValueStack::new` (inconsistent host-chosen limits).
None of them is reachable from a module or from guest execution.

## Host boundary

Security of total execution depends on both VM semantics and host integrations:

- deterministic VM behavior alone is insufficient if host syscalls/imports are nondeterministic or unsafe
- host handlers must enforce their own input validation, resource limits, and side-effect controls

## Denial-of-service considerations

Potential DoS vectors remain relevant at integration layer:

- large inputs/modules
- expensive host calls
- unbounded execution without fuel limits

Mitigations:

- enforce module/section size limits
- use fuel limits for untrusted workloads
- add runtime and host-side timeouts/quotas

## Scope note

This document is not a formal security proof or audit statement.
It describes intended behavior based on current validation/runtime design and should be read alongside code, tests, and external audits.
