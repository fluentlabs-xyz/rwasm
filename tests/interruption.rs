use rwasm::{
    instruction_set,
    wasmtime::{compile_wasmtime_module, WasmtimeExecutor},
    CompilationConfig, ExecutionEngine, ImportLinker, ImportName, RwasmModule, RwasmModuleBuilder,
    RwasmStore, StoreTr, StrategyDefinition, TrapCode, TypedCaller, Value,
};
use rwasm_fuel_policy::{LinearFuelParams, SyscallFuelParams};
use std::sync::Arc;
use wasmparser::ValType;

fn default_import_linker() -> Arc<ImportLinker> {
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("hello", "world"),
        0xff,
        SyscallFuelParams::default(),
        &[],
        &[],
    );
    Arc::new(import_linker)
}

fn interrupting_syscall_handler<T: Send + Sync>(
    _caller: &mut TypedCaller<'_, T>,
    _sys_func_idx: u32,
    _params: &[Value],
    _result: &mut [Value],
) -> Result<(), TrapCode> {
    Err(TrapCode::InterruptionCalled)
}

#[test]
fn test_interrupted_call_rwasm() {
    let module = RwasmModuleBuilder::new(instruction_set! {
        // entrypoint
        Return
        // function
        ConsumeFuel(1u32) // +1
        Call(0xff)
        ConsumeFuel(2u32) // +2
        Return
    })
    .with_source_pc(1)
    .build();
    let import_linker = default_import_linker();
    let mut store = RwasmStore::<()>::new(
        import_linker,
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> {
            // return an empty interruption
            Err(TrapCode::InterruptionCalled)
        },
        Some(100_000),
    );
    let engine = ExecutionEngine::new();
    let err = engine
        .execute(&mut store, &module, &[], &mut [])
        .unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    assert_eq!(store.fuel_consumed(), 1);
    engine.resume(&mut store, &[], &mut []).unwrap();
    assert_eq!(store.fuel_consumed(), 3);
}

#[test]
fn test_interrupted_call_rwasm_with_syscall() {
    let wasm_binary = wat::parse_str(
        r#"
            (module
              (func $default_call (import "hello" "world") (param i32))
              (func (export "main")
                (i32.const 300)
                (call $default_call)
              )
            )
            "#,
    )
    .unwrap();
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("hello", "world"),
        0xee,
        SyscallFuelParams::LinearFuel(LinearFuelParams {
            base_fuel: 7,
            param_index: 1,
            word_cost: 5,
        }),
        &[ValType::I32],
        &[],
    );
    let import_linker = Arc::new(import_linker);

    let (rwasm_module, _) = RwasmModule::compile(
        CompilationConfig::default()
            .with_builtins_consume_fuel(true)
            .with_consume_fuel_for_params_and_locals(false)
            .with_import_linker(import_linker.clone())
            .with_entrypoint_name("main".into()),
        &wasm_binary,
    )
    .unwrap();

    let mut store = RwasmStore::<()>::new(
        import_linker,
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> { Ok(()) },
        Some(100_000),
    );
    let engine = ExecutionEngine::new();
    engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap();
    // 1 - function cost
    // 1 - base cost
    // 10 - call cost
    // 10*5 - linear cost
    // 7 - base cost
    assert_eq!(store.fuel_consumed(), 1 + 1 + 10 + 10 * 5 + 7);
}

#[test]
fn test_interrupted_call_rwasm_with_overflow() {
    let wasm_binary = wat::parse_str(
        r#"
            (module
              (func $default_call (import "hello" "world") (param i32))
              (func (export "main")
                (i32.const 134_217_729)
                (call $default_call)
              )
            )
            "#,
    )
    .unwrap();
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("hello", "world"),
        0xee,
        SyscallFuelParams::LinearFuel(LinearFuelParams {
            base_fuel: 7,
            param_index: 1,
            word_cost: 5,
        }),
        &[ValType::I32],
        &[],
    );
    let import_linker = Arc::new(import_linker);

    let (rwasm_module, _) = RwasmModule::compile(
        CompilationConfig::default()
            .with_builtins_consume_fuel(true)
            .with_import_linker(import_linker.clone())
            .with_entrypoint_name("main".into()),
        &wasm_binary,
    )
    .unwrap();

    let mut store = RwasmStore::<()>::new(
        import_linker,
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> { Ok(()) },
        Some(100_000),
    );
    let engine = ExecutionEngine::new();
    let err = engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap_err();
    assert_eq!(err, TrapCode::IntegerOverflow);
}

