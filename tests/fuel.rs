use rwasm::{CompilationConfig, ExecutionEngine, FuelCosts, RwasmModule, RwasmStore, Store, Value};

#[test]
fn test_locals_consume_fuel() {
    let fuel_limit = 9999;
    let basic_fuel_consumption = 2;

    let mut test_cases: &mut [(usize, usize)] = &mut [
        (16, 0),
        (32, 0),
        (1000, 0),
        // (1001, 0), //function params size is out of bounds (RwasmModule::compile fails)
    ];
    test_cases
        .iter_mut()
        .for_each(|(p_count, fuel)| *fuel = FuelCosts::fuel_for_locals(*p_count as u32) as usize);
    for (locals_count, fuel_cost) in test_cases.iter().cloned() {
        let mut wat_params: Vec<&str> = Vec::with_capacity(locals_count);
        for _ in 0..locals_count {
            wat_params.push("i32")
        }
        let wat_params_str = wat_params.join(" ");
        let wat_str = format!(
            r#"
            (module
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
            Some(fuel_limit - basic_fuel_consumption - fuel_cost as u64),
            remaining_fuel
        );
        assert_eq!(result[0].i32().unwrap(), 111);
    }
}
