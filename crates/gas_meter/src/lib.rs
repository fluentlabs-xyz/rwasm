use wasmparser::Operator;

pub const BASE_FUEL_COST: u32 = 1;
pub const ENTITY_FUEL_COST: u32 = 1;
pub const LOAD_FUEL_COST: u32 = 1;
pub const STORE_FUEL_COST: u32 = 1;
pub const CALL_FUEL_COST: u32 = 1;

pub const MEMORY_BYTES_PER_FUEL: u32 = 64;
pub const MEMORY_BYTES_PER_FUEL_LOG2: u32 = 6;
pub const TABLE_ELEMS_PER_FUEL: u32 = 16;
pub const TABLE_ELEMS_PER_FUEL_LOG2: u32 = 4;
pub const LOCALS_PER_FUEL: u32 = 16;
pub const LOCALS_PER_FUEL_LOG2: u32 = 4;
pub const DROP_KEEP_PER_FUEL: u32 = 16;
pub const DROP_KEEP_PER_FUEL_LOG2: u32 = 4;

#[derive(Debug, PartialEq, Eq)]
pub enum ShouldInject {
    InjectCost(u64),
    None,
}

pub trait CostModel {
    fn cost_for(&self, op: &Operator) -> u64;
}

#[derive(Default)]
pub struct DefaultCostModel;

impl CostModel for DefaultCostModel {
    fn cost_for(&self, op: &Operator) -> u64 {
        use Operator::*;
        match op {
            // Memory operations
            MemoryGrow { .. } | MemoryInit { .. } | MemoryCopy { .. } | MemoryFill { .. } | MemorySize { .. } => (MEMORY_BYTES_PER_FUEL as u64),
            // Table operations
            TableInit { .. } | TableCopy { .. } | TableFill { .. } | TableGet { .. } | TableSet { .. } | TableGrow { .. } | TableSize { .. } => (TABLE_ELEMS_PER_FUEL as u64),
            // Load/store operations
            I32Load { .. } | I64Load { .. } | F32Load { .. } | F64Load { .. } | I32Load8S { .. } | I32Load8U { .. } | I32Load16S { .. } | I32Load16U { .. } | I64Load8S { .. } | I64Load8U { .. } | I64Load16S { .. } | I64Load16U { .. } | I64Load32S { .. } | I64Load32U { .. } => LOAD_FUEL_COST as u64,
            I32Store { .. } | I64Store { .. } | F32Store { .. } | F64Store { .. } | I32Store8 { .. } | I32Store16 { .. } | I64Store8 { .. } | I64Store16 { .. } | I64Store32 { .. } => STORE_FUEL_COST as u64,
            // Call operations
            Call { .. } | CallIndirect { .. } => CALL_FUEL_COST as u64,
            // Control flow, entity ops
            Br { .. } | BrIf { .. } | BrTable { .. } | Return { .. } => ENTITY_FUEL_COST as u64,
            // Locals (get/set/tee)
            LocalGet { .. } | LocalSet { .. } | LocalTee { .. } => LOCALS_PER_FUEL as u64,
            // Drop/keep (drop, select)
            Drop | Select => DROP_KEEP_PER_FUEL as u64,

            //TODO: Compare rwasm and wasmtime opcode cost wasm time op code cost
            // // Nop and drop generate no code, so don't consume fuel for them.
            // Operator::Nop => 0,
            //
            // // Control flow may create branches, but is generally cheap and
            // // free, so don't consume fuel. Note the lack of `if` since some
            // // cost is incurred with the conditional check.
            // Operator::Block { .. }
            // | Operator::Loop { .. }
            // | Operator::Unreachable
            // | Operator::Return
            // | Operator::Else
            // | Operator::End => 0,

            // Most other ops cost base
            _ => BASE_FUEL_COST as u64,
        }
    }
}

/// GasMeter state (cumulative counter, threshold, cost model)
pub struct GasMeter<T: CostModel> {
    gas_spent: u64,
    model: T,
}

impl<T: CostModel + Default> GasMeter<T> {
    pub fn new() -> Self {
        Self {
            gas_spent: 0,
            model: T::default(),
        }
    }

    pub fn charge_gas_for(&mut self, op: &Operator) -> ShouldInject {
        use Operator::*;
        // List of control operators
        let is_control = matches!(op,
            Unreachable | Block { .. } | Loop { .. } | If { .. } | Else | End |
            Br { .. } | BrIf { .. } | BrTable { .. } | Return |
            Call { .. } | CallIndirect { .. } | ReturnCall { .. } | ReturnCallIndirect { .. }
        );
        let cost = self.model.cost_for(op);
        self.gas_spent += cost;
        if is_control {
            let total = self.gas_spent;
            self.gas_spent = 0;
            ShouldInject::InjectCost(total)
        } else {
            ShouldInject::None
        }
    }

    pub fn gas_spent(&self) -> u64 {
        self.gas_spent
    }
}

