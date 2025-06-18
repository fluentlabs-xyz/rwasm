use rwasm::{
    CompilationConfig,
    ExecutionEngine,
    ImportLinker,
    ImportName,
    InstructionSet,
    RwasmModule,
    Store,
    TrapCode,
};
use wasmparser::ValType;

const ATTESTATION_INPUT: &[u8] = include_bytes!("./nitro-verifier/attestation.bin");

#[test]
#[ignore]
fn test_nitro_verifier() {
    let wasm_binary = include_bytes!("./nitro-verifier/lib.wasm");
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
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(import_linker);
    let (rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    let mut engine = ExecutionEngine::new();
    let mut store = Store::<()>::default();
    store.set_syscall_handler(|mut caller, sys_func_idx| -> Result<(), TrapCode> {
        match sys_func_idx {
            // _debug_log
            70 => {
                let (ptr, len) = caller.stack_pop2();
                let mut buffer = vec![0u8; len.as_usize()];
                caller.memory_read(ptr.as_usize(), &mut buffer).unwrap();
                println!("debug_log: {}", core::str::from_utf8(&buffer).unwrap());
            }
            // _input_size
            71 => {
                println!("input_size: {}", ATTESTATION_INPUT.len());
                caller.stack_push(ATTESTATION_INPUT.len() + 1024)
            }
            // _read
            72 => {
                let (target, offset, length) = caller.stack_pop3();
                let offset = offset.as_usize() - 1024; // size of context input
                let length = length.as_usize();
                println!(
                    "read: target={}, offset={}, length={}",
                    target, offset, length
                );
                caller
                    .memory_write(
                        target.as_usize(),
                        &ATTESTATION_INPUT[offset..(offset + length)],
                    )
                    .unwrap();
            }
            // _write
            73 => {
                let (offset, length) = caller.stack_pop2();
                let mut buffer = vec![0u8; length.as_usize()];
                caller.memory_read(offset.as_usize(), &mut buffer).unwrap();
                println!(
                    "write: {:?} ({})",
                    buffer.as_slice(),
                    core::str::from_utf8(&buffer).unwrap_or_else(|_| "can't parse utf-8 text")
                );
            }
            // _exit
            74 => {
                let exit_code = caller.stack_pop();
                println!("exit code: {}", exit_code.as_i32());
                return Err(TrapCode::ExecutionHalted);
            }
            _ => unreachable!(),
        }
        Ok(())
    });
    engine.execute(&mut store, &rwasm_module).unwrap();
}
