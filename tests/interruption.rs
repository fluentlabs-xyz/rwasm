use rwasm::{
    instruction_set,
    wasmtime::WasmtimeExecutor,
    Caller,
    CompilationConfig,
    ExecutionEngine,
    ExecutorConfig,
    ImportLinker,
    ImportName,
    InstructionSet,
    RwasmModule,
    Store,
    TrapCode,
    Value,
};
use std::rc::Rc;

fn import_linker() -> Rc<ImportLinker> {
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

fn syscall_handler<T>(
    _caller: &mut dyn Caller<T>,
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
    let import_linker = import_linker();
    let mut store = Store::<()>::new(
        ExecutorConfig::default().fuel_enabled(true),
        (),
        import_linker,
    );
    store.set_syscall_handler(
        |_caller, _sys_func_idx, _params, _result| -> Result<(), TrapCode> {
            // return an empty interruption
            Err(TrapCode::InterruptionCalled)
        },
    );
    let mut engine = ExecutionEngine::new();
    let err = engine.execute(&mut store, &module).unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    assert_eq!(store.fuel_consumed(), 1);
    engine.resume(&mut store, &module).unwrap();
    assert_eq!(store.fuel_consumed(), 3);
    // make sure the engine is empty
    let sp = engine.value_stack().stack_ptr();
    assert_eq!(engine.value_stack().stack_len(sp), 0);
    assert!(engine.call_stack().is_empty());
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
    (i32.add)
  )
)
"#,
    )
    .unwrap();
    let import_linker = import_linker();
    let (rwasm_module, _) = RwasmModule::compile(
        CompilationConfig::default()
            .with_import_linker(import_linker.clone())
            .with_entrypoint_name("main".into()),
        &wasm_binary,
    )
    .unwrap();
    // run with rwasm
    let mut store = Store::<()>::new(
        ExecutorConfig::default().fuel_enabled(true),
        (),
        import_linker.clone(),
    );
    store.set_syscall_handler(syscall_handler);
    let mut engine = ExecutionEngine::new();
    let err = engine.execute(&mut store, &rwasm_module).unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    engine.resume(&mut store, &rwasm_module).unwrap();
    // make sure the engine is empty
    let sp = engine.value_stack().stack_ptr();
    assert_eq!(engine.value_stack().stack_len(sp), 1);
    assert!(engine.call_stack().is_empty());
    assert_eq!(engine.value_stack().pop().as_u32(), 120);
    // run with wasmtime
    let mut wasmtime_vm = WasmtimeExecutor::compile(
        &rwasm_module.wasm_section,
        import_linker.clone(),
        (),
        syscall_handler,
    );
    let mut result = [wasmtime::Val::I32(0); 1];
    let err = wasmtime_vm.run_typed(&[], &mut result).unwrap_err();
    assert_eq!(err, TrapCode::InterruptionCalled);
    wasmtime_vm.run_typed(&[], &mut result).unwrap();
    assert_eq!(result[0].i32().unwrap(), 120);
}
