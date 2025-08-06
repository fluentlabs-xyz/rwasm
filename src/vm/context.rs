use crate::{
    types::{TrapCode, UntypedValue},
    Caller, RwasmStore, Store, ValueStackPtr,
};

pub struct RwasmCaller<'a, T: Send + Sync + 'static> {
    store: &'a mut RwasmStore<T>,
    program_counter: u32,
    sp: ValueStackPtr,
}

impl<'a, T: Send + Sync> RwasmCaller<'a, T> {
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

impl<'a, T: Send + Sync> Store<T> for RwasmCaller<'a, T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        self.store.global_memory.read(offset, buffer)?;
        Ok(())
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        self.store.global_memory.write(offset, buffer)?;
        #[cfg(feature = "tracing")]
        self.vm
            .store
            .tracer
            .memory_change(offset as u32, buffer.len() as u32, buffer);
        Ok(())
    }

    fn context_mut<R, F: FnMut(&mut T) -> R>(&mut self, mut func: F) -> R {
        func(&mut self.store.context.borrow_mut())
    }

    fn context<R, F: Fn(&T) -> R>(&self, func: F) -> R {
        func(&self.store.context.borrow())
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        self.store.try_consume_fuel(delta)
    }

    fn remaining_fuel(&mut self) -> Option<u64> {
        self.store.remaining_fuel()
    }
}

impl<'a, T: Send + Sync> Caller<T> for RwasmCaller<'a, T> {
    fn program_counter(&self) -> u32 {
        self.program_counter
    }

    fn stack_push(&mut self, value: UntypedValue) {
        self.sp.push(value);
    }
}
