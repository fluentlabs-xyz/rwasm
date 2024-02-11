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
    // fn _sys_halt(code: i32) -> !;
    fn _sys_write(offset: *const u8, length: u32);
    fn _sys_input_size() -> u32;
    fn _sys_read(target: *mut u8, offset: u32, length: u32);
    fn _sys_output_size() -> u32;
    fn _sys_read_output(target: *mut u8, offset: u32, length: u32);
    fn _sys_state() -> u32;
    fn _sys_exec(
        code_offset: *const u8,
        code_len: u32,
        input_offset: *const u8,
        input_len: u32,
        return_offset: *mut u8,
        return_len: u32,
        fuel_offset: *mut u32,
        state: u32,
    ) -> i32;
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
        // _sys_write(r.as_ptr(), r.len() as u32);
        // _sys_halt(0);
    };
}

#[cfg(test)]
mod test {
    use fluentbase_runtime::{
        instruction::runtime_register_shared_handlers,
        types::STATE_MAIN,
        Runtime,
        RuntimeContext,
    };
    use fluentbase_sdk::{LowLevelAPI, LowLevelSDK};
    use rwasm::{Config, Engine, Linker, Module, Store};
    use rwasm_codegen::{
        instruction::INSTRUCTION_SIZE_BYTES,
        Compiler,
        CompilerConfig,
        EXPORT_FUNC_MAIN_NAME,
    };
    use std::{format, println, string::String, vec::Vec};

    static PROVER_TIME_PER_INSTRUCTION_MS: f32 = 2.5;

    fn test_wasm_binary(wasm_binary: &[u8], input: &[u8]) {
        let config = Config::default();
        let engine = Engine::new(&config);
        let module = Module::new(&engine, wasm_binary).unwrap();
        let mut runtime_ctx = RuntimeContext::<()>::default();
        runtime_ctx = runtime_ctx.with_input(input.to_vec());
        let mut store = Store::new(&engine, runtime_ctx);
        let mut linker = Linker::new(&engine);
        runtime_register_shared_handlers::<()>(&mut linker, &mut store);
        let instance = linker
            .instantiate(&mut store, &module)
            .unwrap()
            .start(&mut store)
            .unwrap();
        let main_func = instance.get_func(&store, EXPORT_FUNC_MAIN_NAME).unwrap();
        match main_func.call(&mut store, &[], &mut []) {
            Err(err) => {
                let mut lines = String::new();
                for log in store.tracer().logs.iter() {
                    let stack = log
                        .stack
                        .iter()
                        .map(|v| v.to_bits() as i64)
                        .collect::<Vec<_>>();
                    lines += format!(
                        "{}:sl {}: {:?}\t{:?}\n",
                        log.source_pc,
                        stack.len(),
                        log.opcode,
                        stack
                    )
                    .as_str();
                }
                let _ = std::fs::create_dir("./tmp");
                std::fs::write("./tmp/solid_file.wasm.trace.log", lines).unwrap();
                panic!("err happened during wasm execution: {:?}", err);
            }
            Ok(_) => {}
        }
        let wasm_exit_code = store.data().exit_code();
    }

    #[test]
    fn translate_wasm2rwasm() {
        let cur_dir = std::env::current_dir().unwrap();
        let translator_wasm_binary = include_bytes!("../tmp/solid_file.wasm").to_vec();
        let translatee_wasm_binary = include_bytes!("../tmp/greeting.wasm").to_vec();
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

        LowLevelSDK::with_test_input(translatee_wasm_binary.clone());
        let input_size = LowLevelSDK::sys_input_size();
        println!("input_size {}", input_size);
        test_wasm_binary(&translator_wasm_binary, &translatee_wasm_binary);

        // let import_linker2 = unsafe {
        //     let import_linker: &ImportLinker = transmute(&import_linker);
        //     import_linker
        // };

        let mut compiler = Compiler::new_with_linker(
            &translator_wasm_binary,
            CompilerConfig::default()
                .fuel_consume(false)
                .with_router(true),
            Some(&import_linker),
        )
        .unwrap();
        compiler.translate(Default::default()).unwrap();
        let translator_rwasm_binary = compiler.finalize().unwrap();
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

        #[cfg(feature = "disabled")]
        {
            let mut compiler = Compiler::new_with_linker(
                &translatee_wasm_binary,
                CompilerConfig::default().fuel_consume(false),
                Some(&import_linker2),
            )
            .unwrap();
            compiler.translate(Default::default()).unwrap();
            let translatee_rwasm_binary = compiler.finalize().unwrap();

            let mut rmodule = ReducedModule::new(&translator_rwasm_binary).unwrap();
            let mut translator_instruction_set = rmodule.bytecode().clone();

            let mut rmodule = ReducedModule::new(&translatee_rwasm_binary).unwrap();
            let mut translatee_instruction_set = rmodule.bytecode().clone();

            let instruction_set_out_path_str = "tmp/translator_instruction_set.txt";
            let instruction_set_out_path = Path::new(instruction_set_out_path_str);
            assert!(instruction_set_out_path.exists());
            fs::write(instruction_set_out_path, translator_instruction_set.trace()).unwrap();

            let instruction_set_out_path_str = "tmp/translatee_instruction_set.txt";
            let instruction_set_out_path = Path::new(instruction_set_out_path_str);
            assert!(instruction_set_out_path.exists());
            // fs::write(instruction_set_out_path, translatee_instruction_set.trace()).unwrap();
        }

        let next_ctx = RuntimeContext::new(translator_rwasm_binary.clone())
            .with_input(translatee_wasm_binary)
            .with_state(STATE_MAIN)
            .with_fuel_limit(0);
        let execution_result = Runtime::<()>::run_with_context(next_ctx, &import_linker).unwrap();
        assert_eq!(execution_result.data().exit_code(), 0);
        let len_old: i64 = 24438;
        let len_new: i64 = execution_result.tracer().logs.len() as i64;
        let proof_gen_time_old_ms = len_old as f32 * PROVER_TIME_PER_INSTRUCTION_MS;
        let proof_gen_time_new_ms = len_new as f32 * PROVER_TIME_PER_INSTRUCTION_MS;

        println!(
            "instructions spent while proving old {} new {} (change {}, ratio {}) proof gen time ms (est.) old {} new {} (change {})",
                len_old, len_new, len_new-len_old, len_new as f32 / len_old as f32, proof_gen_time_old_ms,
                proof_gen_time_new_ms, proof_gen_time_new_ms-proof_gen_time_old_ms,
        );
        // println!("logs:");
        // for l in execution_result.tracer().clone().logs {
        //     println!("{}: {:?}", l.program_counter, l.opcode);
        // }
    }
}
