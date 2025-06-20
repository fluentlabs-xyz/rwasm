use crate::{
    split_i64_to_i32,
    types::{TrapCode, UntypedValue},
    vm::store::Store,
    InstructionPtr,
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
}

pub struct RwasmCaller<'vm, 'a, T> {
    vm: &'vm mut RwasmExecutor<'a, T>,
}

impl<'vm, 'a, T> RwasmCaller<'vm, 'a, T> {
    pub fn new(vm: &'vm mut RwasmExecutor<'a, T>) -> Self {
        Self { vm }
    }

    pub fn stack_push<I: Into<UntypedValue>>(&mut self, value: I) {
        self.vm.sp.push_as(value);
    }

    pub fn stack_push_u32(&mut self, value: u32) {
        self.vm.sp.push(UntypedValue::from_bits(value));
    }

    pub fn stack_push_i32(&mut self, value: i32) {
        self.vm.sp.push(UntypedValue::from_bits(value as u32));
    }

    pub fn stack_push_u64(&mut self, value: u64) {
        let (lo, hi) = split_i64_to_i32(value as i64);
        self.vm.sp.push(UntypedValue::from_bits(lo as u32));
        self.vm.sp.push(UntypedValue::from_bits(hi as u32));
    }

    pub fn stack_push_i64(&mut self, value: i64) {
        let (lo, hi) = split_i64_to_i32(value);
        self.vm.sp.push(UntypedValue::from_bits(lo as u32));
        self.vm.sp.push(UntypedValue::from_bits(hi as u32));
    }

    pub fn stack_pop(&mut self) -> UntypedValue {
        self.vm.sp.pop()
    }

    pub fn stack_pop_as<I: From<UntypedValue>>(&mut self) -> I {
        let lhs = self.stack_pop();
        I::from(lhs)
    }

    pub fn stack_pop2_as<I: From<UntypedValue>>(&mut self) -> (I, I) {
        let (lhs, rhs) = self.stack_pop2();
        (I::from(lhs), I::from(rhs))
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

    pub fn instruction_ptr(&self) -> InstructionPtr {
        self.vm.ip
    }

    pub fn store_mut(&mut self) -> &mut Store<T> {
        &mut self.vm.store
    }

    pub fn store(&self) -> &Store<T> {
        &self.vm.store
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
}
