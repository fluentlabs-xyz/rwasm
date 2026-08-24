use crate::{types::TrapCode, StoreTr, TypedCaller, Value};
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
        caller: &mut TypedCaller<SimpleCallContext>,
        params: &[Value],
        _result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let exit_code = params[0].i32().unwrap();
        caller.data_mut().exit_code = exit_code;
        Err(TrapCode::ExecutionHalted)
    }

    fn fn_get_state(
        caller: &mut TypedCaller<SimpleCallContext>,
        _params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        result[0] = Value::I32(caller.data().state as i32);
        Ok(())
    }

    fn fn_read_input(
        caller: &mut TypedCaller<SimpleCallContext>,
        params: &[Value],
        _result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let target = params[0].i32().unwrap() as usize;
        let offset = params[1].i32().unwrap() as usize;
        let length = params[2].i32().unwrap() as usize;
        caller.data_mut().exit_code = -2020;
        let input = caller
            .data()
            .input
            .get(offset..(offset + length))
            .unwrap()
            .to_vec();
        caller.memory_write(target, &input)?;
        Ok(())
    }

    fn fn_input_size(
        caller: &mut TypedCaller<SimpleCallContext>,
        _params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        result[0] = Value::I32(caller.data().input.len() as i32);
        Ok(())
    }

    fn fn_write_output(
        caller: &mut TypedCaller<SimpleCallContext>,
        params: &[Value],
        _result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let offset = params[0].i32().unwrap() as usize;
        let length = params[1].i32().unwrap() as usize;
        let mut buffer = vec![0u8; length];
        caller.memory_read(offset, &mut buffer)?;
        caller.data_mut().output.extend_from_slice(&buffer);
        Ok(())
    }

    fn fn_keccak256(
        caller: &mut TypedCaller<SimpleCallContext>,
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
        _ => unreachable!("rwasm: unknown function ({})", func_idx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{always_failing_syscall_handler, ImportLinker, Pages, RwasmCaller, RwasmStore};
    use alloc::sync::Arc;

    #[test]
    fn simple_call_handler_exercises_host_state_and_memory() {
        let context = SimpleCallContext {
            input: b"abcdef".to_vec(),
            state: 42,
            ..Default::default()
        };
        let mut store = RwasmStore::new(
            Arc::new(ImportLinker::default()),
            context,
            always_failing_syscall_handler,
            None,
            Some(1),
        );
        assert_eq!(
            store.global_memory.grow(Pages::new(1).unwrap()),
            Some(Pages::default())
        );
        let mut caller = TypedCaller::Rwasm(RwasmCaller::new(&mut store));

        let mut result = [Value::I32(0)];
        simple_call_handler_syscall_handler(&mut caller, 0x0002, &[], &mut result).unwrap();
        assert_eq!(result[0], Value::I32(42));

        simple_call_handler_syscall_handler(&mut caller, 0x0004, &[], &mut result).unwrap();
        assert_eq!(result[0], Value::I32(6));

        let read_params = [Value::I32(16), Value::I32(1), Value::I32(3)];
        simple_call_handler_syscall_handler(&mut caller, 0x0003, &read_params, &mut []).unwrap();
        assert_eq!(caller.memory_read_into_vec(16, 3).unwrap(), b"bcd");

        let write_params = [Value::I32(16), Value::I32(3)];
        simple_call_handler_syscall_handler(&mut caller, 0x0005, &write_params, &mut []).unwrap();
        assert_eq!(caller.data().output, b"bcd");

        let hash_params = [Value::I32(16), Value::I32(3), Value::I32(64)];
        simple_call_handler_syscall_handler(&mut caller, 0x0101, &hash_params, &mut []).unwrap();
        assert_eq!(
            caller.memory_read_into_vec(64, 32).unwrap(),
            hex_literal::hex!("c08bb9a33a7cd38850fa6ce966af52a86dba268e2d9502b4ccbd012668969455")
        );

        let exit_params = [Value::I32(7)];
        assert_eq!(
            simple_call_handler_syscall_handler(&mut caller, 0x0001, &exit_params, &mut []),
            Err(TrapCode::ExecutionHalted)
        );
        assert_eq!(caller.data().exit_code, 7);
    }
}
