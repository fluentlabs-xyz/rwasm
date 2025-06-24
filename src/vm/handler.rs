use crate::{types::TrapCode, Caller, Value};
use alloc::{vec, vec::Vec};

#[derive(Default)]
#[allow(dead_code)]
pub struct SimpleCallContext {
    pub exit_code: i32,
    pub input: Vec<u8>,
    pub state: u32,
    pub output: Vec<u8>,
}

#[derive(Default)]
#[allow(dead_code)]
struct SimpleCallHandler;

#[allow(dead_code)]
impl SimpleCallHandler {
    fn fn_proc_exit(
        caller: &mut dyn Caller<SimpleCallContext>,
        params: &[Value],
        _result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let exit_code = params[0].i32().unwrap();
        caller.context_mut().exit_code = exit_code;
        Err(TrapCode::ExecutionHalted)
    }

    fn fn_get_state(
        caller: &mut dyn Caller<SimpleCallContext>,
        _params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        result[0] = Value::I32(caller.context().state as i32);
        Ok(())
    }

    fn fn_read_input(
        caller: &mut dyn Caller<SimpleCallContext>,
        params: &[Value],
        _result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let target = params[0].i32().unwrap() as usize;
        let offset = params[1].i32().unwrap() as usize;
        let length = params[2].i32().unwrap() as usize;
        caller.context_mut().exit_code = -2020;
        let input = caller
            .context()
            .input
            .get(offset..(offset + length))
            .unwrap()
            .to_vec();
        caller.memory_write(target, &input)?;
        Ok(())
    }

    fn fn_input_size(
        caller: &mut dyn Caller<SimpleCallContext>,
        _params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        result[0] = Value::I32(caller.context().input.len() as i32);
        Ok(())
    }

    fn fn_write_output(
        caller: &mut dyn Caller<SimpleCallContext>,
        params: &[Value],
        _result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let offset = params[0].i32().unwrap() as usize;
        let length = params[1].i32().unwrap() as usize;
        let mut buffer = vec![0u8; length];
        caller.memory_read(offset, &mut buffer)?;
        caller.context_mut().output.extend_from_slice(&buffer);
        Ok(())
    }

    fn fn_keccak256(
        caller: &mut dyn Caller<SimpleCallContext>,
        params: &[Value],
        _result: &mut [Value],
    ) -> Result<(), TrapCode> {
        use tiny_keccak::Hasher;
        let data_offset = params[0].i32().unwrap() as usize;
        let data_len = params[1].i32().unwrap() as usize;
        let output32_offset = params[2].i32().unwrap() as usize;
        let mut buffer = vec![0u8; data_len];
        caller.memory_read(data_offset, &mut buffer)?;
        let mut hash = tiny_keccak::Keccak::v256();
        hash.update(&buffer);
        let mut output = [0u8; 32];
        hash.finalize(&mut output);
        caller.memory_write(output32_offset, &output)?;
        Ok(())
    }
}

#[allow(dead_code)]
pub(crate) fn simple_call_handler_syscall_handler(
    caller: &mut dyn Caller<SimpleCallContext>,
    func_idx: u32,
    params: &[Value],
    result: &mut [Value],
) -> Result<(), TrapCode> {
    match func_idx {
        0x0001 => SimpleCallHandler::fn_proc_exit(caller, params, result),
        0x0002 => SimpleCallHandler::fn_get_state(caller, params, result),
        0x0003 => SimpleCallHandler::fn_read_input(caller, params, result),
        0x0004 => SimpleCallHandler::fn_input_size(caller, params, result),
        0x0005 => SimpleCallHandler::fn_write_output(caller, params, result),
        0x0101 => SimpleCallHandler::fn_keccak256(caller, params, result),
        _ => unreachable!("rwasm: unknown function ({})", func_idx),
    }
}
