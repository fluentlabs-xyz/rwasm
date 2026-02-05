use rwasm::{
    always_failing_syscall_handler, compile_wasmtime_module, CompilationConfig, FuelConfig,
    ImportLinker, Strategy, Value,
};
use std::sync::Arc;
use wasmtime::{Engine, Instance, Module, Store, TypedFunc};

pub fn run_main<Results>(wat: &str) -> anyhow::Result<Results>
where
    Results: wasmtime::WasmResults,
{
    let engine = Engine::default();
    let module = Module::new(&engine, wat)?;
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[])?;
    let run: TypedFunc<(), Results> = instance.get_typed_func(&mut store, "main")?;
    run.call(&mut store, ())
}

#[test]
fn test_wasmtime_disabled_f32_sqrt() -> anyhow::Result<()> {
    let wat = r#"
        (module
            (func (export "main") (result f32)
                f32.const 9.0
                f32.sqrt
            )
        )
    "#;
    let result = run_main::<f32>(wat);
    let trap = result
        .err()
        .expect("execution should fail")
        .downcast_ref::<wasmtime::Trap>()
        .expect("execution should fail with a trap")
        .clone();
    matches!(trap, wasmtime::Trap::DisabledOpcode);
    Ok(())
}

#[test]
fn test_wasmtime_disabled_f64_div() -> anyhow::Result<()> {
    let wat = r#"
        (module
            (func (export "main") (result f64)
                f64.const 9.0
                f64.const 3.0
                f64.div
                f64.const 10.0
                f64.add
            )
        )
    "#;
    let result = run_main::<f64>(wat);
    let trap = result
        .err()
        .expect("execution should fail")
        .downcast_ref::<wasmtime::Trap>()
        .expect("execution should fail with a trap")
        .clone();
    matches!(trap, wasmtime::Trap::DisabledOpcode);
    Ok(())
}

#[test]
fn test_wasmtime_f32_const() -> anyhow::Result<()> {
    let wat = r#"
        (module
            (func (export "main") (result f32)
                f32.const 9.0
            )
        )
    "#;
    let result = run_main::<f32>(wat);
    matches!(result, Ok(9.0));
    Ok(())
}

#[test]
fn test_wasmtime_f64_const() -> anyhow::Result<()> {
    let wat = r#"
        (module
            (func (export "main") (result f64)
                f64.const 9.0
            )
        )
    "#;
    let result = run_main::<f64>(wat);
    matches!(result, Ok(9.0));
    Ok(())
}

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
        store.set_fuel(u64::MAX);
        strategy.execute(&mut store, "main", &[], &mut []).unwrap();
    }
    let mut store = strategy.empty_store();
    store.set_fuel(u64::MAX);
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
