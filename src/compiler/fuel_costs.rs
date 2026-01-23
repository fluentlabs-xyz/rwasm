use crate::{
    compiler::drop_keep::DropKeep, BASE_FUEL_COST, CALL_FUEL_COST, DROP_KEEP_PER_FUEL,
    ENTITY_FUEL_COST, LOAD_FUEL_COST, LOCALS_PER_FUEL, MEMORY_BYTES_PER_FUEL, STORE_FUEL_COST,
    TABLE_ELEMS_PER_FUEL,
};
use core::num::NonZeroU32;

/// Type storing all kinds of fuel costs of instructions.
#[derive(Default, Debug, Copy, Clone)]
pub struct FuelCosts;

impl FuelCosts {
    pub const BASE: u32 = BASE_FUEL_COST;
    pub const ENTITY: u32 = ENTITY_FUEL_COST;
    pub const LOAD: u32 = LOAD_FUEL_COST;
    pub const STORE: u32 = STORE_FUEL_COST;
    pub const CALL: u32 = CALL_FUEL_COST;

    /// Returns the fuel consumption of the number of items with costs per items.
    pub fn costs_per(len_items: u32, items_per_fuel: u32) -> u32 {
        if len_items == 0 {
            return 0;
        }
        NonZeroU32::new(items_per_fuel)
            .map(|items_per_fuel_nz| {
                (len_items.saturating_add(items_per_fuel) - 1) / items_per_fuel_nz
            })
            .unwrap_or(0)
    }

    /// Returns the fuel consumption for branches and returns using the given [`DropKeep`].
    pub fn fuel_for_drop_keep(drop_keep: DropKeep) -> u32 {
        if drop_keep.drop == 0 {
            return 0;
        }
        Self::costs_per(u32::from(drop_keep.keep), DROP_KEEP_PER_FUEL)
    }

    /// Returns the fuel consumption for calling a function with the amount of local variables.
    ///
    /// # Note
    ///
    /// Function parameters are also treated as local variables.
    pub fn fuel_for_locals(locals: u32) -> u32 {
        Self::costs_per(locals, LOCALS_PER_FUEL)
    }

    /// Returns the fuel consumption for processing the amount of memory bytes.
    pub fn fuel_for_bytes(bytes: u32) -> u32 {
        Self::costs_per(bytes, MEMORY_BYTES_PER_FUEL)
    }

    /// Returns the fuel consumption for processing the amount of table elements.
    pub fn fuel_for_elements(elements: u32) -> u32 {
        Self::costs_per(elements, TABLE_ELEMS_PER_FUEL)
    }
}
