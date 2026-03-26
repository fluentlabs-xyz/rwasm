use rwasm::{CompilationConfig, ExecutionEngine, ImportLinker, RwasmModule, RwasmStore, Value};

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
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary)?;
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let instance =
        ImportLinker::default().instantiate(&mut store, ExecutionEngine::new(), rwasm_module)?;
    let mut params = vec![Value::I64(0); 18];
    params.push(Value::I32(0));
    let mut result = [];
    instance.execute(&mut store, &params, &mut result)?;
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
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary)?;
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let instance =
        ImportLinker::default().instantiate(&mut store, ExecutionEngine::new(), rwasm_module)?;
    let params = vec![Value::I32(0); 32];
    let mut result = [];
    instance.execute(&mut store, &params, &mut result)?;
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
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary)?;
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let instance =
        ImportLinker::default().instantiate(&mut store, ExecutionEngine::new(), rwasm_module)?;
    let params = vec![Value::I32(0); 33];
    let mut result = [];
    instance.execute(&mut store, &params, &mut result)?;
    Ok(())
}
