use crate::{
    types::{RwasmError, UntypedValue},
    vm::context::Caller,
};
use alloc::{vec, vec::Vec};

pub type SyscallHandler<T> = fn(Caller<T>, u32) -> Result<(), RwasmError>;

pub fn always_failing_syscall_handler<T>(
    _caller: Caller<T>,
    func_idx: u32,
) -> Result<(), RwasmError> {
    Err(RwasmError::UnknownExternalFunction(func_idx))
}

#[derive(Default)]
pub struct SimpleCallContext {
    pub input: Vec<u8>,
    pub state: u32,
    pub output: Vec<u8>,
}

#[derive(Default)]
struct SimpleCallHandler;

impl SimpleCallHandler {
    fn fn_proc_exit(mut caller: Caller<SimpleCallContext>) -> Result<(), RwasmError> {
        let exit_code = caller.stack_pop();
        Err(RwasmError::ExecutionHalted(i32::from(exit_code)))
    }

    fn fn_get_state(mut caller: Caller<SimpleCallContext>) -> Result<(), RwasmError> {
        caller.stack_push(UntypedValue::from(caller.context().state));
        Ok(())
    }

    fn fn_read_input(mut caller: Caller<SimpleCallContext>) -> Result<(), RwasmError> {
        let [target, offset, length] = caller.stack_pop_n();
        let input = caller
            .context()
            .input
            .get(offset.as_usize()..(offset.as_usize() + length.as_usize()))
            .ok_or(RwasmError::ExecutionHalted(-2020))?
            .to_vec();
        caller.memory_write(target.as_usize(), &input)?;
        Ok(())
    }

    fn fn_input_size(mut caller: Caller<SimpleCallContext>) -> Result<(), RwasmError> {
        caller.stack_push(UntypedValue::from(caller.context().input.len() as i32));
        Ok(())
    }

    fn fn_write_output(mut caller: Caller<SimpleCallContext>) -> Result<(), RwasmError> {
        let [offset, length] = caller.stack_pop_n();
        let mut buffer = vec![0u8; length.as_usize()];
        caller.memory_read(offset.as_usize(), &mut buffer)?;
        caller.context_mut().output.extend_from_slice(&buffer);
        Ok(())
    }

    fn fn_keccak256(mut caller: Caller<SimpleCallContext>) -> Result<(), RwasmError> {
        use tiny_keccak::Hasher;
        let [data_offset, data_len, output32_offset] = caller.stack_pop_n();
        let mut buffer = vec![0u8; data_len.as_usize()];
        caller.memory_read(data_offset.as_usize(), &mut buffer)?;
        let mut hash = tiny_keccak::Keccak::v256();
        hash.update(&buffer);
        let mut output = [0u8; 32];
        hash.finalize(&mut output);
        caller.memory_write(output32_offset.as_usize(), &output)?;
        Ok(())
    }
}

pub(crate) fn simple_call_handler_syscall_handler(
    caller: Caller<SimpleCallContext>,
    func_idx: u32,
) -> Result<(), RwasmError> {
    match func_idx {
        0x0001 => SimpleCallHandler::fn_proc_exit(caller),
        0x0002 => SimpleCallHandler::fn_get_state(caller),
        0x0003 => SimpleCallHandler::fn_read_input(caller),
        0x0004 => SimpleCallHandler::fn_input_size(caller),
        0x0005 => SimpleCallHandler::fn_write_output(caller),
        0x0101 => SimpleCallHandler::fn_keccak256(caller),
        _ => unreachable!("rwasm: unknown function ({})", func_idx),
    }
}
