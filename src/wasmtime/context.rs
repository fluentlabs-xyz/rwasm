use crate::{
    checked_memory_range_end, compiler::block_fuel::syscall_fuel_temporary_slots,
    wasmtime::engine::VALUE_STACK_WINDOW, CallerTr, StoreTr, SyscallHandler, TrapCode, TypedCaller,
    Value, N_BYTES_PER_MEMORY_PAGE,
};
use rwasm_fuel_policy::{SyscallFuelParams, FUEL_MAX_LINEAR_X, FUEL_MAX_QUADRATIC_X};
use std::collections::HashMap;
use wasmparser::ValType;
use wasmtime::{AsContext, AsContextMut, ResourceLimiter, StoreLimits};

const ENGINE_FUEL_EXPECTED: &str = "wasmtime: fuel metering was enabled at store creation";

/// Charges `delta` fuel against the engine counter or, when the engine does not meter fuel,
/// against the soft counter kept in the context.
pub(crate) fn try_consume_fuel<T: 'static>(
    mut ctx: impl AsContextMut<Data = WrappedContext<T>>,
    delta: u64,
) -> Result<(), TrapCode> {
    let mut ctx = ctx.as_context_mut();
    if ctx.data().fuel_enabled {
        let remaining_fuel = ctx.get_fuel().expect(ENGINE_FUEL_EXPECTED);
        let new_fuel = remaining_fuel
            .checked_sub(delta)
            .ok_or(TrapCode::OutOfFuel)?;
        ctx.set_fuel(new_fuel).expect(ENGINE_FUEL_EXPECTED);
    } else if let Some(fuel) = ctx.data_mut().fuel.as_mut() {
        *fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
    }
    Ok(())
}

/// Returns the remaining fuel from whichever counter is active, if any.
pub(crate) fn remaining_fuel<T: 'static>(
    ctx: impl AsContext<Data = WrappedContext<T>>,
) -> Option<u64> {
    let ctx = ctx.as_context();
    if ctx.data().fuel_unbounded {
        return None;
    }
    if ctx.data().fuel_enabled {
        ctx.get_fuel().ok()
    } else {
        ctx.data().fuel
    }
}

/// Resets whichever counter is active to `new_fuel_limit`.
pub(crate) fn reset_fuel<T: 'static>(
    mut ctx: impl AsContextMut<Data = WrappedContext<T>>,
    new_fuel_limit: u64,
) {
    let mut ctx = ctx.as_context_mut();
    if ctx.data().fuel_enabled {
        ctx.set_fuel(new_fuel_limit).expect(ENGINE_FUEL_EXPECTED);
        ctx.data_mut().fuel_unbounded = false;
    } else {
        ctx.data_mut().fuel = Some(new_fuel_limit);
    }
}

/// Computes the syscall fuel `policy` charges for a call with `params`, or `None` when the policy
/// charges nothing.
///
/// This is the host-side twin of the trampoline prologue `compile_block_params` emits into rwasm
/// bytecode and follows it operation by operation, including its 32-bit wrapping arithmetic, so a
/// host call is charged the same amount on both strategies whichever way the guest reached it.
/// The metered parameter is addressed like `LinearFuelParams::param_index` /
/// `QuadraticFuelParams::local_depth`: counted from the last parameter, `1` being the last.
///
/// # Errors
///
/// - [`TrapCode::IntegerOverflow`] when the metered parameter exceeds the policy's bound
///   (`FUEL_MAX_LINEAR_X` / `FUEL_MAX_QUADRATIC_X`), the guard the rwasm prologue runs first.
/// - [`TrapCode::IntegerDivisionByZero`] for a quadratic policy with a zero `divisor`, which the
///   prologue's `i32.div_u` traps on.
/// - [`TrapCode::BadSignature`] when the metered parameter does not exist or is not an `i32`:
///   the rwasm compiler rejects such a linker entry (`InvalidSyscallFuelParam`), so a Wasmtime
///   executor only sees it through an import linker the module was not compiled with.
pub(crate) fn syscall_fuel_charge(
    policy: &SyscallFuelParams,
    params: &[Value],
) -> Result<Option<u64>, TrapCode> {
    fn metered_param(params: &[Value], param_index: u32) -> Result<u32, TrapCode> {
        usize::try_from(param_index)
            .ok()
            .filter(|index| *index >= 1)
            .and_then(|index| params.len().checked_sub(index))
            .and_then(|index| params.get(index))
            .and_then(Value::i32)
            .map(|value| value as u32)
            .ok_or(TrapCode::BadSignature)
    }
    // rounds bytes up to 32-byte words with the wrapping `i32.add` the bytecode uses
    fn words(bytes: u32) -> u32 {
        bytes.wrapping_add(31) / 32
    }
    Ok(match policy {
        SyscallFuelParams::None => None,
        // the bytecode carries the base as a `ConsumeFuel(u32)` immediate
        SyscallFuelParams::Const(base) => Some(u64::from(*base as u32)),
        SyscallFuelParams::LinearFuel(fuel_params) => {
            let bytes = metered_param(params, fuel_params.param_index)?;
            if bytes > FUEL_MAX_LINEAR_X {
                return Err(TrapCode::IntegerOverflow);
            }
            let fuel = words(bytes)
                .wrapping_mul(fuel_params.word_cost)
                .wrapping_add(fuel_params.base_fuel);
            Some(u64::from(fuel))
        }
        SyscallFuelParams::QuadraticFuel(fuel_params) => {
            let bytes = metered_param(params, fuel_params.local_depth)?;
            if bytes > FUEL_MAX_QUADRATIC_X {
                return Err(TrapCode::IntegerOverflow);
            }
            let words = words(bytes);
            let linear = words.wrapping_mul(fuel_params.word_cost);
            let quadratic = words
                .wrapping_mul(words)
                .checked_div(fuel_params.divisor)
                .ok_or(TrapCode::IntegerDivisionByZero)?;
            let fuel = linear
                .wrapping_add(quadratic)
                .wrapping_mul(fuel_params.fuel_denom_rate);
            Some(u64::from(fuel))
        }
    })
}

