# Changelog

All notable changes to rwasm, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Versions are `0.MINOR.PATCH` and describe rwasm **bytecode compatibility**, not the Rust API: the
minor version moves only when the instruction set changes (a module compiled by the new compiler may
not run on an older VM), the patch version for everything else. The rule and the release procedure
are in `.claude/skills/bump-version/SKILL.md`.

## [Unreleased]

No new opcodes, no change to the emitted bytes or the fuel schedule.

### Fixed
- Wasmtime backend: a `return_call` or `return_call_indirect` made after a plain call in the same
  frame read the stack counters that call had published for its callee, so a tail-recursive loop
  with a call in its body trapped `StackOverflow` after `N_MAX_RECURSION_DEPTH` iterations on
  Wasmtime while the rwasm VM ran it. A tail call now publishes the caller's own depth and base
  again, like `ReturnCallInternal`. Needs `wasmtime-rwasm` 45.0.0-rwasm.4; pinned by
  `tests/stack-overflow.rs`.

## [0.7.1] - 2026-09-25

No new opcodes. The bytes emitted for the same wasm change (a smaller `StackCheck` for a function
with dead code after `end`, the entries of active segments in the data and element tables, the
intrinsic in an import trampoline), so module hashes move while older artifacts still decode. The
fuel schedule is unchanged. The Wasmtime backend now traps and caps memory where the rwasm VM does:
a module that recursed past `N_MAX_RECURSION_DEPTH` or the value-stack window, or grew its memory
past the compile-time page cap, ran on Wasmtime before and traps on both engines now.

