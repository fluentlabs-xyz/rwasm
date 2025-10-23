use fluent_rwasm::{
    always_failing_syscall_handler, CompilationConfig, ExecutionEngine, FuelConfig, ImportLinker,
    RwasmModule, Strategy, Value,
};
use std::sync::Arc;

#[test]
fn test_fib256_iter() {
    let wasm_binary = include_bytes!("../benchmarks/lib.wasm");
    let config = CompilationConfig::default()
        .with_entrypoint_name("fib64".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_consume_fuel(false);
    let (module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    let strategy = Strategy::Rwasm {
        module: module.clone(),
        engine: ExecutionEngine::acquire_shared(),
    };
    for _ in 0..10_000 {
        let mut store = strategy.create_store(
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            FuelConfig::default(),
        );
        let mut result = [];
        strategy
            .execute(
                &mut store,
                "fib256",
                &[Value::I32(0), Value::I64(43)],
                &mut result,
            )
            .unwrap();
        core::hint::black_box(result);
    }
}
