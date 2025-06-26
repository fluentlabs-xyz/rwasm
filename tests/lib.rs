use rwasm::{CompilationConfig, ExecutionEngine, RwasmModule, RwasmStore};

#[test]
fn test_fib() {
    let wasm_binary = include_bytes!("../benchmarks/lib.wasm");
    let config = CompilationConfig::default().with_entrypoint_name("main".into());
    let (rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let mut engine = ExecutionEngine::new();
    engine.value_stack().push(43.into());
    engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap();
    let result = engine.value_stack().pop();
    assert_eq!(result.as_i64(), 433494437);
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
    let mut engine = ExecutionEngine::new();
    engine.value_stack().push(0.into());
    engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap();
    let result = engine.value_stack().pop();
    assert_eq!(result.as_i32(), 0);
    let result = engine.value_stack().pop();
    assert_eq!(result.as_i32(), 97);
    println!("{:?}", engine.value_stack().as_slice());
    assert!(engine.value_stack().as_slice().is_empty());
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
    let mut engine = ExecutionEngine::new();
    engine.value_stack().push(0.into());
    engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap();
    let hi = engine.value_stack().pop().to_bits() as u64;
    let lo = engine.value_stack().pop().to_bits() as u64;
    assert!(engine.value_stack().as_slice().is_empty());
    let value = (hi << 32) | lo;
    assert_eq!(value, 0x6867666564636261);
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
    let mut engine = ExecutionEngine::new();
    engine.value_stack().push(5000.into());
    engine.value_stack().push(0.into());
    engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap();
    let _result = engine.value_stack().pop();
    let _result = engine.value_stack().pop();
}
