use crate::{
    types::{TrapCode, UntypedValue},
    Caller,
    RwasmExecutor,
    Store,
};
use core::cell::{Ref, RefMut};

pub struct RwasmCaller<'vm, 'a, T> {
    vm: &'vm mut RwasmExecutor<'a, T>,
}

impl<'vm, 'a, T> RwasmCaller<'vm, 'a, T> {
    pub fn new(vm: &'vm mut RwasmExecutor<'a, T>) -> Self {
        Self { vm }
    }
}

impl<'vm, 'a, T> Store<T> for RwasmCaller<'vm, 'a, T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        self.vm.store.global_memory.read(offset, buffer)?;
        Ok(())
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        self.vm.store.global_memory.write(offset, buffer)?;
        #[cfg(feature = "tracing")]
        self.vm
            .store
            .tracer
            .memory_change(offset as u32, buffer.len() as u32, buffer);
        Ok(())
    }

    fn context_mut(&mut self) -> RefMut<T> {
        self.vm.store.context.borrow_mut()
    }

    fn context(&self) -> Ref<T> {
        self.vm.store.context.borrow()
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        self.vm.store.try_consume_fuel(delta)
    }

    fn remaining_fuel(&mut self) -> Option<u64> {
        self.vm.store.remaining_fuel()
    }
}

impl<'vm, 'a, T> Caller<T> for RwasmCaller<'vm, 'a, T> {
    fn program_counter(&self) -> u32 {
        self.vm.program_counter()
    }

    fn sync_stack_ptr(&mut self) {
        self.vm.value_stack.sync_stack_ptr(self.vm.sp);
    }

    fn stack_push(&mut self, value: UntypedValue) {
        self.vm.sp.push(value);
    }
}
