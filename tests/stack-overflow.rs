use rwasm::{CompilationConfig, ExecutionEngine, RwasmModule, RwasmStore, Value};

#[test]
fn test_stack_overflow_number_of_params() -> anyhow::Result<()> {
    let wat = r#"
(module
  (type (;0;) (func (param i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i32)))
  (func (;0;) (export "main") (type 0) (param i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i32)
    global.get 0
    i32.eqz
    if  ;; label = @1
      unreachable
    end
    global.get 0
    i32.const 1
    i32.sub
    global.set 0)
  (global (;0;) (mut i32) (i32.const 1000))
  (export "" (func 0)))
    "#;
    let wasm_binary = wat::parse_str(wat)?;
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let engine = ExecutionEngine::new();
    let params = vec![Value::I32(0); 37];
    let mut result = [Value::I64(0); 0];
    engine.execute(&mut store, &rwasm_module, &params, &mut result)?;
    Ok(())
}

#[test]
fn test_stack_overflow_32_params() -> anyhow::Result<()> {
    let wat = r#"
(module
  (type (;0;) (func (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)))
  (func (;0;) (export "main") (type 0) (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)
    global.get 0
    i32.eqz
    if  ;; label = @1
      unreachable
    end
    global.get 0
    i32.const 1
    i32.sub
    global.set 0)
  (global (;0;) (mut i32) (i32.const 1000))
  (export "" (func 0)))
    "#;
    let wasm_binary = wat::parse_str(wat)?;
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let engine = ExecutionEngine::new();
    let params = vec![Value::I32(0); 37];
    let mut result = [Value::I64(0); 0];
    engine.execute(&mut store, &rwasm_module, &params, &mut result)?;
    Ok(())
}

#[test]
fn test_stack_overflow_33_params() -> anyhow::Result<()> {
    let wat = r#"
(module
  (type (;0;) (func (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)))
  (func (;0;) (export "main") (type 0) (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)
    global.get 0
    i32.eqz
    if  ;; label = @1
      unreachable
    end
    global.get 0
    i32.const 1
    i32.sub
    global.set 0)
  (global (;0;) (mut i32) (i32.const 1000))
  (export "" (func 0)))
    "#;
    let wasm_binary = wat::parse_str(wat)?;
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let engine = ExecutionEngine::new();
    // 37 stands for x18 i64 (36) + x1 i32 (1) = 36+1=37
    let params = vec![Value::I32(0); 37];
    let mut result = [Value::I64(0); 0];
    engine.execute(&mut store, &rwasm_module, &params, &mut result)?;
    Ok(())
}
