use rwasm::{Caller, CompilationConfig};

fn time_us() -> u128 {
    use std::time::SystemTime;
    let duration_since_epoch = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    duration_since_epoch.as_micros()
}

#[test]
fn test_rwasm() {
    use rwasm::{ExecutorConfig, RwasmExecutor, RwasmModule};
    use std::sync::Arc;

    let wasm = include_bytes!("./lib.wasm");

    let time = time_us();
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, wasm).unwrap();
    let encoded_rwasm_module = rwasm_module.serialize();
    println!("Compilation time: {} us", time_us() - time);

    let time = time_us();
    let rwasm_module = RwasmModule::new(&encoded_rwasm_module);
    println!("Deserialization time: {} us", time_us() - time);

    let time = time_us();
    let mut vm = RwasmExecutor::new(Arc::new(rwasm_module), ExecutorConfig::default(), ());
    println!("Executor creation time: {} us", time_us() - time);
    Caller::new(&mut vm).stack_push(43);
    let time = time_us();
    vm.run().unwrap();
    println!("Execution time: {} us", time_us() - time);
    let result: i32 = Caller::new(&mut vm).stack_pop_as();
    core::hint::black_box(result);
    assert_eq!(result, 433494437);
    vm.reset(None, false);
}
