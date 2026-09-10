use crate::{
    wasmtime::{context::WrappedContext, WasmtimeCaller},
    TrapCode, Value, F32, F64,
};
use core::mem::MaybeUninit;
use smallvec::SmallVec;
use wasmparser::ValType;
use wasmtime::{Val, ValRaw};

/// Wasmtime import trampoline that executes a single runtime syscall.
///
/// Maps input params and results between Wasmtime (`Val`) and rWasm (`Value`),
/// then calls `invoke_runtime_handler` with a `CallerAdapter` providing memory/context access.
///
/// Returns `Ok(())` on success, or a Wasmtime error that may wrap a trap.
pub fn wasmtime_syscall_handler<'a, T: 'static>(
    sys_func_idx: u32,
    caller: wasmtime::Caller<'a, WrappedContext<T>>,
    params: &[Val],
    result: &mut [Val],
) -> wasmtime::Result<()> {
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
    let should_terminate = syscall_result
        .map(|_| false)
        .or_else(|trap_code| {
            if trap_code == TrapCode::ExecutionHalted {
                Ok(true)
            } else {
                Err(trap_code)
            }
        })
        .map_err(wasmtime::Error::new)?;

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
        return Err(wasmtime::Error::new(TrapCode::ExecutionHalted));
    }

    Ok(())
}

/// Wasmtime import trampoline over raw value slots, for imports whose signature is numeric only.
///
/// Reads the parameters straight out of the raw slots and writes the results back into them,
/// skipping the per-value `Val` boxing the checked trampoline pays on both sides.
///
/// # Safety
///
/// `params` and `result` must be exactly the signature the import was registered with, and
/// must contain numeric types only. Wasmtime then guarantees that the first `params.len()`
/// slots hold initialized values of those types and that `slots` has room for the results,
/// and this function writes exactly `result.len()` values of the declared result types.
pub unsafe fn wasmtime_syscall_handler_raw<'a, T: 'static>(
    sys_func_idx: u32,
    params: &'static [ValType],
    result: &'static [ValType],
    caller: wasmtime::Caller<'a, WrappedContext<T>>,
    slots: &mut [MaybeUninit<ValRaw>],
) -> wasmtime::Result<()> {
    let mut buffer = SmallVec::<[Value; 32]>::new();
    for (slot, ty) in slots.iter().zip(params) {
        // SAFETY: wasmtime initializes the parameter slots before invoking the host function.
        let raw = unsafe { slot.assume_init_ref() };
        buffer.push(match ty {
            ValType::I32 => Value::I32(raw.get_i32()),
            ValType::I64 => Value::I64(raw.get_i64()),
            ValType::F32 => Value::F32(F32::from_bits(raw.get_f32())),
            ValType::F64 => Value::F64(F64::from_bits(raw.get_f64())),
            _ => unreachable!("wasmtime: raw trampoline registered for a non-numeric import"),
        });
    }
    buffer.extend(core::iter::repeat_n(Value::I32(0), result.len()));

    let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
    let syscall_handler = caller.data().syscall_handler;
    let mut caller_adapter = WasmtimeCaller::<'a>::wrap_typed(caller);
    let syscall_result = syscall_handler(
        &mut caller_adapter,
        sys_func_idx,
        mapped_params,
        mapped_result,
    );

    // Treat `ExecutionHalted` as a controlled termination rather than a hard error.
    let should_terminate = syscall_result
        .map(|_| false)
        .or_else(|trap_code| {
            if trap_code == TrapCode::ExecutionHalted {
                Ok(true)
            } else {
                Err(trap_code)
            }
        })
        .map_err(wasmtime::Error::new)?;

    // Terminate execution if requested; wasmtime discards the results of a failed host call.
    if should_terminate {
        return Err(wasmtime::Error::new(TrapCode::ExecutionHalted));
    }

    // A handler that produces a value of the wrong type must not reach wasm: the slot would be
    // reinterpreted as the declared type.
    for ((slot, value), ty) in slots.iter_mut().zip(mapped_result.iter()).zip(result) {
        let raw = match (value, ty) {
            (Value::I32(value), ValType::I32) => ValRaw::i32(*value),
            (Value::I64(value), ValType::I64) => ValRaw::i64(*value),
            (Value::F32(value), ValType::F32) => ValRaw::f32(value.to_bits()),
            (Value::F64(value), ValType::F64) => ValRaw::f64(value.to_bits()),
            _ => return Err(wasmtime::Error::new(TrapCode::BadSignature)),
        };
        slot.write(raw);
    }

    Ok(())
}
