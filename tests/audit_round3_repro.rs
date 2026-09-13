//! Reproductions for the findings of the 2026-09-13 round-3 audit.
//!
//! Every test in this file is written against the **correct** behaviour, so at the audited revision
//! the ones marked `R3-*` fail and print the two backend outcomes side by side. They are the
//! reproduction for the findings in `audits/2026-09-13-rwasm-audit-round3.md`; once the findings
//! are fixed they turn green and become regression tests.
//!
//! `out_of_range_branch_target_aborts_the_process` is different: it pins behaviour that the
//! maintainers documented as intentional (`tests/instruction_pointer_bounds.rs`), so it passes
//! today by asserting that the child process dies from a fatal signal. If rwasm ever validates
//! branch targets, that test has to be inverted into "the module is rejected".

use rwasm::{
    always_failing_syscall_handler, instruction_set, wasmtime::WasmtimeExecutor, wasmtime::WasmtimeModule,
    CompilationConfig, ExecutionEngine, ImportLinker, ImportName, RwasmModule, RwasmModuleBuilder,
    RwasmStore, StrategyDefinition, StoreTr, SyscallFuelParams, TrapCode, TypedCaller, ValType, Value,
};
use std::sync::{Arc, Mutex};

type Ctx = Arc<Mutex<Vec<u32>>>;

fn test_config(linker: &Arc<ImportLinker>) -> CompilationConfig {
    CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(linker.clone())
}

fn noop(_: &mut TypedCaller<'_, Ctx>, _: u32, _: &[Value], _: &mut [Value]) -> Result<(), TrapCode> {
    Ok(())
}

fn rwasm_module(linker: &Arc<ImportLinker>, wat: &str) -> RwasmModule {
    let wasm = wat::parse_str(wat).expect("the test module parses");
    RwasmModule::compile(test_config(linker), &wasm)
        .expect("rwasm compiles the test module")
        .0
}

/// Runs `module` on a fresh store of `store`, which may already have hosted another instance.
fn rwasm_execute(
    linker: &Arc<ImportLinker>,
    store: &mut RwasmStore<Ctx>,
    module: RwasmModule,
    result: &mut [Value],
) -> Result<(), TrapCode> {
    let engine = ExecutionEngine::new();
    let instance = linker.instantiate(store, engine, module)?;
    instance.execute(store, &[], result)
}

fn wasmtime_executor(
    linker: &Arc<ImportLinker>,
    wat: &str,
) -> WasmtimeExecutor<Ctx> {
    let wasm = wat::parse_str(wat).expect("the test module parses");
    let module = rwasm::wasmtime::compile_wasmtime_module(test_config(linker), &wasm)
        .expect("wasmtime compiles the test module");
    WasmtimeExecutor::new(module, linker.clone(), Ctx::default(), noop, Some(1_000_000), None)
        .expect("wasmtime instantiates the test module")
}

fn wasmtime_instantiate(executor: &mut WasmtimeExecutor<Ctx>, wat: &str) {
    let wasm = wat::parse_str(wat).expect("the test module parses");
    let module = WasmtimeModule::new(executor.store.engine(), &wasm)
        .expect("wasmtime builds the second module");
    executor
        .instantiate(&module)
        .expect("wasmtime instantiates the second module");
}

// ---------------------------------------------------------------------------------------------
// R3-1: `RwasmStore` instance state (tables, memory, segment flags) survives a second
// instantiation on the same store, so contract B observes contract A's leftovers.
// ---------------------------------------------------------------------------------------------

const TABLE_A: &str = r#"(module
     (memory (export "memory") 1)
     (type $t (func (result i32)))
     (table 1 funcref)
     (func $f (result i32) (i32.const 111))
     (elem func $f)
     (func (export "main") (result i64)
       (table.init 0 (i32.const 0) (i32.const 0) (i32.const 1))
       (i64.extend_i32_u (call_indirect (type $t) (i32.const 0)))))"#;

const TABLE_B: &str = r#"(module
     (memory (export "memory") 1)
     (type $t (func (result i32)))
     (table 1 funcref)
     (func $g (result i32) (i32.const 222))
     (elem func $g)
     (func (export "main") (result i64)
       (i64.extend_i32_u (call_indirect (type $t) (i32.const 0)))))"#;

