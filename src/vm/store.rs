use crate::{
    CallStack, GlobalIdx, GlobalMemory, ImportLinker, InstructionPtr, Pages, RwasmModule,
    SignatureIdx, StoreTr, SyscallHandler, TableEntity, TableIdx, TrapCode, UntypedValue,
    ValueStack, N_DEFAULT_MAX_MEMORY_PAGES,
};
use alloc::{sync::Arc, vec::Vec};
use bitvec::{order::Lsb0, vec::BitVec};
use hashbrown::HashMap;

/// Host-side store that holds memory, tables, globals, and host context for a rwasm instance.
/// It also tracks fuel for metering and provides access to imported functions and syscalls.
/// The store is passed to host callbacks and persists across invocations of the same module.
pub struct RwasmStore<T: 'static> {
    /// Total amount of fuel consumed by the currently running instance.
    pub(crate) consumed_fuel: u64,
    /// The linear memory shared by the running module and the host.
    pub(crate) global_memory: GlobalMemory,
    /// User-defined context available to host functions and syscalls.
    pub(crate) data: T,
    /// The last used signature index used for validating indirect calls.
    pub(crate) last_signature: Option<SignatureIdx>,
    /// Runtime-managed tables (may differ from compile-time layout due to mutations).
    pub(crate) tables: HashMap<TableIdx, TableEntity>,
    /// Runtime values of mutable and immutable globals.
    pub(crate) global_variables: HashMap<GlobalIdx, UntypedValue>,
    /// Bitset tracking which data segments have been consumed/emptied.
    pub(crate) empty_data_segments: BitVec,
    /// Bitset tracking which element segments have been consumed/emptied.
    pub(crate) empty_elem_segments: BitVec,
    /// Dispatcher for system calls made by the guest.
    pub(crate) syscall_handler: SyscallHandler<T>,
    /// Linker that resolves imports to host functions/globals.
    pub(crate) import_linker: Arc<ImportLinker>,
    /// If set, contains the instruction/value-stack pointers to resume after a suspension.
    pub(crate) resumable_context: Option<ReusableContext>,
    /// A fuel config (None stands for no limit).
    pub(crate) fuel_limit: Option<u64>,
    /// Execution tracer used when the `tracing` feature is enabled.
    #[cfg(feature = "tracing")]
    pub tracer: crate::Tracer,
}

pub struct ReusableContext {
    pub module: RwasmModule,
    pub call_stack: CallStack,
    pub ip: InstructionPtr,
    pub value_stack: ValueStack,
}

impl<T: 'static + Default> Default for RwasmStore<T> {
    fn default() -> Self {
        Self::new(
            Arc::new(ImportLinker::default()),
            T::default(),
            crate::always_failing_syscall_handler,
            None,
            None,
        )
    }
}

impl<T: 'static> StoreTr<T> for RwasmStore<T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        self.global_memory.read(offset, buffer)
    }

    fn memory_read_into_vec(&mut self, offset: usize, length: usize) -> Result<Vec<u8>, TrapCode> {
        self.global_memory.read_into_vec(offset, length)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        self.global_memory.write(offset, buffer)?;
        #[cfg(feature = "tracing")]
        self.tracer
            .memory_change(offset as u32, buffer.len() as u32, buffer);
        Ok(())
    }

    fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }

    fn data(&self) -> &T {
        &self.data
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        let consumed_fuel = self.consumed_fuel.saturating_add(delta);
        if let Some(fuel_limit) = self.fuel_limit {
            if consumed_fuel > fuel_limit {
                return Err(TrapCode::OutOfFuel);
            }
        }
        self.consumed_fuel = consumed_fuel;
        Ok(())
    }

    fn remaining_fuel(&self) -> Option<u64> {
        Some(self.fuel_limit? - self.consumed_fuel)
    }

    fn reset_fuel(&mut self, new_fuel_limit: u64) {
        // If new fuel limit is presented then change it
        self.fuel_limit = Some(new_fuel_limit);
        // Reset consumed fuel to 0 (indicating we have the entire fuel limit unspent)
        self.consumed_fuel = 0;
    }
}

