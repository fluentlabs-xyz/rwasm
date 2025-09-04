use rwasm::{
    always_failing_syscall_handler, compile_wasmtime_module, instruction_set, CompilationConfig,
    ExecutionEngine, ExecutorConfig, ImportLinker, ImportName, InstructionSet, RwasmModule,
    RwasmStore, Store, Strategy, TrapCode, TypedCaller, Value, WasmtimeStore,
};
use std::rc::Rc;

fn default_import_linker() -> Rc<ImportLinker> {
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("hello", "world"),
        0xff,
        InstructionSet::default(),
        &[],
        &[],
    );
    Rc::new(import_linker)
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
        ExecutorConfig::default().fuel_enabled(true),
        import_linker,
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> {
            // return an empty interruption
            Err(TrapCode::InterruptionCalled)
        },
    );
    let mut engine = ExecutionEngine::new();
    let err = engine
        .execute(&mut store, &module, &[], &mut [])
        .unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    assert_eq!(store.fuel_consumed(), 1);
    engine.resume(&mut store, &module, &[], &mut []).unwrap();
    assert_eq!(store.fuel_consumed(), 3);
}

#[test]
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
        ExecutorConfig::default().fuel_enabled(true),
        import_linker.clone(),
        (),
        always_failing_syscall_handler,
    );
    store.set_syscall_handler(interrupting_syscall_handler);
    let mut engine = ExecutionEngine::new();
    let mut result = [Value::I32(0); 1];
    let err = engine
        .execute(&mut store, &rwasm_module, &[], &mut result)
        .unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    let err = engine
        .resume(&mut store, &rwasm_module, &[], &mut result)
        .unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    engine
        .resume(&mut store, &rwasm_module, &[], &mut result)
        .unwrap();
    assert_eq!(result[0].i32().unwrap(), 123);
    // run with wasmtime
    let module =
        Rc::new(compile_wasmtime_module(CompilationConfig::default(), &wasm_binary).unwrap());
    let mut wasmtime_worker = WasmtimeStore::new(
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
        ExecutorConfig::default().fuel_enabled(true),
        import_linker,
        (),
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> {
            // return an empty interruption
            Err(TrapCode::InterruptionCalled)
        },
    );
    let mut engine = ExecutionEngine::new();
    let err = engine
        .execute(&mut store, &module, &[], &mut [])
        .unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    let err = engine
        .resume(&mut store, &module, &[], &mut [])
        .unwrap_err();
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
            ExecutorConfig::default(),
            import_linker.clone(),
            (),
            |caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> {
                caller.memory_write(0, &[0x01, 0x02, 0x03, 0x04])?;
                // return an empty interruption
                Err(TrapCode::InterruptionCalled)
            },
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
        module: Rc::new(module),
        engine: ExecutionEngine::acquire_shared(),
    });
}
