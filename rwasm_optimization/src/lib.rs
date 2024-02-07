#![cfg_attr(not(test), no_std)]
#![feature(core_intrinsics)]

mod allocator_and_panic;

// #[cfg(not(test))]
extern crate alloc;
// extern crate fluentbase_sdk;
// uncomment in case of using git repo for rwasm-codegen
#[cfg(test)]
extern crate std;

use alloc::vec::Vec;
use fluentbase_sdk::LowLevelSDK;
use rwasm_codegen::{Compiler, CompilerConfig, ImportLinker};

// #[no_mangle]
fn translate_binary(wasm_binary: &[u8]) -> Vec<u8> {
    // translate and compile module
    let import_linker = ImportLinker::default();

    let mut translator = Compiler::new_with_linker(
        wasm_binary,
        CompilerConfig::default()
            .fuel_consume(false)
            .with_router(false),
        Some(&import_linker),
    )
    .unwrap();
    translator.translate(Default::default()).unwrap();
    let binary = translator.finalize().unwrap();
    binary
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "fluentbase_v1alpha")]
extern "C" {
    fn _sys_read(target: *mut u8, offset: u32, length: u32);
    pub(crate) fn _sys_input_size() -> u32;
    fn _sys_write(offset: *const u8, length: u32);
    fn _sys_halt(code: i64) -> !;
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
fn main() {
    unsafe {
        let s = _sys_input_size();
        let mut v = alloc::vec![0u8; s as usize];

        _sys_read(v.as_mut_ptr(), 0, s);
        let r = translate_binary(&v);
        _sys_write(r.as_ptr(), r.len() as u32);
        _sys_halt(0);
    };
}

#[cfg(test)]
mod test {
    static PROVER_TIME_PER_INSTRUCTION_MS: f32 = 2.5;

    use fluentbase_runtime::{types::STATE_MAIN, Runtime, RuntimeContext};
    use rwasm_codegen::{
        instruction::INSTRUCTION_SIZE_BYTES,
        Compiler,
        CompilerConfig,
        ImportLinker,
    };
    use std::{format, fs, intrinsics::transmute_unchecked, println};

    #[test]
    fn translate_wasm2rwasm() {
        let cur_dir = std::env::current_dir().unwrap();
        let wasm_binary = include_bytes!("../tmp/solid_file.wasm").to_vec();
        // translate and compile module
        let import_linker = Runtime::<()>::new_shared_linker();

        let wasm_binary_len_old: i64 = 617963;
        let wasm_binary_len_new: i64 = wasm_binary.len() as i64;
        println!(
            "wasm binary len old {} new {} (decrease {})",
            wasm_binary_len_old,
            wasm_binary_len_new,
            wasm_binary_len_old - wasm_binary_len_new
        );

        let import_linker2 = unsafe {
            let import_linker: &ImportLinker = transmute_unchecked(&import_linker);
            import_linker
        };

        let mut translator = Compiler::new_with_linker(
            &wasm_binary,
            CompilerConfig::default()
                .fuel_consume(false)
                .with_router(true),
            Some(&import_linker2),
        )
        .unwrap();
        translator.translate(Default::default()).unwrap();
        let binary = translator.finalize().unwrap();
        let binary_len_old: i64 = 2697012;
        let binary_instr_count_old = binary_len_old / INSTRUCTION_SIZE_BYTES as i64;
        let binary_len_new: i64 = binary.len() as i64;
        let binary_instr_count_new = binary_len_new / INSTRUCTION_SIZE_BYTES as i64;
        println!("rwasm binary len old {} new {} (decrease {}) instructions count old {} new {} (decrease {})", binary_len_old, binary_len_new, binary_len_old - binary_len_new, binary_instr_count_old, binary_instr_count_new, binary_instr_count_old - binary_instr_count_new);
        let out_file = format!("{}/tmp/out.rwasm", cur_dir.to_str().unwrap());
        let res = fs::write(out_file, &binary);
        assert!(res.is_ok());

        let next_ctx = RuntimeContext::new(binary.clone())
            .with_state(STATE_MAIN)
            .with_fuel_limit(0);
        let execution_result = Runtime::<()>::run_with_context(next_ctx, &import_linker).unwrap();
        // let len_old: i64 = 19831; // when including from git, using special allocator,
        let len_old: i64 = 21876;
        let len_new: i64 = execution_result.tracer().logs.len() as i64;
        let proof_gen_time_old_ms = len_old as f32 * PROVER_TIME_PER_INSTRUCTION_MS;
        let proof_gen_time_new_ms = len_new as f32 * PROVER_TIME_PER_INSTRUCTION_MS;

        println!("instructions spent while proving old {} new {} (decrease {}) ratio {} proof gen time ms (estimated) old {} new {} (decrease {})", len_old, len_new, len_old - len_new, len_old as f32 / len_new as f32, proof_gen_time_old_ms, proof_gen_time_new_ms, proof_gen_time_old_ms - proof_gen_time_new_ms);
        // println!("logs:");
        // for l in execution_result.tracer().clone().logs {
        //     println!("{}: {:?}", l.program_counter, l.opcode);
        // }
    }
}
