//! Regression tests for out-of-bounds value stack accesses driven by hand-crafted bytecode.
//!
//! Every module below is rejected by `RwasmModule::new_verified`; the tests additionally run the
//! bytecode through the interpreter to prove that the value stack itself refuses the access, even
//! when the module was decoded through a path that does not verify.

use rwasm::{
    instruction_set, ExecutionEngine, ImportLinker, InstructionSet, RwasmModule,
    RwasmModuleBuilder, RwasmStore, TrapCode,
};
use std::sync::Arc;

fn execute(code_section: InstructionSet) -> Result<(), TrapCode> {
    let module = RwasmModuleBuilder::new(code_section).build();
    let mut store = RwasmStore::new(
        Arc::new(ImportLinker::default()),
        (),
        rwasm::always_failing_syscall_handler,
        Some(1_000_000),
        None,
    );
    ExecutionEngine::new().execute(&mut store, &module, &[], &mut [])
}

fn verify(code_section: InstructionSet) -> Result<(), rwasm::RwasmModuleError> {
    let sink = RwasmModuleBuilder::new(code_section).build().serialize();
    RwasmModule::new_verified(&sink).map(|_| ())
}

#[test]
fn local_get_beyond_the_stack_traps_instead_of_reading_out_of_bounds() {
    let code = instruction_set! {
        StackCheck(16)
        LocalGet(0x0800_0000)
        Drop
        Return
    };
    assert!(verify(code.clone()).is_err());
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

#[test]
fn local_set_beyond_the_stack_traps_instead_of_writing_out_of_bounds() {
    let code = instruction_set! {
        StackCheck(16)
        I32Const(0xdead_beefu32 as i32)
        LocalSet(0x0800_0000)
        Return
    };
    assert!(verify(code.clone()).is_err());
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

#[test]
fn local_tee_beyond_the_stack_traps_instead_of_writing_out_of_bounds() {
    let code = instruction_set! {
        StackCheck(16)
        I32Const(0xdead_beefu32 as i32)
        LocalTee(0x0800_0000)
        Drop
        Return
    };
    assert!(verify(code.clone()).is_err());
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

#[test]
fn dropping_below_the_stack_base_traps() {
    let code = instruction_set! {
        StackCheck(16)
        Drop
        Return
    };
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

#[test]
fn bulk_drop_below_the_stack_base_traps() {
    let code = instruction_set! {
        StackCheck(16)
        BulkDrop(0x0800_0000)
        Return
    };
    assert!(verify(code.clone()).is_err());
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

#[test]
fn popping_below_the_stack_base_traps() {
    let code = instruction_set! {
        StackCheck(16)
        I32Const(1)
        I32Add
        Return
    };
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

#[test]
fn pushing_past_the_reserved_stack_window_traps() {
    // `StackCheck` only reserves a single cell, so the second push leaves the value stack.
    let mut code = instruction_set! { StackCheck(1) };
    for _ in 0..(rwasm::N_MAX_STACK_SIZE + 1) {
        code.op_i32_const(1);
    }
    code.op_return();
    assert_eq!(execute(code), Err(TrapCode::StackOverflow));
}

/// Guards against the verifier rejecting bytecode this crate produces itself.
#[test]
fn compiled_modules_pass_verification() {
    use rwasm::{CompilationConfig, ImportName};
    use rwasm_fuel_policy::SyscallFuelParams;
    use wasmparser::ValType;

    const I32X3: &[ValType] = &[ValType::I32; 3];
    let mut import_linker = ImportLinker::default();
    for (name, idx, params, results) in [
        ("_debug_log", 70u32, 2usize, 0usize),
        ("_input_size", 71, 0, 1),
        ("_output_size", 72, 0, 1),
        ("_read", 73, 3, 0),
        ("_write", 74, 2, 0),
        ("_exit", 75, 1, 0),
        ("_read_output", 76, 3, 0),
    ] {
        import_linker.insert_function(
            ImportName::new("fluentbase_v1preview", name),
            idx,
            SyscallFuelParams::default(),
            &I32X3[..params],
            &I32X3[..results],
        );
    }
    let import_linker = Arc::new(import_linker);
    for wasm_binary in [
        include_bytes!("assets/nitro-verifier-stack-ub.wasm").as_slice(),
        include_bytes!("assets/panic-stack-ub.wasm").as_slice(),
        include_bytes!("assets/secp256k1-stack-ub.wasm").as_slice(),
    ] {
        let config = CompilationConfig::default()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_import_linker(import_linker.clone());
        let (module, _) = RwasmModule::compile(config, wasm_binary).expect("rwasm compiles");
        module.verify().expect("compiled module verifies");
    }
}
