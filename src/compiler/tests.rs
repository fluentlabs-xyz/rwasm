use crate::{CompilationConfig, ExecutionEngine, RwasmModule, Store};

#[test]
fn test_fib() {
    let wasm_binary = include_bytes!("../../benchmarks/lib.wasm");
    let config = CompilationConfig::default().with_entrypoint_name("main".into());
    let (rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = Store::<()>::default();
    let mut engine = ExecutionEngine::new();
    engine.value_stack().push(43.into());
    engine.execute(&mut store, &rwasm_module).unwrap();
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
    let mut engine = ExecutionEngine::new();
    engine.value_stack().push(0x132.into());
    engine.value_stack().push(0.into());
    engine.execute(&mut store, &rwasm_module).unwrap();
    let result = engine.value_stack().pop();
    let result = engine.value_stack().pop();
    // assert_eq!(result.as_i64(), 433494437);
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
    let mut store = Store::<()>::default();
    let mut engine = ExecutionEngine::new();
    engine.value_stack().push(5000.into());
    engine.value_stack().push(0.into());
    engine.execute(&mut store, &rwasm_module).unwrap();
    let result = engine.value_stack().pop();
    let result = engine.value_stack().pop();
}
