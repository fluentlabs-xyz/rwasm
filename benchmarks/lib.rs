use alloy_primitives::U256;

#[no_mangle]
pub fn fib32(n: u32) -> u32 {
    let (mut a, mut b) = (0, 1);
    for _ in 0..n {
        let temp = a;
        a = b;
        b = temp + b;
    }
    a
}

#[no_mangle]
pub fn fib64(n: u64) -> u64 {
    let (mut a, mut b) = (0, 1);
    for _ in 0..n {
        let temp = a;
        a = b;
        b = temp + b;
    }
    a
}

#[no_mangle]
pub fn fib256(n: u64) -> U256 {
    let (mut a, mut b) = (U256::ZERO, U256::ONE);
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

    const FIB_VALUE: i32 = 41;

    #[test]
    fn fib32_test() {
        let wasm_binary = include_bytes!("./lib.wasm");
        let config = CompilationConfig::default()
            .with_entrypoint_name("fib32".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_consume_fuel(false);
        let (module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
        let strategy = Strategy::Rwasm {
            module: module.clone(),
            engine: ExecutionEngine::acquire_shared(),
        };
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
        core::hint::black_box(result);
    }
}