/// The 32-bit value-stack slots `values` take on the rwasm VM.
fn value_slots(values: &[Value]) -> u32 {
    values
        .iter()
        .map(|value| match value {
            Value::I64(_) | Value::F64(_) => 2,
            _ => 1,
        })
        .sum()
}

/// The 32-bit value-stack slots values of `types` take on the rwasm VM.
fn type_slots(types: &[ValType]) -> u32 {
    types
        .iter()
        .map(|ty| match ty {
            ValType::I64 | ValType::F64 => 2,
            _ => 1,
        })
        .sum()
}

/// Traps `StackOverflow` unless `need` more slots fit the value-stack window above `base`, like
/// `ValueStack::reserve` on the rwasm VM.
fn check_value_stack_window(base: u32, need: u32) -> Result<(), TrapCode> {
    if u64::from(base) + u64::from(need) > u64::from(VALUE_STACK_WINDOW) {
        return Err(TrapCode::StackOverflow);
    }
    Ok(())
}

/// Performs the `StackCheck` of the rwasm import trampoline for the host function about to run.
///
/// On rwasm every path into an import goes through a trampoline frame of its own: the caller's
/// `CallInternal` pushes it (the engine checks the call depth at the call site, see
/// `rwasm_stack_limits` in the engine module), and its prologue reserves the temporaries
/// of its syscall fuel prologue on top of the import's `params`, which the caller left on the
/// value stack above the frame base it published through the store's rwasm stack counters. A
/// module compiled without `builtins_consume_fuel` has no schedule and no temporaries.
pub(crate) fn check_syscall_frame<T: 'static>(
    ctx: impl AsContext<Data = WrappedContext<T>>,
    sys_func_idx: u32,
    params: &[Value],
) -> Result<(), TrapCode> {
    let ctx = ctx.as_context();
    let temporaries = ctx
        .data()
        .syscall_fuel
        .get(&sys_func_idx)
        .map_or(0, syscall_fuel_temporary_slots);
    let base = ctx.rwasm_stack_counters().stack_slots;
    check_value_stack_window(base, value_slots(params).saturating_add(temporaries))
}

/// Reserves the slots of the host function's results, after the syscall fuel was charged: the
/// rwasm `Call` pops the parameters and reserves the result slots before invoking the host
/// (`RwasmExecutor::invoke_syscall`), so a full stack traps here rather than in the host.
pub(crate) fn check_syscall_results_room<T: 'static>(
    ctx: impl AsContext<Data = WrappedContext<T>>,
    result_types: &[ValType],
) -> Result<(), TrapCode> {
    let base = ctx.as_context().rwasm_stack_counters().stack_slots;
    check_value_stack_window(base, type_slots(result_types))
}

/// Charges the syscall fuel the store's schedule assigns to `sys_func_idx`, if any, before the
/// host function runs. See [`syscall_fuel_charge`] for the amount and the traps.
pub(crate) fn charge_syscall_fuel<T: 'static>(
    mut ctx: impl AsContextMut<Data = WrappedContext<T>>,
    sys_func_idx: u32,
    params: &[Value],
) -> Result<(), TrapCode> {
    let mut ctx = ctx.as_context_mut();
    let Some(policy) = ctx.data().syscall_fuel.get(&sys_func_idx) else {
        return Ok(());
    };
    match syscall_fuel_charge(policy, params)? {
        Some(fuel) => try_consume_fuel(&mut ctx, fuel),
        None => Ok(()),
    }
}

