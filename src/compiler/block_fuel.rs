use crate::{
    instruction_set_internal, BranchOffset, InstructionSet, LocalDepth, TrapCode, UntypedValue,
};
use rwasm_fuel_policy::{SyscallFuelParams, FUEL_MAX_LINEAR_X, FUEL_MAX_QUADRATIC_X};

pub(crate) fn compile_block_params(
    isa: &mut InstructionSet,
    syscall_fuel_param: SyscallFuelParams,
) {
    match syscall_fuel_param {
        SyscallFuelParams::None => {}
        SyscallFuelParams::Const(base) => isa.op_consume_fuel(base as u32),
        SyscallFuelParams::LinearFuel(fuel_params) => {
            isa.op_local_get(LocalDepth::from(fuel_params.param_index));
            isa.op_i32_const(UntypedValue::from(FUEL_MAX_LINEAR_X));
            isa.op_i32_gt_u();
            isa.op_br_if_eqz(BranchOffset::from(2));
            isa.op_trap(TrapCode::IntegerOverflow);
            isa.op_local_get(LocalDepth::from(fuel_params.param_index as u32));
            isa.op_i32_const(UntypedValue::from(31));
            isa.op_i32_add();
            isa.op_i32_const(UntypedValue::from(32));
            isa.op_i32_div_u();
            isa.op_i32_const(UntypedValue::from(fuel_params.word_cost));
            isa.op_i32_mul();
            if fuel_params.base_fuel != 0 {
                isa.op_i32_const(UntypedValue::from(fuel_params.base_fuel));
                isa.op_i32_add();
            }
            isa.op_consume_fuel_stack()
        }
        SyscallFuelParams::QuadraticFuel(fuel_params) => {
            instruction_set_internal! {
                isa,
                 // Runtime overflow check
                LocalGet(fuel_params.local_depth)
                I32Const(FUEL_MAX_QUADRATIC_X)
                I32GtU
                BrIfEqz(2)
                Trap(TrapCode::IntegerOverflow)
                // Linear part: word_cost × words
                LocalGet(fuel_params.local_depth)
                I32Const(31)
                I32Add
                I32Const(32)
                I32DivU
                I32Const(fuel_params.word_cost)
                I32Mul
                // Quadratic part: words² / divisor
                LocalGet(fuel_params.local_depth + 1) // linear part left words on stack
                I32Const(31)
                I32Add
                I32Const(32)
                I32DivU
                LocalGet(fuel_params.local_depth + 2) // linear and first words on stack
                I32Const(31)
                I32Add
                I32Const(32)
                I32DivU
                I32Mul
                I32Const(fuel_params.divisor)
                I32DivU
                // Sum: linear + quadratic
                I32Add
                // Convert gas -> fuel
                I32Const(fuel_params.fuel_denom_rate)
                I32Mul
                ConsumeFuelStack
            }
        }
    }
}