#[test]
// Note: this test can't pass, because we don't support interruptions for wasmtime anymore
#[ignore]
fn test_interrupted_call_wasmtime() {
    let wasm_binary = wat::parse_str(
        r#"
(module
  (func $interrupt (import "hello" "world"))
  (func (export "main") (result i32)
    (i32.const 100)
    (call $interrupt)
    (i32.const 20)
    (call $interrupt)
    (i32.const 3)
    (i32.add)
    (i32.add)
  )
)
"#,
    )
    .unwrap();
    let import_linker = default_import_linker();
    let (rwasm_module, _) = RwasmModule::compile(
        CompilationConfig::default()
            .with_import_linker(import_linker.clone())
            .with_entrypoint_name("main".into()),
        &wasm_binary,
    )
    .unwrap();
    // run with rwasm
    let mut store = RwasmStore::<()>::new(
        import_linker.clone(),
        (),
        interrupting_syscall_handler,
        None,
    );
    let engine = ExecutionEngine::new();
    let mut result = [Value::I32(0); 1];
    let err = engine
        .execute(&mut store, &rwasm_module, &[], &mut result)
        .unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    let err = engine.resume(&mut store, &[], &mut result).unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    engine.resume(&mut store, &[], &mut result).unwrap();
    assert_eq!(result[0].i32().unwrap(), 123);
    // run with wasmtime
    let module = compile_wasmtime_module(
        CompilationConfig::default().with_consume_fuel(false),
        &wasm_binary,
    )
    .unwrap();
    let mut wasmtime_worker = WasmtimeExecutor::new(
        module,
        import_linker.clone(),
        (),
        interrupting_syscall_handler,
        None,
    );
    let mut result = [Value::I32(0); 1];
    let err = wasmtime_worker
        .execute("main", &[], &mut result)
        .unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    let err = wasmtime_worker.resume(&[], &mut result).unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    wasmtime_worker.resume(&[], &mut result).unwrap();
    assert_eq!(result[0].i32().unwrap(), 123);
}

#[test]
fn test_call_stack_empty_after_trap_in_nested_call() {
    let module = RwasmModuleBuilder::new(instruction_set! {
        CallInternal(2) // call to --+
        Return //                    |
        ConsumeFuel(1u32) //    <----+
        Call(0xff)
        Trap(TrapCode::UnreachableCodeReached)
    })
    .build();
    let import_linker = default_import_linker();
    let mut store = RwasmStore::<()>::new(
        import_linker,
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> {
            // return an empty interruption
            Err(TrapCode::InterruptionCalled)
        },
        None,
    );
    let engine = ExecutionEngine::new();
    let err = engine
        .execute(&mut store, &module, &[], &mut [])
        .unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    let err = engine.resume(&mut store, &[], &mut []).unwrap_err();
    assert_eq!(err, TrapCode::UnreachableCodeReached);
}

#[test]
fn test_memory_write_during_interruption() {
    let module = RwasmModuleBuilder::new(instruction_set! {
        // entrypoint
        Return
        // init some memory
        I32Const(1)
        MemoryGrow
        Drop
        // call interruption
        Call(0xff)
        // copy 4 bytes from memory
        I32Const(0)
        I32Load(0)
        // exit
        Return
    })
    .with_source_pc(1)
    .build();
    let import_linker = default_import_linker();

    let test_strategy = |strategy: StrategyDefinition| {
        let mut executor = strategy
            .create_executor(
                import_linker.clone(),
                (),
                |caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> {
                    caller.memory_write(0, &[0x01, 0x02, 0x03, 0x04])?;
                    // return an empty interruption
                    Err(TrapCode::InterruptionCalled)
                },
                None,
            )
            .unwrap();
        let mut result = [Value::I32(0); 1];
        let err = executor.execute("main", &[], &mut result).unwrap_err();
        assert_eq!(err, TrapCode::InterruptionCalled);
        executor.resume(&[], &mut result).unwrap();
        assert_eq!(result[0].i32().unwrap(), 0x04030201);
    };

    test_strategy(StrategyDefinition::Rwasm {
        module,
        engine: ExecutionEngine::acquire_shared(),
    });
}
