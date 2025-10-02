#![no_main]

use rwasm::{
    always_failing_syscall_handler, CompilationConfig, ExecutionEngine, FuelConfig, ImportLinker,
    RwasmModule, Strategy, Value,
};
use std::sync::Arc;

#[no_mangle]
pub fn main() {
    const FIB_VALUE: i32 = 43;
    #[inline(never)]
    fn bench_strategy(strategy: &Strategy) {
        let mut store = strategy.create_store(
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            FuelConfig::default(),
        );
        let mut result = [Value::I32(0)];
        strategy
            .execute(&mut store, "main", &[Value::I32(FIB_VALUE)], &mut result)
            .unwrap();
        core::hint::black_box(result.clone());
    }
    let wasm_binary = include_bytes!("../benchmarks/lib.wasm");
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_consume_fuel(false);
    let (module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    println!("module = {}", module);
    let strategy = Strategy::Rwasm {
        module: module.clone(),
        engine: ExecutionEngine::acquire_shared(),
    };
    for _ in 0..1000 {
        bench_strategy(&strategy);
    }
}
