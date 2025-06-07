use crate::{CompilationConfig, ExecutionEngine, RwasmModule, Store};

#[test]
fn test_fib() {
    let wasm_binary = include_bytes!("../../benchmarks/lib.wasm");
    let config = CompilationConfig::default().with_entrypoint_name("main".into());
    let (rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = Store::<()>::default();
    let mut engine = ExecutionEngine::new(&mut store);
    engine.value_stack().push(43.into());
    engine.execute(&rwasm_module).unwrap();
    let result = engine.value_stack().pop();
    assert_eq!(result.as_i64(), 433494437);
}

#[test]
fn test_block() {
    let wasm_binary = wat::parse_str(
        r#"
(module
  (func $const-i32 (result i32) (i32.const 0x132))
  (func (export "as-select-first") (result i32)
    (select (call $const-i32) (i32.const 2) (i32.const 3))
  )
)"#,
    )
    .unwrap();
    let config = CompilationConfig::default()
        .with_entrypoint_name("as-select-first".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = Store::<()>::default();
    let mut engine = ExecutionEngine::new(&mut store);
    engine.value_stack().push(0x132.into());
    engine.value_stack().push(0.into());
    engine.execute(&rwasm_module).unwrap();
    let result = engine.value_stack().pop();
    let result = engine.value_stack().pop();
    // assert_eq!(result.as_i64(), 433494437);
}
