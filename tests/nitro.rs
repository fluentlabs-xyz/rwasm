use rwasm::{CompilationConfig, ImportLinker, ImportName, InstructionSet, RwasmModule};
use wasmparser::ValType;

#[test]
fn test_nitro_verifier() {
    let wasm_binary = include_bytes!("./assets/nitro-verifier.wasm");
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
        72,
        InstructionSet::default(),
        &[ValType::I32; 2],
        &[],
    );
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_exit"),
        72,
        InstructionSet::default(),
        &[ValType::I32; 1],
        &[],
    );
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(import_linker);
    let (_rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
}
