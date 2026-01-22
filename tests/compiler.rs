use rwasm::{CompilationConfig, RwasmModule};

#[test]
fn test_i64_type_split_into_2_x_i32_bug() {
    const WAT: &str = r#"
        (module
          (type (;0;) (func))
          (func (;0;) (type 0)
            i64.const 0
            i64.const 0
            i32.const 0
            if (param i64 i64)
              drop
              i64.const 0
              i64.add
              drop
            else
              drop
              i64.const 0
              i64.add
              drop
            end)
          (export "!" (func 0)))
    "#;
    let wasm = wat::parse_str(WAT).expect("valid WAT");

    let config = CompilationConfig::default();

    let _ = RwasmModule::compile(config, &wasm);
}
