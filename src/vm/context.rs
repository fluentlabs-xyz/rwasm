use crate::{types::TrapCode, CallerTr, RwasmStore, StoreTr, ValueStackPtr};

pub struct RwasmCaller<'a, T: 'static> {
    store: &'a mut RwasmStore<T>,
    sp: ValueStackPtr,
}

impl<'a, T: 'static> RwasmCaller<'a, T> {
    pub fn new(store: &'a mut RwasmStore<T>, sp: ValueStackPtr) -> Self {
        Self { store, sp }
    }

    pub fn sp(&self) -> ValueStackPtr {
        self.sp
    }
}

impl<'a, T: 'static> StoreTr<T> for RwasmCaller<'a, T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        self.store.global_memory.read(offset, buffer)?;
        Ok(())
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
}

impl<'a, T: 'static> CallerTr<T> for RwasmCaller<'a, T> {}