/// Store limits that remember which resource denied a grow.
///
/// Wasmtime reports a denied grow during instantiation as a plain error message. The rwasm
/// entrypoint prologue traps with `MemoryOutOfBounds`/`TableOutOfBounds` for the same module, so
/// the recorded resource lets [`crate::wasmtime::WasmtimeExecutor::new`] report the matching trap
/// instead of a generic error.
pub struct RecordingStoreLimits {
    inner: StoreLimits,
    /// The trap matching the most recently denied grow, if any.
    denied: Option<TrapCode>,
}

impl RecordingStoreLimits {
    pub(crate) fn new(inner: StoreLimits) -> Self {
        Self {
            inner,
            denied: None,
        }
    }

    /// Returns the trap of the most recent denied grow since the last [`Self::reset_denied`].
    pub(crate) fn denied(&self) -> Option<TrapCode> {
        self.denied
    }

    /// Forgets any recorded denial, so a following failure is attributed to its own grow.
    pub(crate) fn reset_denied(&mut self) {
        self.denied = None;
    }
}

impl ResourceLimiter for RecordingStoreLimits {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let allowed = self.inner.memory_growing(current, desired, maximum)?;
        if !allowed {
            self.denied = Some(TrapCode::MemoryOutOfBounds);
        }
        Ok(allowed)
    }

    fn memory_grow_failed(&mut self, error: wasmtime::Error) -> wasmtime::Result<()> {
        self.inner.memory_grow_failed(error)
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let allowed = self.inner.table_growing(current, desired, maximum)?;
        if !allowed {
            self.denied = Some(TrapCode::TableOutOfBounds);
        }
        Ok(allowed)
    }

    fn table_grow_failed(&mut self, error: wasmtime::Error) -> wasmtime::Result<()> {
        self.inner.table_grow_failed(error)
    }

    fn instances(&self) -> usize {
        self.inner.instances()
    }

    fn tables(&self) -> usize {
        self.inner.tables()
    }

    fn memories(&self) -> usize {
        self.inner.memories()
    }
}

pub struct WrappedContext<T: 'static> {
    pub(crate) syscall_handler: SyscallHandler<T>,
    /// Soft fuel counter used when the engine does not meter fuel itself.
    pub(crate) fuel: Option<u64>,
    /// Whether the engine meters fuel for this store.
    ///
    /// Resolved once at store creation: probing `get_fuel()` on every fuel access builds an
    /// error value each time metering is off, which is the common case for self-metered
    /// runtimes.
    pub(crate) fuel_enabled: bool,
    /// Set when the executor was created without a fuel limit while the engine meters fuel:
    /// the store then holds `u64::MAX` and reports no remaining fuel, like an rwasm store
    /// without a limit.
    pub(crate) fuel_unbounded: bool,
    /// The instance's exported memory, resolved once per instantiation so host calls don't
    /// look it up by name.
    pub(crate) memory: Option<wasmtime::Memory>,
    /// Syscall fuel charged before each host function runs, by syscall index: the module's
    /// schedule ([`crate::wasmtime::WasmtimeModule::syscall_fuel`]) resolved through the
    /// executor's import linker. Empty when the module was compiled without
    /// `builtins_consume_fuel`.
    pub(crate) syscall_fuel: HashMap<u32, SyscallFuelParams>,
    pub(crate) resource_limiter: RecordingStoreLimits,
    pub(crate) data: T,
}

pub struct WasmtimeCaller<'a, T: 'static> {
    caller: wasmtime::Caller<'a, WrappedContext<T>>,
}

impl<'a, T: 'static> WasmtimeCaller<'a, T> {
    pub fn wrap_typed(caller: wasmtime::Caller<'a, WrappedContext<T>>) -> TypedCaller<'a, T> {
        TypedCaller::Wasmtime(Self { caller })
    }
    pub fn unwrap(self) -> wasmtime::Caller<'a, WrappedContext<T>> {
        self.caller
    }
}

/// Host access to a module that exports no memory.
///
/// The rwasm VM gives every instance a memory of zero pages, so an empty access at offset 0
/// succeeds there and every other range is out of bounds. Answer the same way instead of
/// failing every access, or a guest that hands the host an empty range would run on one
/// strategy and trap on the other.
pub(crate) fn missing_memory_access(offset: usize, length: usize) -> Result<(), TrapCode> {
    if offset == 0 && length == 0 {
        Ok(())
    } else {
        Err(TrapCode::MemoryOutOfBounds)
    }
}

