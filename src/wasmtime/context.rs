use crate::{
    checked_memory_range_end, CallerTr, StoreTr, SyscallHandler, TrapCode, TypedCaller,
    N_BYTES_PER_MEMORY_PAGE,
};
use wasmtime::{AsContext, AsContextMut, StoreLimits};

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
    pub(crate) resource_limiter: StoreLimits,
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

    fn exported_memory(&self) -> Result<wasmtime::Memory, TrapCode> {
        self.caller.data().memory.ok_or(TrapCode::MemoryOutOfBounds)
    }
}

impl<'a, T: 'static> StoreTr<T> for WasmtimeCaller<'a, T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let global_memory = self.exported_memory()?;
        global_memory
            .read(self.caller.as_context(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn memory_read_into_vec(&mut self, offset: usize, length: usize) -> Result<Vec<u8>, TrapCode> {
        let end = checked_memory_range_end(offset, length)?;
        let global_memory = self.exported_memory()?;
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
        let global_memory = self.exported_memory()?;
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
