# rWasm Security Audit — 2026-09-12

- **Date:** 2026-09-12
- **Repository:** `fluentlabs-xyz/rwasm`
- **Audited commit:** `bd935e0b` (v0.5.0, branch `devel`)
- **Method:** full read of `src/**`, `tests/**`, `e2e/**`, `fuzz/**`, `docs/**`, `.github/workflows/**`;
  every finding is backed by an executed proof-of-concept through the crate's public API. Delegated
  deep-dives covered the compiler/ISA, the interpreter, the wire format, the strategy/Wasmtime layer
  and the verification infrastructure; all headline findings were re-verified by the lead auditor.
  Scratch PoCs were removed afterwards; the repro snippets are inline below.
- **Baseline:** `cargo test` (root, default features) is green at this commit.
- **Trust model applied:** per `README.md` → *Trust boundary*, serialized rWasm modules are trusted
  artifacts of this crate's own compilation pipeline; only Wasm input is untrusted. This audit
  therefore separates findings into two classes:
  - **Class A — reachable from untrusted Wasm + a compiler-produced module** (the production threat
    model). These are the findings that matter for consensus safety.
  - **Class B — requires rWasm bytecode the compiler does not produce** (a foreign/crafted artifact,
    or a module built through the safe `RwasmModuleBuilder`). Documented as out of scope; retained
    as hardening, and rated by what the code does rather than by the model's assumption.
- **Ratings:** **CRIT** = silent state/semantic divergence reachable from untrusted Wasm
  (consensus-level miscompilation) or memory unsafety in the production threat model.
  **HIGH** = reachable panic/abort/DoS, cross-backend divergence on limits, or an ungated
  safety property.

## Result

| Severity | Count | Class A (production model) | Class B (foreign bytecode) |
| --- | --- | --- | --- |
| CRIT | 2 | 2 | 0 |
| HIGH | 8 | 5 | 3 |

Headline: **the two CRITs and five of the HIGHs need no crafted bytecode at all.** They are
triggered by ordinary valid Wasm (or by host parameters) against modules produced by this crate's
own compiler, with the `CompilationConfig::default_strategy_compatible()` configuration the docs
recommend — so the documented mitigation for strategy divergence does not cover them.

## Fixes applied

The Class A findings and retained Class B hardening have regression tests (`tests/bulk_init_bounds.rs`,
`tests/strategy_limits.rs`, `tests/instruction_pointer_bounds.rs`, plus updates to the existing
suites). The root suite passes in debug **and release**, the e2e spec-suite run (92 tests) is green,
clippy is clean at `-D warnings`, and the differential fuzzer replays its corpus without a
divergence. Instruction-pointer hardening was excluded after the performance review below;
arbitrary rWasm instruction streams remain outside the supported trust model.

**Reconciliation note.** While this audit was in progress the branch advanced to `744e4a1d`
("close the rWasm audit 2026-09-09 findings", FLU-1355), which independently fixed part of the same
surface. Where the two overlap, that implementation is the one kept (its error variants and cache
policy), and this change set contributes only the parts it does not cover. The table below records
which is which.

