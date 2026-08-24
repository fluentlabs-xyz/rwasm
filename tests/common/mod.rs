//! Shared fluentbase host harness for integration tests.
//!
//! Each integration test crate compiles this module independently and not all
//! of them use every item, so unused-item warnings are suppressed.
#![allow(dead_code)]

use rwasm::{ImportLinker, ImportName, StoreTr, TrapCode, TypedCaller, Value};
use rwasm_fuel_policy::SyscallFuelParams;
use std::{str::from_utf8, sync::Arc};
use wasmparser::ValType;

#[derive(Default, Clone)]
pub struct HostState {
    pub input: Vec<u8>,
    pub output: Vec<u8>,
    pub state: u32,
}

pub fn fluentbase_syscall_handler(
    caller: &mut TypedCaller<HostState>,
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
            println!("debug_log: {}", from_utf8(&buffer).unwrap());
        }
        // _input_size
        71 => {
            result[0] = Value::I32(caller.data().input.len() as i32); // size of context input
        }
        // _output_size
        72 => {
            result[0] = Value::I32(0);
        }
        // _read
        73 => {
            let target = params[0].i32().unwrap() as usize;
            let offset = params[1].i32().unwrap() as usize; // size of context input
            let length = params[2].i32().unwrap() as usize;
            println!(
                "read: target={}, offset={}, length={}",
                target, offset, length
            );
            let data = caller.data().input[offset..(offset + length)].to_vec();
            caller.memory_write(target, &data)?;
        }
        // _write
        74 => {
            let offset = params[0].i32().unwrap() as usize;
            let length = params[1].i32().unwrap() as usize;
            let mut buffer = vec![0u8; length];
            caller.memory_read(offset, &mut buffer)?;
            println!(
                "write: {:?} ({})",
                buffer.as_slice(),
                from_utf8(&buffer).unwrap_or("can't parse utf-8 text")
            );
            caller.data_mut().output.extend_from_slice(&buffer)
        }
        // _exit
        75 => {
            let exit_code = params[0].i32().unwrap();
            println!("exit code: {}", exit_code);
            return Err(TrapCode::ExecutionHalted);
        }
        // _read_output
        76 => {
            unimplemented!("_read_output");
        }
        _ => unreachable!(),
    }
    Ok(())
}

pub fn create_import_linker() -> Arc<ImportLinker> {
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_debug_log"),
        70,
        SyscallFuelParams::default(),
        &[ValType::I32; 2],
        &[],
    );
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_input_size"),
        71,
        SyscallFuelParams::default(),
        &[],
        &[ValType::I32; 1],
    );
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_output_size"),
        72,
        SyscallFuelParams::default(),
        &[],
        &[ValType::I32; 1],
    );
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_read"),
        73,
        SyscallFuelParams::default(),
        &[ValType::I32; 3],
        &[],
    );
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_write"),
        74,
        SyscallFuelParams::default(),
        &[ValType::I32; 2],
        &[],
    );
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_exit"),
        75,
        SyscallFuelParams::default(),
        &[ValType::I32; 1],
        &[],
    );
    import_linker.insert_function(
        ImportName::new("fluentbase_v1preview", "_read_output"),
        76,
        SyscallFuelParams::default(),
        &[ValType::I32; 3],
        &[],
    );
    Arc::new(import_linker)
}
