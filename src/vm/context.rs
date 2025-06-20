use crate::{
    types::{TrapCode, UntypedValue},
    RwasmExecutor,
};
use core::cell::{Ref, RefMut};

pub trait Caller<T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode>;

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode>;

    fn program_counter(&self) -> u32;

    fn sync_stack_ptr(&mut self);

    fn context_mut(&mut self) -> RefMut<'_, T>;

    fn context(&self) -> Ref<'_, T>;

    fn stack_push(&mut self, value: UntypedValue);

    fn remaining_fuel(&mut self) -> Option<u64>;

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode>;
}

pub struct RwasmCaller<'vm, 'a, T> {
    vm: &'vm mut RwasmExecutor<'a, T>,
}

impl<'vm, 'a, T> RwasmCaller<'vm, 'a, T> {
    pub fn new(vm: &'vm mut RwasmExecutor<'a, T>) -> Self {
        Self { vm }
    }
}

impl<'vm, 'a, T> Caller<T> for RwasmCaller<'vm, 'a, T> {
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

    fn program_counter(&self) -> u32 {
        self.vm.program_counter()
    }

    fn sync_stack_ptr(&mut self) {
        self.vm.value_stack.sync_stack_ptr(self.vm.sp);
    }

    fn context_mut(&mut self) -> RefMut<T> {
        self.vm.store.context.borrow_mut()
    }

    fn context(&self) -> Ref<T> {
        self.vm.store.context.borrow()
    }

    fn stack_push(&mut self, value: UntypedValue) {
        self.vm.sp.push(value);
    }

    fn remaining_fuel(&mut self) -> Option<u64> {
        self.vm.store.remaining_fuel()
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        self.vm.store.try_consume_fuel(delta)
    }
}
