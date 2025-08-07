use crate::cost_model::DefaultCostModel;
use crate::{GasInjector, GasInjectorConfig};

fn test_type_injection(wat: &str) -> String {
    let wasm_binary: Vec<u8> = wat::parse_str(wat).unwrap();
    let config = GasInjectorConfig {
        charge_gas_func_name: ("env", "_charge_gas"),
    };
    let gas_injector = GasInjector::new(config, DefaultCostModel::default());
    let new_wasm = gas_injector.inject(&wasm_binary).unwrap();
    let new_wat = wasmprinter::print_bytes(&new_wasm).unwrap();
    println!("{}", new_wat);
    new_wat
}

#[test]
fn test_inject_type_into_module() {
    let new_wat = test_type_injection(
        r#"(module
  (type (;0;) (func (param i32) (result i32)))
  (export "f" (func $f))
  (func $f (;0;) (type 0) (param i32) (result i32)
    local.get 0
  )
)
"#,
    );
    assert_eq!(
        new_wat,
        r#"(module
  (type (;0;) (func (param i32) (result i32)))
  (type (;1;) (func (param i32)))
  (import "env" "_charge_gas" (func (;0;) (type 1)))
  (export "f" (func 1))
  (func (;1;) (type 0) (param i32) (result i32)
    local.get 0
    i32.const 1
    call 0
  )
)
"#
    );
}

#[test]
fn test_type_already_presented() {
    let new_wat = test_type_injection(
        r#"(module
  (type (;0;) (func (param i32) (result i32)))
  (type (;1;) (func (param i32)))
  (export "f" (func $f))
  (func $f (;0;) (type 0) (param i32) (result i32)
    local.get 0
  )
)
"#,
    );
    assert_eq!(
        new_wat,
        r#"(module
  (type (;0;) (func (param i32) (result i32)))
  (type (;1;) (func (param i32)))
  (import "env" "_charge_gas" (func (;0;) (type 1)))
  (export "f" (func 1))
  (func (;1;) (type 0) (param i32) (result i32)
    local.get 0
    i32.const 1
    call 0
  )
)
"#
    );
}

#[test]
fn test_inject_a_second_distinct_type() {
    let new_wat = test_type_injection(
        r#"(module
  (type (func (param i32) (result i32)))
  (func (type 0) (param i32) (result i32)
    local.get 0)
  (func $g (param i64) (result f32)
    f32.const 1.0)
  (export "g" (func $g))
)
"#,
    );
    assert_eq!(
        new_wat,
        r#"(module
  (type (;0;) (func (param i32) (result i32)))
  (type (;1;) (func (param i64) (result f32)))
  (type (;2;) (func (param i32)))
  (import "env" "_charge_gas" (func (;0;) (type 2)))
  (export "g" (func 2))
  (func (;1;) (type 0) (param i32) (result i32)
    local.get 0
    i32.const 1
    call 0
  )
  (func (;2;) (type 1) (param i64) (result f32)
    f32.const 0x1p+0 (;=1;)
    i32.const 1
    call 0
  )
)
"#
    );
}
