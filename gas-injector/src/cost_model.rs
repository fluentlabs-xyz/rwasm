use crate::BASE_FUEL_COST;
use wasmparser::Operator;

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
            // Control flow may create branches, but is generally inexpensive and
            // free, so don't consume fuel.
            // Note the lack of `if` since some
            // cost is incurred with the conditional check.
            Block { .. } | Loop { .. } | Unreachable | Return | Else | End => 0,

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

impl<T: CostModel + Default> Default for GasMeter<T> {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<T: CostModel> GasMeter<T> {
    pub fn new(model: T) -> Self {
        Self {
            gas_spent: 0,
            model,
        }
    }

    pub fn charge_gas_for(&mut self, op: &Operator) -> ShouldInject {
        use Operator::*;
        // List of control operators
        let is_control = matches!(
            op,
            Unreachable
                | Block { .. }
                | Loop { .. }
                | If { .. }
                | Else
                | End
                | Br { .. }
                | BrIf { .. }
                | BrTable { .. }
                | Return
                | Call { .. }
                | CallIndirect { .. }
                | ReturnCall { .. }
                | ReturnCallIndirect { .. }
        );
        let cost = self.model.cost_for(op);
        self.gas_spent += cost;
        if is_control && self.gas_spent > 0 {
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
