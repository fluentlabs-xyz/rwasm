use hex_literal::hex;
use rwasm::{
    CompilationConfig, ExecutionEngine, FuelConfig, ImportLinker, ImportName, Opcode, RwasmModule,
    RwasmStore, StateRouterConfig, Store, TrapCode, TypedCaller, Value,
};
use std::{str::from_utf8, sync::Arc};
use wasmparser::ValType;

const STATE_MAIN: u32 = 1;
const STATE_DEPLOY: u32 = 2;

#[derive(Default, Clone)]
struct HostState {
    input: Vec<u8>,
    output: Vec<u8>,
    state: u32,
}

fn fluentbase_import_linker() -> Arc<ImportLinker> {
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
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_input_size"),
        102,
        Default::default(),
        &[],
        &[ValType::I32; 1],
    );
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_read"),
        103,
        Default::default(),
        &[ValType::I32; 3],
        &[],
    );
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_charge_fuel_manually"),
        104,
        Default::default(),
        &[ValType::I64; 2],
        &[ValType::I64; 1],
    );
    Arc::new(import_linker)
}

fn fluentbase_syscall_handler(
    caller: &mut TypedCaller<HostState>,
    sys_func_idx: u32,
    params: &[Value],
    result: &mut [Value],
) -> Result<(), TrapCode> {
    println!("syscall: {}", sys_func_idx);
    if sys_func_idx == 100 {
        // write
        let mut buffer = vec![0u8; params[1].i32().unwrap() as usize];
        caller.memory_read(params[0].i32().unwrap() as usize, &mut buffer)?;
        println!("write: {}", from_utf8(&buffer).unwrap());
        caller.context_mut(|ctx| ctx.output.append(&mut buffer));
    } else if sys_func_idx == 101 {
        // exit
        let exit_code = params[0].i32().unwrap();
        println!("exit: {}", exit_code);
        return Err(TrapCode::ExecutionHalted);
    } else if sys_func_idx == 102 {
        // input_size
        let input_size = caller.context(|ctx| ctx.input.len()) as i32;
        result[0] = Value::I32(input_size);
    } else if sys_func_idx == 103 {
        // _read
        let target = params[0].i32().unwrap() as usize;
        let offset = params[1].i32().unwrap() as usize;
        let length = params[2].i32().unwrap() as usize;
        let buffer = caller.context(|ctx| ctx.input[offset..(offset + length)].to_vec());
        caller.memory_write(target, &buffer)?;
    } else if sys_func_idx == 104 {
        // _charge_fuel_manually
    } else {
        unreachable!()
    }
    Ok(())
}

fn run_fluentbase_binary(wasm_binary: &[u8], host_state: HostState) -> HostState {
    let import_linker = fluentbase_import_linker();
    let config = CompilationConfig::default()
        .with_state_router(StateRouterConfig {
            states: Box::new([("deploy".into(), STATE_DEPLOY), ("main".into(), STATE_MAIN)]),
            opcode: Some(Opcode::I32Const(STATE_MAIN.into())),
        })
        .with_import_linker(import_linker.clone());
    let (rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    let engine = ExecutionEngine::default();
    let mut store = RwasmStore::new(
        import_linker.clone(),
        host_state,
        fluentbase_syscall_handler,
        FuelConfig::default(),
    );
    engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap();
    store.context(Clone::clone)
}

#[test]
fn test_wasm_panic() {
    let wasm_binary = include_bytes!("assets/panic-stack-ub.wasm");
    let mut host_state = HostState::default();
    host_state.state = STATE_MAIN;
    let host_state = run_fluentbase_binary(wasm_binary, host_state);
    assert_eq!(
        from_utf8(host_state.output.as_slice()).unwrap(),
        "it's panic time"
    )
}

#[test]
fn test_wasm_secp256k1() {
    let wasm_binary = include_bytes!("assets/secp256k1-stack-ub.wasm");
    let mut host_state = HostState::default();
    host_state.state = STATE_MAIN;
    host_state.input = vec![0u8; 1024];
    host_state.input.extend_from_slice(&hex!("a04a451028d0f9284ce82243755e245238ab1e4ecf7b9dd8bf4734d9ecfd0529cf09dd8d0eb3c3968aca8846a249424e5537d3470f979ff902b57914dc77d02316bd29784f668a73cc7a36f4cc5b9ce704481e6cb5b1c2c832af02ca6837ebec044e3b81af9c2234cad09d679ce6035ed1392347ce64ce405f5dcd36228a25de6e47fd35c4215d1edf53e6f83de344615ce719bdb0fd878f6ed76f06dd277956de"));
    run_fluentbase_binary(wasm_binary, host_state);
}
