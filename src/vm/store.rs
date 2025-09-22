use crate::{
    always_failing_syscall_handler, GlobalIdx, GlobalMemory, ImportLinker, Pages, SignatureIdx,
    Store, SyscallHandler, TableEntity, TableIdx, TrapCode, UntypedValue,
};
use alloc::sync::Arc;
use bitvec::{order::Lsb0, vec::BitVec};
use hashbrown::HashMap;

pub struct RwasmStore<T: 'static + Send + Sync> {
    pub(crate) consumed_fuel: u64,
    pub(crate) global_memory: GlobalMemory,
    pub(crate) context: T,
    pub(crate) fuel_limit: Option<u64>,
    // the last used signature (needed for indirect calls type checks)
    pub(crate) last_signature: Option<SignatureIdx>,
    // rwasm modified segments
    pub(crate) tables: HashMap<TableIdx, TableEntity>,
    pub(crate) global_variables: HashMap<GlobalIdx, UntypedValue>,
    // elem/data emptiness flags
    pub(crate) empty_data_segments: BitVec,
    pub(crate) empty_elem_segments: BitVec,
    // list of nested calls return pointers
    pub(crate) syscall_handler: SyscallHandler<T>,
    pub(crate) import_linker: Arc<ImportLinker>,
    #[cfg(feature = "tracing")]
    pub tracer: crate::Tracer,
}

impl<T: 'static + Send + Sync + Default> Default for RwasmStore<T> {
    fn default() -> Self {
        Self::new(
            Arc::new(ImportLinker::default()),
            T::default(),
            always_failing_syscall_handler,
        )
    }
}

impl<T: 'static + Send + Sync> Store<T> for RwasmStore<T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        self.global_memory.read(offset, buffer)?;
        Ok(())
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        self.global_memory.write(offset, buffer)?;
        #[cfg(feature = "tracing")]
        self.tracer
            .memory_change(offset as u32, buffer.len() as u32, buffer);
        Ok(())
    }

    fn context_mut<R, F: FnOnce(&mut T) -> R>(&mut self, func: F) -> R {
        func(&mut self.context)
    }

    fn context<R, F: FnOnce(&T) -> R>(&self, func: F) -> R {
        func(&self.context)
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        let consumed_fuel = self.consumed_fuel.checked_add(delta).unwrap_or(u64::MAX);
        if let Some(fuel_limit) = self.fuel_limit {
            if consumed_fuel > fuel_limit {
                return Err(TrapCode::OutOfFuel);
            }
        }
        self.consumed_fuel = consumed_fuel;
        Ok(())
    }

    fn remaining_fuel(&mut self) -> Option<u64> {
        Some(self.fuel_limit? - self.consumed_fuel)
    }
}

impl<T: 'static + Send + Sync> RwasmStore<T> {
    pub fn new(
        import_linker: Arc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
    ) -> Self {
        let global_memory = GlobalMemory::new(Pages::default());
        Self {
            consumed_fuel: 0,
            global_memory,
            context,
            fuel_limit: None,
            #[cfg(feature = "tracing")]
            tracer: crate::Tracer::default(),
            global_variables: Default::default(),
            tables: Default::default(),
            last_signature: None,
            syscall_handler,
            empty_data_segments: BitVec::EMPTY,
            empty_elem_segments: BitVec::EMPTY,
            import_linker,
        }
    }

    /// Resets the state of the current execution context.
    pub fn reset(&mut self, keep_flags: bool) {
        // reset consumed fuel to 0
        self.consumed_fuel = 0;
        // we might want to keep data/elem flags between calls, it's required for e2e tests
        if !keep_flags {
            // we don't do any assumptions regarding how data segments are used,
            // maybe there is a way to optimize reuse of bitset.
            if self.empty_data_segments.len() <= size_of::<usize>() {
                self.empty_data_segments.fill(false);
            } else {
                self.empty_data_segments = BitVec::<usize, Lsb0>::EMPTY;
            }
            // we don't do any assumptions regarding how tables are used inside the applications,
            // so keep it always empty, probably there is an optimization here.
            if self.empty_elem_segments.len() <= size_of::<usize>() {
                self.empty_elem_segments.fill(false);
            } else {
                self.empty_elem_segments = BitVec::<usize, Lsb0>::EMPTY;
            }
        }
        // in case of a trap, we might have this flag remains active
        self.last_signature = None;
    }

    pub fn fuel_consumed(&self) -> u64 {
        self.consumed_fuel
    }

    pub fn set_syscall_handler(&mut self, handler: SyscallHandler<T>) {
        self.syscall_handler = handler;
    }
}
