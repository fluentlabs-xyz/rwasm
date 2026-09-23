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

/// A wrong-sized result buffer is reported the way the oracle reports it, never ignored: for
/// `(func (export "main") (result i32))` called with an empty buffer, Wasmtime reports
/// `IllegalOpcode`; rwasm used to return `Ok(())` and drop the value in a release build (and trip
/// the value-stack assertion in a debug build). Audit 2026-09-13, HIGH-1.
#[test]
fn a_result_buffer_of_the_wrong_length_is_reported_not_ignored() {
    use rwasm::always_failing_syscall_handler;
    const RESULT_I32: &str = r#"(module
         (memory (export "memory") 1)
         (func (export "main") (result i32) (i32.const 7)))"#;
    let linker = Arc::new(ImportLinker::default());
    let config = CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(linker.clone());
    let wasm = wat::parse_str(RESULT_I32).unwrap();
    let module = RwasmModule::compile(config.clone(), &wasm)
        .expect("rwasm compiles the module")
        .0;
    let mut store = RwasmStore::new(
        linker.clone(),
        (),
        always_failing_syscall_handler,
        Some(1_000_000),
        None,
    );
    let instance = linker
        .instantiate(&mut store, ExecutionEngine::new(), module)
        .expect("the module instantiates");
    let mut empty: [Value; 0] = [];
    let rwasm = instance.execute(&mut store, &[], &mut empty);

    let mut wasmtime = StrategyDefinition::new_as_wasmtime(config, &wasm, None)
        .expect("wasmtime compiles the module")
        .create_executor(
            linker,
            (),
            always_failing_syscall_handler,
            Some(1_000_000),
            None,
        )
        .expect("wasmtime instantiates the module");
    let mut empty: [Value; 0] = [];
    let wasmtime = wasmtime.execute("main", &[], &mut empty);

    assert_eq!(
        wasmtime,
        Err(TrapCode::IllegalOpcode),
        "the oracle validates the result buffer"
    );
    assert_eq!(
        rwasm, wasmtime,
        "a wrong-sized result buffer must be reported like the oracle instead of being ignored"
    );
}

mod host_boundary {
    //! Host boundary contracts from the 2026-09-13 audit (round 2): both backends preserve host
    //! argument widths and result slots, return zeroed results after a halt, and reject a halting
    //! start function.

    use rwasm::{
        CompilationConfig, ImportLinker, ImportName, StrategyDefinition, StrategyExecutor,
        SyscallFuelParams, SyscallHandler, TrapCode, TypedCaller, Value,
    };
    use std::sync::Arc;
    use wasmparser::ValType;

