use rwasm::{CompilationConfig, CompilationError, ConstructorParams, RwasmModule};

fn test_compilation(wat_str: &str) -> Result<(RwasmModule, ConstructorParams), CompilationError> {
    let wasm = wat::parse_str(wat_str).expect("valid WAT");
    let config = CompilationConfig::default();
    RwasmModule::compile(config, &wasm)
}
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
    let _ = test_compilation(WAT);
}
#[test]
fn test_extern_ref_not_supported_as_a_local_bug() {
    const WAT: &str = r#"
        (module
          (type (;0;) (func))
          (global (;0;) (mut i32) i32.const 1000)
          (export "" (func 0))
          (func (;0;) (type 0)
            (local f64 externref)
            global.get 0
            i32.eqz
            if ;; label = @1
              unreachable
            end
            global.get 0
            i32.const 1
            i32.sub
            global.set 0
          )
        (export "!" (func 0)))
    "#;
    let _ = test_compilation(WAT);
}