impl<T: 'static> RwasmStore<T> {
    pub fn new(
        import_linker: Arc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
        fuel_limit: Option<u64>,
        max_allowed_memory_pages: Option<u32>,
    ) -> Self {
        let global_memory = GlobalMemory::new(
            Pages::new_unchecked(0),
            Pages::new_unchecked(max_allowed_memory_pages.unwrap_or(N_DEFAULT_MAX_MEMORY_PAGES)),
        );
        Self {
            consumed_fuel: 0,
            global_memory,
            data: context,
            #[cfg(feature = "tracing")]
            tracer: crate::Tracer::default(),
            global_variables: Default::default(),
            tables: Default::default(),
            last_signature: None,
            syscall_handler,
            empty_data_segments: BitVec::EMPTY,
            empty_elem_segments: BitVec::EMPTY,
            import_linker,
            resumable_context: None,
            fuel_limit,
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

    /// Returns the current linear memory size in bytes.
    ///
    /// Note: rwasm currently supports only the default (index 0) linear memory.
    pub fn memory_size_bytes(&self) -> usize {
        self.global_memory.data().len()
    }

    /// Returns a snapshot of the first `max_bytes` of linear memory.
    ///
    /// This is intended for differential testing/fuzzing where we want to compare post-call
    /// side effects without copying potentially huge memories.
    pub fn memory_snapshot_prefix(&self, max_bytes: usize) -> Vec<u8> {
        let mem = self.global_memory.data();
        let n = core::cmp::min(mem.len(), max_bytes);
        mem[..n].to_vec()
    }

    /// Returns a full snapshot of linear memory.
    ///
    /// This is primarily intended for differential fuzzing using Wasmtime's oracle strategy,
    /// which compares full exported memories.
    pub fn memory_snapshot(&self) -> Vec<u8> {
        self.global_memory.data().to_vec()
    }

    /// Returns per-table snapshots as `(table_index, size, non_null_prefix)` tuples.
    ///
    /// `non_null_prefix` contains `0/1` bytes describing whether each element is null (0) or non-null (1)
    /// for the first `max_elems` elements of the table.
    pub fn table_snapshots_nullness_prefix(&self, max_elems: usize) -> Vec<(u32, u32, Vec<u8>)> {
        let mut out: Vec<(u32, u32, Vec<u8>)> = Vec::new();
        // Ensure deterministic ordering for differential comparisons.
        let mut table_indices: Vec<TableIdx> = self.tables.keys().copied().collect();
        table_indices.sort_unstable();
        for idx in table_indices {
            // TableIdx is a new type wrapper around u32.
            let table = &self.tables[&idx];
            let size = table.size();
            let n = core::cmp::min(size as usize, max_elems);
            let mut prefix = Vec::with_capacity(n);
            for &bits in table.elements.iter().take(n) {
                // In rwasm, a null funcref is represented as 0.
                prefix.push(if bits == 0 { 0 } else { 1 });
            }
            out.push((idx as u32, size, prefix));
        }
        out
    }

    /// Returns whether a raw global word is currently materialized at the given internal index.
    pub fn has_global_word(&self, global_word_index: u32) -> bool {
        self.global_variables.contains_key(&global_word_index)
    }

    /// Returns the raw 32-bit "global word" at the given internal index.
    ///
    /// rwasm stores globals as 32-bit words. `i64`/`f64` globals occupy **two** words:
    /// - low word at `global_index * 2`
    /// - high word at `global_index * 2 + 1`
    ///
    /// This accessor is intended for differential fuzzing/oracles.
    pub fn global_word_bits(&self, global_word_index: u32) -> u32 {
        self.global_variables
            .get(&global_word_index)
            .copied()
            .unwrap_or_default()
            .to_bits()
    }
}
