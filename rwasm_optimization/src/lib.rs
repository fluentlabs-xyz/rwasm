#![cfg_attr(not(test), no_std)]
#![feature(core_intrinsics)]

extern crate alloc;
#[cfg(test)]
extern crate std;

use alloc::vec::Vec;
use fluentbase_sdk::LowLevelAPI;
use rwasm_codegen::{Compiler, CompilerConfig, ImportLinker};

// fn wasm2rwasm(wasm_binary: &[u8], inject_fuel_consumption: bool) -> Vec<u8> {
//     let import_linker = Runtime::<()>::new_sovereign_linker();
//     Compiler::new_with_linker(
//         wasm_binary,
//         CompilerConfig::default().fuel_consume(inject_fuel_consumption),
//         Some(&import_linker),
//     )
//     .unwrap()
//     .finalize()
//     .unwrap()
// }

// #[no_mangle]
fn translate_binary(wasm_binary: &[u8]) -> Vec<u8> {
    // translate and compile module
    let import_linker = ImportLinker::default();

    Compiler::new_with_linker(
        wasm_binary,
        CompilerConfig::default().fuel_consume(false),
        Some(&import_linker),
    )
    .unwrap()
    .finalize()
    .unwrap()
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
        _sys_write(r.as_ptr(), r.len() as u32);
    };
}

#[cfg(test)]
mod test {
    use fluentbase_runtime::{
        instruction::runtime_register_sovereign_handlers,
        types::{RuntimeError, STATE_MAIN},
        ExecutionResult,
        Runtime,
        RuntimeContext,
    };
    use rwasm::{Config, Engine, Linker, Module, Store};
    use rwasm_codegen::{Compiler, CompilerConfig};
    use std::{vec, vec::Vec};

    fn wasm2rwasm(wasm_binary: &[u8], inject_fuel_consumption: bool) -> Vec<u8> {
        let import_linker = Runtime::<()>::new_sovereign_linker();
        Compiler::new_with_linker(
            wasm_binary,
            CompilerConfig::default().fuel_consume(inject_fuel_consumption),
            Some(&import_linker),
        )
        .unwrap()
        .finalize()
        .unwrap()
    }

    fn run_rwasm_with_raw_input(
        wasm_binary: Vec<u8>,
        input_data: &[u8],
        verify_wasm: bool,
    ) -> ExecutionResult<()> {
        // make sure at least wasm binary works well
        let wasm_exit_code = if verify_wasm {
            let config = Config::default();
            let engine = Engine::new(&config);
            let module = Module::new(&engine, wasm_binary.as_slice()).unwrap();
            let ctx = RuntimeContext::<()>::new(vec![])
                .with_state(STATE_MAIN)
                .with_fuel_limit(1_000_000)
                .with_input(input_data.to_vec())
                .with_catch_trap(true);
            let mut store = Store::new(&engine, ctx);
            let mut linker = Linker::new(&engine);
            runtime_register_sovereign_handlers(&mut linker, &mut store);
            let instance = linker
                .instantiate(&mut store, &module)
                .unwrap()
                .start(&mut store)
                .unwrap();
            let main_func = instance.get_func(&store, "main").unwrap();
            match main_func.call(&mut store, &[], &mut []) {
                Err(err) => {
                    let exit_code =
                        Runtime::<RuntimeContext<()>>::catch_trap(&RuntimeError::Rwasm(err));
                    if exit_code != 0 {
                        panic!("err happened during wasm execution: {:?}", exit_code);
                    }
                    // let mut lines = String::new();
                    // for log in store.tracer().logs.iter() {
                    //     let stack = log
                    //         .stack
                    //         .iter()
                    //         .map(|v| v.to_bits() as i64)
                    //         .collect::<Vec<_>>();
                    //     lines += format!("{}\t{:?}\t{:?}\n", log.program_counter, log.opcode,
                    // stack)         .as_str();
                    // }
                    // let _ = fs::create_dir("./tmp");
                    // fs::write("./tmp/cairo.txt", lines).unwrap();
                }
                Ok(_) => {}
            }
            let wasm_exit_code = store.data().exit_code();
            Some(wasm_exit_code)
        } else {
            None
        };
        // compile and run wasm binary
        let rwasm_binary = wasm2rwasm(wasm_binary.as_slice(), false);
        let ctx = RuntimeContext::new(rwasm_binary)
            .with_state(STATE_MAIN)
            .with_fuel_limit(1_000_000)
            .with_input(input_data.to_vec())
            .with_catch_trap(true);
        let import_linker = Runtime::<()>::new_sovereign_linker();
        let mut runtime = Runtime::<()>::new(ctx, &import_linker).unwrap();
        runtime.data_mut().clean_output();
        let execution_result = runtime.call().unwrap();
        if let Some(wasm_exit_code) = wasm_exit_code {
            assert_eq!(execution_result.data().exit_code(), wasm_exit_code);
        }
        execution_result
    }

    #[test]
    fn test_translate_wasm2rwasm_with_rwasm() {
        let cur_dir = std::env::current_dir().unwrap();
        let translator_wasm_binary = include_bytes!("../tmp/solid_file.wasm").to_vec();
        let translatee_wasm_binary = include_bytes!("../tmp/stack.wasm").to_vec();
        // translate and compile module
        let import_linker = Runtime::<()>::new_shared_linker();

        let output =
            run_rwasm_with_raw_input(translator_wasm_binary, &translatee_wasm_binary, false);
        assert_eq!(output.data().exit_code(), 0);
    }
}