/// A table entry written by module A is still there when module B runs on the same store, so B's
/// `call_indirect` dispatches into A's code layout: B returns `I32(222)` — the *wrong function*,
/// reached through a code offset B never wrote. Wasmtime isolates the two instances and traps
/// `IndirectCallToNull`. With a larger first module the stale offset leaves B's code section
/// entirely (see the audit report).
#[test]
fn table_entries_do_not_leak_between_instances() {
    let linker = Arc::new(ImportLinker::default());
    let mut store = RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
    let mut first = [Value::I64(-1)];
    rwasm_execute(&linker, &mut store, rwasm_module(&linker, TABLE_A), &mut first)
        .expect("module A runs");
    assert_eq!(first, [Value::I64(111)]);

    let mut second = [Value::I64(-1)];
    let rwasm = rwasm_execute(&linker, &mut store, rwasm_module(&linker, TABLE_B), &mut second)
        .map(|()| second[0].clone());

    let mut wasmtime = wasmtime_executor(&linker, TABLE_A);
    let mut first = [Value::I64(-1)];
    wasmtime.execute("main", &[], &mut first).unwrap();
    assert_eq!(first, [Value::I64(111)]);
    wasmtime_instantiate(&mut wasmtime, TABLE_B);
    let mut second = [Value::I64(-1)];
    let wasmtime = wasmtime
        .execute("main", &[], &mut second)
        .map(|()| second[0].clone());

    assert_eq!(
        rwasm, wasmtime,
        "the second instance must not see the first instance's table: rwasm={rwasm:?}, \
         wasmtime={wasmtime:?}"
    );
}

const MEMORY_A: &str = r#"(module
     (memory (export "memory") 1)
     (func (export "main") (result i64)
       (drop (memory.grow (i32.const 2)))
       (i32.store (i32.const 100000) (i32.const 0x5eed))
       (i64.extend_i32_u (memory.size))))"#;

const MEMORY_B: &str = r#"(module
     (memory (export "memory") 1)
     (func (export "main") (result i64)
       (i64.or (i64.shl (i64.extend_i32_u (memory.size)) (i64.const 32))
               (i64.extend_i32_u (i32.load (i32.const 100000))))))"#;

/// Module B declares one page and reads offset 100000, which is out of *its* memory. On rwasm it
/// reads module A's bytes instead (cross-instance disclosure) and reports A's page count; on
/// wasmtime it traps `MemoryOutOfBounds`.
#[test]
fn linear_memory_does_not_leak_between_instances() {
    let linker = Arc::new(ImportLinker::default());
    let mut store = RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
    let mut first = [Value::I64(-1)];
    rwasm_execute(&linker, &mut store, rwasm_module(&linker, MEMORY_A), &mut first)
        .expect("module A runs");
    assert_eq!(first, [Value::I64(3)], "A grows to three pages");
    let mut second = [Value::I64(-1)];
    let rwasm = rwasm_execute(&linker, &mut store, rwasm_module(&linker, MEMORY_B), &mut second)
        .map(|()| second[0].clone());

    let mut wasmtime = wasmtime_executor(&linker, MEMORY_A);
    let mut first = [Value::I64(-1)];
    wasmtime.execute("main", &[], &mut first).unwrap();
    wasmtime_instantiate(&mut wasmtime, MEMORY_B);
    let mut second = [Value::I64(-1)];
    let wasmtime = wasmtime
        .execute("main", &[], &mut second)
        .map(|()| second[0].clone());

    assert_eq!(
        rwasm, wasmtime,
        "the second instance must not read the first instance's memory: rwasm={rwasm:?}, \
         wasmtime={wasmtime:?}"
    );
}

const SEGMENTS_A: &str = r#"(module
     (memory (export "memory") 1)
     (data "\aa\bb")
     (func (export "main") (result i64)
       (memory.init 0 (i32.const 0) (i32.const 0) (i32.const 2))
       (data.drop 0)
       (i64.extend_i32_u (i32.load8_u (i32.const 0)))))"#;

const SEGMENTS_B: &str = r#"(module
     (memory (export "memory") 1)
     (data "\cc\dd")
     (func (export "main") (result i64)
       (memory.init 0 (i32.const 8) (i32.const 0) (i32.const 2))
       (data.drop 0)
       (i64.extend_i32_u (i32.load8_u (i32.const 8)))))"#;

