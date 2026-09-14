# rWasm Security Audit — 2026-09-13

- **Date:** 2026-09-13
- **Repository:** `fluentlabs-xyz/rwasm`
- **Audited commits:** `0076acc5` on branch `fix/audit-findings3` (the follow-up to the 2026-09-12
  audit), re-audited after each fix up to `ff4dd79b`. Wasmtime strategy against
  `wasmtime-rwasm 45.0.0-rwasm.2` (fork `12fb86d0e0`).
- **Method:** targeted, executed proofs through the crate's public API for every finding, with
  each pass aimed at what the previous verification could not see: the instance/store lifecycle
  after the `0076acc5` fixes; the import trampoline, the one piece of compiler output the
  differential fuzzer never runs (`max_imports = 0`); the cost of compiling untrusted input, which
  runs before anything is metered; and finally two new differential fuzz targets
  (`fuzz/fuzz_targets/differential_imports.rs`, `fuzz/fuzz_targets/resume_equivalence.rs`) that
  cover imports with every `SyscallFuelParams` variant, `memory.grow`, repeated calls on one
  instance, several tables, and the interruption/resume path that has no Wasmtime oracle. A
  parallel verification pass over the syscall-fuel rework contributed HIGH-5 and HIGH-6.
- **Trust model** (per `README.md` → *Trust boundary*, unchanged from 2026-09-12): Class A is
  untrusted Wasm compiled by this crate — a divergence from the Wasmtime backend, a silent wrong
  result, a panic/abort/DoS on a supported path or nondeterminism is HIGH; memory unsafety,
  cross-instance disclosure and wrong dispatch are CRIT. Class B (hand-built or foreign bytecode)
  stays out of scope by the documented trust model.

## Result

| Severity | Count | Class A (production model) | Class B (foreign bytecode) |
| --- | --- | --- | --- |
| CRIT | 1 | 1 | 0 |
| HIGH | 7 | 7 | 0 |

All eight are reachable from valid Wasm — six with the recommended
`CompilationConfig::default_strategy_compatible()` (plus `builtins_consume_fuel`, the production
schedule) — and all eight are fixed on this branch. Every finding section below is the write-up
at the revision it was found on; the fix and its regression tests are in the table that follows.

## Fixes applied

| Finding | Fix | Commit | Tests |
| --- | --- | --- | --- |
| CRIT-1 two live instances share one store | instance identity on the handle (`RwasmInstance::check_store`), transactional replacement in `RwasmStore::{begin_instantiation, finish_instantiation}` with rollback of memory, tables, globals, segment flags and `last_signature` on a failed or cancelled initializer; stale handles and foreign stores are rejected with `IllegalOpcode` | `4c40f702` | `tests/audit_2026_09_13_repro.rs::instance_isolation` (3), `tests/instance_replacement.rs`, `tests/execution_contracts.rs` |
| HIGH-1 wrong-length result buffer ignored | `run_raw` validates the output slot count against the stack before popping (`IllegalOpcode`), on release and debug alike | `4c40f702` | `::instance_isolation::a_result_buffer_of_the_wrong_length_is_reported_not_ignored` |
| HIGH-2 syscall fuel charged at Cranelift `call` sites only | the schedule no longer goes to the engine; `WasmtimeModule` is a newtype carrying it by import name, resolved through the executor's linker and charged by both host trampolines (`context::charge_syscall_fuel`, the host-side twin of `compile_block_params`) — every path into an import pays | `5c8465d0` | `::syscall_fuel_dispatch` (4), `tests/fuel_alignment.rs::syscall_fuel_matches_on_every_dispatch_path`, `::syscall_fuel_matches_when_start_is_an_import`, `::out_of_fuel_matches_for_metered_builtins_called_through_a_table`, `src/wasmtime/{context,tests}.rs` |
| HIGH-3 trampoline `StackCheck(0)` | `compile_block_params` returns the prologue's peak (0 / 2 / 4) and `process_imports` accounts it in the trampoline's frame | `4c40f702` | `::syscall_fuel_dispatch::{linear,quadratic}_fuel_trampoline_reserves_its_temporaries`, `src/compiler/parser.rs::import_trampoline_stack_check_covers_the_fuel_prologue`, `tests/fuel_alignment.rs::metered_builtin_call_at_stack_capacity_matches` |
| HIGH-4 unbounded code expansion | `CompilationConfig::max_code_len` (default `N_DEFAULT_MAX_CODE_LEN`, 2 Mi instructions) checked after every operator and every `br_table` target while emitting, plus on the merged section; `CompilationError::CodeSizeExceeded`; `br_table` entries with the same `(label, DropKeep)` share one trampoline; part of the codegen identity | `7c506511` | `::code_size_bound` (2), `tests/strategy_limits.rs::code_size_bound_*`, `::br_table_entries_with_the_same_target_share_a_trampoline` |
| HIGH-5 wide metered parameter accepted by rwasm only | `param_slot_depth` rejects a non-`i32` metered parameter with `InvalidSyscallFuelParam`, so both strategies refuse it together (it never metered correctly: the trampoline read the high word, Cranelift the whole value) | `7c506511` | `::metered_import_parameters` (2), `src/compiler/block_fuel.rs` unit tests |
| HIGH-6 metered import at the stack-window boundary traps on rwasm only | `N_STACK_TRAMPOLINE_HEADROOM` (4 slots) above `N_MAX_STACK_SIZE` on the runtime value stack, for the one frame Wasm does not have | `7c506511` | `::metered_import_parameters::stack_window_*` (3) |
| HIGH-7 fuel after a statically out-of-bounds access | the translator mirrors Cranelift's compile-time bounds check (`is_statically_out_of_bounds`, `emit_memory_access`): such an access lowers to `Trap(MemoryOutOfBounds)` and ends the path, as disabled float operators already did | `ff4dd79b` | `::static_out_of_bounds_fuel` (5), `tests/fuel_alignment.rs::fuel_matches_after_statically_out_of_bounds_access`, `src/compiler/parser.rs::statically_out_of_bounds_access_lowers_to_a_trap` |

