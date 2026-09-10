//! The shared engine must be re-entrant: a syscall handler may compile and run another module on
//! `ExecutionEngine::acquire_shared()` while an execution on that engine is in progress. The
//! engine used to guard an empty state with a non-reentrant spin lock, so this busy-spun forever.

use rwasm::{
    CompilationConfig, ExecutionEngine, ImportLinker, ImportName, RwasmModule, RwasmStore,
    SyscallFuelParams, TrapCode, TypedCaller, ValType, Value,
};
use std::{
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};

const OUTER_WAT: &str = r#"
    (module
        (import "host" "nested" (func $nested (result i32)))
        (func (export "main") (result i32)
            call $nested
            i32.const 1
            i32.add))
"#;

const INNER_WAT: &str = r#"
    (module
        (func (export "main") (result i32)
            i32.const 21
            i32.const 2
            i32.mul))
"#;

fn config() -> CompilationConfig {
    CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
}

fn import_linker() -> Arc<ImportLinker> {
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("host", "nested"),
        1,
        SyscallFuelParams::default(),
        &[],
        &[ValType::I32],
    );
    Arc::new(linker)
}

/// Runs the inner module on the shared engine from inside the outer module's syscall.
fn nested_syscall_handler(
    _caller: &mut TypedCaller<()>,
    _sys_func_idx: u32,
    _params: &[Value],
    result: &mut [Value],
) -> Result<(), TrapCode> {
    let wasm = wat::parse_str(INNER_WAT).unwrap();
    let (module, _) = RwasmModule::compile(config(), &wasm).unwrap();
    let mut store = RwasmStore::<()>::default();
    let instance = ImportLinker::default()
        .instantiate(&mut store, ExecutionEngine::acquire_shared(), module)
        .unwrap();
    instance.execute(&mut store, &[], result)
}

fn run_outer() -> i32 {
    let linker = import_linker();
    let wasm = wat::parse_str(OUTER_WAT).unwrap();
    let (module, _) =
        RwasmModule::compile(config().with_import_linker(linker.clone()), &wasm).unwrap();
    let mut store = RwasmStore::new(linker.clone(), (), nested_syscall_handler, None, None);
    let instance = linker
        .instantiate(&mut store, ExecutionEngine::acquire_shared(), module)
        .unwrap();
    let mut result = [Value::I32(0)];
    instance.execute(&mut store, &[], &mut result).unwrap();
    result[0].i32().unwrap()
}

#[test]
fn shared_engine_is_reentrant_from_a_syscall_handler() {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(run_outer());
    });
    let result = receiver
        .recv_timeout(Duration::from_secs(60))
        .expect("nested execution on the shared engine did not finish: deadlock");
    assert_eq!(result, 43);
}
