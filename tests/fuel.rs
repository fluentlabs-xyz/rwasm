use rwasm::{
    always_failing_syscall_handler, for_each_strategy, CompilationConfig, ExecutionEngine,
    FuelConfig, FuelCosts, ImportLinker, RwasmModule, RwasmStore, Store, Value,
};
use std::sync::Arc;

#[test]
fn test_locals_consume_fuel1() {
    let wasm_binary = include_bytes!("../benchmarks/lib.wasm");
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_consume_fuel(true);
    for_each_strategy(
        |strategy| {
            let mut store = strategy.create_store(
                Arc::new(ImportLinker::default()),
                (),
                always_failing_syscall_handler,
                FuelConfig::default().with_fuel_limit(214),
            );
            let mut result = [Value::I32(0); 1];
            strategy.execute(&mut store, "main", &[Value::I32(43)], &mut result)?;
            let remaining_fuel = store.remaining_fuel();
            assert_eq!(Some(0), remaining_fuel);
            assert_eq!(result[0].i32().unwrap(), 433494437);
            Ok(())
        },
        config,
        wasm_binary,
    )
    .unwrap();
}

#[test]
fn test_locals_consume_fuel() {
    let fuel_limit = 999;
    let basic_fuel_consumption = 2;

    let test_cases: &[(usize, usize)] = &[
        (16, FuelCosts::fuel_for_locals(16) as usize),
        (32, FuelCosts::fuel_for_locals(32) as usize),
        // (64, FuelCosts::fuel_for_locals(64) as usize), // stack overflow
        // (4096, FuelCosts::fuel_for_locals(4096) as usize), // stack overflow
    ];
    for (locals_count, fuel_cost_for_locals) in test_cases.iter().cloned() {
        let mut wat_params: Vec<&str> = Vec::with_capacity(locals_count);
        for _ in 0..locals_count {
            wat_params.push("i32")
        }
        let wat_params_str = wat_params.join(" ");
        let wat_str = format!(
            r#"
            (module
              (memory 1)
              (data (i32.const 0) "abcdefghijklmnopqrstuvwxyz")

              (func (export "custom") (param {wat_params_str}) (result i32)
                (i32.const 111)
              )
            )
        "#
        );
        let wasm_binary = wat::parse_str(wat_str).unwrap();
        let config = CompilationConfig::default()
            .with_entrypoint_name("custom".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_consume_fuel(true);
        let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
        println!("{}", rwasm_module);
        let mut store = RwasmStore::<()>::default();
        store.set_fuel(Some(fuel_limit));
        let engine = ExecutionEngine::new();
        let mut result = [Value::I32(0); 1];
        let mut params_values = Vec::with_capacity(locals_count);
        for _ in 0..locals_count {
            params_values.push(Value::I32(0));
        }
        engine
            .execute(&mut store, &rwasm_module, &params_values, &mut result)
            .unwrap();
        let remaining_fuel = store.remaining_fuel();
        assert_eq!(
            Some(fuel_limit - basic_fuel_consumption - fuel_cost_for_locals as u64),
            remaining_fuel
        );
        assert_eq!(result[0].i32().unwrap(), 111);
    }
}
