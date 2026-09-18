//! Replacement initialization must preserve the old instance until it can commit.

use rwasm::{
    instruction_set, CompilationConfig, ExecutionEngine, ImportLinker, ImportName, RwasmInstance,
    RwasmModule, RwasmModuleBuilder, RwasmStore, StoreTr, SyscallFuelParams, TrapCode, TypedCaller,
    Value,
};
use std::{collections::VecDeque, sync::Arc};

const ORIGINAL: &str = r#"(module
    (type $t (func (result i32)))
    (memory 1) (table 2 funcref) (global $g (mut i32) (i32.const 7))
    (data (i32.const 0) "A") (data $live "L") (data $gone "D")
    (func $f (result i32) i32.const 11)
    (elem (i32.const 0) $f) (elem $live_elem func $f) (elem $gone_elem func $f)
    (func (export "main") (param $op i32) (result i32)
      (if (i32.eq (local.get $op) (i32.const 0)) (then
        (global.set $g (i32.const 17)) (data.drop $gone) (elem.drop $gone_elem)
        (return (i32.const 0))))
      (if (i32.eq (local.get $op) (i32.const 1)) (then
        (memory.init $live (i32.const 16) (i32.const 0) (i32.const 1))
        (return (i32.load8_u (i32.const 16)))))
      (if (i32.eq (local.get $op) (i32.const 2)) (then
        (memory.init $gone (i32.const 17) (i32.const 0) (i32.const 1))
        (return (i32.const 0))))
      (if (i32.eq (local.get $op) (i32.const 3)) (then
        (table.init $live_elem (i32.const 0) (i32.const 0) (i32.const 1))
        (return (call_indirect (type $t) (i32.const 0)))))
      (if (i32.eq (local.get $op) (i32.const 4)) (then
        (table.init $gone_elem (i32.const 0) (i32.const 0) (i32.const 1))
        (return (i32.const 0))))
      (i32.add (global.get $g)
        (i32.add (i32.load8_u (i32.const 0)) (call_indirect (type $t) (i32.const 0)))))
)"#;

const REPLACEMENT: &str = r#"(module
    (import "env" "stop" (func $stop))
    (memory 1) (table 3 funcref) (global (mut i32) (i32.const 99))
    (data (i32.const 0) "B") (data $live "X") (data $gone "Y")
    (func $f (result i32) i32.const 99)
    (elem (i32.const 0) $f $f) (elem $live_elem func $f) (elem $gone_elem func $f)
    (func $start (data.drop $live) (elem.drop $live_elem) call $stop call $stop)
    (start $start)
    (func (export "main") (result i32) i32.const 0 i32.load8_u)
)"#;

/// Compiles validated Wasm with the same start and entrypoint configuration for each instance.
fn compile(linker: &Arc<ImportLinker>, wat: &str) -> RwasmModule {
    RwasmModule::compile(
        CompilationConfig::default_strategy_compatible()
            .with_allow_start_section(true)
            .with_entrypoint_name("main".into())
            .with_import_linker(linker.clone()),
        &wat::parse_str(wat).unwrap(),
    )
    .unwrap()
    .0
}

/// Returns the next requested host outcome; consumed entries expose callback side effects.
fn stop(
    caller: &mut TypedCaller<'_, VecDeque<Result<(), TrapCode>>>,
    _: u32,
    _: &[Value],
    _: &mut [Value],
) -> Result<(), TrapCode> {
    caller.data_mut().pop_front().expect("expected host call")
}

/// Links the syscall used to control initialization traps, interruptions, and completion.
fn linker() -> Arc<ImportLinker> {
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "stop"),
        1,
        SyscallFuelParams::None,
        &[],
        &[],
    );
    Arc::new(linker)
}

