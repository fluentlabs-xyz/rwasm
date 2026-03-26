use crate::{
    wasmtime::{context::WrappedContext, WasmtimeCaller},
    TrapCode, Value, F32, F64,
};
use smallvec::SmallVec;
use wasmtime::Val;

/// Wasmtime import trampoline that executes a single runtime syscall.
///
/// Maps input params and results between Wasmtime (`Val`) and rWasm (`Value`),
/// then calls `invoke_runtime_handler` with a `CallerAdapter` providing memory/context access.
///
/// Returns `Ok(())` on success, or an `anyhow::Error` that may wrap a trap.
pub fn wasmtime_syscall_handler<'a, T: 'static>(
    sys_func_idx: u32,
    caller: wasmtime::Caller<'a, WrappedContext<T>>,
    params: &[Val],
    result: &mut [Val],
) -> anyhow::Result<()> {
    // Convert input values from Wasmtime format into rWasm format.
    let mut buffer = SmallVec::<[Value; 32]>::new();
    buffer.extend(params.iter().map(|x| match x {
        Val::I32(value) => Value::I32(*value),
        Val::I64(value) => Value::I64(*value),
        Val::F32(value) => Value::F32(F32::from_bits(*value)),
        Val::F64(value) => Value::F64(F64::from_bits(*value)),
        _ => unreachable!("wasmtime: unsupported type: {:?}", x),
    }));

    // Reserve space for result values (initialized to zeros).
    buffer.extend(core::iter::repeat_n(Value::I32(0), result.len()));

    let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
    let syscall_handler = caller.data().syscall_handler;

    // Caller adapter provides memory/context operations expected by `invoke_runtime_handler`.
    let mut caller_adapter = WasmtimeCaller::<'a>::wrap_typed(caller);
    let syscall_result = syscall_handler(
        &mut caller_adapter,
        sys_func_idx,
        mapped_params,
        mapped_result,
    );

    // Treat `ExecutionHalted` as a controlled termination rather than a hard error.
    let should_terminate = syscall_result.map(|_| false).or_else(|trap_code| {
        if trap_code == TrapCode::ExecutionHalted {
            Ok(true)
        } else {
            Err(trap_code)
        }
    })?;

    // Map all values back to Wasmtime format.
    for (i, value) in mapped_result.iter().enumerate() {
        result[i] = match value {
            Value::I32(value) => Val::I32(*value),
            Value::I64(value) => Val::I64(*value),
            Value::F32(value) => Val::F32(value.to_bits()),
            Value::F64(value) => Val::F64(value.to_bits()),
            _ => unreachable!("wasmtime: unsupported type: {:?}", value),
        };
    }

    // Terminate execution if requested.
    if should_terminate {
        return Err(TrapCode::ExecutionHalted.into());
    }

    Ok(())
}
