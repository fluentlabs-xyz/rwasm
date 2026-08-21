# rWasm Security Audit — 2026-08-07

- **Date:** 2026-08-07 (fixes landed 2026-08-09 → 2026-08-11; several items remain open)
- **Repository:** `fluentlabs-xyz/rwasm`
- **Audited commit:** `b8f60916` (v0.4.6, branch `devel`)
- **Linear:** parent `FLU-1093`; subtasks `FLU-1094`…`FLU-1108`
- **Fix PRs:** #171–#177 (see per-finding)
- **Focus:** End-to-end pass over the compiler, ISA, module format + verification,
  VM, strategy layer, and Wasmtime integration. Baseline `clippy`/`cargo test`
  clean; every finding is a coverage gap, not a regression.

## Scope

Two untrusted inputs reach this crate in a Fluent node: (1) untrusted wasm given to
`RwasmModule::compile` / `StrategyDefinition::new` (deployment); (2) untrusted rwasm
binaries given to `RwasmModule::new_verified*` — the entry point added in #162 so
decoded modules can be trusted. Ratings: **CRIT** = memory unsafety or
consensus-level miscompilation; **HIGH** = remotely reachable panic/DoS or state
divergence; **MID** = broken defense-in-depth, config hazards, latent correctness
traps.

## Result

| Crit | High | Medium |
| --- | --- | --- |
| 2 | 6 | 7 |

Status at report time: CRIT-1/2, HIGH-3/4/7, MID-11/14 fixed; HIGH-5/6/8 and
MID-9/10/12/13/15 remain Backlog.

## Findings

### Critical

#### FLU-1094 — Module verification does not bound local depths or stack pops → OOB value-stack read/write

- **Severity:** Critical · **Status:** Fixed · **Linear:** `FLU-1094` · **Fix:** #171
- **Where:** `src/module/verification.rs` (local-depth check only tests non-zero;
  pops unchecked); `src/vm/value_stack.rs` (`inc_by`/`dec_by` guarded only by
  `debug_assert!`, compiled out in release).
- **Impact:** A module accepted by `new_verified` can read/write memory outside the
  value stack — **confirmed SIGSEGV in release**. `LocalSet`/`LocalTee` write through
  the same unchecked pointer, making it an arbitrary-write primitive against a
  `SmallVec` in the caller's native stack frame.
- **Remediation:** Abstract-interpretation pass in `verify_module` tracking
  per-instruction stack height and rejecting over-pops / out-of-range local depths,
  plus release-mode bounds on value-stack accesses. Documented that `new_checked`
  performs no structural validation.

#### FLU-1095 — SIMD enabled in the validator but unimplemented in the translator → emulated stack desync

- **Severity:** Critical · **Status:** Fixed · **Linear:** `FLU-1095` · **Fix:** #173
- **Where:** `src/compiler/config.rs` (`wasm_features` fills unspecified proposals
  from `Default::default()`; `wasmparser-nostd` defaults `simd` on);
  `src/compiler/func_builder.rs` (wildcard arm validates-and-translates nothing).
- **Impact:** Untrusted wasm with a SIMD instruction passes validation, is silently
  skipped by the translator, and desyncs the emulated value stack → confirmed
  compiler panic (deploy DoS); latent **silent miscompilation** of `drop_keep` /
  local depths, which per FLU-1094 the interpreter didn't bounds-check.
- **Remediation:** Set every unsupported proposal to `false`; make the wildcard arm
  return `Err(NotSupportedOpcode)`; add a compile-rejection test per disabled
  proposal so a wasmparser bump fails CI.

### High

#### FLU-1096 — `InstructionSet::decode` allocates an attacker-controlled capacity

- **Severity:** High · **Status:** Fixed · **Linear:** `FLU-1096` · **Fix:** #174
- **Where:** `src/isa/mod.rs` hand-written `Decode` reads `length: u64` and passes it
  to `Vec::with_capacity` with no cross-check, bypassing bincode's
  `claim_container_read`.
- **Impact:** An 11-byte rwasm binary aborts/OOMs any process that decodes it,
  reachable from every decode entry point before verification runs.
- **Remediation:** Reserve incrementally with `claim_container_read` / `try_reserve`;
  reject lengths unbacked by remaining input; add `N_MAX_CODE_SECTION_LEN`. (Re-find
  of the canceled RWASM-04 / `FLU-950` from the 2026-07-10 audit.)

#### FLU-1097 — Verified modules can panic the interpreter (unknown syscall index, unallocated table index)

- **Severity:** High · **Status:** Fixed · **Linear:** `FLU-1097` · **Fix:** #175
- **Where:** `src/vm/executor.rs` (`unreachable!` on unresolvable `sys_func_idx`) and
  nine `.expect()` table-op call sites (index never grown; `verify_table_index`
  accepts any index `< N_MAX_TABLES`).