/// Every failure path restores live and dropped segments as well as memory, tables, and globals.
#[test]
fn rollback_restores_all_instance_state_after_traps_and_cancellation() {
    let linker = linker();
    let original = compile(&linker, ORIGINAL);
    let replacement = compile(&linker, REPLACEMENT);
    let engine = ExecutionEngine::new();
    for terminal in [
        TrapCode::UnreachableCodeReached,
        TrapCode::ExecutionHalted,
        TrapCode::OutOfFuel,
    ] {
        for interruptions in [0, 1, 2] {
            // Two calls can either fail on the second one or remain parked for cancellation.
            let outcomes = if interruptions == 2 {
                vec![Err(TrapCode::InterruptionCalled); 2]
            } else {
                let mut outcomes = vec![Err(TrapCode::InterruptionCalled); interruptions];
                outcomes.push(Err(terminal));
                outcomes
            };
            for keep_flags in [false, true] {
                let mut store = RwasmStore::new(
                    linker.clone(),
                    outcomes.clone().into(),
                    stop,
                    Some(10000),
                    Some(1),
                );
                let old = linker
                    .instantiate(&mut store, engine, original.clone())
                    .unwrap();
                old.execute(&mut store, &[Value::I32(0)], &mut [Value::I32(0)])
                    .unwrap();
                let memory = store.memory_snapshot();
                let tables = store.table_snapshots_nullness_prefix(4);
                let fuel_before = store.fuel_consumed();
                let mut outcome = linker
                    .instantiate(&mut store, engine, replacement.clone())
                    .err();
                for step in 0..interruptions {
                    assert_eq!(outcome, Some(TrapCode::InterruptionCalled));
                    assert_eq!(
                        old.execute(&mut store, &[Value::I32(5)], &mut [Value::I32(0)]),
                        Err(TrapCode::IllegalOpcode)
                    );
                    if interruptions == 2 && step == 1 {
                        store.reset(keep_flags);
                        assert_eq!(store.fuel_consumed(), 0);
                    } else {
                        outcome = engine.resume(&mut store, &[], &mut []).err();
                    }
                }
                if interruptions < 2 {
                    assert_eq!(outcome, Some(terminal));
                    assert!(
                        store.fuel_consumed() > fuel_before,
                        "failed initialization must not refund fuel"
                    );
                }
                assert!(
                    store.data().is_empty(),
                    "rollback must not undo host callback side effects"
                );
                assert!(
                    store.memory_snapshot() == memory,
                    "previous memory was not restored"
                );
                assert_eq!(store.table_snapshots_nullness_prefix(4), tables);
                let mut result = [Value::I32(0)];
                old.execute(&mut store, &[Value::I32(5)], &mut result)
                    .unwrap();
                assert_eq!(
                    result,
                    [Value::I32(93)],
                    "restored mutable global + memory byte + indirect call"
                );
                for (op, expected) in [(1, 76), (3, 11)] {
                    old.execute(&mut store, &[Value::I32(op)], &mut result)
                        .unwrap();
                    assert_eq!(
                        result,
                        [Value::I32(expected)],
                        "live segment was not restored"
                    );
                }
                for (op, trap) in [
                    (2, TrapCode::MemoryOutOfBounds),
                    (4, TrapCode::TableOutOfBounds),
                ] {
                    assert_eq!(
                        old.execute(&mut store, &[Value::I32(op)], &mut result),
                        Err(trap),
                        "dropped segment became usable again"
                    );
                }
                assert_eq!(
                    engine.resume(&mut store, &[], &mut []),
                    Err(TrapCode::IllegalOpcode)
                );
            }
        }
    }
}

/// Once resumed initialization succeeds, reset must not resurrect the previous instance.
#[test]
fn successful_resumed_initialization_commits_the_replacement() {
    let linker = linker();
    let engine = ExecutionEngine::new();
    for keep_flags in [false, true] {
        let mut store = RwasmStore::new(
            linker.clone(),
            VecDeque::from([Err(TrapCode::InterruptionCalled), Ok(())]),
            stop,
            Some(10000),
            None,
        );
        let old = linker
            .instantiate(&mut store, engine, compile(&linker, ORIGINAL))
            .unwrap();
        let replacement = compile(&linker, REPLACEMENT);
        assert_eq!(
            linker
                .instantiate(&mut store, engine, replacement.clone())
                .err(),
            Some(TrapCode::InterruptionCalled)
        );
        engine.resume(&mut store, &[], &mut []).unwrap();
        store.reset(keep_flags);
        assert_eq!(
            old.execute(&mut store, &[Value::I32(5)], &mut [Value::I32(0)]),
            Err(TrapCode::IllegalOpcode)
        );
        let mut result = [Value::I32(0)];
        engine
            .execute(&mut store, &replacement, &[], &mut result)
            .unwrap();
        assert_eq!(result, [Value::I32(66)]);
        // A completed transaction must also allow another ordinary instantiation.
        assert!(linker
            .instantiate(&mut store, engine, compile(&linker, ORIGINAL))
            .is_ok());
    }
}

