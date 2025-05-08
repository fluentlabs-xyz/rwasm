use crate::types::{DropKeep, UntypedValue};
use core::num::NonZeroU64;

/// Type storing all kinds of fuel costs of instructions.
#[derive(Debug, Copy, Clone)]
pub struct FuelCosts {
    /// The base fuel costs for all instructions.
    pub base: u64,
    /// The fuel cost for instruction operating on Wasm entities.
    ///
    /// # Note
    ///
    /// A Wasm entity is one of `func`, `global`, `memory` or `table`.
    /// Those instructions are usually a bit more costly since they need
    ///  multiple indirect accesses through the Wasm instance and store.
    pub entity: u64,
    /// The fuel cost offset for `memory.load` instructions.
    pub load: u64,
    /// The fuel cost offset for `memory.store` instructions.
    pub store: u64,
    /// The fuel cost offset for `call` and `call_indirect` instructions.
    pub call: u64,
    /// Determines how many moved stack values consume one fuel upon a branch or return
    /// instruction.
    ///
    /// # Note
    ///
    /// If this is zero, then processing [`DropKeep`] costs nothing.
    branch_kept_per_fuel: u64,
    /// Determines how many function locals consume one fuel per function call.
    ///
    /// # Note
    ///
    /// - This is also applied to all function parameters since they are translated to local
    ///   variable slots.
    /// - If this is zero then processing function locals costs nothing.
    func_locals_per_fuel: u64,
    /// How many memory bytes can be processed per fuel in a `bulk-memory` instruction?
    ///
    /// # Note
    ///
    /// If this is zero, then processing memory bytes costs nothing.
    memory_bytes_per_fuel: u64,
    /// How many table elements can be processed per fuel in a `bulk-table` instruction?
    ///
    /// # Note
    ///
    /// If this is zero, then processing table elements costs nothing.
    table_elements_per_fuel: u64,
}

impl FuelCosts {
    /// Returns the fuel consumption of the number of items with costs per items.
    fn costs_per(len_items: u64, items_per_fuel: u64) -> u64 {
        NonZeroU64::new(items_per_fuel)
            .map(|items_per_fuel| len_items / items_per_fuel)
            .unwrap_or(0)
    }

    /// Returns the fuel consumption for branches and returns using the given [`DropKeep`].
    pub fn fuel_for_drop_keep(&self, drop_keep: DropKeep) -> u64 {
        if drop_keep.drop == 0 {
            return 0;
        }
        Self::costs_per(u64::from(drop_keep.keep), self.branch_kept_per_fuel)
    }

    /// Returns the fuel consumption for calling a function with the amount of local variables.
    ///
    /// # Note
    ///
    /// Function parameters are also treated as local variables.
    pub fn fuel_for_locals(&self, locals: u64) -> u64 {
        Self::costs_per(locals, self.func_locals_per_fuel)
    }

    /// Returns the fuel consumption for processing the amount of memory bytes.
    pub fn fuel_for_bytes(&self, bytes: u64) -> u64 {
        Self::costs_per(bytes, self.memory_bytes_per_fuel)
    }

    /// Returns the fuel consumption for processing the amount of table elements.
    pub fn fuel_for_elements(&self, elements: u64) -> u64 {
        Self::costs_per(elements, self.table_elements_per_fuel)
    }
}

impl Default for FuelCosts {
    fn default() -> Self {
        let memory_bytes_per_fuel = 64;
        let bytes_per_register = size_of::<UntypedValue>() as u64;
        let registers_per_fuel = memory_bytes_per_fuel / bytes_per_register;
        Self {
            base: 1,
            entity: 1,
            load: 1,
            store: 1,
            call: 1,
            func_locals_per_fuel: registers_per_fuel,
            branch_kept_per_fuel: registers_per_fuel,
            memory_bytes_per_fuel,
            table_elements_per_fuel: registers_per_fuel,
        }
    }
}
