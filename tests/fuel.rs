use rwasm::{CompilationConfig, ExecutionEngine, RwasmModule, RwasmStore, StoreTr, Value};
use rwasm_fuel_policy::FuelCosts;

#[test]
fn test_locals_consume_fuel() {
    let fuel_limit = 9999;
    let basic_fuel_consumption = 2;

    let test_cases: &mut [(usize, usize)] = &mut [
        (0, 0),
        (1, 0),
        (3, 0),
        (16, 0),
        (32, 0),
        (1000, 0),
        // locals_count>1000 -> 'function params size is out of bounds' (RwasmModule::compile fails)
    ];
    test_cases
        .iter_mut()
        .for_each(|(p_count, fuel)| *fuel = FuelCosts::fuel_for_locals(*p_count as u32) as usize);
    for (locals_count, fuel_cost) in test_cases.iter().cloned() {
        let mut wat_params: Vec<&str> = Vec::with_capacity(locals_count);
        for _ in 0..locals_count {
            wat_params.push("i32")
        }
        let params_or_locals_str = wat_params.join(" ");
        let params_wat_str = format!(
            r#"
            (module
              (func (export "entry") (param {params_or_locals_str}) (result i32)
                (i32.const 111)
              )
            )
        "#
        );
        let locals_wat_str = format!(
            r#"
            (module
              (func (export "entry") (result i32)
                (local {params_or_locals_str})
                (i32.const 111)
              )
            )
        "#
        );
        let params_wasm_binary = wat::parse_str(params_wat_str).unwrap();
        let locals_wasm_binary = wat::parse_str(locals_wat_str).unwrap();
        let config = CompilationConfig::default()
            .with_entrypoint_name("entry".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_consume_fuel(true);
        let (params_rwasm_module, _) =
            RwasmModule::compile(config.clone(), &params_wasm_binary).unwrap();
        println!("params_rwasm_module:{}", params_rwasm_module);
        let (locals_rwasm_module, _) = RwasmModule::compile(config, &locals_wasm_binary).unwrap();
        println!("locals_rwasm_module:{}", locals_rwasm_module);
        let engine = ExecutionEngine::new();
        let mut result = [Value::I32(0); 1];
        let mut params_values = Vec::with_capacity(locals_count);
        for _ in 0..locals_count {
            params_values.push(Value::I32(0));
        }
        for (i, module) in [params_rwasm_module, locals_rwasm_module]
            .iter()
            .enumerate()
        {
            let mut store = RwasmStore::<()>::default();
            store.reset_fuel(fuel_limit);
            engine
                .execute(
                    &mut store,
                    module,
                    if i == 0 { &params_values } else { &[] },
                    &mut result,
                )
                .unwrap();
            let remaining_fuel = store.remaining_fuel();
            assert_eq!(
                Some(fuel_limit - basic_fuel_consumption - fuel_cost as u64),
                remaining_fuel,
                "module {} failed",
                i
            );
            assert_eq!(result[0].i32().unwrap(), 111);
        }
    }
}

#[test]
fn test_memory_fill_fuel_scales_with_length() {
    let wasm_binary = wat::parse_str(
        r#"
        (module
          (memory (export "memory") 1)
          (func (export "entry") (param i32)
            i32.const 1
            memory.grow
            drop
            i32.const 0
            i32.const 7
            local.get 0
            memory.fill
          )
        )
        "#,
    )
    .unwrap();
    let config = CompilationConfig::default()
        .with_entrypoint_name("entry".into())
        .with_consume_fuel(true);
    let (module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    let engine = ExecutionEngine::new();
    let fuel_limit = 100_000;

    let consumed_for_len = |len| {
        let mut store = RwasmStore::<()>::default();
        let mut result = [];
        store.reset_fuel(fuel_limit);
        engine
            .execute(&mut store, &module, &[Value::I32(len)], &mut result)
            .unwrap();
        fuel_limit - store.remaining_fuel().unwrap()
    };

    assert!(consumed_for_len(128) > consumed_for_len(1));
}

#[test]
fn test_table_bulk_ops_compile_with_fuel_metering() {
    let wasm_binary = wat::parse_str(
        r#"
        (module
          (type $t (func))
          (func $f)
          (table 4 funcref)
          (elem funcref (ref.func $f))
          (func (export "entry")
            i32.const 0
            i32.const 0
            i32.const 1
            table.init 0 0

            i32.const 1
            i32.const 0
            i32.const 1
            table.copy 0 0

            i32.const 2
            ref.func $f
            i32.const 1
            table.fill 0
          )
        )
        "#,
    )
    .unwrap();
    let config = CompilationConfig::default()
        .with_entrypoint_name("entry".into())
        .with_consume_fuel(true);

    RwasmModule::compile(config, &wasm_binary).unwrap();
}
