use crate::{Caller, Store, SyscallHandler, TrapCode, TypedCaller, UntypedValue};
use wasmtime::{AsContext, AsContextMut, StoreLimits};

pub struct WrappedContext<T: 'static + Send + Sync> {
    pub(crate) syscall_handler: SyscallHandler<T>,
    pub(crate) fuel: Option<u64>,
    pub(crate) resource_limiter: StoreLimits,
    pub(crate) data: T,
}

pub struct WasmtimeCaller<'a, T: 'static + Send + Sync> {
    caller: wasmtime::Caller<'a, WrappedContext<T>>,
}

impl<'a, T: 'static + Send + Sync> WasmtimeCaller<'a, T> {
    pub fn wrap_typed(caller: wasmtime::Caller<'a, WrappedContext<T>>) -> TypedCaller<'a, T> {
        TypedCaller::Wasmtime(Self { caller })
    }
    pub fn unwrap(self) -> wasmtime::Caller<'a, WrappedContext<T>> {
        self.caller
    }
}

impl<'a, T: 'static + Send + Sync> Store<T> for WasmtimeCaller<'a, T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let global_memory = self
            .caller
            .get_export("memory")
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"));
        global_memory
            .read(self.caller.as_context(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let global_memory = self
            .caller
            .get_export("memory")
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("wasmtime: missing memory export, it's not possible"));
        global_memory
            .write(self.caller.as_context_mut(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn context_mut<R, F: FnOnce(&mut T) -> R>(&mut self, func: F) -> R {
        func(&mut self.caller.data_mut().data)
    }

    fn context<R, F: FnOnce(&T) -> R>(&self, func: F) -> R {
        func(&self.caller.data().data)
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
        // TODO(dmitry123): "do we want to deal with wasmtime's fuel?"
        if let Ok(fuel) = self.caller.get_fuel() {
            Some(fuel)
        } else if let Some(fuel) = self.caller.data().fuel.as_ref() {
            Some(*fuel)
        } else {
            None
        }
    }
}

impl<'a, T: 'static + Send + Sync> Caller<T> for WasmtimeCaller<'a, T> {
    fn program_counter(&self) -> u32 {
        unimplemented!("wasmtime: not allowed im wasmtime mode")
    }

    fn stack_push(&mut self, _value: UntypedValue) {
        unimplemented!("wasmtime: not allowed in wasmtime mode")
    }

    fn consume_fuel(&mut self, fuel: u64) -> Result<(), TrapCode> {
        self.try_consume_fuel(fuel)
    }
}
