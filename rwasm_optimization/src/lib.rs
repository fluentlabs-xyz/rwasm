#![cfg_attr(not(test), no_std)]
#![feature(core_intrinsics)]

extern crate alloc;
// #[cfg(not(test))]
// extern crate fluentbase_sdk;
// uncomment in case of using git repo for rwasm-codegen
#[cfg(test)]
extern crate std;

use alloc::vec::Vec;
use fluentbase_sdk::LowLevelAPI;
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
        assert_ne!(s, 0);
        let mut v = alloc::vec![0u8; s as usize];
        _sys_read(v.as_mut_ptr(), 0, s);
        let r = translate_binary(&v);
        _sys_write(r.as_ptr(), r.len() as u32);
        _sys_halt(0);
    };
}

#[cfg(test)]
mod test {
    use fluentbase_runtime::{types::STATE_MAIN, Runtime, RuntimeContext};
    use rwasm_codegen::{
        instruction::INSTRUCTION_SIZE_BYTES,
        Compiler,
        CompilerConfig,
        ImportLinker,
    };
    use std::{intrinsics::transmute_unchecked, println};

    static PROVER_TIME_PER_INSTRUCTION_MS: f32 = 2.5;

    #[test]
    fn translate_wasm2rwasm() {
        // let mut test_arr = Vec::new();
        //
        // println!("test_arr.capacity {}", test_arr.capacity());
        // for i in 0..21 {
        //     test_arr.push(2);
        //     println!("test_arr.capacity {}", test_arr.capacity());
        // }
        // println!("test_arr.capacity {}", test_arr.capacity());
        // test_arr.clear();
        // println!("test_arr.capacity (after clear) {}", test_arr.capacity());
        //
        // return;

        let cur_dir = std::env::current_dir().unwrap();
        let translator_wasm_binary = include_bytes!("../tmp/solid_file.wasm").to_vec();
        let translatee_wasm_binary = include_bytes!("../tmp/keccak256.wasm").to_vec();
        // translate and compile module
        let import_linker = Runtime::<()>::new_shared_linker();

        let wasm_binary_len_old: i64 = 587299;
        let wasm_binary_len_new: i64 = translator_wasm_binary.len() as i64;
        println!(
            "wasm binary len old {} new {} (change {})",
            wasm_binary_len_old,
            wasm_binary_len_new,
            wasm_binary_len_new - wasm_binary_len_old
        );

        let import_linker2 = unsafe {
            let import_linker: &ImportLinker = transmute_unchecked(&import_linker);
            import_linker
        };

        let mut translator = Compiler::new_with_linker(
            &translator_wasm_binary,
            CompilerConfig::default()
                .fuel_consume(false)
                .with_router(true),
            Some(&import_linker2),
        )
        .unwrap();
        translator.translate(Default::default()).unwrap();
        let translator_rwasm_binary = translator.finalize().unwrap();
        let binary_len_old: i64 = 2590254;
        let binary_instr_count_old = binary_len_old / INSTRUCTION_SIZE_BYTES as i64;
        let binary_len_new: i64 = translator_rwasm_binary.len() as i64;
        let binary_instr_count_new = binary_len_new / INSTRUCTION_SIZE_BYTES as i64;
        println!(
            "rwasm binary len old {} new {} (change {}) instructions old {} new {} (change {})",
            binary_len_old,
            binary_len_new,
            binary_len_new - binary_len_old,
            binary_instr_count_old,
            binary_instr_count_new,
            binary_instr_count_new - binary_instr_count_old
        );
        // let out_file = format!("{}/tmp/out.rwasm", cur_dir.to_str().unwrap());
        // let res = fs::write(out_file, &translator_rwasm_binary);
        // assert!(res.is_ok());

        let mut translator = Compiler::new_with_linker(
            &translatee_wasm_binary,
            CompilerConfig::default().fuel_consume(false),
            Some(&import_linker2),
        )
        .unwrap();
        translator.translate(Default::default()).unwrap();
        let translatee_rwasm_binary = translator.finalize().unwrap();
        // let mut rmodule = ReducedModule::new(&translatee_rwasm_binary).unwrap();
        // let mut translatee_instruction_set = rmodule.bytecode().clone();
        // fs::write(
        //     "tmp/instruction_set.txt",
        //     translatee_instruction_set.trace(),
        // )
        // .unwrap();

        let next_ctx = RuntimeContext::new(translator_rwasm_binary.clone())
            .with_input(translatee_wasm_binary)
            .with_state(STATE_MAIN)
            .with_fuel_limit(0);
        let execution_result = Runtime::<()>::run_with_context(next_ctx, &import_linker).unwrap();
        let len_old: i64 = 24438;
        let len_new: i64 = execution_result.tracer().logs.len() as i64;
        let proof_gen_time_old_ms = len_old as f32 * PROVER_TIME_PER_INSTRUCTION_MS;
        let proof_gen_time_new_ms = len_new as f32 * PROVER_TIME_PER_INSTRUCTION_MS;

        println!(
            "instructions spent while proving old {} new {} (change {}, ratio {}) proof gen time ms (est.) old {} new {} (change {})",
                len_old, len_new, len_new-len_old, len_new as f32/ len_old as f32, proof_gen_time_old_ms,
                proof_gen_time_new_ms, proof_gen_time_new_ms-proof_gen_time_old_ms,
        );
        // println!("logs:");
        // for l in execution_result.tracer().clone().logs {
        //     println!("{}: {:?}", l.program_counter, l.opcode);
        // }
    }
}