/// `data.drop 0` in module A marks module B's segment 0 as dropped, so B traps
/// `MemoryOutOfBounds` where wasmtime copies the data and returns `204`.
#[test]
fn dropped_data_segment_flags_do_not_leak_between_instances() {
    let linker = Arc::new(ImportLinker::default());
    let mut store = RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
    let mut first = [Value::I64(-1)];
    rwasm_execute(&linker, &mut store, rwasm_module(&linker, SEGMENTS_A), &mut first)
        .expect("module A runs");
    assert_eq!(first, [Value::I64(170)]);
    let mut second = [Value::I64(-1)];
    let rwasm = rwasm_execute(&linker, &mut store, rwasm_module(&linker, SEGMENTS_B), &mut second)
        .map(|()| second[0].clone());

    let mut wasmtime = wasmtime_executor(&linker, SEGMENTS_A);
    let mut first = [Value::I64(-1)];
    wasmtime.execute("main", &[], &mut first).unwrap();
    wasmtime_instantiate(&mut wasmtime, SEGMENTS_B);
    let mut second = [Value::I64(-1)];
    let wasmtime = wasmtime
        .execute("main", &[], &mut second)
        .map(|()| second[0].clone());

    assert_eq!(
        rwasm, wasmtime,
        "the second instance must start with its own segment flags: rwasm={rwasm:?}, \
         wasmtime={wasmtime:?}"
    );
}

const ELEMENTS_A: &str = r#"(module
     (type $t (func (result i32)))
     (table 1 funcref)
     (func $f (result i32) (i32.const 111))
     (elem func $f)
     (func (export "main") (result i64)
       (table.init 0 (i32.const 0) (i32.const 0) (i32.const 1))
       (elem.drop 0)
       (i64.extend_i32_u (call_indirect (type $t) (i32.const 0)))))"#;

const ELEMENTS_B: &str = r#"(module
     (type $t (func (result i32)))
     (table 1 funcref)
     (func $g (result i32) (i32.const 222))
     (elem func $g)
     (func (export "main") (result i64)
       (table.init 0 (i32.const 0) (i32.const 0) (i32.const 1))
       (elem.drop 0)
       (i64.extend_i32_u (call_indirect (type $t) (i32.const 0)))))"#;

/// The element-segment mirror of the data-segment leak: `elem.drop 0` in module A used to make
/// module B's `table.init 0` trap `TableOutOfBounds`, because the drop bitset lives in the store
/// and was never cleared for the new instance.
#[test]
fn dropped_element_segment_flags_do_not_leak_between_instances() {
    let linker = Arc::new(ImportLinker::default());
    let mut store = RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
    let mut first = [Value::I64(-1)];
    rwasm_execute(&linker, &mut store, rwasm_module(&linker, ELEMENTS_A), &mut first)
        .expect("module A runs");
    assert_eq!(first, [Value::I64(111)]);
    let mut second = [Value::I64(-1)];
    let rwasm = rwasm_execute(&linker, &mut store, rwasm_module(&linker, ELEMENTS_B), &mut second)
        .map(|()| second[0].clone());

    let mut wasmtime = wasmtime_executor(&linker, ELEMENTS_A);
    let mut first = [Value::I64(-1)];
    wasmtime.execute("main", &[], &mut first).unwrap();
    wasmtime_instantiate(&mut wasmtime, ELEMENTS_B);
    let mut second = [Value::I64(-1)];
    let wasmtime = wasmtime
        .execute("main", &[], &mut second)
        .map(|()| second[0].clone());

    assert_eq!(
        rwasm, wasmtime,
        "the second instance must start with its own element segment flags: rwasm={rwasm:?}, \
         wasmtime={wasmtime:?}"
    );
}

const PAGES: &str = r#"(module
     (memory (export "memory") 1)
     (func (export "main") (result i64) (i64.extend_i32_u (memory.size))))"#;

