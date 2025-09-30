#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub fn main(n: i32) -> i32 {
    let (mut a, mut b) = (0, 1);
    for _ in 0..n {
        let temp = a;
        a = b;
        b = temp + b;
    }
    a
}

#[cfg(test)]
mod tests {
    use rwasm::{
        always_failing_syscall_handler, CompilationConfig, ExecutionEngine, FuelConfig,
        ImportLinker, RwasmModule, Strategy, Value,
    };
    use std::sync::Arc;

    #[test]
    fn strategy_rwasm_test() {
        const FIB_VALUE: i32 = 43;
        fn bench_strategy(strategy: Strategy) {
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
        let wasm_binary = include_bytes!("./lib.wasm");
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
        bench_strategy(strategy);
    }
}
