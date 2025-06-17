use crate::{
    types::{TrapCode, UntypedValue},
    vm::store::Store,
    InstructionPtr,
    RwasmExecutor,
};
use alloc::{vec, vec::Vec};

pub struct Caller<'vm, 'a, T> {
    vm: &'vm mut RwasmExecutor<'a, T>,
}

impl<'vm, 'a, T> Caller<'vm, 'a, T> {
    pub fn new(vm: &'vm mut RwasmExecutor<'a, T>) -> Self {
        Self { vm }
    }

    pub fn stack_push<I: Into<UntypedValue>>(&mut self, value: I) {
        self.vm.sp.push_as(value);
    }

    pub fn sync_stack_ptr(&mut self) {
        self.vm.value_stack.sync_stack_ptr(self.vm.sp);
    }

    pub fn stack_pop(&mut self) -> UntypedValue {
        self.vm.sp.pop()
    }

    pub fn stack_pop_u32(&mut self) -> u32 {
        self.vm.sp.pop().to_bits()
    }

    pub fn stack_pop_i32(&mut self) -> i32 {
        self.vm.sp.pop().to_bits() as i32
    }

    pub fn stack_pop_u64(&mut self) -> u64 {
        let (lo, hi) = self.vm.sp.pop2();
        let value = ((hi.to_bits() as u64) << 32) | (lo.to_bits() as u64);
        value
    }

    pub fn stack_pop_i64(&mut self) -> i64 {
        let (lo, hi) = self.vm.sp.pop2();
        let value = ((hi.to_bits() as u64) << 32) | (lo.to_bits() as u64);
        value as i64
    }

    pub fn stack_pop2(&mut self) -> (UntypedValue, UntypedValue) {
        self.vm.sp.pop2()
    }

    pub fn stack_pop3(&mut self) -> (UntypedValue, UntypedValue, UntypedValue) {
        self.vm.sp.pop3()
    }

    pub fn stack_pop_n<const N: usize>(&mut self) -> [UntypedValue; N] {
        let mut result: [UntypedValue; N] = [UntypedValue::default(); N];
        for i in 0..N {
            result[N - i - 1] = self.vm.sp.pop();
        }
        result
    }

    pub fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        self.vm.store.global_memory.read(offset, buffer)?;
        Ok(())
    }

    pub fn memory_read_fixed<const N: usize>(&self, offset: usize) -> Result<[u8; N], TrapCode> {
        let mut buffer = [0u8; N];
        self.vm.store.global_memory.read(offset, &mut buffer)?;
        Ok(buffer)
    }

    pub fn memory_read_vec(&self, offset: usize, length: usize) -> Result<Vec<u8>, TrapCode> {
        let mut buffer = vec![0u8; length];
        self.vm.store.global_memory.read(offset, &mut buffer)?;
        Ok(buffer)
    }

    pub fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        self.vm.store.global_memory.write(offset, buffer)?;
        #[cfg(feature = "tracing")]
        if let Some(tracer) = self.vm.store.tracer.as_mut() {
            tracer.memory_change(offset as u32, buffer.len() as u32, buffer);
        }
        Ok(())
    }

    pub fn program_counter(&self) -> u32 {
        self.vm.program_counter()
    }

    pub fn instruction_ptr(&self) -> InstructionPtr {
        self.vm.ip
    }

    pub fn store_mut(&mut self) -> &mut Store<T> {
        &mut self.vm.store
    }

    pub fn store(&self) -> &Store<T> {
        &self.vm.store
    }

    pub fn context_mut(&mut self) -> &mut T {
        &mut self.vm.store.context
    }

    pub fn context(&self) -> &T {
        &self.vm.store.context
    }
}