/// Guard for the two resets above: releasing the previous instance's memory at instantiation must
/// not clear the memory of the instance that is running. Wasm keeps one memory per instance, so
/// bytes and grown pages survive every call of that instance.
#[test]
fn memory_persists_across_calls_of_one_instance() {
    const COUNTER: &str = r#"(module
         (memory (export "memory") 1)
         (func (export "main") (result i64)
           (drop (memory.grow (i32.const 1)))
           (i32.store (i32.const 8) (i32.add (i32.load (i32.const 8)) (i32.const 1)))
           (i64.or (i64.shl (i64.extend_i32_u (memory.size)) (i64.const 32))
                   (i64.extend_i32_u (i32.load (i32.const 8))))))"#;
    let linker = Arc::new(ImportLinker::default());
    let mut store = RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
    let module = rwasm_module(&linker, COUNTER);
    let engine = ExecutionEngine::new();
    let instance = linker
        .instantiate(&mut store, engine, module)
        .expect("the module instantiates");

    for round in 1..=2 {
        let mut result = [Value::I64(-1)];
        instance
            .execute(&mut store, &[], &mut result)
            .expect("each call of the same instance runs");
        // Every call grows one page and increments the byte, so the second call must observe the
        // size (3 pages) and the byte (2) left behind by the first one.
        let pages = round as u64 + 1;
        let counter = round as u64;
        assert_eq!(
            result,
            [Value::I64(((pages << 32) | counter) as i64)],
            "round {round}: the instance lost its memory between calls"
        );
    }
}

/// Re-instantiating the same module on a store that was `reset(false)` grows the memory a second
/// time (the init prologue adds its pages to the retained memory) instead of starting from one
/// page, so the same module reports different `memory.size` values per round.
#[test]
fn reset_restores_the_initial_memory_size() {
    let linker = Arc::new(ImportLinker::default());
    let mut store = RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
    let module = rwasm_module(&linker, PAGES);
    let mut rwasm_first = [Value::I64(-1)];
    rwasm_execute(&linker, &mut store, module.clone(), &mut rwasm_first).expect("first round runs");
    assert_eq!(rwasm_first, [Value::I64(1)]);

    store.reset(false);
    let mut rwasm_second = [Value::I64(-1)];
    rwasm_execute(&linker, &mut store, module, &mut rwasm_second).expect("second round runs");

    let mut wasmtime = wasmtime_executor(&linker, PAGES);
    let mut wasmtime_first = [Value::I64(-1)];
    wasmtime.execute("main", &[], &mut wasmtime_first).unwrap();
    wasmtime_instantiate(&mut wasmtime, PAGES);
    let mut wasmtime_second = [Value::I64(-1)];
    wasmtime.execute("main", &[], &mut wasmtime_second).unwrap();

    assert_eq!(
        (rwasm_first[0].clone(), rwasm_second[0].clone()),
        (wasmtime_first[0].clone(), wasmtime_second[0].clone()),
        "a reset store must run the module like a fresh one: rwasm reported {rwasm_first:?} then \
         {rwasm_second:?}, wasmtime {wasmtime_first:?} then {wasmtime_second:?}"
    );
}

/// Guard for the three resets above: clearing the store's instance state at instantiation must not
/// clear the instance that is running. A table entry written by one call of an instance has to be
/// visible to its next call.
#[test]
fn table_entries_persist_across_calls_of_one_instance() {
    const DISPATCH: &str = r#"(module
         (type $t (func (result i32)))
         (table 1 funcref)
         ;; exported, so `ref.func $f` is in the declared-function set the validator requires
         (func $f (export "f") (result i32) (i32.const 7))
         (func (export "main") (param i32) (result i64)
           (if (local.get 0) (then (table.set 0 (i32.const 0) (ref.func $f))))
           (i64.extend_i32_u (call_indirect (type $t) (i32.const 0)))))"#;
    let linker = Arc::new(ImportLinker::default());
    let mut store = RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
    let engine = ExecutionEngine::new();
    let instance = linker
        .instantiate(&mut store, engine, rwasm_module(&linker, DISPATCH))
        .expect("the module instantiates");

    // The first call installs `$f` in the table, the second never touches it and must still reach
    // it: only a new instantiation may drop a table.
    for (round, install) in [(1u32, 1i32), (2, 0)] {
        let mut result = [Value::I64(-1)];
        instance
            .execute(&mut store, &[Value::I32(install)], &mut result)
            .unwrap_or_else(|err| panic!("round {round} trapped: {err:?}"));
        assert_eq!(result, [Value::I64(7)], "round {round}");
    }
}

// ---------------------------------------------------------------------------------------------
// R3-2: `RwasmStore::reset` keeps the parked interruption.
// ---------------------------------------------------------------------------------------------