### Changed
- The Wasmtime backend emulates the rwasm stack limits (#213). Compiled code traps `StackOverflow`
  at `N_MAX_RECURSION_DEPTH` (1024) frames and when a frame does not fit the value-stack window,
  the hidden `i64` snippet frames and the import trampoline included, so both engines stop a call
  chain at the same frame (`tests/stack-overflow.rs`). Needs `wasmtime-rwasm` 45.0.0-rwasm.3
  (`Config::rwasm_stack_limits`); every module compiled for the Wasmtime engine carries the
  `rwasm.frames` custom section (`ModuleParser::frame_heights`). Call-heavy recursion runs 3% to
  5% slower under Wasmtime. Downstreams adapt: `compile_wasmtime_module` runs the rwasm
  translator first, so a config without an import linker no longer compiles a module with
  imports; `wasmtime_syscall_handler` takes the import's result types; `compile_wasmtime_module_on`
  compiles a module for `WasmtimeExecutor::instantiate` on the executor's engine, and
  `wasmtime_engine` is public.
- `WasmtimeExecutor` resolves a module's imports itself, one extern per import (#220): a global
  import of a module compiled with `default_imported_global_value` gets a fresh global holding the
  default (null for a reference), every other import the linker's definition of the same kind and
  type, so a module may import one name as a function and as a global, as on rwasm. The public
  `instance_pre` field is gone. `WasmtimeModule` carries the compiling config's
  `max_allowed_memory_pages` and `default_imported_global_value`.
- New compile-time error `SyscallFuelOutOfBounds`: a `SyscallFuelParams::Const` above `u32::MAX`
  used to be cut to its low 32 bits (#220). A named entrypoint whose signature carries a `funcref`
  or `externref` is `MalformedFuncType` unless `allow_func_ref_function_types` (#220).

### Fixed
- Backend divergence (#220): `memory.grow` on Wasmtime is bounded by the compile-time page cap as
  on rwasm, also across `instantiate`; host memory access to a module without memory succeeds for
  an empty range at offset 0 and traps for every other range on both engines; a reference-typed
  export no longer panics the Wasmtime executor, and a halted checked call reports the declared
  result types' zeros (`FuncRef(null)` for a `funcref`); a module importing globals links on
  Wasmtime with the configured default; an imported reference global starts null instead of
  pointing at a code offset.
- Compiler (#220): dead code after `end` no longer grows the function's `StackCheck`, which made
  a valid function exceed the window with enough dead blocks; active data and element segments
  record their position in the flattened section, so `memory.init` and `table.init` from one
  after `reset(false)` read the right bytes; the import trampoline reached by `ref.func`, element
  segments and `call_indirect` carries the intrinsic instead of making the syscall.
- VM (#220): `run_with_stack_check` cleans up its frames after a trap, so the next entry on the
  store no longer fails with `BadSignature`.
- `fpu` builds (#220): `f32/f64.min/max` return a quiet NaN for a signaling NaN operand, as the
  spec and Wasmtime do.

### Docs
- `docs/pipeline.md` describes the stack-limit emulation instead of the backend difference (#213).

## [0.7.0] - 2026-09-23

Instruction-set change: four new opcodes. Modules that use the wide-arithmetic operators need this
release or newer on every node.

### Added
- The WebAssembly wide-arithmetic proposal (#217). `i64.add128`, `i64.sub128`, `i64.mul_wide_s`
  and `i64.mul_wide_u` lower to the opcodes `I64Add128` (90), `I64Sub128` (91), `I64MulWideS` (92)
  and `I64MulWideU` (93), which work in place on their two-slot `i64` operands: no snippet call, no
  hidden frame, no stack headroom. The interpreter executes each as one 128-bit operation; the
  Wasmtime backend enables the proposal and Cranelift lowers it natively. Both engines charge the
  base fuel. Covered by the proposal's spec test (in `e2e`), a differential suite against native
  128-bit arithmetic, and a guest built by rustc with `-C target-feature=+wide-arithmetic` running
  256-bit and 384-bit Montgomery multiplications. Measured in fluentbase: BLS12-381 pairing 1.3x
  faster under Wasmtime and 1.8x in the interpreter; EVM 256-bit `MUL`/`DIV`/`MULMOD`/`EXP` 1.4x to
  3.9x under Wasmtime.

### Changed
- The compiler parses with `wasmparser` 0.248 instead of the `wasmparser-nostd` 0.100 fork (#216).
  `CompilationConfig::wasm_features` is an explicit union of feature flags pinned by a test, so a
  parser upgrade cannot widen the accepted language; the module parser hands the same set to the
  binary decoder, which now rejects an overlong `memory.grow` memory index as malformed, as
  Wasmtime does. `InstructionTranslator` no longer implements `VisitOperator`; `FuncBuilder` is the
  only place proposals are listed and rejects operators of unlisted proposals after validation.
  Unknown section kinds are rejected instead of skipped.
- `StrategyDefinition::Rwasm` carries `entrypoint_type`, the Wasm signature of the compiled
  entrypoint, and named host calls are validated against it (#211). Downstreams that construct the
  variant pass `None` for state-routed contracts.

### Fixed
- The compiler reserves the hidden frames it injects behind a Wasm instruction, so the emulated
  stack limits match on both backends (#212).

### Docs
- README table of WebAssembly proposal support (#215).

## [0.6.0] - 2026-09-16

Audit fixes (rounds of 2026-09-09, -12 and -13). No new opcodes; the bytes emitted for bulk-init
prologues and `table.grow` guards changed, so module hashes move while older artifacts still decode.

### Changed
- `StrategyDefinition::Rwasm` and `::Wasmtime` carry `entrypoint_name`; calling any other name
  fails with `UnknownExternalFunction` on both backends (#206, #207).
- `StrategyDefinition::new`, `new_as_wasmtime` and `for_each_strategy` reject configs with
  rwasm-only fuel injections (`StrategyIncompatibleConfig`) and modules without an exported memory
  (`MissingMemoryExport`); `new_as_rwasm` and `RwasmModule::compile` keep accepting them (#205).
- New compile-time limits: `StackHeightExceeded` (8192 slots per frame), `TableSizeExceedsLimit`
  (1024 elements), `CodeSizeExceeded` (`max_code_len`, 2 Mi instructions) and
  `InvalidSyscallFuelParam` (#204, #205).
- `ExecutionEngine` is a unit struct (the `spin` dependency is gone), `WasmtimeExecutor::instance`
  is private and `deserialize_wasmtime_module` is `unsafe` (#206, #207).

### Fixed
- Instance state is isolated between executions and syscall results are validated (#207).
- Syscall stack handling and halt semantics (#206).
- Compilation and runtime bounds hardening (#205); the 2026-09-09 audit findings (#204).

### Docs
- README rework with the logo (#203).

## [0.5.0] - 2026-09-10

Instruction-set change: fused 64-bit opcodes. Bytecode shrinks 16% to 21% on the fluentbase
guests; the wasmtime call overhead drops 6.5x and the interpreter runs EVM and keccak workloads
2x to 2.9x faster.

### Added
- `I32And64`, `I32Or64`, `I32Xor64` fuse the `i64` bitwise operators (#189), `I32Sub64` shrinks
  `i64.sub` to 11 instructions (#187), `I64Const32S`/`I64Const32U` emit an `i64.const` as one
  instruction (#190).
- `max_allowed_function_types` bounds the type section (#200).

### Performance
- Hand-written `i64` shift and rotate snippets, 778 to 170 instructions (#184); a shared
  `udivmod64` core for the four div/rem snippets (#188); `i64.eqz` in two instructions and
  branchless ordered `i64` compares (#185).
- Wasmtime backend: cached exports, no fuel-flag lookup per call, raw value marshalling (#201).

### Fixed
- Stack accounting for inline `i64` snippet expansion (#186).
- Trap when the initial memory exceeds the store page limit, with an inclusive limit (#195, #196);
  trap on `memory.init` from an empty passive segment (#194).
- Fuel policy aligned with `wasmtime-rwasm` 45.0.0-rwasm.2 (#202); low-severity audit findings (#197).

### Docs
- rWasm behaviour and trust model; production coverage above 95% (#193).

## [0.4.8] - 2026-08-24

### Changed
- `CompilationConfig::default_strategy_compatible` for configs whose fuel accounting must not
  depend on the strategy; fail-fast documentation; module verification at load dropped (#182).

## [0.4.7] - 2026-08-11

### Added
- `CompilationConfig::codegen_identity`, a fingerprint of the config and cargo features that
  change emitted bytecode (#177).

### Fixed
- The validator disables every unimplemented proposal and the translator rejects their opcodes,
  instead of silently skipping them (#173).
- Section allocations are bounded while decoding a module (#174); unallocated tables read as empty
  instead of panicking (#175); the dropped-segment bitset only grows (#172); stack out-of-bounds
  traps at every executor exit (#178).

### Docs
- Overflow risks in bulk-op fuel and bounds prologues (#176).

## [0.4.6] - 2026-08-06

### Fixed
- The bulk fuel config is respected for the initial memory (#170).

## [0.4.5] - 2026-07-17

### Fixed
- Decoded modules are verified (#162); bulk operations are metered by default (#163); partial
  source-pc fields are rejected (#164); audit follow-ups (#154).

## [0.4.4] - 2026-06-16

### Changed
- `hashbrown` 0.17 and `wasmtime-rwasm` 45.

### Fixed
- Compiler audit findings (#153); the unsafe opcode cast is gone (#149); Wasmtime memory reads are
  validated before allocation (#150).
- Differential fuzzer: harness, table snapshots, deterministic funcref arguments, memory snapshots
  (#134, #135, #138, #139).

### Docs
- Agent instructions (#151).

## [0.4.3] - 2026-04-02

### Changed
- Dependencies come from crates.io and the crate is published from CI on `v*` tags.

## [0.4.2] - 2026-03-26

### Fixed
- Wasmtime enforces the disabled-opcode rule (#130).

## [0.4.1] - 2026-03-26

### Fixed
- Memory writes for stores (#129); a potential overflow in memory bounds checks (#115).

### Changed
- License is Apache 2.0 (#128); `memory_read_into_vec` (#127).

## [0.4.0] - 2026-03-02

First release without a `-dev` suffix, published as `rwasm`.

### Added
- Syscall fuel parameters (constant, linear, quadratic) on the import linker, charged identically
  by the rwasm and Wasmtime strategies, with execution synced across strategies (gas and state
  transition); fuel constants moved to `rwasm-fuel-policy`.
- `BulkConst` and `BulkDrop` opcodes, closing a DoS on locals (#117); fuel for function locals (#97).
- Configurable maximum memory pages (#118); a bound on the serialized module size (#106, #108);
  a stateless Wasmtime runtime (#113).

### Fixed
- `i64.div_u`/`i64.rem_u` (#96); `ref.null` tracked as `i32` (#103); `visit_else` function type
  resolution (#100); `FuelCosts::costs_per` rounds up (#102); fuel rounding on memory and table
  operations (#89); stack length verified before execution (#110); the start section always runs
  before `main` (#104).

### Docs
- Cantina audit report; documentation and quality gates (#121); examples and fuzzer (#116).

## [0.3.2-dev] - 2025-10-23

Published as `fluent-rwasm`.

### Added
- `i64` snippets replace the `i64` instructions to shrink binaries (#32); module view (#54);
  intrinsics that replace or remove an import call (#34); serde support (#52).

### Performance
- Fiber-based context switching for Wasmtime (#38); pooling allocator and store caching (#46);
  `bitvec` for dropped segments (#41); benchmarks in CI (#50).

### Fixed
- No fuel for the entrypoint and one fuel config for all runtimes (#49); stack sync during
  syscalls (#40); stores and engines are `Send + Sync` (#42).

## [0.3.1-dev] - 2025-07-18

### Fixed
- Stack height calculation (#30); returning interruptions for wasmi (#29).

## [0.3.0-dev] - 2025-07-17

### Added
- 64-bit helper opcodes for faster `i64` operations (#25); a wasmi execution environment (#28);
  a Wasmtime thread pool (#26) and module cache (#23).

### Fixed
- Stack height calculation and stack sync (#27).

## [0.2.2-dev] - 2025-07-03

### Added
- Resumable execution and stack optimization (#22).

## [0.2.1-dev] - 2025-07-02

### Changed
- Caching behind a feature flag.

## [0.2.0-dev] - 2025-07-01

### Added
- The new rWasm compiler (#13), the Wasmtime adapter (#16), the gas model (#17), the tracer (#15),
  fuel for imported builtins (#19), a Wasmtime fork that disables FPU opcodes (#20), the original
  wasm embedded as a module section, trampolines for imported functions.

### Fixed
- Stack overflow handling in the execution engine (#21).

## [0.1.0-dev] - 2025-05-15

### Added
- The 32-bit translator for ZK-friendly execution (#1), the executor integrated with the
  fluentbase runtime (#6), fuel procedures for builtins (#8), `no_std` builds.

[0.7.0]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.4.8...v0.5.0
[0.4.8]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.4.7...v0.4.8
[0.4.7]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.4.6...v0.4.7
[0.4.6]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.4.5...v0.4.6
[0.4.5]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.4.4...v0.4.5
[0.4.4]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.4.3...v0.4.4
[0.4.3]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.3.2-dev...v0.4.0
[0.3.2-dev]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.3.1-dev...v0.3.2-dev
[0.3.1-dev]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.3.0-dev...v0.3.1-dev
[0.3.0-dev]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.2.2-dev...v0.3.0-dev
[0.2.2-dev]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.2.1-dev...v0.2.2-dev
[0.2.1-dev]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.2.0-dev...v0.2.1-dev
[0.2.0-dev]: https://github.com/fluentlabs-xyz/rwasm/compare/v0.1.0-dev...v0.2.0-dev
[0.1.0-dev]: https://github.com/fluentlabs-xyz/rwasm/releases/tag/v0.1.0-dev
