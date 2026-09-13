#![cfg(feature = "wasmtime")]

use rwasm::{
    CompilationConfig, ExecutionEngine, ImportLinker, ImportName, RwasmModule, RwasmStore, StoreTr,
    StrategyDefinition, SyscallFuelParams, TrapCode, TypedCaller, ValType, Value,
};
use std::sync::Arc;

/// Compiles a named entrypoint with start sections enabled for lifecycle regression tests.
fn compile_instance(linker: &Arc<ImportLinker>, wat: &str) -> RwasmModule {
    RwasmModule::compile(
        CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_allow_start_section(true)
            .with_import_linker(linker.clone()),
        &wat::parse_str(wat).unwrap(),
    )
    .unwrap()
    .0
}

/// Rejected memory growth, active data, and trapping starts preserve the previous instance.
#[test]
fn failed_replacement_preserves_the_previous_instance() {
    let linker = Arc::new(ImportLinker::default());
    let original = compile_instance(
        &linker,
        r#"(module (memory 1) (data (i32.const 0) "A")
        (global $g (mut i32) (i32.const 7))
        (func (export "main") (result i32) global.get $g))"#,
    );
    for (replacement, expected) in [
        (
            r#"(module (memory 1) (data (i32.const 65536) "B")
            (global (mut i32) (i32.const 99)) (func (export "main")))"#,
            TrapCode::MemoryOutOfBounds,
        ),
        (
            r#"(module (memory 2) (func (export "main")))"#,
            TrapCode::MemoryOutOfBounds,
        ),
        (
            r#"(module (memory 1) (data (i32.const 0) "B")
            (func $start unreachable) (start $start) (func (export "main")))"#,
            TrapCode::UnreachableCodeReached,
        ),
    ] {
        for keep_flags in [false, true] {
            let mut store = RwasmStore::new(
                linker.clone(),
                (),
                rwasm::always_failing_syscall_handler,
                Some(1000),
                Some(1),
            );
            let engine = ExecutionEngine::new();
            let old = linker
                .instantiate(&mut store, engine, original.clone())
                .unwrap();
            let mut result = [Value::I32(0)];
            old.execute(&mut store, &[], &mut result).unwrap();
            assert_eq!(result, [Value::I32(7)]);
            let memory = store.memory_snapshot();
            assert_eq!(
                linker
                    .instantiate(&mut store, engine, compile_instance(&linker, replacement))
                    .err(),
                Some(expected),
            );
            old.execute(&mut store, &[], &mut result).unwrap();
            assert_eq!(result, [Value::I32(7)]);
            assert_eq!(store.memory_snapshot(), memory);
            store.reset(keep_flags);
            old.execute(&mut store, &[], &mut result).unwrap();
            assert_eq!(result, [Value::I32(7)]);
            assert_eq!(store.memory_snapshot(), memory);
            let fresh = linker
                .instantiate(&mut store, engine, original.clone())
                .unwrap();
            fresh.execute(&mut store, &[], &mut result).unwrap();
            assert_eq!(result, [Value::I32(7)]);
            assert_eq!(
                old.execute(&mut store, &[], &mut result),
                Err(TrapCode::IllegalOpcode)
            );
        }
    }
}

/// Only the current instance in the correct store may execute or resume its parked call.
#[test]
fn stale_or_foreign_handles_cannot_consume_a_parked_execution() {
    /// Suspends the call so attempts to resume it through another handle can be checked.
    fn interrupt(
        _: &mut TypedCaller<'_, ()>,
        _: u32,
        _: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
        Err(TrapCode::InterruptionCalled)
    }
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "pause"),
        1,
        SyscallFuelParams::None,
        &[],
        &[],
    );
    let linker = Arc::new(linker);
    let module = compile_instance(
        &linker,
        r#"(module
        (import "env" "pause" (func $pause))
        (func (export "main") (result i32) call $pause i32.const 42))"#,
    );
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::new(linker.clone(), (), interrupt, Some(1000), None);
    let mut foreign_store = RwasmStore::new(linker.clone(), (), interrupt, Some(1000), None);
    let stale = linker
        .instantiate(&mut store, engine, module.clone())
        .unwrap();
    let foreign = linker
        .instantiate(&mut foreign_store, engine, module.clone())
        .unwrap();
    let current = linker.instantiate(&mut store, engine, module).unwrap();
    let mut result = [Value::I32(-1)];
    assert_eq!(
        foreign.execute(&mut store, &[], &mut result),
        Err(TrapCode::IllegalOpcode)
    );
    assert_eq!(
        stale.execute(&mut store, &[], &mut result),
        Err(TrapCode::IllegalOpcode)
    );
    assert_eq!(
        current.execute(&mut store, &[], &mut result),
        Err(TrapCode::InterruptionCalled)
    );
    let fuel = store.remaining_fuel();
    for wrong in [&stale, &foreign] {
        assert_eq!(
            wrong.resume(&mut store, &[], &mut result),
            Err(TrapCode::IllegalOpcode)
        );
        assert_eq!(result, [Value::I32(-1)]);
        assert_eq!(store.remaining_fuel(), fuel);
    }
    current.resume(&mut store, &[], &mut result).unwrap();
    assert_eq!(result, [Value::I32(42)]);
}