/// A module assembled without an initialization prologue (`source_pc == 0`, what the builder
/// produces) has nothing to run at instantiation: the handle becomes live at once and executes
/// the code section from its start.
#[test]
fn module_without_prologue_instantiates_without_running_anything() {
    let module = RwasmModuleBuilder::new(instruction_set! { I32Const(42) Return }).build();
    assert_eq!(module.source_pc, 0);
    let mut store = RwasmStore::<()>::default();
    let instance = RwasmInstance::new(&mut store, ExecutionEngine::new(), module).unwrap();
    assert_eq!(store.fuel_consumed(), 0);
    let mut result = [Value::I32(0)];
    instance.execute(&mut store, &[], &mut result).unwrap();
    assert_eq!(result, [Value::I32(42)]);
}

#[cfg(feature = "wasmtime")]
mod instance_isolation {
    //! Audit 2026-09-13, CRIT-1: an instance handle must not run against another instance's state.
    //!
    //! `CRIT-1`: a store holds one instance's state, but nothing stops a host from keeping two live
    //! [`RwasmInstance`] handles for two different modules on the same store. Executing the first one
    //! after the second was instantiated must be rejected before accessing the new instance's state.
    //! Wasmtime supports several instances per store; rwasm exposes one current instance and
    //! invalidates its previous handles when replacement succeeds.

    use rwasm::{
        always_failing_syscall_handler, CompilationConfig, ExecutionEngine, ImportLinker,
        RwasmModule, RwasmStore, StoreTr, TrapCode, Value,
    };
    use std::sync::Arc;