impl<'a, T: 'static> StoreTr<T> for WasmtimeCaller<'a, T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let Some(global_memory) = self.caller.data().memory else {
            return missing_memory_access(offset, buffer.len());
        };
        global_memory
            .read(self.caller.as_context(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn memory_read_into_vec(&mut self, offset: usize, length: usize) -> Result<Vec<u8>, TrapCode> {
        let end = checked_memory_range_end(offset, length)?;
        let Some(global_memory) = self.caller.data().memory else {
            return missing_memory_access(offset, length).map(|()| Vec::new());
        };
        let memory_size = (global_memory.size(self.caller.as_context()) as usize)
            .checked_mul(N_BYTES_PER_MEMORY_PAGE as usize)
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        if end > memory_size {
            return Err(TrapCode::MemoryOutOfBounds);
        }
        let mut data = vec![0u8; length];
        self.memory_read(offset, &mut data)?;
        Ok(data)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let Some(global_memory) = self.caller.data().memory else {
            return missing_memory_access(offset, buffer.len());
        };
        global_memory
            .write(self.caller.as_context_mut(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn data_mut(&mut self) -> &mut T {
        &mut self.caller.data_mut().data
    }

    fn data(&self) -> &T {
        &self.caller.data().data
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        try_consume_fuel(&mut self.caller, delta)
    }

    fn remaining_fuel(&self) -> Option<u64> {
        remaining_fuel(&self.caller)
    }

    fn reset_fuel(&mut self, new_fuel_limit: u64) {
        reset_fuel(&mut self.caller, new_fuel_limit)
    }
}

impl<'a, T: 'static> CallerTr<T> for WasmtimeCaller<'a, T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use rwasm_fuel_policy::{LinearFuelParams, QuadraticFuelParams};

    fn linear(param_index: u32) -> SyscallFuelParams {
        SyscallFuelParams::LinearFuel(LinearFuelParams {
            base_fuel: 7,
            param_index,
            word_cost: 5,
        })
    }

    fn quadratic(local_depth: u32, divisor: u32) -> SyscallFuelParams {
        SyscallFuelParams::QuadraticFuel(QuadraticFuelParams {
            local_depth,
            word_cost: 3,
            divisor,
            fuel_denom_rate: 4,
        })
    }

    /// The host-side charge is the amount the rwasm trampoline prologue would charge for the
    /// same call, so a syscall reached through a table entry pays what a direct call pays.
    #[test]
    fn charge_follows_the_trampoline_prologue() {
        assert_eq!(syscall_fuel_charge(&SyscallFuelParams::None, &[]), Ok(None));
        assert_eq!(
            syscall_fuel_charge(&SyscallFuelParams::Const(17), &[]),
            Ok(Some(17))
        );
        // 300 bytes are 10 words: 10 * 5 + 7
        assert_eq!(
            syscall_fuel_charge(&linear(1), &[Value::I32(300)]),
            Ok(Some(57))
        );
        // `param_index` counts from the last parameter, whatever its width
        assert_eq!(
            syscall_fuel_charge(&linear(2), &[Value::I32(320), Value::I64(0)]),
            Ok(Some(57))
        );
        // (10 * 3 + 10 * 10 / 2) * 4
        assert_eq!(
            syscall_fuel_charge(&quadratic(1, 2), &[Value::I32(300)]),
            Ok(Some(320))
        );
    }

    /// The prologue's overflow guard runs before anything is charged.
    #[test]
    fn oversized_parameter_traps_with_integer_overflow() {
        assert_eq!(
            syscall_fuel_charge(&linear(1), &[Value::I32(FUEL_MAX_LINEAR_X as i32)]),
            Ok(Some(u64::from(FUEL_MAX_LINEAR_X / 32) * 5 + 7))
        );
        assert_eq!(
            syscall_fuel_charge(&linear(1), &[Value::I32(FUEL_MAX_LINEAR_X as i32 + 1)]),
            Err(TrapCode::IntegerOverflow)
        );
        assert_eq!(
            syscall_fuel_charge(
                &quadratic(1, 2),
                &[Value::I32(FUEL_MAX_QUADRATIC_X as i32 + 1)]
            ),
            Err(TrapCode::IntegerOverflow)
        );
        // a negative i32 is a huge unsigned length, like the `i32.gt_u` in the prologue sees it
        assert_eq!(
            syscall_fuel_charge(&linear(1), &[Value::I32(-1)]),
            Err(TrapCode::IntegerOverflow)
        );
    }

    #[test]
    fn misaddressed_metered_parameter_is_a_bad_signature() {
        assert_eq!(
            syscall_fuel_charge(&linear(0), &[Value::I32(1)]),
            Err(TrapCode::BadSignature)
        );
        assert_eq!(
            syscall_fuel_charge(&linear(2), &[Value::I32(1)]),
            Err(TrapCode::BadSignature)
        );
        assert_eq!(
            syscall_fuel_charge(&linear(1), &[Value::I64(1)]),
            Err(TrapCode::BadSignature)
        );
        assert_eq!(
            syscall_fuel_charge(&quadratic(1, 0), &[Value::I32(64)]),
            Err(TrapCode::IntegerDivisionByZero)
        );
    }
}