/// Either reset mode cancels a parked initializer and restores the original memory and handle.
#[test]
fn cancelling_interrupted_initialization_restores_the_previous_instance() {
    /// Suspends initialization after its active data has replaced the old memory contents.
    fn interrupt(
        _: &mut TypedCaller<'_, ()>,
        _: u32,
        _: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
        Err(TrapCode::InterruptionCalled)
    }
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "pause"),
        1,
        SyscallFuelParams::None,
        &[],
        &[],
    );
    let linker = Arc::new(linker);
    let old_module = compile_instance(
        &linker,
        r#"(module (memory 1) (data (i32.const 0) "A")
        (func (export "main") (result i32) i32.const 0 i32.load8_u))"#,
    );
    let replacement = compile_instance(
        &linker,
        r#"(module
        (import "env" "pause" (func $pause)) (memory 1) (data (i32.const 0) "B")
        (start $pause) (func (export "main")))"#,
    );
    let engine = ExecutionEngine::new();
    for keep_flags in [false, true] {
        let mut store = RwasmStore::new(linker.clone(), (), interrupt, Some(1000), None);
        let old = linker
            .instantiate(&mut store, engine, old_module.clone())
            .unwrap();
        assert_eq!(
            linker
                .instantiate(&mut store, engine, replacement.clone())
                .err(),
            Some(TrapCode::InterruptionCalled)
        );
        store.reset(keep_flags);
        let mut result = [Value::I32(0)];
        old.execute(&mut store, &[], &mut result).unwrap();
        assert_eq!(result, [Value::I32(65)]);
        assert_eq!(
            old.resume(&mut store, &[], &mut []),
            Err(TrapCode::IllegalOpcode)
        );
    }
}

/// Rejects short and long result buffers without partially writing them or poisoning later calls.
#[test]
fn output_slot_mismatches_trap_and_allow_a_subsequent_correct_call() {
    let linker = Arc::new(ImportLinker::default());
    let module = compile_instance(
        &linker,
        r#"(module
        (func (export "main") (result i32 i64) i32.const 7 i64.const 4294967297))"#,
    );
    let mut store = RwasmStore::<()>::default();
    let instance = linker
        .instantiate(&mut store, ExecutionEngine::new(), module)
        .unwrap();
    for mut output in [
        vec![],
        vec![Value::I32(-1)],
        vec![Value::I64(-1), Value::I64(-1)],
    ] {
        let before = output.clone();
        assert_eq!(
            instance.execute(&mut store, &[], &mut output),
            Err(TrapCode::IllegalOpcode)
        );
        assert_eq!(
            output, before,
            "rejected output buffers must not be partially written"
        );
        let mut correct = [Value::I32(0), Value::I64(0)];
        instance.execute(&mut store, &[], &mut correct).unwrap();
        assert_eq!(correct, [Value::I32(7), Value::I64(4294967297)]);
    }
}

/// Negative i32 element offsets are valid to compile but out of bounds during initialization.
#[test]
fn negative_active_element_offset_traps_during_instantiation() {
    let linker = Arc::new(ImportLinker::default());
    for offset in [-1, i32::MIN] {
        let module = compile_instance(
            &linker,
            &format!(
                r#"(module
            (table 1 funcref) (elem (i32.const {offset}) $f)
            (func $f) (func (export "main")))"#
            ),
        );
        let mut store = RwasmStore::<()>::default();
        assert_eq!(
            linker
                .instantiate(&mut store, ExecutionEngine::new(), module)
                .err(),
            Some(TrapCode::TableOutOfBounds)
        );
    }
}

/// Dynamic syscall fuel temporaries reserve enough stack at initial and grown capacity limits.
#[test]
fn dynamic_syscall_fuel_trampolines_grow_the_stack_at_capacity_boundaries() {
    use rwasm_fuel_policy::{LinearFuelParams, QuadraticFuelParams};
    /// Accepts the host call so only compiler-injected fuel and stack behavior affect the result.
    fn accept(
        _: &mut TypedCaller<'_, ()>,
        _: u32,
        _: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
        Ok(())
    }
    for policy in [
        SyscallFuelParams::LinearFuel(LinearFuelParams {
            param_index: 1,
            word_cost: 3,
            base_fuel: 7,
        }),
        SyscallFuelParams::QuadraticFuel(QuadraticFuelParams {
            local_depth: 1,
            word_cost: 3,
            divisor: 512,
            fuel_denom_rate: 1,
        }),
    ] {
        let mut linker = ImportLinker::default();
        linker.insert_function(
            ImportName::new("env", "builtin"),
            1,
            policy,
            &[ValType::I32],
            &[],
        );
        let linker = Arc::new(linker);
        let config = CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_builtins_consume_fuel(true)
            .with_import_linker(linker.clone());
        // Exercise both the initial 32-slot allocation and a subsequent allocation boundary.
        for local_count in [27, 28, 29, 30, 31, 32, 63, 64] {
            let wasm = wat::parse_str(format!(
                r#"(module
                (import "env" "builtin" (func $builtin (param i32)))
                (memory (export "memory") 1)
                (func (export "main") (param i32) (result i32) (local {})
                    local.get 0 call $builtin local.get 0))"#,
                vec!["i32"; local_count].join(" ")
            ))
            .unwrap();
            let mut remaining_fuel = Vec::new();
            for definition in [
                StrategyDefinition::new_as_rwasm(config.clone(), &wasm).unwrap(),
                StrategyDefinition::new_as_wasmtime(config.clone(), &wasm, None).unwrap(),
            ] {
                let mut executor = definition
                    .create_executor(linker.clone(), (), accept, Some(1000), None)
                    .unwrap();
                let mut result = [Value::I32(0)];
                assert_eq!(
                    executor.execute("main", &[Value::I32(64)], &mut result),
                    Ok(()),
                    "{local_count} locals"
                );
                assert_eq!(result, [Value::I32(64)]);
                remaining_fuel.push(executor.remaining_fuel());
            }
            assert_eq!(remaining_fuel[0], remaining_fuel[1]);
        }
    }
}