    const INSTANCE_A: &str = r#"(module
         (memory (export "memory") 1)
         (data (i32.const 0) "AAAA")
         (func (export "main") (result i64) (i64.load8_u (i32.const 0))))"#;

    const INSTANCE_B: &str = r#"(module
         (memory (export "memory") 1)
         (data (i32.const 0) "BBBB")
         (func (export "main") (result i64) (i64.load8_u (i32.const 0))))"#;

    /// Writes `4242` at offset 4 and reads byte 0, so a caller can tell whose memory the instance used.
    const WRITER_A: &str = r#"(module
         (memory (export "memory") 1)
         (data (i32.const 0) "AAAA")
         (func (export "main") (result i64)
           (i32.store (i32.const 4) (i32.const 4242))
           (i64.load8_u (i32.const 0))))"#;

    fn config(linker: &Arc<ImportLinker>) -> CompilationConfig {
        CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_import_linker(linker.clone())
    }

    fn module(linker: &Arc<ImportLinker>, wat: &str) -> RwasmModule {
        let wasm = wat::parse_str(wat).expect("the test module parses");
        RwasmModule::compile(config(linker), &wasm)
            .expect("rwasm compiles the test module")
            .0
    }

    /// A stale handle must not read the replacement's memory. rwasm rejects it; Wasmtime's separate
    /// per-instance storage lets its old handle keep reading the original memory.
    #[test]
    fn a_live_instance_does_not_run_against_another_instances_state() {
        let linker = Arc::new(ImportLinker::default());
        let engine = ExecutionEngine::new();
        let mut store = RwasmStore::new(
            linker.clone(),
            (),
            always_failing_syscall_handler,
            Some(1_000_000),
            None,
        );

        let first = linker
            .instantiate(&mut store, engine, module(&linker, INSTANCE_A))
            .expect("module A instantiates");
        let mut before = [Value::I64(-1)];
        first
            .execute(&mut store, &[], &mut before)
            .expect("module A runs");
        assert_eq!(
            before,
            [Value::I64(65)],
            "A reads its own data before B exists"
        );

        let second = linker
            .instantiate(&mut store, engine, module(&linker, INSTANCE_B))
            .expect("module B instantiates");
        let mut b = [Value::I64(-1)];
        second
            .execute(&mut store, &[], &mut b)
            .expect("module B runs");
        assert_eq!(b, [Value::I64(66)], "B reads its own data");

        let mut after = [Value::I64(-1)];
        let outcome = first.execute(&mut store, &[], &mut after);

        // The Wasmtime oracle: two instances in one store keep their own memories.
        let engine = wasmtime::Engine::default();
        let mut wasmtime_store = wasmtime::Store::new(&engine, ());
        let linker = wasmtime::Linker::new(&engine);
        let wasm_a = wasmtime::Module::new(&engine, wat::parse_str(INSTANCE_A).unwrap()).unwrap();
        let wasm_b = wasmtime::Module::new(&engine, wat::parse_str(INSTANCE_B).unwrap()).unwrap();
        let instance_a = linker.instantiate(&mut wasmtime_store, &wasm_a).unwrap();
        let a_func = instance_a
            .get_typed_func::<(), i64>(&mut wasmtime_store, "main")
            .unwrap();
        let instance_b = linker.instantiate(&mut wasmtime_store, &wasm_b).unwrap();
        let b_func = instance_b
            .get_typed_func::<(), i64>(&mut wasmtime_store, "main")
            .unwrap();
        let wasmtime_after = a_func.call(&mut wasmtime_store, ()).unwrap();
        let wasmtime_b = b_func.call(&mut wasmtime_store, ()).unwrap();
        assert_eq!(wasmtime_b, 66);
        assert_eq!(wasmtime_after, 65, "the oracle isolates the two instances");

        assert_eq!(
            outcome,
            Err(TrapCode::IllegalOpcode),
            "the replaced rwasm handle must be rejected before reading the current instance's memory"
        );
    }

    /// The handle check alone leaves the low-level engine API open: `ExecutionEngine::execute` and
    /// `entrypoint` take any module and any store. With an instance active, the store accepts only
    /// that instance's module (by allocation or, for a module decoded again, by content), so A's
    /// memory cannot be run under B's code even without going through a handle.
    #[test]
    fn direct_engine_execution_is_bound_to_the_active_instances_module() {
        let linker = Arc::new(ImportLinker::default());
        let engine = ExecutionEngine::new();
        let mut store = RwasmStore::new(
            linker.clone(),
            (),
            always_failing_syscall_handler,
            Some(1_000_000),
            None,
        );
        let module_a = module(&linker, INSTANCE_A);
        let module_b = module(&linker, INSTANCE_B);
        let _instance = linker
            .instantiate(&mut store, engine, module_a.clone())
            .expect("module A instantiates");

        let mut result = [Value::I64(-1)];
        assert_eq!(
            engine.execute(&mut store, &module_b, &[], &mut result),
            Err(TrapCode::IllegalOpcode),
            "B must not run on A's state"
        );
        assert_eq!(
            engine.entrypoint(&mut store, &module_b),
            Err(TrapCode::IllegalOpcode),
            "B's prologue must not run on A's state"
        );
        assert_eq!(
            store
                .memory_read_into_vec(0, 4)
                .expect("A's memory is intact"),
            b"AAAA"
        );

        // A itself runs, also through a fresh decoding of the same bytecode.
        engine
            .execute(&mut store, &module_a, &[], &mut result)
            .expect("A runs on its own store");
        assert_eq!(result, [Value::I64(65)]);
        let decoded_again = RwasmModule::new(&module_a.serialize()).0;
        engine
            .execute(&mut store, &decoded_again, &[], &mut result)
            .expect("the same module decoded again is still A");
        assert_eq!(result, [Value::I64(65)]);

        // A store that was never instantiated keeps running any module.
        let mut legacy_store = RwasmStore::new(
            linker.clone(),
            (),
            always_failing_syscall_handler,
            Some(1_000_000),
            None,
        );
        engine
            .entrypoint(&mut legacy_store, &module_b)
            .expect("legacy stores are not bound");
        engine
            .execute(&mut legacy_store, &module_b, &[], &mut result)
            .expect("legacy stores are not bound");
        assert_eq!(result, [Value::I64(66)]);
    }

    /// The same aliasing on the write side: A's store at offset 4 must land in A's memory, not in the
    /// memory of the instance that now owns the store.
    #[test]
    fn a_live_instance_does_not_write_into_another_instances_memory() {
        let linker = Arc::new(ImportLinker::default());
        let engine = ExecutionEngine::new();
        let mut store = RwasmStore::new(
            linker.clone(),
            (),
            always_failing_syscall_handler,
            Some(1_000_000),
            None,
        );

        let writer = linker
            .instantiate(&mut store, engine, module(&linker, WRITER_A))
            .expect("module A instantiates");
        let owner = linker
            .instantiate(&mut store, engine, module(&linker, INSTANCE_B))
            .expect("module B instantiates");

        let mut result = [Value::I64(-1)];
        assert_eq!(
            writer.execute(&mut store, &[], &mut result),
            Err(TrapCode::IllegalOpcode)
        );

        // Whatever `store` now holds belongs to module B; A's write must not appear in it.
        let mut buffer = [0u8; 4];
        store
            .memory_read(4, &mut buffer)
            .expect("module B has a page");
        assert_eq!(
            buffer, [0; 4],
            "module A's store leaked into the memory of the instance that owns the store (the value \
             read back is A's 4242 little-endian)"
        );

        // The Wasmtime oracle: B's memory is untouched by A's call.
        let engine = wasmtime::Engine::default();
        let mut wasmtime_store = wasmtime::Store::new(&engine, ());
        let wt_linker = wasmtime::Linker::new(&engine);
        let wasm_a = wasmtime::Module::new(&engine, wat::parse_str(WRITER_A).unwrap()).unwrap();
        let wasm_b = wasmtime::Module::new(&engine, wat::parse_str(INSTANCE_B).unwrap()).unwrap();
        let instance_a = wt_linker.instantiate(&mut wasmtime_store, &wasm_a).unwrap();
        let instance_b = wt_linker.instantiate(&mut wasmtime_store, &wasm_b).unwrap();
        let a_func = instance_a
            .get_typed_func::<(), i64>(&mut wasmtime_store, "main")
            .unwrap();
        a_func.call(&mut wasmtime_store, ()).unwrap();
        let memory_b = instance_b
            .get_export(&mut wasmtime_store, "memory")
            .and_then(|export| export.into_memory())
            .expect("B exports its memory");
        assert_eq!(
            &memory_b.data(&wasmtime_store)[4..8],
            &[0, 0, 0, 0],
            "the oracle keeps A's write out of B's memory"
        );
        let _ = owner;
    }

    /// The `call_indirect` face of the same aliasing: the first instance's table *is* the second
    /// instance's table, so the dispatch target is a code offset from the other module's code layout
    /// applied to this module's code section. Module A here is 18 instructions long and never
    /// initializes its table; module B has 200 functions and an active element segment, so B's
    /// instantiation installs an offset ~1000 instructions into B. rwasm therefore fetches far past
    /// the end of A's code section (an instruction-window check measured `instruction offset=1011`
    /// against an 18-instruction window); Wasmtime traps `IndirectCallToNull` because A's own table
    /// entry is null.
    #[test]
    fn a_live_instance_does_not_dispatch_through_another_instances_table() {
        const DISPATCH_A: &str = r#"(module
             (type $t (func (result i32)))
             (table 1 funcref)
             (func (export "main") (result i64)
               (i64.extend_i32_u (call_indirect (type $t) (i32.const 0)))))"#;

        let mut dispatch_b = String::from(
            "(module (type $t (func (result i32))) (table 1 funcref) (elem (i32.const 0) $f199)\n",
        );
        for i in 0..200 {
            dispatch_b.push_str(&format!("(func $f{i} (result i32) (i32.const {i}))\n"));
        }
        dispatch_b.push_str("(func (export \"main\") (result i64) (i64.const 0)))");

        let linker = Arc::new(ImportLinker::default());
        let engine = ExecutionEngine::new();
        let mut store = RwasmStore::new(
            linker.clone(),
            (),
            always_failing_syscall_handler,
            Some(1_000_000),
            None,
        );
        let a_module = module(&linker, DISPATCH_A);
        let b_module = module(&linker, &dispatch_b);
        assert!(
            b_module.code_section.len() > a_module.code_section.len() * 16,
            "the fixture needs B's code section to be much larger than A's ({} vs {})",
            a_module.code_section.len(),
            b_module.code_section.len()
        );

        let first = linker
            .instantiate(&mut store, engine, a_module)
            .expect("module A instantiates");
        let _second = linker
            .instantiate(&mut store, engine, b_module)
            .expect("module B instantiates");

        let mut result = [Value::I64(-1)];
        let outcome = first.execute(&mut store, &[], &mut result);

        // Oracle: Wasmtime keeps A's table, whose entry 0 is null, so the call traps.
        let engine = wasmtime::Engine::default();
        let mut wasmtime_store = wasmtime::Store::new(&engine, ());
        let wasmtime_linker = wasmtime::Linker::new(&engine);
        let wasm_a = wasmtime::Module::new(&engine, wat::parse_str(DISPATCH_A).unwrap()).unwrap();
        let wasm_b = wasmtime::Module::new(&engine, wat::parse_str(&dispatch_b).unwrap()).unwrap();
        let instance_a = wasmtime_linker
            .instantiate(&mut wasmtime_store, &wasm_a)
            .unwrap();
        let _instance_b = wasmtime_linker
            .instantiate(&mut wasmtime_store, &wasm_b)
            .unwrap();
        let func_a = instance_a
            .get_typed_func::<(), i64>(&mut wasmtime_store, "main")
            .unwrap();
        assert!(
            func_a.call(&mut wasmtime_store, ()).is_err(),
            "the oracle traps because A's own table is null"
        );

        assert_eq!(
            outcome,
            Err(TrapCode::IllegalOpcode),
            "the replaced handle must trap before any dispatch through the replacement's table"
        );
    }
}

