#![no_main]

use rwasm::{
    always_failing_syscall_handler, CompilationConfig, ExecutionEngine, FuelConfig, ImportLinker,
    ImportName, InstructionSet, RwasmModule, RwasmStore, TrapCode, TypedCaller, Value,
};
use std::sync::Arc;

fn interrupting_syscall_handler<T: Send + Sync>(
    _caller: &mut TypedCaller<'_, T>,
    _sys_func_idx: u32,
    _params: &[Value],
    _result: &mut [Value],
) -> Result<(), TrapCode> {
    Err(TrapCode::InterruptionCalled)
}
fn default_import_linker() -> Arc<ImportLinker> {
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("hello", "world"),
        0xff,
        InstructionSet::default(),
        &[],
        &[],
    );
    Arc::new(import_linker)
}

#[no_mangle]
pub fn main() {
    let wasm_binary = wat::parse_str(
        r#"
            (module
              (memory 1)
              (data (i32.const 0) "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzab")
              (func (export "64_good1") (param $i i32) (result i64)
                (i64.load offset=0 (local.get $i)) ;; 0x6867666564636261 'abcdefgh'
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
    let engine = ExecutionEngine::new();
    let mut result = [Value::I64(0); 1];
    fn bench_execute(
        engine: &ExecutionEngine,
        store: &mut RwasmStore<()>,
        rwasm_module: &RwasmModule,
        result: &mut [Value; 1],
    ) {
        engine
            .execute(store, rwasm_module, &[Value::I32(0)], result)
            .unwrap();
        assert_eq!(result[0].i64().unwrap(), 0x6867666564636261);
    }
    for _ in 0..1000 {
        core::hint::black_box(bench_execute(
            &engine,
            &mut store,
            &rwasm_module,
            &mut result,
        ));
    }
}
