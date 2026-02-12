use rwasm::{
    for_each_strategy, CompilationConfig, ExecutionEngine, ImportLinker, RwasmModule, RwasmStore,
    Value,
};

#[test]
fn test_fib() {
    let wasm_binary = include_bytes!("../benchmarks/lib.wasm");
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_consume_fuel(false);
    for_each_strategy(
        |strategy| {
            let mut result = [Value::I32(0); 1];
            strategy
                .default_executor()
                .unwrap()
                .execute("main", &[Value::I32(43)], &mut result)?;
            assert_eq!(result[0].i32().unwrap(), 433494437);
            Ok(())
        },
        config,
        wasm_binary,
    )
    .unwrap();
}

#[test]
fn test_i64_load8_s() {
    let wasm_binary = wat::parse_str(
        r#"
(module
  (memory 1)
  (data (i32.const 0) "abcdefghijklmnopqrstuvwxyz")

  (func (export "8s_good1") (param $i i32) (result i64)
    (i64.load8_s offset=0 (local.get $i))                   ;; 97 'a'
  )
)
"#,
    )
    .unwrap();
    let config = CompilationConfig::default()
        .with_entrypoint_name("8s_good1".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let instance = ImportLinker::default()
        .instantiate(&mut store, ExecutionEngine::new(), rwasm_module)
        .unwrap();
    let mut result = [Value::I64(0); 1];
    instance
        .execute(&mut store, &[Value::I32(0)], &mut result)
        .unwrap();
    assert_eq!(result[0].i64().unwrap(), 97);
}

#[test]
fn test_i64_load() {
    let wasm_binary = wat::parse_str(
        r#"
(module
  (memory 1)
  (data (i32.const 0) "abcdefghijklmnopqrstuvwxyz")
  (func (export "64_good1") (param $i i32) (result i64)
    (i64.load offset=0 (local.get $i))                     ;; 0x6867666564636261 'abcdefgh'
  )
)
"#,
    )
    .unwrap();
    let config = CompilationConfig::default()
        .with_entrypoint_name("64_good1".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let instance = ImportLinker::default()
        .instantiate(&mut store, ExecutionEngine::new(), rwasm_module)
        .unwrap();
    let mut result = [Value::I64(0); 1];
    instance
        .execute(&mut store, &[Value::I32(0)], &mut result)
        .unwrap();
    assert_eq!(result[0].i64().unwrap(), 0x6867666564636261);
}

#[test]
fn test_bulk_bench() {
    let wasm_binary = wat::parse_str(
        r#"
(module
    (memory 8 8)

    ;; The maximum amount of bytes to process per iteration.
    (global $MAX_N i64 (i64.const 250000))

    (func (export "run") (param $N i64) (result i64)
        (local $i i32)
        (local $n i32)
        (if (i64.gt_u (local.get $N) (global.get $MAX_N))
            (then (unreachable))
        )
        (local.set $i (i32.const 0))
        (local.set $n (i32.wrap_i64 (local.get $N)))
        (block $break
            (loop $continue
                ;; if i >= N: break
                (br_if $break
                    (i32.ge_u (local.get $i) (local.get $n))
                )
                ;; mem[0..n].fill(i)
                (memory.fill
                    (i32.const 0) ;; dst
                    (local.get $i) ;; value
                    (local.get $n) ;; len
                )
                ;; mem[n..n*2].copy(mem[0..n])
                (memory.copy
                    (local.get $i) ;; dst
                    (i32.const 0) ;; src
                    (local.get $n) ;; len
                )
                ;; i += 1
                (local.set $i (i32.add (local.get $i) (i32.const 1)))
                (br $continue)
            )
        )
        (i64.const 0)
    )
)"#,
    )
    .unwrap();
    let config = CompilationConfig::default()
        .with_entrypoint_name("run".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let instance = ImportLinker::default()
        .instantiate(&mut store, ExecutionEngine::new(), rwasm_module)
        .unwrap();
    let mut result = [Value::I64(0); 1];
    instance
        .execute(&mut store, &[Value::I64(5000)], &mut result)
        .unwrap();
}

#[test]
fn test_reduce_binary() {}

#[test]
fn test_multi_value_params() {
    let wasm_binary = wat::parse_str(
        r#"
(module
  (type (;0;) (func (result i64 i64)))
  (global (;0;) (mut i32) i32.const 1000)
  (export "\u{a}++" (func 0))
  (func (export "main") (;0;) (type 0) (result i64 i64)
    global.get 0
    i32.eqz
    if ;; label = @1
      unreachable
    end
    global.get 0
    i32.const 1
    i32.sub
    global.set 0
    i64.const 2251799813685248
    i64.const 0
  )
)
"#,
    )
    .unwrap();
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let instance = ImportLinker::default()
        .instantiate(&mut store, ExecutionEngine::new(), rwasm_module)
        .unwrap();
    let mut result = [Value::I64(0), Value::I64(0)];
    instance.execute(&mut store, &[], &mut result).unwrap();
    assert_eq!(result[0].i64().unwrap(), 2251799813685248);
    assert_eq!(result[1].i64().unwrap(), 0);
}
