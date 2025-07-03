use rwasm::{
    compile_wasmtime_module,
    CompilationConfig,
    ExecutionEngine,
    ExecutorConfig,
    ImportLinker,
    ImportName,
    InstructionSet,
    RwasmModule,
    RwasmStore,
    Store,
    Strategy,
    TrapCode,
    TypedCaller,
    Value,
};
use std::rc::Rc;
use wasmparser::ValType;

const ATTESTATION_INPUT: &[u8] = include_bytes!("./nitro-verifier/attestation.bin");

fn fluentbase_syscall_handler<T: Send + Sync>(
    caller: &mut TypedCaller<T>,
    sys_func_idx: u32,
    params: &[Value],
    result: &mut [Value],
) -> Result<(), TrapCode> {
    match sys_func_idx {
        // _debug_log
        70 => {
            let ptr = params[0].i32().unwrap() as usize;
            let len = params[1].i32().unwrap() as usize;
            let mut buffer = vec![0u8; len];
            caller.memory_read(ptr, &mut buffer)?;
            println!("debug_log: {}", core::str::from_utf8(&buffer).unwrap());
        }
        // _input_size
        71 => {
            println!("input_size: {}", ATTESTATION_INPUT.len());
            result[0] = Value::I32(ATTESTATION_INPUT.len() as i32 + 1024); // size of context input
        }
        // _read
        72 => {
            let target = params[0].i32().unwrap() as usize;
            let offset = params[1].i32().unwrap() as usize - 1024; // size of context input
            let length = params[2].i32().unwrap() as usize;
            println!(
                "read: target={}, offset={}, length={}",
                target, offset, length
            );
            caller.memory_write(target, &ATTESTATION_INPUT[offset..(offset + length)])?;
        }
        // _write
        73 => {
            let offset = params[0].i32().unwrap() as usize;
            let length = params[1].i32().unwrap() as usize;
            let mut buffer = vec![0u8; length];
            caller.memory_read(offset, &mut buffer)?;
            println!(
                "write: {:?} ({})",
                buffer.as_slice(),
                core::str::from_utf8(&buffer).unwrap_or_else(|_| "can't parse utf-8 text")
            );
        }
        // _exit
        74 => {
            let exit_code = params[0].i32().unwrap();
            println!("exit code: {}", exit_code);
            return Err(TrapCode::ExecutionHalted);
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn import_linker() -> ImportLinker {
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_debug_log"),
        70,
        InstructionSet::default(),
        &[ValType::I32; 2],
        &[],
    );
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_input_size"),
        71,
        InstructionSet::default(),
        &[],
        &[ValType::I32; 1],
    );
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_read"),
        72,
        InstructionSet::default(),
        &[ValType::I32; 3],
        &[],
    );
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_write"),
        73,
        InstructionSet::default(),
        &[ValType::I32; 2],
        &[],
    );
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_exit"),
        74,
        InstructionSet::default(),
        &[ValType::I32; 1],
        &[],
    );
    import_linker
}

#[test]
#[ignore] // run this test manually with the "--release" flag
fn test_nitro_verifier_rwasm() {
    let wasm_binary = include_bytes!("./nitro-verifier/lib.wasm");
    let import_linker = Rc::new(import_linker());
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(import_linker.clone());
    let (rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    let mut engine = ExecutionEngine::new();
    let mut store = RwasmStore::<()>::new(
        ExecutorConfig::default(),
        import_linker.clone(),
        (),
        fluentbase_syscall_handler,
    );
    engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap();
}

#[cfg(feature = "wasmtime")]
#[test]
fn test_nitro_verifier_wasmtime() {
    use rwasm::WasmtimeWorker;
    let wasm_binary = include_bytes!("./nitro-verifier/lib.wasm");
    let import_linker = Rc::new(import_linker());
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(import_linker.clone());
    let (rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    // compile & run using wasmtime
    let module = Rc::new(compile_wasmtime_module(&rwasm_module.wasm_section).unwrap());
    let mut worker =
        WasmtimeWorker::new(module, import_linker, (), fluentbase_syscall_handler, None);
    worker.execute("main", &[], &mut []).unwrap();
}

#[cfg(feature = "wasmtime")]
#[test]
#[ignore] // run this test manually with the "--release" flag
fn test_nitro_verifier_strategy() {
    let wasm_binary = include_bytes!("./nitro-verifier/lib.wasm");
    let import_linker = Rc::new(import_linker());
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(import_linker.clone());
    let (rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    // compile & run using wasmtime
    let exec_strategy = |strategy: Strategy| {
        let mut store = strategy.create_store(
            ExecutorConfig::default(),
            import_linker.clone(),
            (),
            fluentbase_syscall_handler,
        );
        strategy.execute::<()>(&mut store, "main", &[], &mut [])
    };
    let rwasm_module = Rc::new(rwasm_module);
    // run with rwasm strategy first
    exec_strategy(Strategy::Rwasm {
        module: rwasm_module.clone(),
        engine: ExecutionEngine::acquire_shared(),
    })
    .unwrap();
    // run with wasmtime strategy
    let module = compile_wasmtime_module(&rwasm_module.wasm_section)
        .unwrap()
        .into();
    exec_strategy(Strategy::Wasmtime {
        module,
        resumable: true,
    })
    .unwrap();
}
