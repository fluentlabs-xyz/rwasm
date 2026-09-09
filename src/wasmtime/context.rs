use crate::{
    checked_memory_range_end, CallerTr, StoreTr, SyscallHandler, TrapCode, TypedCaller,
    N_BYTES_PER_MEMORY_PAGE,
};
use wasmtime::{AsContext, AsContextMut, StoreLimits};

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
        if self.caller.data().fuel_enabled {
            let remaining_fuel = self.caller.get_fuel().unwrap_or_else(|_| {
                unreachable!("wasmtime: fuel metering was enabled at store creation")
            });
            let new_fuel = remaining_fuel
                .checked_sub(delta)
                .ok_or(TrapCode::OutOfFuel)?;
            self.caller.set_fuel(new_fuel).unwrap_or_else(|_| {
                unreachable!("wasmtime: fuel metering was enabled at store creation")
            });
        } else if let Some(fuel) = self.caller.data_mut().fuel.as_mut() {
            *fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
        }
        Ok(())
    }

    fn remaining_fuel(&self) -> Option<u64> {
        if self.caller.data().fuel_enabled {
            self.caller.get_fuel().ok()
        } else {
            self.caller.data().fuel
        }
    }

    fn reset_fuel(&mut self, new_fuel_limit: u64) {
        if self.caller.data().fuel_enabled {
            self.caller.set_fuel(new_fuel_limit).unwrap_or_else(|_| {
                unreachable!("wasmtime: fuel metering was enabled at store creation")
            });
        } else {
            self.caller.data_mut().fuel = Some(new_fuel_limit)
        }
    }
}

impl<'a, T: 'static> CallerTr<T> for WasmtimeCaller<'a, T> {}
