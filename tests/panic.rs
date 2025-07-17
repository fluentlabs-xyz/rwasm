use rwasm::{
    CompilationConfig, ExecutionEngine, ExecutorConfig, ImportLinker, ImportName, Opcode,
    RwasmModule, RwasmStore, StateRouterConfig, Store, TrapCode,
};
use std::rc::Rc;
use std::str::from_utf8;
use wasmparser::ValType;

const STATE_MAIN: u32 = 1;
const STATE_DEPLOY: u32 = 2;

#[test]
fn test_wasm_panic() {
    // deploy greeting WASM contract
    let wasm_binary = include_bytes!("assets/panic-stack-ub.wasm");
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_write"),
        100,
        Default::default(),
        &[ValType::I32; 2],
        &[],
    );
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_exit"),
        101,
        Default::default(),
        &[ValType::I32; 1],
        &[],
    );
    let import_linker = Rc::new(import_linker);
    let config = CompilationConfig::default()
        .with_state_router(StateRouterConfig {
            states: Box::new([("deploy".into(), STATE_DEPLOY), ("main".into(), STATE_MAIN)]),
            opcode: Some(Opcode::I32Const(STATE_MAIN.into())),
        })
        .with_import_linker(import_linker.clone());
    let (rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    let mut engine = ExecutionEngine::default();
    let mut store = RwasmStore::new(
        ExecutorConfig::default(),
        import_linker.clone(),
        (),
        |caller, sys_func_idx, params, _result| {
            if sys_func_idx == 100 {
                let mut buffer = vec![0u8; params[1].i32().unwrap() as usize];
                caller.memory_read(params[0].i32().unwrap() as usize, &mut buffer)?;
                assert_eq!(from_utf8(&buffer).unwrap(), "it's panic time");
            } else if sys_func_idx == 101 {
                let exit_code = params[0].i32().unwrap();
                println!("exit: {}", exit_code);
                return Err(TrapCode::ExecutionHalted);
            } else {
                unreachable!()
            }
            Ok(())
        },
    );
    engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap();
}