#[cfg(feature = "wasmtime")]
mod instance_state {
    //! Audit 2026-09-13, round 3 (R3-1, R3-2): a store's tables, memory and segment flags belong to
    //! one instance. They must not survive into the next instance on the same store, must persist
    //! across calls of one instance, and `RwasmStore::reset` must discard a parked interruption.

    use rwasm::{
        wasmtime::{WasmtimeExecutor, WasmtimeModule},
        CompilationConfig, ExecutionEngine, ImportLinker, ImportName, RwasmModule, RwasmStore,
        StoreTr, SyscallFuelParams, TrapCode, TypedCaller, Value,
    };
    use std::sync::{Arc, Mutex};

    type Ctx = Arc<Mutex<Vec<u32>>>;

    fn test_config(linker: &Arc<ImportLinker>) -> CompilationConfig {
        CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_import_linker(linker.clone())
    }

    fn noop(
        _: &mut TypedCaller<'_, Ctx>,
        _: u32,
        _: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
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

    fn wasmtime_executor(linker: &Arc<ImportLinker>, wat: &str) -> WasmtimeExecutor<Ctx> {
        let wasm = wat::parse_str(wat).expect("the test module parses");
        let module = rwasm::wasmtime::compile_wasmtime_module(test_config(linker), &wasm)
            .expect("wasmtime compiles the test module");
        WasmtimeExecutor::new(
            module,
            linker.clone(),
            Ctx::default(),
            noop,
            Some(1_000_000),
            None,
        )
        .expect("wasmtime instantiates the test module")
    }

    fn wasmtime_instantiate(executor: &mut WasmtimeExecutor<Ctx>, wat: &str) {
        let wasm = wat::parse_str(wat).expect("the test module parses");
        let module = wasmtime::Module::new(executor.store.engine(), &wasm)
            .expect("wasmtime builds the second module");
        executor
            .instantiate(&WasmtimeModule::from(module))
            .expect("wasmtime instantiates the second module");
    }

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
        let mut store =
            RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
        let mut first = [Value::I64(-1)];
        rwasm_execute(
            &linker,
            &mut store,
            rwasm_module(&linker, TABLE_A),
            &mut first,
        )
        .expect("module A runs");
        assert_eq!(first, [Value::I64(111)]);

        let mut second = [Value::I64(-1)];
        let rwasm = rwasm_execute(
            &linker,
            &mut store,
            rwasm_module(&linker, TABLE_B),
            &mut second,
        )
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
        let mut store =
            RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
        let mut first = [Value::I64(-1)];
        rwasm_execute(
            &linker,
            &mut store,
            rwasm_module(&linker, MEMORY_A),
            &mut first,
        )
        .expect("module A runs");
        assert_eq!(first, [Value::I64(3)], "A grows to three pages");
        let mut second = [Value::I64(-1)];
        let rwasm = rwasm_execute(
            &linker,
            &mut store,
            rwasm_module(&linker, MEMORY_B),
            &mut second,
        )
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
        let mut store =
            RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
        let mut first = [Value::I64(-1)];
        rwasm_execute(
            &linker,
            &mut store,
            rwasm_module(&linker, SEGMENTS_A),
            &mut first,
        )
        .expect("module A runs");
        assert_eq!(first, [Value::I64(170)]);
        let mut second = [Value::I64(-1)];
        let rwasm = rwasm_execute(
            &linker,
            &mut store,
            rwasm_module(&linker, SEGMENTS_B),
            &mut second,
        )
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

    /// Guard for the table half of `R3-1` with offsets large enough to leave the second module's code
    /// section: module A has 200 functions and installs the offset of its last one, module B is a few
    /// instructions long and never initializes its table. Before the fix, B's `call_indirect` fetched
    /// from `B.code + ~1000` (an instrumented `InstructionPtr` window check reported
    /// `instruction offset=1030` against a 23-instruction window); now B's table is its own and both
    /// backends trap `IndirectCallToNull`.
    #[test]
    fn table_entries_do_not_leak_between_instances_with_large_offsets() {
        let mut large_a = String::from(
            r#"(module
                 (type $t (func (result i32)))
                 (table 1 funcref)
                 (elem func $f199)
                 (func (export "main") (result i64)
                   (table.init 0 (i32.const 0) (i32.const 0) (i32.const 1))
                   (i64.const 0))
    "#,
        );
        for i in 0..200 {
            large_a.push_str(&format!("(func $f{i} (result i32) (i32.const {i}))\n"));
        }
        large_a.push(')');

        let linker = Arc::new(ImportLinker::default());
        let mut store =
            RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
        let mut result = [Value::I64(-1)];
        rwasm_execute(
            &linker,
            &mut store,
            rwasm_module(&linker, &large_a),
            &mut result,
        )
        .expect("the large module runs");

        let mut second = [Value::I64(-1)];
        let rwasm = rwasm_execute(
            &linker,
            &mut store,
            rwasm_module(&linker, TABLE_B),
            &mut second,
        )
        .map(|()| second[0].clone());

        let mut wasmtime = wasmtime_executor(&linker, &large_a);
        let mut result = [Value::I64(-1)];
        wasmtime.execute("main", &[], &mut result).unwrap();
        wasmtime_instantiate(&mut wasmtime, TABLE_B);
        let mut second = [Value::I64(-1)];
        let wasmtime = wasmtime
            .execute("main", &[], &mut second)
            .map(|()| second[0].clone());

        assert_eq!(
            rwasm, wasmtime,
            "a dispatch target left by a much larger module must not be applied to this module's code: \
             rwasm={rwasm:?}, wasmtime={wasmtime:?}"
        );
    }

    /// The element-segment mirror of the data-segment leak: `elem.drop 0` in module A used to make
    /// module B's `table.init 0` trap `TableOutOfBounds`, because the drop bitset lives in the store
    /// and was never cleared for the new instance.
    #[test]
    fn dropped_element_segment_flags_do_not_leak_between_instances() {
        let linker = Arc::new(ImportLinker::default());
        let mut store =
            RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
        let mut first = [Value::I64(-1)];
        rwasm_execute(
            &linker,
            &mut store,
            rwasm_module(&linker, ELEMENTS_A),
            &mut first,
        )
        .expect("module A runs");
        assert_eq!(first, [Value::I64(111)]);
        let mut second = [Value::I64(-1)];
        let rwasm = rwasm_execute(
            &linker,
            &mut store,
            rwasm_module(&linker, ELEMENTS_B),
            &mut second,
        )
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
        let mut store =
            RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
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
        let mut store =
            RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
        let module = rwasm_module(&linker, PAGES);
        let mut rwasm_first = [Value::I64(-1)];
        rwasm_execute(&linker, &mut store, module.clone(), &mut rwasm_first)
            .expect("first round runs");
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
        let mut store =
            RwasmStore::new(linker.clone(), Ctx::default(), noop, Some(1_000_000), None);
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
        instance
            .execute(&mut store, &[], &mut result)
            .expect("the second run completes");
        assert_eq!(
            result,
            [Value::I64(1)],
            "the second run increments the counter once"
        );
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
}