| Finding | Fix in this change set | Also fixed upstream (#204) |
| --- | --- | --- |
| CRIT-1 `memory.init`/`table.init` | two-step unsigned bound check (`s > len`, `len - s < n`) and an existing-opcode branch that skips the blob-offset rewrite when `s == 0 && n == 0`; the init instruction still validates dropped segments and destination bounds | partial: #204 made the VM's operand conversion unsigned (FLU-1363), which does not cover the injected guard's wrap |
| CRIT-2 table limits | the `table.grow` guard is unsigned over a maximum clamped to the cap, and the bootstrap grow result is verified instead of dropped | yes for the initial-size cap and the store limit (FLU-1356); the `max >= 2^31` guard misfire is fixed here |
| HIGH-1 stack limits | `InstructionTranslator::finish` rejects a peak above `N_MAX_STACK_SIZE` (`StackHeightExceeded`); the call-depth difference is documented | no |
| HIGH-2 `execute(func_name)` | both strategies record the compiled entrypoint name and reject any other name | no |
| HIGH-3 Wasmtime path | engine feature pinning, fallible import linker (unsupported types/collisions report errors), memory resolved by export kind | yes for validation-first, error propagation and the panicking constructor (FLU-1361, FLU-1362) |
| HIGH-4 fuel/limit divergence | — | yes (FLU-1357 `StrategyIncompatibleConfig`, FLU-1364 compile-time page cap) |
| HIGH-5 memory binding | memory is resolved by export **kind**, and the Wasmtime strategy rejects a module that declares an unexported memory (`MissingMemoryExport`) | no |
| HIGH-6 foreign bytecode | partial hardening only: `source_pc` validated at entry; executor errors become traps; `br_table` uses `checked_sub`; drop indices bounded; `BulkConst` stops at the stack window. Instruction movement/fetch remains unchecked for trusted compiler output | partial: #204 fixed the reference handler (FLU-1368) and `resume` (FLU-1369) |
| HIGH-7 deserialize/cache | the bytecode identity is part of the module cache key, so a reused caller key cannot return a module compiled from other bytes | yes for `unsafe` deserialize (FLU-1365) and the codegen-identity key (FLU-1366) |
| HIGH-8 verification gaps | CI runs `cargo test --release` and clippy with `-D warnings`; a new `fuzz.yml` runs the bounded differential target; publish is gated on tests; the oracle compares trap codes; `tests/fuzz.rs` asserts both strategies ran | no |

### Compatibility notes for integrators

- **Emitted bytecode changed** for bulk-init prologues and `table.grow` guards, using existing
  opcode encodings and module wire version 1. The codegen identity domain remains version 1 and
  fingerprints configuration/features; pin the compiler revision separately to identify the
  changed lowering. Pre-audit artifacts keep decoding but retain their old
  guards until recompiled. The proposed segment-liveness codes `90`/`91` were removed; artifacts
  containing them must be recompiled from Wasm before loading on this runtime.
- **Input-compatibility changes**: tables above 1024 elements and functions needing more than 8192
  value-stack slots are rejected at compile time; compiling for the Wasmtime strategy rejects a
  config that charges rwasm-only fuel, a module that does not export its linear memory, and (from
  #204) a module whose memory exceeds the compile-time page cap.
- **API changes**: `RwasmInstance::execute_named`,
  `WasmtimeExecutor::with_entrypoint_name` and the `StrategyDefinition::{Rwasm,Wasmtime}`
  `entrypoint_name` field; `deserialize_wasmtime_module` is `unsafe`; new `CompilationError`
  variants (`StackHeightExceeded`, `MissingMemoryExport`, plus #204's `TableSizeExceedsLimit`,
  `StrategyIncompatibleConfig`, `WasmtimeCompilationFailed`). Raw instruction-pointer access is
  now internal: `InstructionPtr`, `ReusableContext`, `CallStack::{push,pop}` and
  `RwasmExecutor::{new,step}` are crate-private. Public execution still requires trusted bytecode.
- **Residual differences**: call-depth limits are still not synchronized between the engines (rwasm
  stops at 1024 frames or the stack window, Wasmtime at its native stack); the differential fuzzer
  still treats trailing zero memory bytes as equivalent and skips `memory.grow`.

---

## Class A — CRITICAL (untrusted Wasm, compiler-produced bytecode)

### CRIT-1 — `memory.init` / `table.init` source index wraps in the compiler's own guard code: silent wrong data instead of a trap — **FIXED**

- **Where:** `src/isa/memory.rs:251-289` (`op_memory_init_checked`, guard at `:262-270`, rewrite at
  `:271-277`), emitted from `src/compiler/translator.rs:2395-2400` (`visit_memory_init`); identical
  pattern at `src/isa/table.rs:30-64` (guard `:39-45`, rewrite `:46-52`), emitted from
  `src/compiler/translator.rs:2474-2480`. Runtime check: `src/vm/executor/memory.rs:140-147`,
  `src/vm/executor/table.rs:133-138`.
- **Impact:** a valid Wasm module with a runtime-computed source index near 2^32 reads **another
  segment's bytes** (or installs another element segment's function) instead of trapping
  `MemoryOutOfBounds`/`TableOutOfBounds`. rwasm returns a value where the spec — and wasmtime —
  trap: a silent miscompilation and a cross-backend divergence. No crafted bytecode is involved; the
  bug lives in the guard/rewrite sequence the compiler emits for *any* module with a passive
  segment.
- **Evidence (executed, `default_strategy_compatible()`):**

  ```
  (memory 1) (data (i32.const 0) "\11") (data "\aa\bb\cc\dd")
  main(dst, src, len) = { memory.init 1 (dst) (src) (len); i32.load8_u 0 }

  memory.init dst=0 src=1  len=1: rwasm=Ok(187)                 wasmtime=Ok(187)
  memory.init dst=0 src=9  len=1: rwasm=Err(MemoryOutOfBounds)  wasmtime=Err(MemoryOutOfBounds)
  memory.init dst=0 src=-1 len=1: rwasm=Ok(17)   <- wrong byte  wasmtime=Err(MemoryOutOfBounds)

  table.init  dst=0 src=-1 len=1: rwasm=Ok(100)  <- wrong func  wasmtime=Err(TableOutOfBounds)
  ```

- **Root cause:** the guard computes `n + s` with a wrapping `i32.add` plus a signed `i32.gt_s`,
  and the rewrite computes `s + segment_blob_offset` with the same wrapping add *before* the runtime
  bounds check sees it. The runtime then validates the already-wrapped offset against the
  concatenated blob that holds **all** segments (`src/compiler/segment_builder.rs:135`/`:157`), so
  the wrapped value lands in range. `RwasmModule` carries only the flattened `data_section`/
  `elem_section`, so the VM cannot check the segment's own window.
- **Same defect, opposite direction (also executed):** the rewrite is applied unconditionally, so a
  **dropped** segment (substituted by an empty slice in the VM) is still addressed with its flat
  offset and a legitimate zero-length operation falsely traps:

  ```
  (data "\11\22") (data "\aa\bb\cc\dd") + (data.drop 1)
  (memory.init 1 (i32.const 0) (i32.const 0) (i32.const 0))
      rwasm = Err(MemoryOutOfBounds)   wasmtime = Ok(1)     // spec: dropped segment has length 0
  (elem func $f0) (elem func $f1)       + (elem.drop 1)
  (table.init 1 (i32.const 0) (i32.const 0) (i32.const 0))
      rwasm = Err(TableOutOfBounds)    wasmtime = Ok(2)
  ```

  It escapes the in-tree regression test (`tests/memory.rs`) only because that test's segment has
  flat offset 0, which skips the rewrite (`.filter(|v| *v > 0)`).
- **Why the existing checks do not cover it:** the SAFETY NOTEs (`src/isa/memory.rs:7-20`,
  `src/isa/table.rs:4-21`) analyse only the *fuel* round-up wrap and assume the runtime bounds check
  backstops everything; the comment at `src/isa/memory.rs:259-261` states that assumption
  explicitly. `tests/fuel_alignment.rs` and the differential fuzzer never generate `src` values near
  2^32, and the spec suite only covers `n >= 1` after a drop.
- **Fix:** two-step non-wrapping check (`s > length` → trap; `n > length - s` → trap, unsigned)
  against the segment's original length. Skip the flattened-blob offset exactly when
  `s == 0 && n == 0`, using existing stack and branch instructions. The existing init instruction
  rejects every other input after a drop and still checks the destination for the empty copy.
  This requires neither per-segment metadata in the wire format nor new runtime opcodes.

### CRIT-2 — Table size limits are neither validated at compile time nor clamped consistently: valid modules silently get an empty or un-growable table — **FIXED**

- **Where:** `src/compiler/segment_builder.rs:107-119` (`emit_table_segment` drops the `table.grow`
  result), `src/vm/table_entity.rs:42-57` (`grow_untyped` caps at `N_MAX_TABLE_SIZE = 1024`),
  `src/compiler/translator.rs:2574-2604` (`visit_table_grow` feeds the *module-declared* maximum into
  the guard), `src/isa/table.rs:67-97` (guard uses a signed `i32.gt_s` against that constant).
- **Impact:** `table.size`, `table.grow`, `elem` initialization and `call_indirect` disagree between
  the backends for a valid module; for a declared maximum ≥ 2^31 the guard is *always* "overflow"
  because the limit constant is negative as `i32`.
- **Evidence (executed, `default_strategy_compatible()`):**

  ```
  (table 5000 funcref) (func (export "main") (result i32) (table.size 0))
      rwasm = 0        wasmtime = 5000
  (table 5000 funcref) (func $f) (elem (i32.const 0) $f) + table.size
      rwasm = instantiation Err(TableOutOfBounds)   wasmtime = Ok(5000)
  (table 1 3000000000 funcref) + table.grow 0 (ref.null func) 1
      rwasm = -1       wasmtime = 1
  ```

- **Why the existing checks do not cover it:** `N_MAX_TABLE_SIZE` appears only in the VM entity and
  in comments; `process_tables`/`emit_table_segment` never compare `table_type.initial`/`maximum`
  against it (contrast `add_memory_pages`, which verifies its grow and returns
  `MaxReadonlyDataReached`). wasmparser validates only `initial <= maximum` (10M entry cap), so all
  three modules above are legal. The SAFETY NOTEs in `src/isa/table.rs`/`src/isa/memory.rs` also
  *assume* "a table holds at most 1024 elements" as the premise that makes their signed compares
  unreachable — false for module-declared sizes.
- **Fix:** reject (or clamp and propagate) tables whose `initial`/`maximum` exceeds
  `N_MAX_TABLE_SIZE` at compile time, stop discarding the initial `table.grow` result, clamp the
  declared maximum before it reaches the signed compare, and add `.table_elements(...)` to the
  Wasmtime store limits so both engines reject the same modules.

---

## Class A — HIGH (untrusted Wasm, compiler-produced bytecode)

### HIGH-1 — Interpreter stack limits have no Wasmtime counterpart: deep operand stacks and deep recursion diverge — **FIXED** (operand stack; recursion documented)

- **Where:** `src/types/mod.rs:38-40` (`N_MAX_STACK_SIZE = 8192`, `N_MAX_RECURSION_DEPTH = 1024`),
  `src/vm/value_stack.rs:70-74`, `src/vm/executor/system.rs:34-44`,
  `src/vm/executor/control_flow.rs:112`/`:151` vs `src/wasmtime/engine.rs:22`
  (`max_wasm_stack(N_MAX_STACK_SIZE * 4)` bounds the *native* stack, not operand slots). The compiler
  never rejects a function whose tracked peak exceeds the rwasm window.
- **Evidence (executed):** `(func (export "main") (result i32) <8200 × i32.const 1> <8200 × drop> i32.const 42)`
  → `rwasm = Err(StackOverflow)`, `wasmtime = Ok(42)`. Delegated measurements: infinite
  self-recursion burns rwasm 11,275 vs wasmtime 10,747 fuel; the same function with 200 locals burns
  rwasm 533 vs wasmtime 12,701 (rwasm exhausts the 8192-slot window after ~40 frames).
- **Fix:** reject at compile time functions whose `max_stack_height` exceeds `N_MAX_STACK_SIZE`, and
  define one call-depth budget enforced by both engines; pin it with a differential test.

### HIGH-2 — `StrategyExecutor::execute(func_name)` silently ignores the name on the rwasm backend — **FIXED**

- **Where:** `src/strategy/module.rs:229-240` (rwasm arm drops `func_name`),
  `src/vm/instance.rs:22-29` vs `src/wasmtime/instance.rs:241-255` (resolves the export by name).
- **Evidence (executed):** module with `main` → 1 and `other` → 2, compiled with
  `.with_entrypoint_name("main")`: `execute("other")` returns **1 on rwasm** and **2 on wasmtime**.
- **Impact:** a host that dispatches by export name (the natural reading of the shared signature)
  runs different code per backend. Every test calls the configured entrypoint, so it never shows.
- **Fix:** resolve/validate `func_name` in the rwasm arm (needs an export table in the module model)
  or reject any name other than the configured entrypoint on both backends.

### HIGH-3 — The Wasmtime path validates a superset language and panics on untrusted Wasm — **FIXED**

- **Where:** `src/wasmtime/engine.rs:16-52` (`CompilationConfig::wasm_features()` is never applied),
  `src/compiler/config.rs:142-166`, `src/strategy/module.rs:29-38`, `:62-77` (`.expect`),
  `:111-120` + `src/wasmtime/instance.rs:111-128` (`panic!`), `src/wasmtime/instance.rs:369-375`
  (`unimplemented!`), `src/strategy.rs:31-34`.
- **Impact:** modules the rwasm compiler rejects are compiled *and executed* by the default strategy
  (a rwasm node rejects the deployment, a wasmtime node runs it), and malformed or non-instantiable
  modules abort the process instead of returning the `Result`/`TrapCode` the signatures promise.
  Untrusted Wasm is exactly the input this API is fed.
- **Evidence (executed, default features):**
  - SIMD `(i32x4.extract_lane 0 (v128.const i32x4 7 2 3 4))` → `RwasmModule::compile` =
    `Err(NotSupportedOpcode)`, while `StrategyDefinition::new` (the default entry point) = `Ok` and
    the module executes, returning 7. (Delegated run also shows multi-memory accepted and bound to a
    module-chosen memory.)
  - `StrategyExecutor::compile_and_instantiate(cfg, [0u8; 12], …)` → panic at
    `src/strategy/module.rs:73`.
  - Valid `(module (memory 2000) (func (export "main")))` → `create_executor` panics at
    `src/wasmtime/instance.rs:127` (memory limiter). Same class: unresolved import, trapping start
    section. `try_new` exists but no public path calls it.
  - `StrategyExecutor::resume` routes to `WasmtimeExecutor::resume`, which is `unimplemented!()`
    (`tests/interruption.rs` is `#[ignore]`d for exactly this reason).
- **Status:** the still-open FLU-1098/FLU-1099 class, now with witnesses.
- **Fix:** `module?` in `new_as_wasmtime`; call `try_new` from `create_executor` and map failures to
  `TrapCode`; disable the unsupported proposals on the Wasmtime engine config; return an error from
  `resume`.

### HIGH-4 — The shipped default configuration is still fuel-divergent, and `max_allowed_memory_pages` is enforced on one backend only — **FIXED**

- **Where:** `src/compiler/config.rs:81-100` (defaults `consume_fuel_for_bulk_ops = true`,
  `consume_fuel_for_params_and_locals = true`), `:117-130` (`is_strategy_compatible` — no caller in
  `src/`), `src/strategy/module.rs:62-77`; `src/compiler/segment_builder.rs:81-84` +
  `src/compiler/translator.rs:1561-1566` vs `src/wasmtime/mod.rs:43-55` and
  `src/strategy/module.rs:79-87`, `:217-225` (runtime limit hardcoded to `None`).
- **Impact:** with `CompilationConfig::default()` the same module burns different fuel per backend
  (measured: 4 locals + `memory.fill(…, 64)` → rwasm 1,039 vs wasmtime 10), and a non-default
  `max_allowed_memory_pages` changes accept/reject and `memory.grow` results (measured with the
  limit set to 16: rwasm `-1`, wasmtime `1`; `(memory 100)` compiles on wasmtime,
  `Err(MaxReadonlyDataReached)` on rwasm). `9d65e2c7` aligned the *shared* region schedule, so
  `default_strategy_compatible()` is clean — but nothing rejects the divergent configuration on the
  wasmtime path, though `config.rs:47-49` claims a "higher layer" does.
- **Fix:** reject non-strategy-compatible configs in `new_as_wasmtime`/`create_executor` (or make the
  extra metering an explicit opt-in), and propagate `max_allowed_memory_pages` into the Wasmtime
  store limit.

### HIGH-5 — Host linear memory binds by export name on Wasmtime and by index on rwasm — **FIXED**

- **Where:** `src/wasmtime/instance.rs:61-98` (`Extern::Memory(..) if name == "memory"`),
  `src/wasmtime/context.rs:90-92` vs `src/vm/store.rs:118-122` (rwasm always uses memory 0).
- **Evidence (executed):** `(module (memory (export "mem") 1) (func (export "main") (i32.store 0 0x04030201)))`
  → `memory_read(0, ..)` = `Ok([1,2,3,4])` on rwasm, `Err(MemoryOutOfBounds)` on wasmtime. Every
  host-side memory access (syscall handlers, `snapshot_memory`) inherits this.
- **Fix:** resolve memory by index (`instance.get_memory(store, 0)`) instead of by name, or reject
  modules that do not export memory 0 as `"memory"` at compile time so both backends agree.

---

## Class B — HIGH (requires rWasm bytecode the compiler does not produce)

Foreign instruction streams are out of scope under the documented trust model. The observations
below concern manually constructed or unverified artifacts, not compiler-produced instruction
targets. Decoding checks encoding only; hosts must establish provenance and integrity before
executing distributed rWasm. The retained defensive checks do not make arbitrary rWasm safe to run.

### HIGH-6 — Instruction-pointer memory unsafety, executor panics and resource amplification from foreign bytecode — **PARTIAL HARDENING; POINTER CHECKS EXCLUDED**

- **Where:** `src/vm/instr_ptr.rs:36-41` (`offset`), `:44-49` (`add`), `:59-64` (`get`);
  `src/vm/engine.rs:99-101` (`source_pc`, guarded only by `debug_assert!` at `:100`);
  `src/vm/executor.rs:176`, `:395-402`, `:460-471`;
  `src/vm/executor/control_flow.rs:19-49`, `:64-159`; `src/module/mod.rs:82-86`, `:201-211`,
  `:280-283`.
- **Impact:** out-of-bounds read through a raw pointer (UB) that aborts the process; the fetched
  word is interpreted as an `Opcode` complete with its immediate. The deleted `verify_module`
  (`src/module/verification.rs`, removed in `f8342864`) established exactly this invariant; nothing
  replaced it, and `InstructionPtr::is_valid` exists only under `#[cfg(feature = "tracing")]` with
  its single call site commented out (`src/vm/executor.rs:334-341`).
- **Evidence (executed):**
  - `RwasmModuleBuilder::new(instruction_set! { I32Const(0) Drop Br(i32::MAX) }).build()` +
    `ExecutionEngine::execute` → **SIGSEGV (signal 11)**; the same module via
    `serialize() → RwasmModule::new_checked() → execute` → **SIGSEGV**. Debug builds panic at
    `src/vm/engine.rs:100`/`control_flow.rs:46` instead, proving the guards are debug-only.
  - A 43-byte buffer (`code_section = [Return]`, `source_pc = 0xFFFFFFFF`) → **SIGBUS (exit 138)**
    in release (delegated run; `RwasmModuleBuilder::with_source_pc(u32::MAX)` reaches it without any
    wire bytes).
  - `Call(idx)` absent from the import linker → panic at `src/vm/executor.rs:467` (wasmtime returns
    `TrapCode::UnknownExternalFunction` for the same module — a behavior split and a regression of
    the syscall half of FLU-1097). `CallIndirect`/`TableInit` without their `TableGet` payload word →
    panic at `src/vm/executor.rs:400`. `BrTable(0)` → `targets as usize - 1` underflow
    (`control_flow.rs:46`).
  - `DataDrop(u32::MAX)`/`ElemDrop(u32::MAX)` → ~512 MiB bitset (measured peak RSS 538,624,000 B)
    from one instruction (`src/vm/executor/memory.rs:158-168`, `src/vm/executor/table.rs:145-155`).
    `BulkConst(0xFFFF_FFFF)` → 4.29e9 interpreter iterations with **zero fuel charged** (measured
    33.7 s debug / 7.9 s release; fuel is charged only by compiler-emitted
    `ConsumeFuel`/`ConsumeFuelStack`, so a hand-built `[Br(0)]` loop is unmetered).
- **Scope decision:** retain the one-time `source_pc` entry check, executor error handling,
  `checked_sub` in `visit_br_table`, bounded drop-segment indices, and the `BulkConst` limit. Restore
  the single-pointer `#[repr(transparent)]` representation and unchecked `offset`/`add`/`get`:
  instruction targets are established by Wasm validation and trusted code generation. Tests that
  execute out-of-range branches or unterminated instruction streams are outside that contract.
  The syscall trap remains useful for a linker/module mismatch in the supported execution model.
  The low-level pointer API is crate-private, and both execution loops reject empty code before
  their first fetch. Table-payload checks only reject an incorrect opcode in an existing slot;
  the compiler guarantees that the slot exists. They do not validate truncated instruction streams.
- **Performance evidence:** on an Apple M5 Max with Rust 1.93.1, release mode, and the `std`
  interpreter, seven interleaved samples using identical compiled bytecode measured metered warm
  `examples/fib` execution at 694 ns with the checked pointer versus 316 ns with only the thin
  pointer restored (devel: 317 ns). The expanded pointer was 32 bytes instead of 8, and the inline
  `CallStack` grew from 144 to 528 bytes. Other control-flow and memory kernels showed 1.7–2.6x
  slowdowns; a locals-initialization kernel was unchanged. This is local microbenchmark evidence,
  not an end-to-end application estimate. The per-instruction bounds checks are not retained.

### HIGH-7 — Safe public wrapper over `unsafe wasmtime::Module::deserialize`; module cache keyed on caller bytes only — **FIXED**

- **Where:** `src/wasmtime/mod.rs:29-41` (safe `pub fn` around `unsafe Module::deserialize`, no
  `# Safety`, no authentication; used by the `cache-compiled-artifacts` path at
  `src/wasmtime/engine.rs:55-67`), `:59-78` (LRU keyed by the caller's `[u8; 32]`; the comment admits
  the config is not checked), `src/wasmtime/engine.rs:11-14` (`OnceLock` pins the first caller's
  config).
- **Impact:** only if a host feeds it bytes it did not compile (the model forbids this), but a safe
  `pub fn` that can load attacker-chosen native code is unsound API surface, and a key collision
  silently returns a module compiled from different bytes/under a different config (delegated run:
  wasm A with key `[7;32]`, then wasm B with the same key → A is returned and executed).
- **Fix:** make the deserializer `unsafe` with a documented contract or authenticate artifacts
  (keyed MAC); include a hash of the bytes and of the codegen/fuel-relevant config in the cache key;
  drop or fix the config-pinning shared engine.

### HIGH-8 — Verification gaps that let every finding above ship undetected — **PARTIALLY FIXED** (fuzz job, release tests, clippy gate, publish gate, trap-code oracle; the oracle still treats trailing zero memory bytes as equal and skips `memory.grow`)

- **The differential fuzzer is never run or compiled by CI:** no workflow references `fuzz/`
  (`fuzz/Cargo.toml:45` isolates it as its own workspace, so `make test` and
  `cargo clippy --all-targets` never reach it; corpora/artifacts are gitignored). Precisely the
  divergences in CRIT-1/CRIT-2 and HIGH-1/2/5 were found by manual fuzzing and probes, not by CI.
- **No release-mode test run:** the only `--release` invocation is `Makefile:27`, filtered to two
  `#[ignore]`d tests that compare nothing (`tests/fluentbase.rs:97` discards per-strategy results).
  The release-mode value-stack guards (the FLU-1094 fix) and the `#[cfg(debug_assertions)]`
  watermark assertions are never exercised where they matter.
- **The fuzz oracle cannot see acceptance divergence or trap identity:** it skips any input rwasm
  rejects (`fuzz_targets/differential.rs:338-340`, `:418-425`) or wasmtime rejects (`:216-220`),
  treats any two traps as equal (`:383-384`), compares fuel only when both sides succeed
  (`:368-374`), excludes `memory.grow` (`:205-208`), accepts zero-padded memory differences
  (`:714-727`), and sets `max_imports = 0`, so the syscall/host-ABI surface is never fuzzed.
- **Fuel parity is asserted only for `default_strategy_compatible()`** (`tests/fuel_alignment.rs:65`);
  the shipped default has no gate, and `tests/fuzz.rs:39-42` degrades to a self-comparison when only
  one strategy is present (no `assert_eq!(outcomes.len(), 2)`).
- **Executor panic-freedom is not gated:** `unreachable!` remains at `src/vm/executor.rs:63`, `:74`,
  `:400`, `:467`; the 2026-08-07 re-review recommended a scoped `deny(clippy::unwrap_used)`; clippy
  CI runs without `-- -D warnings`.
- **Publish is not gated on tests** (`publish.yml` runs a version check and `cargo publish --dry-run`
  only), and the `tracing` surface (~950 lines) is compiled but never executed by any job.
- **Fix (priority order):** add a bounded `cargo +nightly fuzz run differential` job plus
  `cargo +nightly fuzz build differential`; add `cargo test --release`; make the oracle compare
  `(trap_code, remaining_fuel)` and exported memory length and stop skipping acceptance mismatches;
  add the missing strategy-count assertion; enable `-D warnings` with scoped panic lints; gate
  publish on CI.

---

## Trust-model caveat (question for the maintainers)

The trust boundary holds only if **every** production path compiles the Wasm itself. Two facts make
that worth confirming rather than assuming:

1. `RwasmModuleInner::hint_section` stores the *original* Wasm inside the compiled artifact
   (`src/module/mod.rs:151-155`), and `source_pc` addresses the entrypoint inside the compiled
   stream. That design only pays off if compiled rWasm is what gets distributed; the README's own
   wording — *"If a compiled rWasm module is distributed, the host must verify its integrity and
   provenance and check its accompanying codegen identity before execution"* — anticipates exactly
   that, but the crate ships no API to perform the check (`codegen_identity()` is not embedded in
   the wire format, and structural verification was deleted).
2. The `README` warning is not machine-enforced: `RwasmModule::new_checked` + `ExecutionEngine::execute`
   is ordinary safe API, and `e2e/src/group.rs:44` itself decodes and executes an artifact.

If any node executes compiled rWasm received from a deployer (rather than recompiling the Wasm
locally), then CRIT-1/CRIT-2 here become attacker-reachable and HIGH-6 becomes a CRIT. Cheap ways to
make the boundary real, in increasing cost: document the precondition on `new_checked` as `unsafe` or
put the decode behind a `TrustedArtifact` token; re-verify at load by recompiling `hint_section` and
comparing bytecode (the original Wasm is already embedded); or restore a structural pass and require
it for any module that was not produced in-process.

---

## Status of prior findings (2026-08-07 audit, verified at `bd935e0b`)

| Finding | Status now |
| --- | --- |
| FLU-1094 (value-stack OOB read/write) | **Fixed** — every `ValueStackPtr` access is bounds-checked in all profiles and the sticky OOB flag is observed in `step` before any host call; regression tests pass. |
| FLU-1095 (SIMD accepted, translator skips it) | **Fixed on the rwasm path, still open end-to-end** — the validator/translator reject the proposal, but the Wasmtime engine never receives `wasm_features()`, so the default strategy still accepts and executes SIMD (HIGH-3). |
| FLU-1096 (decode allocation) | **Fixed** — verified: 11–35-byte buffers announcing `u64::MAX`/`2^40`/`2^62` peak at ≤ 64 KiB and fail with `UnexpectedEnd`. |
| FLU-1097 (executor panics) | **Partially regressed** — the table half is fixed (`resolve_table`); the syscall half panics again at `executor.rs:467` (now documented as intentional) and diverges from the wasmtime backend. New panics: `fetch_table_index` (`:400`), `BrTable(0)`. |
| FLU-1098 (`new_as_wasmtime` panic / no rwasm validation on the default path) | **Open**, re-confirmed with witnesses (HIGH-3). |
| FLU-1099 (`WasmtimeExecutor::new` panics) | **Open**, re-confirmed (HIGH-3). |
| FLU-1100 (`data.drop`/`elem.drop` bitset truncation) | **Fixed** (grow-only update) — the remaining `init`/`drop` defects are CRIT-1, a different root cause. |
| FLU-1101 (default config fuel divergence) | **Open but narrower** — the shared schedule is aligned; the two rwasm-only flags still diverge under `CompilationConfig::default()`, and nothing enforces `is_strategy_compatible()` (HIGH-4). |
| FLU-1102 (safe `deserialize`) / FLU-1103 (caches without config) | **Open** (HIGH-7). |
| FLU-1104 (bulk-op guard overflow) | **Fixed and now load-bearing**: the guards were "unreachable-unsound" only under the 1024-element table assumption, which module-declared table sizes break (CRIT-2). |
| FLU-1105/1106/1108 (handler allocation, spin mutex, panicking APIs) | **Open**; the reference handler remains the copyable example (`src/vm/handler.rs`). |
| FLU-1107 (codegen identity not on the wire) | **Open by design**; the `fpu`-build/default-build split stays a real divergence if a host mixes builds without checking `codegen_identity()`. |
| Verification removal (`f8342864`) | The claim "the verification pass no longer guards anything the VM doesn't already enforce" is **false** (IP bounds, table-payload shape, syscall resolution). Under the trusted-artifact model this is a hardening gap (HIGH-6); it becomes a CRIT if foreign artifacts are ever executed. |

## Verified sound (no findings)

- Value-stack cell access, pointer-window invariant, sticky OOB propagation, reallocation handling,
  and the stack-depth ceiling (`src/vm/value_stack.rs`, `src/vm/executor.rs:215-221`).
- Linear memory: `checked_add` effective addresses, slice bounds checks, fallible growth
  (`src/vm/memory.rs:45-66`), `Pages` range checks; table entity operations.
- i64 arithmetic and snippet lowering (div/rem edge cases, limb order, shift masking, `MSH_*`
  accounting) — hand-verified and fuzz-covered; `DropKeep`/`local.tee`/`select`/`br_if` depth
  semantics; `br_table` offset arithmetic; branch-offset range errors; locals/table-index bounds.
- Wire format: opcode codes unique and gap-free, all 160 variants round-trip byte-identically,
  400k mutated/truncated buffers decoded with zero panics, no integer→`Opcode` transmute.
- No hash-map-iteration or host-state dependence in emitted bytecode; no input-reachable compiler
  panic found.
- Trap-code propagation through the Wasmtime raw trampoline, and fuel parity for the shared schedule
  under `default_strategy_compatible()` (21 differential cases in `tests/fuel_alignment.rs`).

## Recommended order of work

1. **CRIT-1** (`memory.init`/`table.init` wrap and dropped-segment false trap) — smallest fix,
   highest certainty, no crafted input needed.
2. **CRIT-2** (table limits) — compile-time rejection plus the Wasmtime table limit.
3. **HIGH-3/HIGH-4** — one accepted language, one limit set, one fuel schedule across strategies; no
   panics on untrusted Wasm.
4. **HIGH-1/HIGH-2/HIGH-5** — pin stack limits, entrypoint dispatch and memory binding differentially.
5. **HIGH-8** — close the CI gates (fuzz job first), so regressions of the divergences above cannot
   ship again.
6. **HIGH-6/HIGH-7** — the foreign-bytecode hardening set, together with a decision on the trust
   boundary (enforce it in the API, or keep it documentary and accept the residual risk).
