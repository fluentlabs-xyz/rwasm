use rwasm::{CompilationConfig, RwasmModule};

#[no_mangle]
pub fn main() {
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
    let (result, _) = RwasmModule::compile(CompilationConfig::default(), &wasm_binary).unwrap();
    core::hint::black_box(result);
}
