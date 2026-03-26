use crate::{types::TrapCode, CallerTr, RwasmStore, StoreTr};
use alloc::vec::Vec;

pub struct RwasmCaller<'a, T: 'static> {
    store: &'a mut RwasmStore<T>,
}

impl<'a, T: 'static> RwasmCaller<'a, T> {
    pub fn new(store: &'a mut RwasmStore<T>) -> Self {
        Self { store }
    }
}

impl<'a, T: 'static> StoreTr<T> for RwasmCaller<'a, T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        self.store.global_memory.read(offset, buffer)
    }

    fn memory_read_into_vec(&mut self, offset: usize, length: usize) -> Result<Vec<u8>, TrapCode> {
        self.store.global_memory.read_into_vec(offset, length)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        self.store.global_memory.write(offset, buffer)?;
        #[cfg(feature = "tracing")]
        self.store
            .tracer
            .memory_change(offset as u32, buffer.len() as u32, buffer);
        Ok(())
    }

    fn data_mut(&mut self) -> &mut T {
        &mut self.store.data
    }

    fn data(&self) -> &T {
        &self.store.data
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        self.store.try_consume_fuel(delta)
    }

    fn remaining_fuel(&self) -> Option<u64> {
        self.store.remaining_fuel()
    }

    fn reset_fuel(&mut self, new_fuel_limit: u64) {
        self.store.reset_fuel(new_fuel_limit)
    }
}

impl<'a, T: 'static> CallerTr<T> for RwasmCaller<'a, T> {}
