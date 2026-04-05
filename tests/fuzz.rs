use rwasm::{
    always_failing_syscall_handler, for_each_strategy, CompilationConfig, RwasmModule, StoreTr,
    Value,
};

fn run_rwasm_vs_wasmtime_fuel_check(wasm_binary: &[u8], params: &[Value]) {
    let config = CompilationConfig::default()
        .with_entrypoint_name("".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_allow_start_section(true)
        .with_consume_fuel(true)
        .with_consume_fuel_for_params_and_locals(false)
        .with_allow_func_ref_function_types(false);
    let (module, _) = RwasmModule::compile(config.clone(), &wasm_binary).unwrap();
    println!("{}", module);
    let fuel_consumed = for_each_strategy(
        |strategy| {
            let mut executor = strategy
                .create_executor(
                    Default::default(),
                    (),
                    always_failing_syscall_handler,
                    Some(1_000_000),
                    None,
                )
                .unwrap();
            let fuel_before = executor.remaining_fuel().unwrap();
            let trap_code = executor.execute("", params, &mut []).err();
            let fuel_consumed = fuel_before - executor.remaining_fuel().unwrap();
            let memory_snapshot = executor.snapshot_memory();
            Ok((trap_code, fuel_consumed, memory_snapshot))
        },
        config,
        &wasm_binary,
    )
    .unwrap();
    let value_should_be = fuel_consumed[0].clone();
    for x in fuel_consumed {
        assert_eq!(x, value_should_be);
    }
}

#[test]
fn test_fuel_mismatch_locals() {
    run_rwasm_vs_wasmtime_fuel_check(
        &wat::parse_str(
            r#"
(module
  (type (;0;) (func (param i32)))
  (table (;0;) 0 funcref)
  (memory (;0;) 0)
  (export "" (func 0))
  (export "1" (table 0))
  (export "memory" (memory 0))
  (func (;0;) (type 0) (param i32))
)
"#,
        )
        .unwrap(),
        &[Value::I32(0)],
    );
}

#[test]
fn test_fuel_mismatch() {
    run_rwasm_vs_wasmtime_fuel_check(
        &wat::parse_str(
            r#"
(module
  (type (;0;) (func))
  (table (;0;) 0 funcref)
  (memory (;0;) 0)
  (export "" (func 0))
  (export "1" (table 0))
  (export "memory" (memory 0))
  (start 0)
  (func (;0;) (type 0))
)
"#,
        )
        .unwrap(),
        &[],
    );
}

#[test]
fn test_fuel_mismatch_2() {
    run_rwasm_vs_wasmtime_fuel_check(
        &wat::parse_str(
            r#"
(module
  (type (;0;) (func))
  (table (;0;) 2076 funcref)
  (table (;1;) 2795 264955 externref)
  (memory (;0;) 0 65341)
  (global (;0;) i32 i32.const 1487)
  (export "" (func 0))
  (export "1" (table 0))
  (export "2" (table 1))
  (export "memory" (memory 0))
  (export "4" (global 0))
  (func (;0;) (type 0))
)
"#,
        )
        .unwrap(),
        &[],
    );
}

#[test]
fn test_fuel_memory_oom() {
    run_rwasm_vs_wasmtime_fuel_check(
        &wat::parse_str(
            r#"
(module
  (type (;0;) (func))
  (table (;0;) 610 funcref)
  (memory (;0;) 1000 30470)
  (global (;0;) i32 i32.const 0 i32.const 0 i32.mul)
  (export "" (func 0))
  (export "1" (table 0))
  (export "memory" (memory 0))
  (export "3" (global 0))
  (func (;0;) (type 0))
)
"#,
        )
        .unwrap(),
        &[],
    );
}

#[test]
#[ignore]
fn test_wasmtime_compilation_failure() {
    run_rwasm_vs_wasmtime_fuel_check(
        &wat::parse_str(
            r#"
(module
  (type (;0;) (func))
  (table (;0;) 298 funcref)
  (memory (;0;) 489)
  (export "" (func 0))
  (export "1" (table 0))
  (export "memory" (memory 0))
  (func (;0;) (type 0)
    table.size 0
    f32.convert_i32_s
    i32.const -1903260018
    f32.convert_i32_s
    unreachable
    unreachable
  )
)
"#,
        )
        .unwrap(),
        &[],
    );
}