    fn wat_type(ty: &ValType) -> &'static str {
        match ty {
            ValType::I32 => "i32",
            ValType::I64 => "i64",
            ValType::F32 => "f32",
            ValType::F64 => "f64",
            _ => panic!("non-numeric test type"),
        }
    }

    fn config(linker: &Arc<ImportLinker>) -> CompilationConfig {
        CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_import_linker(linker.clone())
    }

    /// Compiles `wasm` for both strategies and instantiates each with `linker`/`handler`.
    fn both_executors(
        wasm: &[u8],
        linker: Arc<ImportLinker>,
        handler: SyscallHandler<()>,
    ) -> (StrategyExecutor<()>, StrategyExecutor<()>) {
        let instantiate = |definition: StrategyDefinition| {
            definition
                .create_executor(linker.clone(), (), handler, Some(1_000_000), None)
                .expect("the module must instantiate")
        };
        let rwasm = instantiate(
            StrategyDefinition::new_as_rwasm(config(&linker), wasm)
                .expect("rwasm compiles the module"),
        );
        let wasmtime = instantiate(
            StrategyDefinition::new_as_wasmtime(config(&linker), wasm, None)
                .expect("wasmtime compiles the module"),
        );
        (rwasm, wasmtime)
    }

    /// `R2-1`, signature `(i64, i32) -> i64`.
    ///
    /// Wasm pushes arguments left to right, so the *last* argument is on top of the operand stack and
    /// each parameter has to be popped with its own width. `RwasmExecutor::invoke_syscall` pops the
    /// last argument first (correct) but applies `params[i]`'s width to it (wrong), so a signature
    /// whose width pattern is not palindromic delivers permuted, corrupted values: here the handler
    /// receives `[I32, I64]` instead of `[I64, I32]`, and typed accessors such as
    /// `params[0].i64().unwrap()` panic inside the host.
    #[test]
    fn host_syscall_parameters_keep_their_declared_widths() {
        let wasm = wat::parse_str(
            r#"(module
                 (import "env" "s" (func $s (param i64 i32) (result i64)))
                 (func (export "main") (result i64)
                   i64.const 0x1_0000_0002
                   i32.const 7
                   call $s))"#,
        )
        .unwrap();
        let mut linker = ImportLinker::default();
        linker.insert_function(
            ImportName::new("env", "s"),
            1,
            SyscallFuelParams::default(),
            &[ValType::I64, ValType::I32],
            &[ValType::I64],
        );
        let linker = Arc::new(linker);

        fn handler(
            _caller: &mut TypedCaller<'_, ()>,
            _sys_func_idx: u32,
            params: &[Value],
            result: &mut [Value],
        ) -> Result<(), TrapCode> {
            match (params[0].i64(), params[1].i32()) {
                (Some(wide), Some(narrow)) => {
                    result[0] = Value::I64(wide ^ ((narrow as i64) << 32))
                }
                // The parameters arrived with the wrong types: report a marker the assertion can name.
                _ => result[0] = Value::I64(-1),
            }
            Ok(())
        }

        let expected = 0x1_0000_0002_i64 ^ (7_i64 << 32);
        let (mut rwasm, mut wasmtime) = both_executors(&wasm, linker, handler);

        let mut rwasm_result = [Value::I64(0)];
        let mut wasmtime_result = [Value::I64(0)];
        wasmtime
            .execute("main", &[], &mut wasmtime_result)
            .expect("wasmtime executes");
        rwasm
            .execute("main", &[], &mut rwasm_result)
            .expect("rwasm executes");

        assert_eq!(
            wasmtime_result[0].i64(),
            Some(expected),
            "wasmtime delivers the values in the declared order"
        );
        assert_eq!(
            rwasm_result[0].i64(),
            Some(expected),
            "R2-1: rwasm delivered {:?} to the handler instead of [{expected:#x}]",
            rwasm_result[0]
        );
    }

    /// `R2-1`, signature `(i32, i64) -> i64`: the same defect from the other side, so a fix that only
    /// reverses the pop order without matching widths still fails here.
    #[test]
    fn host_syscall_parameters_keep_their_declared_widths_with_reversed_signature() {
        let wasm = wat::parse_str(
            r#"(module
                 (import "env" "s" (func $s (param i32 i64) (result i64)))
                 (func (export "main") (result i64)
                   i32.const 7
                   i64.const 0x1_0000_0002
                   call $s))"#,
        )
        .unwrap();
        let mut linker = ImportLinker::default();
        linker.insert_function(
            ImportName::new("env", "s"),
            1,
            SyscallFuelParams::default(),
            &[ValType::I32, ValType::I64],
            &[ValType::I64],
        );
        let linker = Arc::new(linker);

        fn handler(
            _caller: &mut TypedCaller<'_, ()>,
            _sys_func_idx: u32,
            params: &[Value],
            result: &mut [Value],
        ) -> Result<(), TrapCode> {
            match (params[0].i32(), params[1].i64()) {
                (Some(narrow), Some(wide)) => result[0] = Value::I64((narrow as i64) ^ wide),
                _ => result[0] = Value::I64(-1),
            }
            Ok(())
        }

        let expected = 7_i64 ^ 0x1_0000_0002_i64;
        let (mut rwasm, mut wasmtime) = both_executors(&wasm, linker, handler);

        let mut rwasm_result = [Value::I64(0)];
        let mut wasmtime_result = [Value::I64(0)];
        wasmtime
            .execute("main", &[], &mut wasmtime_result)
            .expect("wasmtime executes");
        rwasm
            .execute("main", &[], &mut rwasm_result)
            .expect("rwasm executes");

        assert_eq!(wasmtime_result[0].i64(), Some(expected));
        assert_eq!(
            rwasm_result[0].i64(),
            Some(expected),
            "R2-1: rwasm delivered {:?} to the handler instead of [{expected:#x}]",
            rwasm_result[0]
        );
    }

    /// `R2-3`: after a host handler returns `ExecutionHalted` both engines report success, but the
    /// caller's result buffer keeps its previous contents on rwasm while the Wasmtime backend writes
    /// zeros. The engines have to agree on this observable output.
    #[test]
    fn halted_call_reports_the_same_result_buffer_on_both_backends() {
        let wasm = wat::parse_str(
            r#"(module
                 (import "env" "exit" (func $exit (result i32)))
                 (func (export "main") (result i32) call $exit))"#,
        )
        .unwrap();
        let mut linker = ImportLinker::default();
        linker.insert_function(
            ImportName::new("env", "exit"),
            1,
            SyscallFuelParams::default(),
            &[],
            &[ValType::I32],
        );
        let linker = Arc::new(linker);

        fn halting_handler(
            _caller: &mut TypedCaller<'_, ()>,
            _sys_func_idx: u32,
            _params: &[Value],
            result: &mut [Value],
        ) -> Result<(), TrapCode> {
            result[0] = Value::I32(4242);
            Err(TrapCode::ExecutionHalted)
        }

        let (mut rwasm, mut wasmtime) = both_executors(&wasm, linker, halting_handler);

        // A template the caller reused from a previous execution: "untouched" and "zeroed" differ.
        let mut rwasm_result = [Value::I32(-1)];
        let mut wasmtime_result = [Value::I32(-1)];
        let rwasm_outcome = rwasm.execute("main", &[], &mut rwasm_result);
        let wasmtime_outcome = wasmtime.execute("main", &[], &mut wasmtime_result);

        assert_eq!(rwasm_outcome, Ok(()), "a halt is a successful termination");
        assert_eq!(wasmtime_outcome, Ok(()));
        assert_eq!(wasmtime_result, [Value::I32(0)]);
        assert_eq!(
            rwasm_result[0].i32(),
            wasmtime_result[0].i32(),
            "R2-3: rwasm left {:?} in the result buffer where wasmtime wrote {:?}",
            rwasm_result[0],
            wasmtime_result[0]
        );
    }

    /// A halted call reports zeros of the declared result types. The Wasmtime backend used to
    /// prepare `i32` placeholders for every result of a checked call, so an export returning a
    /// `funcref` reported `I32(0)` where rwasm reports the null reference.
    #[test]
    fn halted_call_zeroes_a_reference_result_on_both_backends() {
        use rwasm::FuncRef;
        let wasm = wat::parse_str(
            r#"(module
                 (import "env" "exit" (func $exit))
                 (func (export "main") (result funcref) call $exit ref.null func))"#,
        )
        .unwrap();
        let mut linker = ImportLinker::default();
        linker.insert_function(
            ImportName::new("env", "exit"),
            1,
            SyscallFuelParams::default(),
            &[],
            &[],
        );
        let linker = Arc::new(linker);

        fn halting_handler(
            _caller: &mut TypedCaller<'_, ()>,
            _sys_func_idx: u32,
            _params: &[Value],
            _result: &mut [Value],
        ) -> Result<(), TrapCode> {
            Err(TrapCode::ExecutionHalted)
        }

        let config = config(&linker).with_allow_func_ref_function_types(true);
        let definitions = [
            StrategyDefinition::new_as_rwasm(config.clone(), &wasm).expect("rwasm compiles"),
            StrategyDefinition::new_as_wasmtime(config, &wasm, None).expect("wasmtime compiles"),
        ];
        let outcomes = definitions.map(|definition| {
            let mut executor = definition
                .create_executor(linker.clone(), (), halting_handler, Some(1_000_000), None)
                .expect("the module must instantiate");
            let mut result = [Value::I32(-1)];
            let outcome = executor.execute("main", &[], &mut result);
            (outcome, result)
        });
        assert_eq!(outcomes[0], outcomes[1], "rwasm and wasmtime diverged");
        assert_eq!(outcomes[0], (Ok(()), [Value::FuncRef(FuncRef::null())]));
    }

    /// `R2-4`: a start function that halts on a host syscall is accepted by the rwasm entrypoint path
    /// (`ExecutionHalted` is mapped to `Ok(())` there) and rejects instantiation on the Wasmtime
    /// backend, so the same module is deployable on one engine and not on the other.
    #[test]
    fn start_function_that_halts_agrees_on_both_backends() {
        let wasm = wat::parse_str(
            r#"(module
                 (import "env" "exit" (func $exit))
                 (memory (export "memory") 1)
                 (global $state (mut i32) (i32.const 0))
                 (start $init)
                 (func $init
                   i32.const 7
                   global.set $state
                   call $exit
                   i32.const 9
                   global.set $state)
                 (func (export "main") (result i32) global.get $state))"#,
        )
        .unwrap();
        let mut linker = ImportLinker::default();
        linker.insert_function(
            ImportName::new("env", "exit"),
            1,
            SyscallFuelParams::default(),
            &[],
            &[],
        );
        let linker = Arc::new(linker);
        let cfg = config(&linker).with_allow_start_section(true);

        fn halting_handler(
            _caller: &mut TypedCaller<'_, ()>,
            _sys_func_idx: u32,
            _params: &[Value],
            _result: &mut [Value],
        ) -> Result<(), TrapCode> {
            Err(TrapCode::ExecutionHalted)
        }

        let instantiate = |definition: StrategyDefinition| {
            definition
                .create_executor(linker.clone(), (), halting_handler, Some(1_000_000), None)
                .map(|_| ())
        };
        let rwasm =
            instantiate(StrategyDefinition::new_as_rwasm(cfg.clone(), &wasm).expect("compiles"));
        let wasmtime =
            instantiate(StrategyDefinition::new_as_wasmtime(cfg, &wasm, None).expect("compiles"));

        assert_eq!(rwasm, Err(TrapCode::ExecutionHalted));
        assert_eq!(wasmtime, Err(TrapCode::ExecutionHalted));
    }

    /// Mixed numeric signatures and multiple results exercise both ordering and slot widths.
    #[test]
    fn mixed_numeric_syscalls_preserve_values_and_multiple_results() {
        let signatures = [
            vec![Value::I64(0x1234_5678_9abc_def0), Value::I32(-7)],
            vec![Value::I32(-7), Value::I64(0x1234_5678_9abc_def0)],
            vec![Value::F64(123.5.into()), Value::I32(-7)],
            vec![
                Value::I32(-7),
                Value::F32((-5.25).into()),
                Value::I64(-123456789012),
                Value::F64(123.5.into()),
            ],
        ];
        for args in signatures {
            let params: &'static [ValType] = Box::leak(
                args.iter()
                    .map(Value::ty)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            );
            let results: &'static [ValType] = Box::leak(
                params
                    .iter()
                    .rev()
                    .copied()
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            );
            let param_types = params.iter().map(wat_type).collect::<Vec<_>>().join(" ");
            let result_types = results.iter().map(wat_type).collect::<Vec<_>>().join(" ");
            let loads = (0..args.len())
                .map(|i| format!("local.get {i}"))
                .collect::<Vec<_>>()
                .join(" ");
            let wasm = wat::parse_str(format!(
                r#"(module
                (import "env" "s" (func $s (param {param_types}) (result {result_types})))
                (func (export "main") (param {param_types}) (result {result_types}) {loads} call $s))"#
            ))
            .unwrap();
            let mut linker = ImportLinker::default();
            linker.insert_function(
                ImportName::new("env", "s"),
                1,
                SyscallFuelParams::default(),
                params,
                results,
            );
            fn handler(
                _: &mut TypedCaller<'_, ()>,
                _: u32,
                params: &[Value],
                result: &mut [Value],
            ) -> Result<(), TrapCode> {
                for (out, arg) in result.iter_mut().zip(params.iter().rev()) {
                    *out = arg.clone();
                }
                Ok(())
            }
            let (rwasm, wasmtime) = both_executors(&wasm, Arc::new(linker), handler);
            let expected = args.iter().rev().cloned().collect::<Vec<_>>();
            for mut executor in [rwasm, wasmtime] {
                let mut result = results
                    .iter()
                    .map(|ty| Value::default(*ty))
                    .collect::<Vec<_>>();
                executor.execute("main", &args, &mut result).unwrap();
                assert_eq!(result, expected);
            }
        }
    }

    #[test]
    fn halted_call_zeros_every_numeric_result_type() {
        let wasm = wat::parse_str(
            r#"(module
            (import "env" "exit" (func $exit))
            (func (export "main") (result i32 i64 f32 f64) call $exit unreachable))"#,
        )
        .unwrap();
        let mut linker = ImportLinker::default();
        linker.insert_function(
            ImportName::new("env", "exit"),
            1,
            SyscallFuelParams::default(),
            &[],
            &[],
        );
        fn handler(
            _: &mut TypedCaller<'_, ()>,
            _: u32,
            _: &[Value],
            _: &mut [Value],
        ) -> Result<(), TrapCode> {
            Err(TrapCode::ExecutionHalted)
        }
        let (rwasm, wasmtime) = both_executors(&wasm, Arc::new(linker), handler);
        for mut executor in [rwasm, wasmtime] {
            let mut result = [
                Value::I32(-1),
                Value::I64(-2),
                Value::F32(3.5.into()),
                Value::F64(4.5.into()),
            ];
            executor.execute("main", &[], &mut result).unwrap();
            assert_eq!(
                result,
                [
                    Value::I32(0),
                    Value::I64(0),
                    Value::F32(0.0.into()),
                    Value::F64(0.0.into())
                ]
            );
        }
    }

    #[test]
    fn syscall_reserves_all_wide_result_slots() {
        use rwasm::{instruction_set, ExecutionEngine, RwasmModuleBuilder, RwasmStore};
        // No StackCheck: exercise the host ABI's own reservation past the default 32-slot capacity.
        let module = RwasmModuleBuilder::new(instruction_set! { Call(1) Return }).build();
        let mut linker = ImportLinker::default();
        linker.insert_function(
            ImportName::new("env", "s"),
            1,
            SyscallFuelParams::default(),
            &[],
            &[ValType::I64; 33],
        );
        fn handler(
            _: &mut TypedCaller<'_, ()>,
            _: u32,
            _: &[Value],
            result: &mut [Value],
        ) -> Result<(), TrapCode> {
            for (i, out) in result.iter_mut().enumerate() {
                *out = Value::I64(0x1234567800000000 + i as i64);
            }
            Ok(())
        }
        let mut store = RwasmStore::new(Arc::new(linker), (), handler, None, None);
        let mut result = vec![Value::I64(0); 33];
        ExecutionEngine::new()
            .execute(&mut store, &module, &[], &mut result)
            .unwrap();
        assert_eq!(
            result,
            (0..33)
                .map(|i| Value::I64(0x1234567800000000 + i))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn resumed_initialization_preserves_the_halt_error() {
        use rwasm::{instruction_set, ExecutionEngine, RwasmModuleBuilder, RwasmStore};
        let module = RwasmModuleBuilder::new(instruction_set! { Call(1) Call(2) Return }).build();
        let mut linker = ImportLinker::default();
        for (name, index) in [("pause", 1), ("exit", 2)] {
            linker.insert_function(
                ImportName::new("env", name),
                index,
                SyscallFuelParams::default(),
                &[],
                &[],
            );
        }
        fn handler(
            _: &mut TypedCaller<'_, ()>,
            index: u32,
            _: &[Value],
            _: &mut [Value],
        ) -> Result<(), TrapCode> {
            Err(if index == 1 {
                TrapCode::InterruptionCalled
            } else {
                TrapCode::ExecutionHalted
            })
        }
        let linker = Arc::new(linker);
        let engine = ExecutionEngine::new();
        for initializing in [false, true] {
            let mut store = RwasmStore::new(linker.clone(), (), handler, None, None);
            let outcome = if initializing {
                engine.entrypoint(&mut store, &module)
            } else {
                engine.execute(&mut store, &module, &[], &mut [])
            };
            assert_eq!(outcome, Err(TrapCode::InterruptionCalled));
            assert_eq!(
                engine.resume(&mut store, &[], &mut []),
                if initializing {
                    Err(TrapCode::ExecutionHalted)
                } else {
                    Ok(())
                }
            );
            assert_eq!(
                engine.resume(&mut store, &[], &mut []),
                Err(TrapCode::IllegalOpcode)
            );
        }
    }
}

mod syscall_results {
    //! Audit 2026-09-13, round 3 (R3-3): the syscall result buffer contract is the same on both
    //! backends, for a result the handler never wrote and for a result of the wrong type.

    use rwasm::{
        CompilationConfig, ImportLinker, ImportName, StrategyDefinition, SyscallFuelParams,
        TrapCode, TypedCaller, ValType, Value,
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
            .create_executor(
                linker.clone(),
                Ctx::default(),
                handler,
                Some(1_000_000),
                None,
            )
            .expect("rwasm instantiates");
        let mut rwasm = rwasm;
        let mut result = [Value::I64(-1)];
        let rwasm = rwasm
            .execute("main", &[], &mut result)
            .map(|()| result[0].clone());

        let wasmtime = StrategyDefinition::new_as_wasmtime(test_config(linker), &wasm, None)
            .expect("wasmtime compiles")
            .create_executor(
                linker.clone(),
                Ctx::default(),
                handler,
                Some(1_000_000),
                None,
            )
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
}
