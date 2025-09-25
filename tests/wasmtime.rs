use rwasm::{compile_wasmtime_module, CompilationConfig, Strategy};

#[test]
fn test_10001_instances_in_a_row() {
    let wasm_binary = wat::parse_str(
        r#"
(module
  (func (export "main")
    (i32.const 100)
    (i32.const 20)
    (i32.const 3)
    (i32.add)
    (i32.add)
    (drop)
  )
)
"#,
    )
    .unwrap();
    let strategy = Strategy::Wasmtime {
        module: compile_wasmtime_module(CompilationConfig::default(), &wasm_binary).unwrap(),
    };
    for _ in 0..10_000 {
        let mut store = strategy.empty_store();
        strategy
            .execute(&mut store, "main", &[], &mut [], None)
            .unwrap();
    }
    let mut store = strategy.empty_store();
    strategy
        .execute(&mut store, "main", &[], &mut [], None)
        .unwrap();
}
