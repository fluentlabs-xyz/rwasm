use crate::{
    checked_memory_range_end, CallerTr, StoreTr, SyscallHandler, TrapCode, TypedCaller,
    N_BYTES_PER_MEMORY_PAGE,
};
use wasmtime::{AsContext, AsContextMut, StoreLimits};

pub struct WrappedContext<T: 'static> {
    pub(crate) syscall_handler: SyscallHandler<T>,
    pub(crate) fuel: Option<u64>,
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

    fn exported_memory(&mut self) -> Result<wasmtime::Memory, TrapCode> {
        self.caller
            .get_export("memory")
            .and_then(|export| export.into_memory())
            .ok_or(TrapCode::MemoryOutOfBounds)
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
        if let Ok(remaining_fuel) = self.caller.get_fuel() {
            let new_fuel = remaining_fuel
                .checked_sub(delta)
                .ok_or(TrapCode::OutOfFuel)?;
            self.caller
                .set_fuel(new_fuel)
                .unwrap_or_else(|_| unreachable!("wasmtime: fuel mode is disabled in wasmtime"));
        } else if let Some(fuel) = self.caller.data_mut().fuel.as_mut() {
            *fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
        }
        Ok(())
    }

    fn remaining_fuel(&self) -> Option<u64> {
        if let Ok(fuel) = self.caller.get_fuel() {
            Some(fuel)
        } else {
            self.caller.data().fuel.as_ref().copied()
        }
    }

    fn reset_fuel(&mut self, new_fuel_limit: u64) {
        let has_fuel_enabled = self.caller.get_fuel().is_ok();
        if has_fuel_enabled {
            self.caller.set_fuel(new_fuel_limit).unwrap();
        } else {
            self.caller.data_mut().fuel = Some(new_fuel_limit)
        }
    }
}

impl<'a, T: 'static> CallerTr<T> for WasmtimeCaller<'a, T> {}