- **Impact:** Two one-instruction modules that pass `new_verified` panic mid-execution
  instead of trapping, leaving `RwasmStore` dirty if caught upstream.
- **Remediation:** Return `TrapCode::UnknownExternalFunction` for unknown syscalls;
  treat unallocated tables as empty across all call sites.

#### FLU-1098 — `StrategyDefinition::new_as_wasmtime` panics on compile failure; default path skips rwasm validation

- **Severity:** High · **Status:** Open (Backlog) · **Linear:** `FLU-1098` · **Fix:** —
- **Where:** `src/strategy/module.rs` (`.expect()` on compile); default features
  route only through wasmtime, so `RwasmModule::compile` never runs.
- **Impact:** Malformed wasm panics `StrategyDefinition::new` (confirmed with 12
  bytes); the accepted language becomes whatever wasmtime 45 accepts, so a module can
  execute under the Wasmtime strategy while uncompilable under rwasm.
- **Remediation:** Propagate the error; run rwasm validation on the wasmtime path (or
  pin wasmtime `Config` to `wasm_features()`) so both strategies agree; add negative
  tests for both feature configs.

#### FLU-1099 — `WasmtimeExecutor::new` panics on instantiation failure

- **Severity:** High · **Status:** Open (Backlog) · **Linear:** `FLU-1099` · **Fix:** —
- **Where:** `src/wasmtime/instance.rs` (`panic!` on pre-instantiate/instantiate);
  signature returns `Self`, not `Result`.
- **Impact:** A module whose start function traps crashes the process (confirmed);
  same for unresolved imports and hitting the memory limiter — all attacker-reachable.
- **Remediation:** Change to `Result<Self, TrapCode>` and map failures through
  `map_wasmtime_error`; test trapping start, unresolved import, oversized data
  segment.

#### FLU-1100 — `data.drop`/`elem.drop` truncate the dropped-segment bitset

- **Severity:** High · **Status:** Fixed · **Linear:** `FLU-1100` · **Fix:** #172
- **Where:** `src/vm/executor/memory.rs` (`BitVec::resize` shrinks); identical
  `visit_element_drop`.
- **Impact:** Dropping a lower index after a higher one erases the higher segment's
  dropped record; a later `memory.init`/`table.init` then copies real data where the
  spec requires a zero-length trap → rwasm/wasmtime state divergence (confirmed).
- **Remediation:** Grow-only bitset update in both ops; extend the differential
  fuzzer to generate descending drop indices.

#### FLU-1101 — Default `CompilationConfig` charges different fuel on rwasm vs Wasmtime strategy

- **Severity:** High · **Status:** Open (Backlog) · **Linear:** `FLU-1101` · **Fix:** —
- **Where:** `src/compiler/config.rs` (`consume_fuel_for_bulk_ops` /
  `consume_fuel_for_params_and_locals` default true but honored only by the rwasm
  compiler).
- **Impact:** The same module consumes different fuel depending on strategy → a
  consensus split appearing only under specific workloads; combined with FLU-1098 the
  shipped default routes through wasmtime.
- **Remediation:** Reject the divergent combination in `new_as_wasmtime`, or implement
  equivalent metering in the wasmtime fork; add a differential fuel test.
  (Continuation of F2/F3/F13 from the 2026-06-18 audit.)

### Medium

#### FLU-1102 — `unsafe wasmtime::Module::deserialize` behind a safe public API, no artifact authentication

- **Severity:** Medium · **Status:** Open (Backlog) · **Linear:** `FLU-1102` · **Fix:** —
- **Where:** `src/wasmtime/mod.rs` (`deserialize_wasmtime_module` safe `pub fn`; no
  `SAFETY` note); on-disk artifact cache read back unauthenticated.
- **Impact:** Arbitrary bytes to wasmtime's `unsafe` deserializer is native-code load
  with no validation → attacker-influenced bytes get code execution, not a parse error.
- **Remediation:** Mark `unsafe`/document the trust precondition, or authenticate
  artifacts (keyed MAC); gate behind a non-default feature. (Continuation of F6 from
  the 2026-06-18 audit.)

#### FLU-1103 — Wasmtime engine and module caches keyed without `CompilationConfig`

