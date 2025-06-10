use crate::{
    types::{TrapCode, UntypedValue},
    vm::store::Store,
    InstructionPtr,
    ValueStackPtr,
};
use alloc::{vec, vec::Vec};

pub struct Caller<'a, T> {
    store: &'a mut Store<T>,
    sp: &'a mut ValueStackPtr,
    program_counter: u32,
    ip: InstructionPtr,
}

impl<'a, T> Caller<'a, T> {
    pub fn new(
        store: &'a mut Store<T>,
        sp: &'a mut ValueStackPtr,
        program_counter: u32,
        ip: InstructionPtr,
    ) -> Self {
        Self {
            store,
            sp,
            program_counter,
            ip,
        }
    }

    pub fn stack_push<I: Into<UntypedValue>>(&mut self, value: I) {
        self.sp.push_as(value);
    }

    pub fn stack_pop(&mut self) -> UntypedValue {
        self.sp.pop()
    }

    pub fn stack_pop_as<I: From<UntypedValue>>(&mut self) -> I {
        self.sp.pop_as()
    }

    pub fn stack_pop2(&mut self) -> (UntypedValue, UntypedValue) {
        self.sp.pop2()
    }

    pub fn stack_pop2_as<I: From<UntypedValue>>(&mut self) -> (I, I) {
        let (lhs, rhs) = self.stack_pop2();
        (I::from(lhs), I::from(rhs))
    }

    pub fn stack_pop_n<const N: usize>(&mut self) -> [UntypedValue; N] {
        let mut result: [UntypedValue; N] = [UntypedValue::default(); N];
        for i in 0..N {
            result[N - i - 1] = self.sp.pop();
        }
        result
    }

    pub fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        self.store.global_memory.read(offset, buffer)?;
        Ok(())
    }

    pub fn memory_read_fixed<const N: usize>(&self, offset: usize) -> Result<[u8; N], TrapCode> {
        let mut buffer = [0u8; N];
        self.store.global_memory.read(offset, &mut buffer)?;
        Ok(buffer)
    }

    pub fn memory_read_vec(&self, offset: usize, length: usize) -> Result<Vec<u8>, TrapCode> {
        let mut buffer = vec![0u8; length];
        self.store.global_memory.read(offset, &mut buffer)?;
        Ok(buffer)
    }

    pub fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        self.store.global_memory.write(offset, buffer)?;
        #[cfg(feature = "tracing")]
        if let Some(tracer) = self.store.tracer.as_mut() {
            tracer.memory_change(offset as u32, buffer.len() as u32, buffer);
        }
        Ok(())
    }

    pub fn program_counter(&self) -> u32 {
        self.program_counter
    }

    pub fn instruction_ptr(&self) -> InstructionPtr {
        self.ip
    }

    pub fn store_mut(&mut self) -> &mut Store<T> {
        &mut self.store
    }

    pub fn store(&self) -> &Store<T> {
        &self.store
    }

    pub fn context_mut(&mut self) -> &mut T {
        &mut self.store.context
    }

    pub fn context(&self) -> &T {
        &self.store.context
    }
}
