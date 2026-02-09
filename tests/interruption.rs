use rwasm::{
    always_failing_syscall_handler, compile_wasmtime_module, instruction_set, CompilationConfig,
    ExecutionEngine, FuelConfig, ImportLinker, ImportName, RwasmModule, RwasmStore, Store,
    Strategy, TrapCode, TypedCaller, Value, WasmtimeStore,
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
    let module = RwasmModule::with_one_function(instruction_set! {
        ConsumeFuel(1u32) // +1
        Call(0xff)
        ConsumeFuel(2u32) // +2
        Return
    });
    let import_linker = default_import_linker();
    let mut store = RwasmStore::<()>::new(
        import_linker,
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> {
            // return an empty interruption
            Err(TrapCode::InterruptionCalled)
        },
        FuelConfig::default().with_fuel_limit(100_000),
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
        FuelConfig::default().with_fuel_limit(100_000),
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
        FuelConfig::default().with_fuel_limit(100_000),
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
        always_failing_syscall_handler,
        FuelConfig::default(),
    );
    store.set_syscall_handler(interrupting_syscall_handler);
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
    let mut wasmtime_worker = WasmtimeStore::new(
        module,
        import_linker.clone(),
        (),
        interrupting_syscall_handler,
        FuelConfig::default(),
    );
    let mut result = [Value::I32(0); 1];
    let err = wasmtime_worker
        .execute("main", &[], &mut result)
        .unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    let err = wasmtime_worker.resume(Ok(&[]), &mut result).unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    wasmtime_worker.resume(Ok(&[]), &mut result).unwrap();
    assert_eq!(result[0].i32().unwrap(), 123);
}

#[test]
fn test_call_stack_empty_after_trap_in_nested_call() {
    let module = RwasmModule::with_one_function(instruction_set! {
        CallInternal(2) // call to --+
        Return //                    |
        ConsumeFuel(1u32) //    <----+
        Call(0xff)
        Trap(TrapCode::UnreachableCodeReached)
    });
    let import_linker = default_import_linker();
    let mut store = RwasmStore::<()>::new(
        import_linker,
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> {
            // return an empty interruption
            Err(TrapCode::InterruptionCalled)
        },
        FuelConfig::default(),
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
    let module = RwasmModule::with_one_function(instruction_set! {
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
    });
    let import_linker = default_import_linker();

    let test_strategy = |strategy: Strategy| {
        let mut store = strategy.create_store(
            import_linker.clone(),
            (),
            |caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> {
                caller.memory_write(0, &[0x01, 0x02, 0x03, 0x04])?;
                // return an empty interruption
                Err(TrapCode::InterruptionCalled)
            },
            FuelConfig::default(),
        );
        let mut result = [Value::I32(0); 1];
        let err = strategy
            .execute(&mut store, "main", &[], &mut result)
            .unwrap_err();
        assert_eq!(err, TrapCode::InterruptionCalled);
        strategy.resume(&mut store, &[], &mut result).unwrap();
        assert_eq!(result[0].i32().unwrap(), 0x04030201);
    };

    test_strategy(Strategy::Rwasm {
        module,
        engine: ExecutionEngine::acquire_shared(),
    });
}
