use fluent_rwasm::{
    always_failing_syscall_handler, compile_wasmtime_module, CompilationConfig, FuelConfig,
    ImportLinker, Strategy, Value,
};
use std::sync::Arc;

#[test]
fn test_10001_instances_in_a_row() {
    let wasm_binary = wat::parse_str(
        r#"
(module
  (func (export "main")
    (i32.const 100)
    (i32.const 20)
    (i32.const 3)
    (i32.add)
    (i32.add)
    (drop)
  )
)
"#,
    )
    .unwrap();
    let strategy = Strategy::Wasmtime {
        module: compile_wasmtime_module(CompilationConfig::default(), &wasm_binary).unwrap(),
    };
    for _ in 0..10_000 {
        let mut store = strategy.empty_store();
        strategy.execute(&mut store, "main", &[], &mut []).unwrap();
    }
    let mut store = strategy.empty_store();
    strategy.execute(&mut store, "main", &[], &mut []).unwrap();
}

#[test]
fn test_fib_bench() {
    let wasm_binary = include_bytes!("../benchmarks/lib.wasm");
    let strategy = Strategy::Wasmtime {
        module: compile_wasmtime_module(CompilationConfig::default(), wasm_binary).unwrap(),
    };
    // it fails on iter number 32'165...
    for _ in 0..32_165 {
        let mut store = strategy.create_store(
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            FuelConfig::default().with_fuel_limit(1_000_000),
        );
        let mut result = [Value::I32(0)];
        strategy
            .execute(&mut store, "main", &[Value::I32(43)], &mut result)
            .unwrap();
        core::hint::black_box(result);
    }
}

#[test]
fn test_instance_reuse() {
    use wasmtime::*;
    let wasm_binary = include_bytes!("../benchmarks/lib.wasm");
    let mut config = Config::new();
    let pooling_allocator = PoolingAllocationConfig::default();
    config.allocation_strategy(InstanceAllocationStrategy::Pooling(pooling_allocator));
    let engine = Engine::new(&config).unwrap();
    let module = Module::new(&engine, wasm_binary).unwrap();
    let linker = Linker::new(module.engine());
    let instance_pre = linker.instantiate_pre(&module).unwrap();
    // it fails on iter number 32'165...
    for _ in 0..32_165 {
        let mut store = Store::new(module.engine(), ());
        let instance = instance_pre.instantiate(store.as_context_mut()).unwrap();
        let entrypoint = instance.get_func(store.as_context_mut(), "main").unwrap();
        let mut result = [Val::I32(0)];
        entrypoint
            .call(store.as_context_mut(), &[Val::I32(43)], &mut result)
            .unwrap();
        assert_eq!(result[0].i32(), Some(433_494_437));
    }
}