- **Severity:** Medium · **Status:** Open (Backlog) · **Linear:** `FLU-1103` · **Fix:** —
- **Where:** `src/wasmtime/mod.rs` (module LRU keyed by wasm key alone),
  `src/wasmtime/engine.rs` (`OnceLock` pins the first caller's config).
- **Impact:** A module/engine built under one config is handed to a caller expecting
  another → wrong fuel/memory limits (state divergence by process order).
- **Remediation:** Include a config hash in the cache key; make the shared engine
  reject or ignore-explicitly a differing config.

#### FLU-1104 — Bulk-op fuel and bounds prologues use signed comparisons and wrapping i32 adds

- **Severity:** Medium · **Status:** Fixed (documented) · **Linear:** `FLU-1104` · **Fix:** #176
- **Where:** `src/isa/memory.rs`, `src/isa/table.rs` (injected guards use
  `op_i32_gt_s` on unsigned quantities and a wrapping `n + 63` round-up).
- **Impact:** Guards can be defeated by overflow (`n = 0xFFFF_FFFF` collapses the fuel
  charge to 0); not exploitable today because the underlying op traps out of bounds,
  so safety rests entirely on the runtime bounds check.
- **Remediation:** Filed fix documents the overflow risk (PR #176); full fix is
  `op_i32_gt_u` + an overflow-safe round-up.

#### FLU-1105 — Reference syscall handler allocates an attacker-controlled length before validating

- **Severity:** Medium · **Status:** Open (Backlog) · **Linear:** `FLU-1105` · **Fix:** —
- **Where:** `src/vm/handler.rs` (`#[allow(dead_code)]` reference impl; `vec![0u8;
  length]` before `memory_read`).
- **Impact:** Reintroduces the exact bug fixed for the wasmtime path in #150; no
  production impact, but it's the example a host integrator will copy.
- **Remediation:** Rewrite using `memory_read_into_vec`, return `TrapCode`s instead of
  unwrapping, or move under `#[cfg(test)]`.

#### FLU-1106 — `ExecutionEngine::acquire_shared` uses a non-reentrant `spin::Mutex`

- **Severity:** Medium · **Status:** Open (Backlog) · **Linear:** `FLU-1106` · **Fix:** —
- **Where:** `src/vm/engine.rs`.
- **Impact:** A syscall handler that re-enters the engine on the process-wide shared
  engine deadlocks by busy-spinning (deterministic); `ExecutionEngineInner` is empty,
  so the mutex protects nothing and also serializes all execution across threads.
- **Remediation:** Remove the mutex / use a per-execution owned value; never hold a
  future stack pool across the syscall callback.

#### FLU-1107 — Compiled bytecode depends on cargo features; module header carries no config identity

- **Severity:** Medium · **Status:** Fixed · **Linear:** `FLU-1107` · **Fix:** #177
- **Where:** `src/isa/mod.rs` (`impl_fpu_opcode!` emits `Trap(IllegalOpcode)` without
  `fpu`); `code_snippets` changes emitted functions and offsets; the wire format
  records none of it.
- **Impact:** Differently-built nodes produce different module bytes/hashes for one
  contract, and a module compiled under one config can run under another with no error.
- **Remediation:** Add a codegen config/feature identity to the module header,
  checked at load in `verify_module`. (Continuation of F11 from the 2026-06-18 audit.)

#### FLU-1108 — Panicking public APIs behind total-looking signatures

- **Severity:** Medium · **Status:** Open (Backlog) · **Linear:** `FLU-1108` · **Fix:** —
- **Where:** `RwasmModule::new`/`serialize`, `GlobalMemory::new`, `ValueStack::new`,
  `ExecutionEngineInner::resume`, `ImportLinker::insert_entity`.
- **Impact:** Several public entry points look infallible but abort on inputs a caller
  can't pre-validate → "call rwasm from a node without a `catch_unwind` net" is not
  safe advice.
- **Remediation:** Make `new`/`serialize`/`resume` return `Result`; document which
  entry points are panic-free.

## Notes

**Cross-cutting root cause.** FLU-1094, FLU-1096, FLU-1097 share a root cause: the
interpreter assumes bytecode came from its own translator, but `new_verified` accepts
foreign bytecode. Either `verify_module` must establish every invariant the
interpreter relies on (stack heights, local depths, syscall/table indices — real
abstract interpretation), or the interpreter must bounds-check in release; the middle
ground gives callers a false guarantee.

**Open items to close before relying on the Wasmtime strategy in production:** the
panics + fuel divergence on the default path (FLU-1098, FLU-1099, FLU-1101) and the
hardening set (FLU-1102, FLU-1103, FLU-1105, FLU-1106, FLU-1108).

## Re-review checklist for future audits

- Confirm `verify_module` runs full abstract interpretation and the interpreter
  bounds-checks in release (FLU-1094).
- Confirm unsupported wasm proposals are rejected by the validator and the translator
  wildcard arm errors (FLU-1095).
- Confirm all decode entry points bound allocations before reading opcodes (FLU-1096).
- Confirm the executor has no `unwrap`/`expect`/`unreachable!` on values derived from
  module bytes (FLU-1097, FLU-1108) — a `deny(clippy::unwrap_used)` on `vm::executor`
  keeps it from regressing.
- Track the open Backlog items above before the Wasmtime strategy is relied on in
  production.
