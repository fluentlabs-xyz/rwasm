#![no_main]

use rwasm::{
    always_failing_syscall_handler, compile_wasmtime_module, CompilationConfig, FuelConfig,
    ImportLinker, Strategy, Value,
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
            .execute(&mut store, "fib32", &[Value::I32(FIB_VALUE)], &mut result)
            .unwrap();
        core::hint::black_box(result.clone());
        assert_eq!(result[0].i32(), Value::I32(433494437).i32());
    }
    let wasm_binary = include_bytes!("../benchmarks/lib.wasm");
    let config = CompilationConfig::default().with_consume_fuel(false);
    let module = compile_wasmtime_module(config, wasm_binary).unwrap();
    let strategy = Strategy::Wasmtime {
        module: module.clone(),
    };
    for _ in 0..1 {
        bench_strategy(&strategy);
    }
}