/// Rejected replacement preserves a parked export call; reset explicitly cancels that call.
#[test]
fn pending_execution_survives_rejected_replacement_and_reset_cancels_it() {
    /// Writes observable memory before suspending the export call.
    fn interrupt(
        caller: &mut TypedCaller<'_, ()>,
        _: u32,
        _: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
        caller.memory_write(0, &[42])?;
        Err(TrapCode::InterruptionCalled)
    }
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "pause"),
        1,
        SyscallFuelParams::None,
        &[],
        &[],
    );
    let linker = Arc::new(linker);
    let wasm = wat::parse_str(
        r#"(module
        (import "env" "pause" (func $pause))
        (memory 1)
        (func (export "main") (result i32) call $pause i32.const 0 i32.load8_u))"#,
    )
    .unwrap();
    let config = CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_import_linker(linker.clone());
    let module = RwasmModule::compile(config, &wasm).unwrap().0;
    let engine = ExecutionEngine::new();
    for keep_flags in [false, true] {
        let mut store = RwasmStore::new(linker.clone(), (), interrupt, Some(1000), None);
        let instance = linker
            .instantiate(&mut store, engine, module.clone())
            .unwrap();
        let mut result = [Value::I32(0)];
        assert_eq!(
            instance.execute(&mut store, &[], &mut result),
            Err(TrapCode::InterruptionCalled)
        );
        assert_eq!(
            instance.execute(&mut store, &[], &mut result),
            Err(TrapCode::IllegalOpcode)
        );
        assert_eq!(
            engine.entrypoint(&mut store, &module),
            Err(TrapCode::IllegalOpcode)
        );
        assert_eq!(
            linker.instantiate(&mut store, engine, module.clone()).err(),
            Some(TrapCode::IllegalOpcode)
        );
        instance.resume(&mut store, &[], &mut result).unwrap();
        assert_eq!(
            result,
            [Value::I32(42)],
            "failed replacement must preserve the parked memory"
        );
        assert_eq!(
            instance.execute(&mut store, &[], &mut result),
            Err(TrapCode::InterruptionCalled)
        );
        store.reset(keep_flags);
        assert_eq!(
            instance.resume(&mut store, &[], &mut result),
            Err(TrapCode::IllegalOpcode)
        );
        assert!(linker
            .instantiate(&mut store, engine, module.clone())
            .is_ok());
    }
}

/// Unwritten syscall outputs retain typed zero values on both execution strategies.
#[test]
fn untouched_mixed_numeric_syscall_results_are_typed_zeroes() {
    /// Leaves the runtime-provided result slots untouched to exercise their initial values.
    fn leave_results(
        _: &mut TypedCaller<'_, ()>,
        _: u32,
        _: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
        Ok(())
    }
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "zeroes"),
        1,
        SyscallFuelParams::None,
        &[],
        &[ValType::I32, ValType::I64, ValType::F32, ValType::F64],
    );
    let linker = Arc::new(linker);
    let config = CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_import_linker(linker.clone());
    let wasm = wat::parse_str(
        r#"(module
        (import "env" "zeroes" (func $zeroes (result i32 i64 f32 f64)))
        (func (export "main") (result i32 i64 f32 f64) call $zeroes))"#,
    )
    .unwrap();
    for definition in [
        StrategyDefinition::new_as_rwasm(config.clone(), &wasm).unwrap(),
        StrategyDefinition::new_as_wasmtime(config, &wasm, None).unwrap(),
    ] {
        let mut executor = definition
            .create_executor(linker.clone(), (), leave_results, Some(1000), None)
            .unwrap();
        let expected = [
            Value::default(ValType::I32),
            Value::default(ValType::I64),
            Value::default(ValType::F32),
            Value::default(ValType::F64),
        ];
        let mut result = expected.clone();
        executor.execute("main", &[], &mut result).unwrap();
        assert_eq!(result, expected);
    }
}
