use crate::{TrapCode, TypedCaller, Value};

pub type SyscallHandler<T> =
    fn(&mut TypedCaller<'_, T>, u32, &[Value], &mut [Value]) -> Result<(), TrapCode>;

pub fn always_failing_syscall_handler<T: 'static>(
    _caller: &mut TypedCaller<'_, T>,
    _func_idx: u32,
    _params: &[Value],
    _result: &mut [Value],
) -> Result<(), TrapCode> {
    Err(TrapCode::UnknownExternalFunction)
}
