use rwasm::{instruction_set, ExecutionEngine, ExecutorConfig, RwasmModule, Store, TrapCode};

#[test]
fn test_interrupted_call() {
    let module = RwasmModule::with_one_function(instruction_set! {
        ConsumeFuel(1u32) // +1
        Call(0xff)
        ConsumeFuel(2u32) // +2
        Return
    });
    let mut store = Store::<()>::new(ExecutorConfig::default().fuel_enabled(true), ());
    store.set_syscall_handler(|_caller, _sys_func_idx| -> Result<(), TrapCode> {
        // return an empty interruption
        Err(TrapCode::InterruptionCalled)
    });
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