The whole suite is green in release and debug (`cargo test --release --features wasmtime`,
`cargo test --features wasmtime`), the `fpu` variant of the library and fuel suites too, the e2e
spec run (92 files) passes, clippy is clean at `-D warnings`, and the differential fuzz corpus
replays without a divergence.

### Compatibility notes for integrators

- **Emitted bytecode changed** in three places, none of which affects a module a real compiler
  produces: the import trampoline reserves its fuel-prologue temporaries (`StackCheck` 2 or 4
  instead of 0); `br_table` entries repeating a target that needs a `DropKeep` share one
  trampoline (smaller code); and a memory access that can never be in bounds becomes an
  unconditional trap with its dead tail dropped. The golden hashes in
  `tests/codegen_determinism.rs` are unchanged. Pin the compiler revision to identify the lowering.
- **Input-compatibility changes:** a module compiling to more than `max_code_len` instructions is
  rejected (`CodeSizeExceeded`); a linker entry whose metered parameter is not an `i32` is rejected
  (`InvalidSyscallFuelParam`) — the host's schedules meter `i32` lengths only.
- **API changes:** `WasmtimeModule` is a struct (newtype over `wasmtime::Module`, `Deref`,
  `From<wasmtime::Module>` for a bare module that charges no syscall fuel); `compile_wasmtime_module*`
  and `deserialize_wasmtime_module` return it and `WasmtimeExecutor::{new,try_new,instantiate}`
  take it — callers that pass those results straight through compile unchanged. New:
  `CompilationConfig::max_code_len` / `with_max_code_len`, `N_DEFAULT_MAX_CODE_LEN`,
  `N_STACK_TRAMPOLINE_HEADROOM`, `CompilationError::CodeSizeExceeded`. The fork's
  `Config::syscall_fuel_params` is no longer used by this crate.
- **Runtime value stack** is `N_MAX_STACK_SIZE + N_STACK_TRAMPOLINE_HEADROOM` slots; the
  compile-time frame bound stays `N_MAX_STACK_SIZE`.

---

## Findings (Class A — untrusted Wasm, compiler-produced bytecode)

### CRIT-1 — two live instances share one store: the first one runs against the second one's state, dispatches through its table and fetches outside its own code section — **FIXED**

**Where:** `src/vm/store.rs:13-41` (one `global_memory`/`tables`/segment-flag set per store),
`src/vm/instance.rs:17-45` (`RwasmInstance` holds only the module and the engine; nothing records
which instance the store currently describes), `src/vm/instance.rs:41-82`
(`execute`/`execute_named`/`resume` run against whatever the store holds now). The guard added in
the 2026-09-12 follow-up (`0076acc5`) covers only the *parked* case, not the *replaced* case.

**Reproduced** (`tests/audit_2026_09_13_repro.rs`, three failing tests, release and debug):

```
module A: (memory 1) (data (i32.const 0) "AAAA") main = i64.load8_u(0)   -> 65
module B: (memory 1) (data (i32.const 0) "BBBB") main = i64.load8_u(0)   -> 66

instantiate A on store S, execute A            rwasm: 65
instantiate B on store S, execute B            rwasm: 66
execute A again                                rwasm: 66   wasmtime: 65
```

and on the write side, with `A = (data "AAAA") main = store(4, 4242); load8_u(0)`:

```
instantiate A on S, instantiate B on S, execute A
store.memory_read(4, ..)                       rwasm: [146, 16, 0, 0] (= 4242 LE)
instance B's own memory at offset 4            wasmtime: [0, 0, 0, 0]
```

**Third proof (second failing test file entry) — wild dispatch and an out-of-window instruction fetch.** Module A declares a table
and calls `call_indirect 0` without initializing it (`(table 1 funcref)`, no element segment), so
its own table entry is null. Module B has 200 functions and an active element segment, so B's
instantiation installs a ~1000-instruction code offset into the shared table. Sequence: instantiate
A, instantiate B (B is never executed), execute A.

```
A code section = 18 instructions, B code section = 1021 instructions
rwasm : A's call_indirect             -> Err(UnreachableCodeReached)   (garbage decoded at A.code + ~1000)
wasmtime: A's call_indirect           -> IndirectCallToNull            (A's own table entry is null)
```

Compiling the interpreter with a temporary instruction-window check (a thread-local window pushed on
executor entry, checked in `InstructionPtr::{offset, add, get}`; patch reverted afterwards and the
working tree verified byte-identical to `0076acc5`) turns the garbage decode into a deterministic
report:

```
AUDIT-IP-BOUNDS: branch target outside the code window:
  ptr=0x1065ea7b8, window=0x1065e8820..0x1065e88b0, instruction offset=1011
```

The window is 0x90 = 144 bytes = 18 instructions; the branch lands 1011 instructions past its end,
i.e. roughly 8 KB outside the module's `InstructionSet` allocation. `visit_call_indirect`
(`src/vm/executor/control_flow.rs`) builds the target as `self.module.code_section.as_ptr() +
instr_ref`, so an offset produced for *another* module's code layout is applied to this module's code
and the interpreter fetches an instruction from outside its code section — an out-of-bounds read
(undefined behaviour in Rust), not merely a wrong value.

