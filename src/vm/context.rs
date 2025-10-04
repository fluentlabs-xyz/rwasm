use crate::{
    types::{TrapCode, UntypedValue},
    Caller, RwasmStore, Store, ValueStackPtr,
};

pub struct RwasmCaller<'a, T: 'static + Send + Sync> {
    store: &'a mut RwasmStore<T>,
    program_counter: u32,
    sp: ValueStackPtr,
}

impl<'a, T: 'static + Send + Sync> RwasmCaller<'a, T> {
    pub fn new(store: &'a mut RwasmStore<T>, program_counter: u32, sp: ValueStackPtr) -> Self {
        Self {
            store,
            program_counter,
            sp,
        }
    }

    pub fn sp(&self) -> ValueStackPtr {
        self.sp
    }
}

impl<'a, T: 'static + Send + Sync> Store<T> for RwasmCaller<'a, T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        self.store.get_global_memory().read(offset, buffer)?;
        Ok(())
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        self.store.get_global_memory().write(offset, buffer)?;
        #[cfg(feature = "tracing")]
        self.store
            .tracer
            .memory_change(offset as u32, buffer.len() as u32, buffer);
        Ok(())
    }

    fn context_mut<R, F: FnOnce(&mut T) -> R>(&mut self, func: F) -> R {
        func(&mut self.store.context)
    }

    fn context<R, F: FnOnce(&T) -> R>(&self, func: F) -> R {
        func(&self.store.context)
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        self.store.try_consume_fuel(delta)
    }

    fn remaining_fuel(&self) -> Option<u64> {
        self.store.remaining_fuel()
    }
}

impl<'a, T: 'static + Send + Sync> Caller<T> for RwasmCaller<'a, T> {
    fn program_counter(&self) -> u32 {
        self.program_counter
    }

    fn stack_push(&mut self, value: UntypedValue) {
        self.sp.push(value);
    }
}
