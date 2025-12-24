use rwasm::{
    always_failing_syscall_handler, compile_wasmi_module, CompilationConfig, FuelConfig,
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
    let strategy = Strategy::Wasmi {
        module: compile_wasmi_module(CompilationConfig::default(), &wasm_binary).unwrap(),
    };
    for _ in 0..10_000 {
        let mut store = strategy.create_store(
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            FuelConfig::default().with_fuel_limit(u64::MAX),
        );
        strategy.execute(&mut store, "main", &[], &mut []).unwrap();
    }
    let mut store = strategy.create_store(
        Arc::new(ImportLinker::default()),
        (),
        always_failing_syscall_handler,
        FuelConfig::default().with_fuel_limit(u64::MAX),
    );
    strategy.execute(&mut store, "main", &[], &mut []).unwrap();
}

#[test]
fn test_fib_bench() {
    let wasm_binary = include_bytes!("../benchmarks/lib.wasm");
    let strategy = Strategy::Wasmi {
        module: compile_wasmi_module(CompilationConfig::default(), wasm_binary).unwrap(),
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