**Why it is a bug.** `RwasmStore` holds exactly one instance's memory, tables, globals and segment
drop flags, and the crate's own Wasmtime wrapper treats instantiation as a *swap*: the test at
`src/wasmtime/tests.rs:455-470` ("Re-instantiating through the executor swaps both the function
table and the memory") re-instantiates a second module through one `WasmtimeExecutor` and asserts
that the previous module's exports and memory are gone — the swapped-out instance is unreachable
there. On the rwasm side `RwasmInstance` is an independent public handle, so the swapped-out
instance is still callable and now runs against the *new* instance's state; nothing in the API
(`RwasmInstance::{execute, execute_named, resume}` all take `&mut RwasmStore`) marks it as stale.
Instantiation releases the store's state and rebuilds it for the new module (the `0076acc5` fix), so
the older handle keeps working but silently:
* reads return the *new* instance's bytes (`66` instead of `65`) — a wrong result on a valid
  module, and a divergence from Wasmtime for the identical host sequence;
* writes land in the *new* instance's memory (A's `4242` appears in B's page), so one contract can
  mutate another contract's state;
* `call_indirect` uses the *new* instance's table, which turns a null dispatch into a jump to
  another module's code offset — with a large enough offset, an instruction fetch outside the
  module's code section;
* globals and the data/element drop flags are shared in the same way.

The same aliasing is reachable without `ImportLinker::instantiate` at all: `ExecutionEngine::execute`
is public and runs a module against whatever state the store currently holds, so a host that keeps
one store and drives two modules through `execute` sees the same effects.

`docs/security-considerations.md` states the crate's central promise — "malicious inputs are
expected to produce controlled failures (validation errors/traps), not undefined behavior execution"
— and lists `ImportLinker::instantiate`, `ExecutionEngine::execute` and `ExecutionEngine::resume`
among the panic-free, guest-facing entry points. CRIT-1 breaks that promise with a *valid* module: the
out-of-window fetch of the third proof is undefined behaviour reached through those entry points,
and the two wrong-result cases are silent.

**Reachability.** The high-level path is safe: `StrategyDefinition::create_executor` returns
`StrategyExecutor::Rwasm { store, instance }`, which owns the store and its single instance together,
so one executor cannot alias another. The finding is in the layer below it — `ImportLinker::instantiate`
plus a host-held store, and the public `ExecutionEngine::{entrypoint, execute}` entry points — which
is the API the crate's own tests and `e2e/src/group.rs` use. A host must instantiate (or execute) a
second module on a store whose previous instance it still intends to call; nothing in the API or the
documentation warns that this invalidates the earlier handle.

**Suggested fix.** Give the store an instance generation (`instance_generation: u64`) bumped by
`RwasmInstance::new` (or by `ImportLinker::instantiate`), record it in the `RwasmInstance`, and have
`execute`/`execute_named`/`resume` return `TrapCode::IllegalOpcode` when it no longer matches — the
same contract the parked-context guard already enforces, and cheap (one integer compare per call).
The principled alternative is to move the instance state (memory, tables, globals, segment flags)
into a per-instance object so several instances can coexist, as Wasmtime does; that is a larger API
change, but it is the only shape in which the documented `RwasmStore` API (`memory_read`,
`snapshot_memory`, …) can address more than one instance.

**Repro:** `tests/audit_2026_09_13_repro.rs::instance_isolation::a_live_instance_does_not_run_against_another_instances_state`,
`::a_live_instance_does_not_write_into_another_instances_memory`,
`::a_live_instance_does_not_dispatch_through_another_instances_table` (red when found).

---

### HIGH-1 — a result buffer of the wrong length is silently ignored (and panics in a debug build) — **FIXED**

**Where:** `src/vm/executor.rs:120-155` (`run_raw` pops `result.len()` values and asserts the stack
is empty) vs `src/wasmtime/instance.rs:339-341` (the Wasmtime backend compares `result.len()` with
the function's result count and returns `IllegalOpcode`). Pre-existing, not touched by the `0076acc5`
diff.

**Reproduced** (`tests/audit_2026_09_13_repro.rs::instance_isolation::a_result_buffer_of_the_wrong_length_is_reported_not_ignored`):

```
module: (memory 1) (func (export "main") (result i32) (i32.const 7))
call  : instance.execute(&mut store, &[], &mut [])      // host passes no result slots

rwasm, release : Ok(())          (the value is dropped, the host is told the call succeeded)
rwasm, debug   : panic at src/vm/executor.rs:148: "after execution the value stack must be empty"
wasmtime       : Err(IllegalOpcode)
```

**Why it is a bug.** The same module and the same host call produce three different outcomes, and two
of them violate documented behaviour: `docs/security-considerations.md` lists
`ExecutionEngine::execute`/`resume` and `StrategyExecutor::execute` among the entry points that
"report every failure on guest-controlled input as a `Result`", and the release build reports success
while discarding the return value — a host that trusts `Ok(())` records a wrong result. Only a host
that sizes the buffer incorrectly triggers it (the e2e harness sizes it from the export type), so the
fix is a boundary check rather than a VM change: compare `result.len()` with the entrypoint's result
count (available from the compiled module/router) or with the wasmtime backend's check and return
`TrapCode::IllegalOpcode`/`BadSignature`.

**Repro:** `tests/audit_2026_09_13_repro.rs::instance_isolation::a_result_buffer_of_the_wrong_length_is_reported_not_ignored`
(red when found; it panicked instead of failing the assertion in a debug build).

---

### HIGH-2 — syscall fuel is only charged at Cranelift `call` sites on the Wasmtime strategy: `call_indirect`, tail calls, an exported import and a `start` import run a metered builtin for free — **FIXED**

**Where:** rwasm charges `SyscallFuelParams` *inside the import trampoline*
(`src/compiler/parser.rs:process_imports` → `compile_block_params`, `src/compiler/block_fuel.rs`),
so every path into the import pays: direct `call`, `return_call`, `call_indirect` through a table
entry (`elem` or `ref.func` + `table.set`), `return_call_indirect`, the import exported as the
entrypoint (`ReturnCallInternal(trampoline)`), and the import as `start`
(`CallInternal(trampoline)` in the init prologue). The Wasmtime strategy hands the same parameters
to the engine by import *name* (`src/wasmtime/engine.rs:52-66`, `cfg.syscall_fuel_params`), and
the fork charges them in `rwasm_eval_fuel_policy`
(`wasmtime-rwasm/crates/cranelift/src/func_environ.rs:541-575`), which matches only
`Operator::Call { function_index } | Operator::ReturnCall { function_index }` and resolves the
index against the import list. An indirect call has no static callee, an exported import and a
`start` import are invoked by the runtime rather than by generated code, so none of them charge.
The crate's host trampolines (`src/wasmtime/syscall_handler.rs`) charge nothing either.

**Reproduced** (`tests/audit_2026_09_13_repro.rs`, release and debug; `env.flat` has
`SyscallFuelParams::Const(1000)`, budget 100 000, remaining fuel after the call):

| path | rwasm | Wasmtime |
| --- | --- | --- |
| `call $flat` (control) | 98 989 | 98 989 |
| `(call_indirect (type $t) (i32.const 0))`, `(elem (i32.const 0) $flat)` | 98 988 | **99 988** |
| `(return_call_indirect (type $t) (i32.const 0))` | 98 988 | **99 988** |
| `ref.func $flat` → `table.set` → `call_indirect` | 98 983 | **99 983** |
| `(export "main" (func $flat))` | 99 000 | **100 000** |
| `(start $flat)` (fuel after instantiation) | 99 000 | **100 000** |

The gap is exactly the configured 1000 in every row; the base costs of the instructions around the
call agree, so this is the syscall charge itself.

**Consequence (executed, `indirect_builtin_calls_cannot_bypass_fuel_on_wasmtime`).** `env.lin` is
a `LinearFuel { word_cost: 3, base_fuel: 7, param_index: 1 }` builtin, i.e. the shape of every
hashing / copying host function. A loop of 100 `call_indirect` invocations with a 1 MiB length
argument on a 1 000 000 fuel budget:

```
rwasm    : Err(OutOfFuel), remaining 62 209      (each call costs 7 + 3·31 250 = 93 757)
wasmtime : Ok(()),         remaining 997 996     (100 MiB of metered host work for ~2 000 fuel)
```

Two nodes on different strategies disagree on whether the transaction succeeds, and on the
Wasmtime strategy the syscall fuel schedule is not a limit at all: a contract puts the builtin in
its table (three lines of WAT, or any function-pointer use of the import in Rust that LLVM does not
devirtualise) and the host does unbounded work per base fuel. `docs`/`tests/fuel_alignment.rs`
promise the opposite ("the same wasm module must burn the same fuel, stop at the same point and
report the same remaining fuel"); the alignment suite only ever calls its imports directly.

**Rating.** HIGH on the crate's scale (cross-backend fuel/limit divergence on valid Wasm with the
recommended config, as HIGH-4 in the 2026-09-12 report). It is a stronger case than a fuel
mismatch: the *outcome* diverges (`OutOfFuel` vs `Ok`) and the Wasmtime side is a metering bypass
for every metered host function. If gas used is part of the consensus state for the deployment
this is CRIT by the report's own definition ("silent state/semantic divergence reachable from
untrusted Wasm"); it is left at HIGH here only because that depends on the host.

**Suggested fix (in this crate).** Charge the syscall fuel where rwasm charges it — in the
import's own body — rather than at the call site: run the `SyscallFuelParams` schedule inside
`wasmtime_syscall_handler_raw` / `wasmtime_syscall_handler` (`src/wasmtime/syscall_handler.rs`),
which already have the typed parameters and a caller with `try_consume_fuel`, and stop passing
`syscall_fuel_params` to the engine in `wasmtime_engine` so direct calls are not charged twice.
The `IntegerOverflow` guard on the metered parameter must run before the charge, as
`compile_block_params` does, and a failed charge must leave the counter untouched
(`try_consume_fuel` already does). This covers every path with one implementation and removes the
fork-side `rwasm_eval_fuel_policy` from the trust base. The alternative is to fix the fork so the
host-call trampoline charges; either way `tests/fuel_alignment.rs` needs cases for
`call_indirect`, `return_call_indirect`, an exported import and a `start` import.

**Repro:** `tests/audit_2026_09_13_repro.rs::syscall_fuel_dispatch::indirect_syscall_fuel_is_charged_on_both_strategies`,
`::entrypoint_and_start_imports_charge_syscall_fuel_on_both_strategies`,
`::indirect_builtin_calls_cannot_bypass_fuel_on_wasmtime` (red when found);
`::direct_syscall_fuel_is_charged_on_both_strategies` is the control.

---

### HIGH-3 — the import trampoline's `StackCheck` is `0`, so a `LinearFuel`/`QuadraticFuel` builtin called with the value stack near capacity traps `StackOverflow` on rwasm while Wasmtime runs the module — **FIXED**

**Where:** `src/compiler/parser.rs:process_imports` builds the trampoline as
`SignatureCheck; [ConsumeFuel]; StackCheck(u32::MAX); <fuel prologue>; Call(sys); Return` and
calls `compile_block_params` (`src/compiler/block_fuel.rs`) to emit the prologue **directly into
`translator.alloc.instruction_set`**, bypassing `translator.stack_height`. `InstructionTranslator::finish`
then patches `StackCheck` with `stack_height.max_stack_height()`, which is `0`. The `LinearFuel`
prologue peaks at two operand slots (`LocalGet`, `I32Const`), `QuadraticFuel` at four; every other
compiler-injected sequence (`op_memory_grow_checked`, `op_memory_init_checked`, …) accounts its
temporaries through `stack_height.push_n(MSH_*)`, only this one does not.

At run time `StackCheck(0)` is `ValueStack::reserve(0)`, a no-op, and `reserve` grows the buffer
only when a request *exceeds* the capacity (`src/vm/value_stack.rs:reserve`). A fresh
`ValueStack` has `N_DEFAULT_STACK_SIZE = 32` slots, so the entry function's own `StackCheck(H)`
with `params + H ≤ 32` leaves the capacity at 32 and the stack legitimately reaches 32 at its
peak. If that peak is the call to the builtin, the prologue's first push hits `ptr == end`,
`ValueStackPtr::set` parks the pointer and raises the sticky out-of-bounds flag, and `step` turns
it into `TrapCode::StackOverflow`. The same happens after growth whenever a callee's peak lands
within the prologue's window of the current capacity.

**Reproduced** (`tests/audit_2026_09_13_repro.rs`, release and debug). Compiled trampoline for
`(import "env" "lin" (func (param i32)))`:

```
 9: SignatureCheck(0)
10: ConsumeFuel(0)
11: StackCheck(0)            <- nothing reserved
12: LocalGet(1)              <- push #1
13: I32Const(134217728)      <- push #2
14: I32GtU
...
26: ConsumeFuelStack
27: Call(18)
28: Return
```

`main (param i32) (local i32 ×30) = call $lin (local.get 0); local.get 0` — peak 32 = capacity:

```
rwasm    : Err(StackOverflow), result 0
wasmtime : Ok(()),             result 64
```

Sweeping the local count: `LinearFuel` diverges at peaks 31 and 32, `QuadraticFuel` at 29–32 —
exactly the two- and four-slot windows of the two prologues; every other peak agrees.

**Why it matters.** The compiler accepted the function (`finish` checks `params + H ≤ 8192`), so
a valid module that Wasmtime executes traps on rwasm for a reason that depends on a runtime buffer
capacity the developer cannot see. A contract tested on the Wasmtime strategy ships and fails on
rwasm nodes — the same divergence class as HIGH-1 (stack limits) in the 2026-09-12 report. It
needs `builtins_consume_fuel` with a `LinearFuel` or `QuadraticFuel` import, which is the
production schedule; `Const`/`None` emit no temporaries and are unaffected.

**Suggested fix.** Account the prologue's temporaries: have `compile_block_params` return the peak
height it emits (0 / 2 / 4) and, in `process_imports`, apply `translator.stack_height.push_n(peak);
pop_n(peak)` before `translator.finish()`, so the trampoline's `StackCheck` reserves them like every
other injected sequence. Add a compile-time assertion (or a unit test over the emitted
`StackCheck`) per `SyscallFuelParams` variant so a future prologue cannot regress it. The
`u32::MAX` placeholder in `op_stack_check(u32::MAX)` is also worth replacing by the real peak
outright, so the trampoline no longer depends on `finish` patching it.

**Repro:** `tests/audit_2026_09_13_repro.rs::syscall_fuel_dispatch::linear_fuel_trampoline_reserves_its_temporaries`,
`::quadratic_fuel_trampoline_reserves_its_temporaries` (red when found).

---

### HIGH-4 — the translator has no bound on emitted code: a `br_table` compiles to ~16 KB of bytecode per byte of input, so a 1 MiB deployment allocates ~16 GB and takes the node down — **FIXED**

**Where:** `DropKeep::translate_drop_keep` (`src/compiler/drop_keep.rs`) lowers a branch that keeps
`k` values and drops at least one into `2k + 1` instructions (`k` × `LocalGet`/`LocalSet` pairs
plus a `BulkDrop`). `k` is the branch target's arity, up to Wasm's limit of 1000 block results.
`visit_br_table` (`src/compiler/translator.rs:1010-1152`) emits one such trampoline **per
target**, with no deduplication of identical targets, and `visit_br_if`/`visit_br` emit one per
branch. Nothing anywhere in the compiler checks the length of `instruction_set`; the only guard is
`current_pc()`'s `u32::try_from`, which panics past 4 G instructions — 32 GB of allocation later.

**Measured** (`tests/audit_2026_09_13_repro.rs`; release; a block with 1000 results, one value
underneath so every branch drops, and a `br_table` whose targets all name that block):

| Wasm input | rwasm instructions | bytecode | compile time | peak RSS |
| --- | --- | --- | --- | --- |
| 5 053 B (1 000 targets) | 2 008 016 | 16 MiB | 8 ms | — |
| 14 053 B (10 000 targets) | 20 044 016 | 160 MiB | 90 ms | — |
| 104 056 B (100 000 targets) | 200 404 016 | 1.6 GiB | 0.9 s | **5.4 GiB** |

Linear in the target count: 2001 instructions (16 KB) per one-byte target. Wasmtime compiles the
104 KB module in 157 ms with no expansion. `br_if` gives the same class at a lower rate
(44 KB → 20 M instructions, 160 MiB).

**Why it matters.** The host admits deployments up to `WASM_MAX_CODE_SIZE = 1 MiB`
(`fluentbase/crates/types/src/lib.rs:142`) and only bounds the *output* afterwards
(`RWASM_MAX_CODE_SIZE = 12 MiB`). A 1 MiB module holds ~1 M targets → ~2 G instructions ≈ 16 GB
of bytecode, plus the intermediate `br_table_branches`/`trampoline_ixs` buffers and the clone
into the `Arc` (the 100 K case peaks at 3.4× the final size) — roughly 50 GB, i.e. an OOM abort
on any validator that compiles the transaction, which is every validator. The cost to the
attacker is one deployment priced by its 1 MiB input. On the crate's own scale this is a
reachable abort/DoS from valid Wasm: HIGH; as a network-wide, unprivileged, repeatable outage it
is the most severe kind of DoS the threat model lists.

**Suggested fix.** Bound the output by the input:

1. A `CompilationConfig::max_code_len` (instructions; default on the order of the host's
   `RWASM_MAX_CODE_SIZE`, e.g. 2 M), checked *before* each expansion is emitted — in
   `visit_br_table` against `targets × (2·keep + 1)` before the trampolines are built, and in the
   `translate_if_reachable` epilogue after every operator (a single operator expands by at most
   ~2001 instructions, so one check per operator is enough) — reporting a new
   `CompilationError::CodeSizeExceeded { len, limit }`. `check_compile_limits` on the Wasmtime
   path should apply the same bound so the strategies keep agreeing on the accepted language.
2. Independently, deduplicate `br_table` trampolines: targets with the same label and `DropKeep`
   can share one trampoline (the common case in real code is a handful of distinct targets), which
   turns the 2001× factor into a constant for the pathological input without changing semantics.

Both are cheap; (1) is the security fix, (2) is the size fix.

**Repro:** `tests/audit_2026_09_13_repro.rs::code_size_bound::br_table_expansion_is_bounded`,
`::br_if_expansion_is_bounded` (sized at 10 000 targets so they are safe to run —
the 100 000 case above needs 5.4 GiB).

---

### HIGH-5 — a fuel schedule that meters a wide (`i64`/`f64`) parameter is accepted by rwasm and rejected by the Wasmtime strategy — **FIXED**

**Where:** `src/wasmtime/instance.rs:248-285` (`WasmtimeExecutor::resolve_syscall_fuel`), the
`is_i32` filter at `:268-281`. The rwasm side of the same contract accepts the wide parameter and
charges fuel for its 32-bit words (`ModuleParser::process_imports` → `compile_block_params`).

**Reproduced** (`tests/audit_2026_09_13_repro.rs::metered_import_parameters::wide_metered_syscall_parameters_agree_between_strategies`,
fails; release and debug). Module `(param i32 i64)`, linker
`LinearFuel { base_fuel: 3, param_index: 1, word_cost: 2 }` (index 1 counts from the last parameter,
so the metered parameter *is* the `i64`), `default_strategy_compatible()` + `builtins_consume_fuel(true)`:

```
rwasm    : Ok(())            compiles, instantiates and charges fuel (999983)
wasmtime : Err(BadSignature) create_executor fails at instantiation
```

| import parameters | metered parameter | rwasm | wasmtime |
| --- | --- | --- | --- |
| `i32, i32` | last | Ok | Ok |
| `i32, i64` | last | Ok | **`BadSignature`** |
| `i32, i64` | first (`i32`) | Ok | Ok |
| `i64, i32` | last (`i32`) | Ok | Ok |
| `i64, i32` | first (`i64`) | Ok | **`BadSignature`** |
| `i64, i64` | last | Ok | **`BadSignature`** |
| `i32, f64` | last | Ok | **`BadSignature`** |
| any | index 0 or beyond the count | compile `InvalidSyscallFuelParam` | compile `InvalidSyscallFuelParam` |

**Why it is a bug.** The strategies disagree about whether the same module + linker configuration can
be instantiated at all, in the production fuel configuration: a host whose schedule meters a wide
parameter can deploy and run on the rwasm strategy, while the same binary refuses the configuration on
the Wasmtime strategy, and `for_each_strategy` (the crate's own differential harness) fails outright.
The rejection rests on a false premise: the comment at `:251-253` says "as the rwasm compiler rejects
the same linker entry at compile time", but the rwasm compiler accepts it and meters the full 64-bit
value (the charge differs between `1` and `0x1_0000_0001`). Fix: either charge wide parameters on the
host side the way the compiler does (drop the `is_i32` filter and read the full value), or reject them
in the rwasm front end with `InvalidSyscallFuelParam` so both strategies refuse together.

---

### HIGH-6 — a module at the stack-window boundary traps only on rwasm (fuel trampoline temporaries are missing from the frame check) — **FIXED**

**Where:** the frame-height check in `src/compiler/translator.rs` (`param_slots + max_stack_height`
against `N_MAX_STACK_SIZE = 8192`) versus the trampoline peak reported by `compile_block_params`
(`src/compiler/block_fuel.rs`) and fed into the translator by `process_imports`
(`src/compiler/parser.rs`). The peak reaches the trampoline's own `StackCheck` but not the frame
height, so the module is accepted and then traps at run time only on rwasm.

**Reproduced** (`tests/audit_2026_09_13_repro.rs::metered_import_parameters::stack_window_boundary_for_metered_imports_agrees_between_strategies`,
fails; `::stack_window_below_the_boundary_agrees` is the control). Import `(param i32)` metered with
`LinearFuel { base_fuel: 3, param_index: 1, word_cost: 5 }`, `main` declares N locals, pushes one
argument and calls it:

| N locals | rwasm | wasmtime |
| --- | --- | --- |
| ≤ 8189 (`None`/`Const`/`Linear`), ≤ 8187 (`Quadratic`) | Ok | Ok (equal fuel) |
| **8190 (`Linear`)** | **`Err(StackOverflow)`** | **`Ok`** |
| **8188–8190 (`Quadratic`)** | **`Err(StackOverflow)`** | **`Ok`** |
| ≥ 8191 (every policy) | compile `StackHeightExceeded { height: N + 2, limit: 8192 }` | same error |

The reported height is `N + 2` for every policy — `None`, `Const`, `Linear` and `Quadratic` alike —
so the fuel policy's temporary requirement never reaches the frame calculation. Both compilers accept
the module; the rwasm interpreter then traps `StackOverflow` inside the trampoline while the Wasmtime
backend executes it and charges fuel. This is the residual of the first HIGH-3 fix (the trampoline now
reserves its temporaries, so the failure is a clean trap instead of a corrupted window, but the
compile-time check still accepts a module the rwasm VM cannot run). Fix: include the peak in the frame
height (both strategies then reject together) or reserve it in the caller's frame.

---

### HIGH-7 — after a memory access Cranelift proves out of bounds at compile time, the Wasmtime strategy charges the region only up to that access; rwasm charges the whole region — **FIXED**

**Where:** `wasmtime-rwasm/crates/cranelift/src/func_environ.rs`, `fuel_before_op` (`:527-536`)
returns early when the Cranelift translation state is unreachable. Cranelift makes the state
unreachable not only after Wasm control flow (`unreachable`, `br`, `return`, … — where rwasm
also stops translating) but also after a memory access it can prove will always trap
(`crates/cranelift/src/bounds_checks.rs:246-253`): `offset + access_size > maximum_byte_size`,
where the maximum is the memory's declared maximum in bytes, or 4 GiB when it declares none.
The operators that follow such an access in the same region are then never translated and never
added to the region's cost, so the entry charge — patched with the region total when the region
closes — is short by exactly their cost. rwasm has no static analysis of this kind: the access
is an ordinary trapping load or store and the region's `ConsumeFuel` covers everything after it.

**Reproduced** (`tests/audit_2026_09_13_repro.rs`; `default_strategy_compatible()`, 1000 fuel,
remaining fuel after the trap; both sides trap `MemoryOutOfBounds` in every row):

| module | rwasm | Wasmtime |
| --- | --- | --- |
| `(memory 1 2)`, `i32.load offset=131068` (dynamic OOB, control) | 994 | 994 |
| `(memory 1 2)`, `i32.load offset=131072` (static OOB) | 994 | **996** |
| `(memory 0 1)`, `i32.store offset=65536` (static OOB) | 993 | **995** |
| `(memory 0 0)`, any access | 994 | **996** |
| `(memory 1 1)`, `i32.load offset=65536` + 500 charged operators | 496 | **996** |

Division by zero, `unreachable`, `table.get` out of bounds, `memory.fill` out of bounds and a
null `call_indirect` all leave the same counter on both engines (scratch probes, deleted); the gap is specific to the static memory-trap path and grows with the region.

**Why it matters.** `tests/fuel_alignment.rs` states the contract both engines implement: "a
trap inside a region … leaves the same counter on both engines", and the host reads the store's
remaining fuel after a trapped frame (`fluentbase/crates/runtime/src/executor.rs:289-290`,
`handle_execution_result` overwrites `fuel_consumed` with the store's value for every `Err`). A
module that declares a memory maximum and contains one access beyond it — three lines of WAT,
invisible to a reader as anything but dead code — makes an rwasm node and a Wasmtime node
disagree on the gas of the trapping frame by up to the region's size (a region is a straight-line
block: a function body can be a single region of a million operators). HIGH on the crate's
scale (cross-backend fuel divergence on valid Wasm with the recommended config, as HIGH-4 in
2026-09-12 and HIGH-2). Today the host runs untrusted contracts on the rwasm strategy only, which is
why this is not rated higher.

**Fix (applied, in this crate).** The translator now mirrors Cranelift's compile-time bounds
check: `InstructionTranslator::is_statically_out_of_bounds` (`offset + access_size >
declared_max_bytes`, 4 GiB without a maximum; the access size is `1 << memarg.max_align`), and
`translate_load`/`translate_store` go through `emit_memory_access`, which emits an unconditional
`Trap(MemoryOutOfBounds)` and ends the path instead of the access — the same treatment the
translator already gave disabled float operators (`end_path_if_illegal_opcode`), which keeps
precedence. The access could never have done anything else, so only the metering of the dead
tail changes, and it now matches the Wasmtime backend on every case in the table (both engines
charge the region up to and including the access). This is the in-crate alternative to fixing
the fork's `fuel_before_op`; it was preferred because it ships without a fork release and
because the fork's rule (`bounds_check_and_compute_addr`) is small and pinned by version. A
fork-side fix was also written and verified (keep accumulating the open
region's cost after `before_unconditionally_trapping_memory_access`) and is *not* to be applied
on top of this one — the two together would reopen the gap in the other direction.

Tests: `tests/fuel_alignment.rs::fuel_matches_after_statically_out_of_bounds_access` (offset
beyond the maximum, offset + size beyond it, the exact boundary as a dynamic control, a
zero-maximum memory, a store, the 4 GiB rule without a maximum in both directions, and the
disabled-float precedence, each pinned to an absolute fuel figure and compared across engines);
`src/compiler/parser.rs::statically_out_of_bounds_access_lowers_to_a_trap` (the emitted
bytecode); `tests/audit_2026_09_13_repro.rs` (5/5 green).

**Bytecode change:** only modules containing an access that can never be in bounds compile
differently (the access becomes a trap and the rest of its block is not emitted); the golden
hashes in `tests/codegen_determinism.rs` are unchanged.

**Repro:** `tests/audit_2026_09_13_repro.rs` (4 of 5 fail against the released fork; the dynamic
out-of-bounds case is the passing control).

---

## Lower-severity observations (verified, below the HIGH bar)

* **Fuel is not released at instantiation, although the field is documented per instance.**
  `RwasmStore::consumed_fuel` is documented as "Total amount of fuel consumed by the currently
  running instance" (`src/vm/store.rs:14-15`) and `RwasmInstance::new` now releases memory, tables
  and segment flags, but not the counter. Measured: A costs 9,006 fuel and B 13,506 with
  `default_strategy_compatible`; on a store with limit 18,009 that ran A, B traps `OutOfFuel` with
  result `[-1]`, while the same module on a fresh store of the same limit returns `1500`. The
  Wasmtime backend behaves the same way for a reused store (its fuel is store-scoped too), so this is
  an internal consistency/documentation defect rather than a backend divergence — but a host that
  reuses a store without `reset_fuel` gets a smaller budget than a fresh store and the field doc
  claims otherwise. Fix: either release the counter with the other per-instance state, or document
  that fuel is store-scoped and must be renewed with `reset_fuel`.
* **`resume` values are not validated** (now with executed evidence):
  `resume(&mut store, &[Value::I64(42), Value::I64(1)])` for a one-result syscall leaves the operand
  stack dirty — release returns `Ok(())` with a wrong result (`I64(9)` where the uninterrupted run
  returns `I64(50)`), debug panics at `src/vm/executor.rs:148`, and the error class for a
  type-mismatch variant is `StackOverflow`, not `BadSignature`. A host that re-supplies the values
  the interrupted syscall produced is unaffected. Fix: record the parked syscall's result types in
  `ReusableContext` and reject a mismatch in `resume`, as `invoke_syscall` now does for the normal
  return path.
* **`global_variables` survives instantiation.** Host-visible through `has_global_word` /
  `global_word_bits` (after an i64 global was set to 4242 by A, word 1 still reports 4242 after B is
  instantiated; a fresh store reports no word 1). Guests are not affected on the supported path —
  the init prologue rewrites every global the module can name — but it is the same mechanism as
  CRIT-1 and becomes guest-visible through the `engine.execute` bypass. Clearing `global_variables`
  (and `last_signature`) in the same release is the cheap hardening.
* **Unverified, flagged:** the `tracing` feature's `Tracer` is not reset anywhere, so a reused
  store's trace keeps the previous instance's events. No `--features tracing` probe was run; treat it as an open verification gap.

* `RwasmInstance::execute` with fewer parameters than the entrypoint declares traps
  `StackOverflow` (the missing slots are read as out-of-bounds cells), while the Wasmtime
  executor reports `IllegalOpcode` from its argument-count check. Host misuse; noted for
  consistency.

---

## Status of the 2026-09-12 follow-up findings (verified at `0076acc5` and after)

| finding | status |
| --- | --- |
| store instance state leaked across instantiations (tables, memory, segment flags, page accumulation) — CRIT | fixed in `0076acc5`; 6 tests green plus the within-instance guards (`tests/audit_round3_repro.rs`) |
| `reset` kept the parked interruption; `execute` panicked on a parked context in debug — HIGH | fixed in `0076acc5` |
| syscall result-buffer contract differed per backend — HIGH | fixed in `0076acc5` (Wasmtime seeds per declared type, rwasm validates handler writes) |
| valid Wasm rejected: a negative active-segment offset was a compile error — HIGH | fixed in `4c40f702` (the unsigned bits are kept until the initializer runs and trap there) |
| strategies accepted different languages (`MissingMemoryExport`) — HIGH | closed as documented behaviour (`docs/pipeline.md`, "Backend differences that remain"); the test now pins the asymmetry |
| safe public API aborts on an out-of-range branch target — HIGH | open by the documented trusted-bytecode decision (characterisation test) |
| unmatched state-router state returned a silent wrong result — below bar | closed by the HIGH-1 slot-count check |

---

## Verified clean

* **The instantiation lifecycle after the fixes:** the resets keep the store's configured page
  ceiling, the init prologue's ability to grow memory and each declared table, the `StoreTr`
  snapshots and the fuel accounting; the parked-context guard rejects replacement before any state
  is released; `reset(keep_flags)` keeps its documented meaning and cancels a parked initializer
  by rolling back. `last_signature` and `global_variables` cannot leak into a guest on the
  supported path (every function body starts with `SignatureCheck`, every trap path clears the
  field, the prologue rewrites every global the module can name). No codegen or fuel
  nondeterminism (`tests/codegen_determinism.rs`: cross-process, hash-seed, insertion order).
* **Fuel as observed by the host:** the remaining fuel seen inside a syscall placed mid-block, and
  after `OutOfFuel`, `ExecutionHalted` and a handler-raised trap, agree between the engines (both
  charge a region on entry); `i32 i64 i32 → i64 i32` syscall marshalling agrees; `memory.grow`
  across eight calls on one instance with `0`, `-1` and `0x7fff_ffff` deltas agrees, including
  `memory_read_into_vec` and `snapshot_memory` on the grown region.
* **Compile-time cost:** every other one-operator-to-many lowering has a constant factor (`i64`
  snippets, `memory.*`/`table.*` guards, `BulkConst` for locals, `select`, `call_indirect`, the
  segment prologues under `N_MAX_DATA_SEGMENTS`, capped function types, the import trampoline);
  `TypeStack::slot_depth` is O(1); const expressions are evaluated iteratively. Three real
  contracts (`tests/assets`: secp256k1 1.1 MB, nitro-verifier 465 KB, panic 41 KB) compile at
  ~0.6 instructions per byte, serialize/deserialize byte-for-byte and agree between strategies.
* **Fuzzing:** 50 104 inputs with imports, `memory.grow`, repeated calls and 2–4 tables
  (`differential_imports`), 146 966 interrupt/resume pairs (`resume_equivalence`) and the 9 513-file
  corpus of the original target — no divergence beyond HIGH-7 and the documented `StackOverflow`
  depth residual (rwasm: 1024 frames / the value-stack window, Wasmtime: its native stack).
* Runtime cost per fuel unit of `memory.grow` (in-place `realloc`), bulk operations under the
  production schedule, and the transactional instantiation added in `4c40f702` (state moved aside
  and restored together, second `begin_instantiation` blocked while one is parked).

## Verification gaps

The e2e harness ignores the expected error message and fakes `(get …)` globals, `(module quote …)`
directives are skipped by the runner, `snippets/extractor.rs` still has its opcode assertions
commented out, `.github/workflows/bench.yml` uploads the wrong criterion directory, and the
original differential target still sets `max_imports = 0` and skips `memory.grow` — the two new
targets cover that ground but are not yet in `fuzz.yml`. The `tracing` feature's `Tracer` is not
reset between instances and was not probed.

## Reproduction

```bash
# every finding of this audit: 22 tests, all green after the fixes (all were red when found)
cargo test --release --features wasmtime --test audit_2026_09_13_repro

# the shared fuel schedule, including the cases added for HIGH-2, HIGH-3 and HIGH-7
cargo test --release --features wasmtime --test fuel_alignment

# the new fuzz targets
cd fuzz && cargo +nightly fuzz run differential_imports -- -max_total_time=600 -rss_limit_mb=4096
cd fuzz && cargo +nightly fuzz run resume_equivalence  -- -max_total_time=600 -rss_limit_mb=4096
```
