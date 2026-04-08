use rwasm::{
    always_failing_syscall_handler, for_each_strategy, CompilationConfig, RwasmModule, StoreTr,
    Value, F64,
};

fn run_rwasm_vs_wasmtime_fuel_check(wasm_binary: &[u8], params: &[Value], result: &mut [Value]) {
    let config = CompilationConfig::default()
        .with_entrypoint_name("".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_allow_start_section(true)
        .with_consume_fuel(true)
        .with_consume_fuel_for_params_and_locals(false)
        .with_allow_func_ref_function_types(false)
        .with_max_allowed_memory_pages(4096);
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
                    Some(4096),
                )
                .unwrap();
            let fuel_before = executor.remaining_fuel().unwrap();
            let trap_code = executor.execute("", params, result).err();
            let fuel_consumed = fuel_before - executor.remaining_fuel().unwrap();
            let memory_snapshot = executor.snapshot_memory().len();
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
        &mut [],
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
        &mut [],
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
        &mut [],
    );
}

#[test]
fn test_fuel_mismatch_3() {
    run_rwasm_vs_wasmtime_fuel_check(
        &wat::parse_str(
            r#"
(module
  (type (;0;) (func))
  (type (;1;) (func))
  (table (;0;) 0 externref)
  (memory (;0;) 0)
  (global (;0;) (mut i32) i32.const 621216000)
  (export "" (func 0))
  (export "1" (func 1))
  (export "2" (table 0))
  (export "memory" (memory 0))
  (export "4" (global 0))
  (func (;0;) (type 1)
    (local f32 i64 i32)
  )
  (func (;1;) (type 0))
)
"#,
        )
        .unwrap(),
        &[],
        &mut [],
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
        &mut [],
    );
}

#[test]
fn test_fuel_bad_memory() {
    run_rwasm_vs_wasmtime_fuel_check(
        &wat::parse_str(
            r#"
(module
  (type (;0;) (func (param i32 i32 i32 i32 i32 i32 i32 i32)))
  (table (;0;) 996180 996225 funcref)
  (memory (;0;) 1543)
  (global (;0;) f32 f32.const -0x1.0b58fcp+127 (;=-177682950000000000000000000000000000000;))
  (global (;1;) (mut i32) i32.const 7936)
  (export "" (func 0))
  (export "1" (func 1))
  (export "2" (table 0))
  (export "memory" (memory 0))
  (export "4" (global 0))
  (export "5" (global 1))
  (func (;0;) (type 0) (param i32 i32 i32 i32 i32 i32 i32 i32))
  (func (;1;) (type 0) (param i32 i32 i32 i32 i32 i32 i32 i32))
)
"#,
        )
        .unwrap(),
        &[const { Value::I32(0) }; 8],
        &mut [],
    );
}

// This test has different behavior for wasmtime because of it relies on
// stack only for recursive calls, but rwasm checks depth level.
#[test]
#[ignore]
fn wasmtime_stack_overflow_behaviour_mismatch() {
    let mut result = vec![Value::I32(0)];
    run_rwasm_vs_wasmtime_fuel_check(
        &wat::parse_str(
            r#"
(module
  (type (;0;) (func (result i32)))
  (type (;1;) (func (result f64)))
  (table (;0;) 274 funcref)
  (memory (;0;) 449)
  (export "" (func 0))
  (export "1" (table 0))
  (export "memory" (memory 0))
  (func (;0;) (type 1) (result f64)
    call 0
    i32.const -1936946036
    f32.convert_i32_s
    f32.const -0x1.191918p-102 (;=-0.00000000000000000000000000000021655004;)
    i32.trunc_sat_f32_s
    f64.convert_i32_s
    i32.const 0
    unreachable
  )
)
"#,
        )
        .unwrap(),
        &[],
        &mut result,
    );
}

#[test]
fn test_rwasm_table_size_fatal() {
    let mut result = vec![Value::I64(0), Value::I64(0), Value::I32(0)];
    run_rwasm_vs_wasmtime_fuel_check(
        &wat::parse_str(
            r#"
(module
  (type (;0;) (func (result externref f64 f32)))
  (type (;1;) (func))
  (type (;2;) (func (result f64 f64 f32)))
  (type (;3;) (func))
  (type (;4;) (func (result f64 i32)))
  (table (;0;) 4473 476776 externref)
  (memory (;0;) 0)
  (export "" (func 0))
  (export "1" (func 1))
  (export "2" (table 0))
  (export "memory" (memory 0))
  (elem (;0;) externref)
  (func (;0;) (type 2) (result f64 f64 f32)
    (local i32)
    local.get 0
    table.size 0
    unreachable
  )
  (func (;1;) (type 2) (result f64 f64 f32)
    unreachable
  )
  (data (;0;) "\df\db\df\00")
)
"#,
        )
        .unwrap(),
        &[],
        &mut result,
    );
}

#[test]
fn test_bad_memory() {
    run_rwasm_vs_wasmtime_fuel_check(
        &wat::parse_str(
            r#"
(module
  (type (;0;) (func))
  (table (;0;) 7 funcref)
  (memory (;0;) 11)
  (export "L\u{c}$" (func 0))
  (export "" (func 1))
  (export "2" (table 0))
  (export "memory" (memory 0))
  (func (;0;) (type 0))
  (func (;1;) (type 0))
  (data (;0;) (i32.const 505) "L")
)
"#,
        )
        .unwrap(),
        &[],
        &mut [],
    );
}

#[test]
fn unresolved_table_segment() {
    run_rwasm_vs_wasmtime_fuel_check(
        &wat::parse_str(
            r#"
(module
  (type (;0;) (func (param f64)))
  (table (;0;) 0 externref)
  (memory (;0;) 11 19286)
  (export "" (func 0))
  (export "|||||||" (table 0))
  (export "memory" (memory 0))
  (func (;0;) (type 0) (param f64)
    f64.const 0x1.c7c7c7c7c7c7cp+968 (;=4441723041807660500000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000;)
    table.size 0
    i64.extend_i32_s
    f32.const 0x1.f8f8f8p+121 (;=5243934600000000000000000000000000000;)
    unreachable
  )
)
"#,
        )
        .unwrap(),
        &[Value::F64(F64::from(0))],
        &mut [],
    );
}

#[test]
fn memory_out_of_bounds() {
    run_rwasm_vs_wasmtime_fuel_check(
        &wat::parse_str(
            r#"
(module
  (type (;0;) (func))
  (table (;0;) 159 586 externref)
  (memory (;0;) 2548 50607)
  (global (;0;) (mut f32) f32.const -0x1.777776p-8 (;=-0.0057291663;))
  (global (;1;) (mut f32) f32.const 0x1.d8d8d8p+89 (;=1143274000000000000000000000;))
  (export "" (func 0))
  (export "llllllllllllllllllllll" (func 1))
  (export "2" (table 0))
  (export "memory" (memory 0))
  (export "4" (global 0))
  (export "5" (global 1))
  (start 1)
  (elem (;0;) declare externref (ref.null extern) (ref.null extern) (ref.null extern) (ref.null extern) (ref.null extern) (ref.null extern) (ref.null extern) (ref.null extern) (ref.null extern) (ref.null extern) (ref.null extern))
  (func (;0;) (type 0))
  (func (;1;) (type 0))
  (data (;0;) "\00")
  (data (;1;) (i32.const 0) "\00")
)
"#,
        )
        .unwrap(),
        &[],
        &mut [],
    );
}
