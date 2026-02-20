use fib_example::FIB_WASM;
use rwasm::{
    always_failing_syscall_handler, wasmtime::compile_wasmtime_module, CompilationConfig,
    ImportLinker, StrategyDefinition, Value,
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
fn test_wasmtime_disabled_f32_sqrt() {
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
    assert_eq!(trap, wasmtime::Trap::DisabledOpcode);
}

#[test]
fn test_wasmtime_disabled_f64_div() {
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
    assert_eq!(trap, wasmtime::Trap::DisabledOpcode);
}

#[test]
fn test_wasmtime_f32_const() {
    let wat = r#"
        (module
            (func (export "main") (result f32)
                f32.const 9.0
            )
        )
    "#;
    let result = run_main::<f32>(wat).unwrap();
    assert_eq!(result, 9.0);
}

#[test]
fn test_wasmtime_f64_const() {
    let wat = r#"
        (module
            (func (export "main") (result f64)
                f64.const 9.0
            )
        )
    "#;
    let result = run_main::<f64>(wat).unwrap();
    assert_eq!(result, 9.0);
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
    let strategy = StrategyDefinition::Wasmtime {
        module: compile_wasmtime_module(
            CompilationConfig::default().with_consume_fuel(false),
            &wasm_binary,
        )
        .unwrap(),
    };
    for _ in 0..10_001 {
        strategy
            .default_executor()
            .unwrap()
            .execute("main", &[], &mut [])
            .unwrap();
    }
}

#[test]
fn test_fib_bench() {
    let strategy = StrategyDefinition::Wasmtime {
        module: compile_wasmtime_module(CompilationConfig::default(), FIB_WASM).unwrap(),
    };
    // it fails on iter number 32'165...
    for _ in 0..32_165 {
        let mut executor = strategy
            .create_executor(
                Arc::new(ImportLinker::default()),
                (),
                always_failing_syscall_handler,
                Some(1_000_000),
            )
            .unwrap();
        let mut result = [Value::I32(0)];
        executor
            .execute("main", &[Value::I32(43)], &mut result)
            .unwrap();
        core::hint::black_box(result);
    }
}

#[test]
fn test_instance_reuse() {
    use wasmtime::*;
    let mut config = Config::new();
    let pooling_allocator = PoolingAllocationConfig::default();
    config.allocation_strategy(InstanceAllocationStrategy::Pooling(pooling_allocator));
    let engine = Engine::new(&config).unwrap();
    let module = Module::new(&engine, FIB_WASM).unwrap();
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