#[test]
fn reset_discards_the_parked_interruption() {
    fn interrupt_once(
        ctx: &mut TypedCaller<'_, Ctx>,
        _: u32,
        _: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
        let mut calls = ctx.data().lock().unwrap();
        calls.push(0);
        if calls.len() == 1 {
            return Err(TrapCode::InterruptionCalled);
        }
        Ok(())
    }

    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "tick"),
        0x68,
        SyscallFuelParams::default(),
        &[],
        &[],
    );
    let linker = Arc::new(linker);
    let module = rwasm_module(
        &linker,
        r#"(module
             (import "env" "tick" (func $i))
             (memory (export "memory") 1)
             (func (export "main") (result i64)
               (call $i)
               (i32.store (i32.const 4) (i32.add (i32.load (i32.const 4)) (i32.const 1)))
               (i64.extend_i32_u (i32.load (i32.const 4)))))"#,
    );
    let engine = ExecutionEngine::new();
    let ctx = Ctx::default();
    let mut store = RwasmStore::new(
        linker.clone(),
        ctx.clone(),
        interrupt_once,
        Some(1_000_000),
        None,
    );
    let instance = linker.instantiate(&mut store, engine, module).unwrap();

    let mut result = [Value::I64(-1)];
    assert_eq!(
        instance.execute(&mut store, &[], &mut result),
        Err(TrapCode::InterruptionCalled)
    );

    store.reset(false);
    let mut result = [Value::I64(-1)];
    instance.execute(&mut store, &[], &mut result).expect("the second run completes");
    assert_eq!(result, [Value::I64(1)], "the second run increments the counter once");
    let ctx_after_second_run = ctx.lock().unwrap().len();

    // The host asked `reset` to drop the execution state, so there is nothing left to resume.
    let mut result = [Value::I64(-1)];
    let resumed = instance.resume(&mut store, &[], &mut result);
    assert_eq!(
        resumed,
        Err(TrapCode::IllegalOpcode),
        "a reset store must not resume the discarded execution (it returned {resumed:?} and \
         re-ran the body: result={result:?})"
    );
    assert_eq!(
        ctx.lock().unwrap().len(),
        ctx_after_second_run,
        "resuming after `reset` must not invoke the host again"
    );
}

// ---------------------------------------------------------------------------------------------
// R3-3: the syscall result buffer contract differs between the two backends.
// ---------------------------------------------------------------------------------------------

fn import_linker_with_result(linker: &mut ImportLinker, result: &'static [ValType]) {
    linker.insert_function(
        ImportName::new("hello", "world"),
        0x66,
        SyscallFuelParams::default(),
        &[ValType::I32],
        result,
    );
}

fn execute_both(
    linker: &Arc<ImportLinker>,
    wat: &str,
    handler: rwasm::SyscallHandler<Ctx>,
) -> (Result<Value, TrapCode>, Result<Value, TrapCode>) {
    let wasm = wat::parse_str(wat).unwrap();
    let rwasm = StrategyDefinition::new_as_rwasm(test_config(linker), &wasm)
        .expect("rwasm compiles")
        .create_executor(linker.clone(), Ctx::default(), handler, Some(1_000_000), None)
        .expect("rwasm instantiates");
    let mut rwasm = rwasm;
    let mut result = [Value::I64(-1)];
    let rwasm = rwasm
        .execute("main", &[], &mut result)
        .map(|()| result[0].clone());

    let wasmtime = StrategyDefinition::new_as_wasmtime(test_config(linker), &wasm, None)
        .expect("wasmtime compiles")
        .create_executor(linker.clone(), Ctx::default(), handler, Some(1_000_000), None)
        .expect("wasmtime instantiates");
    let mut wasmtime = wasmtime;
    let mut result = [Value::I64(-1)];
    let wasmtime = wasmtime
        .execute("main", &[], &mut result)
        .map(|()| result[0].clone());
    (rwasm, wasmtime)
}

/// A handler that returns `Ok(())` without writing its result is answered with a typed zero on
/// rwasm and with `BadSignature` by the Wasmtime trampoline: the same module and handler produce
/// different outcomes per backend.
#[test]
fn unwritten_syscall_result_agrees_between_backends() {
    let mut linker = ImportLinker::default();
    import_linker_with_result(&mut linker, &[ValType::I64]);
    let linker = Arc::new(linker);
    let (rwasm, wasmtime) = execute_both(
        &linker,
        r#"(module
             (import "hello" "world" (func $i (param i32) (result i64)))
             (func (export "main") (result i64) (call $i (i32.const 1))))"#,
        noop,
    );
    assert_eq!(
        rwasm, wasmtime,
        "an unwritten result must be answered the same way by both backends: rwasm={rwasm:?}, \
         wasmtime={wasmtime:?}"
    );
}

