use rwasm::{CompilationConfig, RwasmModule};

#[no_mangle]
pub fn main() {
    let (result, _) =
        RwasmModule::compile(CompilationConfig::default(), include_bytes!("./fib.wasm")).unwrap();
    core::hint::black_box(result);
}
