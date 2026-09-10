use crate::{types::TrapCode, StoreTr, TypedCaller, Value};
use alloc::vec::Vec;

#[derive(Default)]
#[allow(dead_code)]
pub struct SimpleCallContext {
    pub exit_code: i32,
    pub input: Vec<u8>,
    pub state: u32,
    pub output: Vec<u8>,
}

/// A reference syscall handler for a minimal host: input/output buffers, a state word and
/// `keccak256`.
///
/// It is also the pattern integrators copy, so it does what a production handler must do: every
/// guest-supplied offset and length is range-checked before any buffer is allocated
/// (`memory_read_into_vec` validates against the memory size first), arithmetic on guest values
/// is checked, and malformed parameters are reported as traps rather than panics.
#[derive(Default)]
#[allow(dead_code)]
struct SimpleCallHandler;

/// Reads parameter `index` as a guest `i32` reinterpreted as an unsigned offset or length.
///
/// A missing or mistyped parameter means the guest called the import with a signature the host
/// did not expect; that is a `BadSignature`, not a panic.
fn param_u32(params: &[Value], index: usize) -> Result<u32, TrapCode> {
    params
        .get(index)
        .and_then(Value::i32)
        .map(|value| value as u32)
        .ok_or(TrapCode::BadSignature)
}

fn param_usize(params: &[Value], index: usize) -> Result<usize, TrapCode> {
    param_u32(params, index).map(|value| value as usize)
}

#[allow(dead_code)]
impl SimpleCallHandler {
    fn fn_proc_exit(
        caller: &mut TypedCaller<SimpleCallContext>,
        params: &[Value],
        _result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let exit_code = params
            .first()
            .and_then(Value::i32)
            .ok_or(TrapCode::BadSignature)?;
        caller.data_mut().exit_code = exit_code;
        Err(TrapCode::ExecutionHalted)
    }

    fn fn_get_state(
        caller: &mut TypedCaller<SimpleCallContext>,
        _params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let state = caller.data().state as i32;
        *result.first_mut().ok_or(TrapCode::BadSignature)? = Value::I32(state);
        Ok(())
    }

    fn fn_read_input(
        caller: &mut TypedCaller<SimpleCallContext>,
        params: &[Value],
        _result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let target = param_usize(params, 0)?;
        let offset = param_usize(params, 1)?;
        let length = param_usize(params, 2)?;
        caller.data_mut().exit_code = -2020;
        // the range is checked on the host buffer before anything is copied; `offset + length`
        // is guest-controlled and may wrap
        let end = offset
            .checked_add(length)
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        let input = caller
            .data()
            .input
            .get(offset..end)
            .ok_or(TrapCode::MemoryOutOfBounds)?
            .to_vec();
        caller.memory_write(target, &input)?;
        Ok(())
    }

    fn fn_input_size(
        caller: &mut TypedCaller<SimpleCallContext>,
        _params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let size =
            i32::try_from(caller.data().input.len()).map_err(|_| TrapCode::IntegerOverflow)?;
        *result.first_mut().ok_or(TrapCode::BadSignature)? = Value::I32(size);
        Ok(())
    }

    fn fn_write_output(
        caller: &mut TypedCaller<SimpleCallContext>,
        params: &[Value],
        _result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let offset = param_usize(params, 0)?;
        let length = param_usize(params, 1)?;
        // validates the range against the memory size before allocating `length` bytes
        let buffer = caller.memory_read_into_vec(offset, length)?;
        caller.data_mut().output.extend_from_slice(&buffer);
        Ok(())
    }

    fn fn_keccak256(
        caller: &mut TypedCaller<SimpleCallContext>,
        params: &[Value],
        _result: &mut [Value],
    ) -> Result<(), TrapCode> {
        use tiny_keccak::Hasher;
        let data_offset = param_usize(params, 0)?;
        let data_len = param_usize(params, 1)?;
        let output32_offset = param_usize(params, 2)?;
        let buffer = caller.memory_read_into_vec(data_offset, data_len)?;
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
    caller: &mut TypedCaller<SimpleCallContext>,
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
        // an import the module linked but this host does not implement
        _ => Err(TrapCode::UnknownExternalFunction),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/vm/handler_tests.rs"]
mod tests;