/// A handler that writes the wrong value type is rejected with `BadSignature` by Wasmtime, while
/// rwasm pushes it onto the operand stack: the extra `i64` cell desynchronizes the stack, so the
/// guest computes `0x11223344 + 5` from the low half of the value instead of trapping.
#[test]
fn mistyped_syscall_result_is_rejected_by_both_backends() {
    fn wrong_type(
        _: &mut TypedCaller<'_, Ctx>,
        _: u32,
        _: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        result[0] = Value::I64(0x1122_3344_5566_7788);
        Ok(())
    }
    let mut linker = ImportLinker::default();
    import_linker_with_result(&mut linker, &[ValType::I32]);
    let linker = Arc::new(linker);
    let (rwasm, wasmtime) = execute_both(
        &linker,
        r#"(module
             (import "hello" "world" (func $i (param i32) (result i32)))
             (func (export "main") (result i64)
               (i64.extend_i32_u (i32.add (call $i (i32.const 1)) (i32.const 5)))))"#,
        wrong_type,
    );
    assert_eq!(
        rwasm, wasmtime,
        "a mistyped handler result must be rejected the same way by both backends: \
         rwasm={rwasm:?}, wasmtime={wasmtime:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Previously reported (round 2): the safe public API still aborts the process on an out-of-range
// branch target. Kept here because the round-2 report is no longer in the repository.
// ---------------------------------------------------------------------------------------------

/// Test name of the crashing payload, used to re-invoke this binary in child mode.
const CRASH_TEST: &str = "out_of_range_branch_target_aborts_the_process";

fn run_payload(which: &str) {
    let module = match which {
        // A module built through the safe builder API. `Br` is a compiler-internal opcode, but
        // `RwasmModuleBuilder` is `pub` and accepts any instruction set.
        "builder" => RwasmModuleBuilder::new(instruction_set! { Br(i32::MAX) }).build(),
        // A module loaded from a byte string, the documented way a host restores a cached module.
        // The branch immediate survives `serialize` / `new_checked` unchanged.
        "bytecode" => {
            let bytes = RwasmModuleBuilder::new(instruction_set! { Br(i32::MAX) })
                .build()
                .serialize();
            RwasmModule::new_checked_exact(&bytes).expect("the encoding is well formed")
        }
        other => panic!("unknown payload: {other}"),
    };
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::new(
        ImportLinker::default().into(),
        (),
        always_failing_syscall_handler,
        None,
        None,
    );
    // The interpreter fetches the next instruction from `code_section + i32::MAX`, far outside
    // the code section, and dereferences it.
    let outcome = engine.execute(&mut store, &module, &[], &mut []);
    panic!("rwasm returned {outcome:?} instead of aborting on an out-of-range branch target");
}

#[test]
fn out_of_range_branch_target_aborts_the_process() {
    // Child mode: perform the crashing execution and let the signal kill this process.
    if let Some(which) = std::env::var_os("RWASM_ROUND3_CHILD") {
        run_payload(&which.to_string_lossy());
        unreachable!("the child must not return from its payload");
    }

    // Parent mode: re-invoke this test binary for each payload and require a fatal signal.
    for which in ["builder", "bytecode"] {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CRASH_TEST, "--nocapture"])
            .env("RWASM_ROUND3_CHILD", which)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("the test binary must be re-executable");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let status = loop {
            match child.try_wait().expect("waiting for the child must succeed") {
                Some(status) => break status,
                None if std::time::Instant::now() > deadline => {
                    let _ = child.kill();
                    panic!("the `{which}` payload neither crashed nor returned within 30s");
                }
                None => std::thread::sleep(std::time::Duration::from_millis(25)),
            }
        };

        use std::os::unix::process::ExitStatusExt;
        let signal = status.signal();
        assert!(
            matches!(signal, Some(4) | Some(6) | Some(10) | Some(11)),
            "the `{which}` payload must abort the process, but it exited with {status:?} \
             (signal={signal:?}); a clean exit means the interpreter survived an out-of-range \
             branch target, which would be a correctness bug instead of a crash"
        );
    }
}
