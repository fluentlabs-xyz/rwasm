use crate::{
    instruction_set_internal, BranchOffset, CompilationError, InstructionSet, LocalDepth, TrapCode,
    UntypedValue,
};
use rwasm_fuel_policy::{SyscallFuelParams, FUEL_MAX_LINEAR_X, FUEL_MAX_QUADRATIC_X};
use wasmparser::ValType;

/// Converts the parameter position a syscall fuel policy refers to into the stack depth
/// `LocalGet` expects inside the import trampoline.
///
/// `LinearFuelParams::param_index` and `QuadraticFuelParams::local_depth` count the imported
/// function's parameters from the last one (`1` is the last parameter), where every parameter is
/// one value. On the rwasm stack an `i64` or `f64` parameter occupies two 32-bit slots, so the
/// depth has to skip two slots for each such parameter above the metered one.
///
/// The metered parameter itself is a byte length and must be an `i32`: the policies are defined
/// over 32-bit lengths (`FUEL_MAX_LINEAR_X`/`FUEL_MAX_QUADRATIC_X` are `u32`), and a wide
/// parameter has no single slot to meter — the trampoline used to read its high word while the
/// Wasmtime host trampoline reads the whole value, so the two strategies charged different fuel.
/// Such a schedule is rejected with [`CompilationError::InvalidSyscallFuelParam`], on both
/// strategies since the Wasmtime one runs this front end first.
fn param_slot_depth(params: &[ValType], param_index: u32) -> Result<LocalDepth, CompilationError> {
    let param_index = usize::try_from(param_index)
        .ok()
        .filter(|index| (1..=params.len()).contains(index))
        .filter(|index| params[params.len() - index] == ValType::I32)
        .ok_or(CompilationError::InvalidSyscallFuelParam)?;
    let slots_above = params[params.len() - param_index + 1..]
        .iter()
        .map(|ty| match ty {
            ValType::I64 | ValType::F64 => 2,
            _ => 1,
        })
        .sum::<u32>();
    Ok(slots_above + 1)
}

/// The peak temporary stack usage of the syscall metering prologue for `syscall_fuel_param`, in
/// 32-bit slots on top of the import's parameters; the import trampoline's `StackCheck`.
pub(crate) fn syscall_fuel_temporary_slots(syscall_fuel_param: &SyscallFuelParams) -> u32 {
    match syscall_fuel_param {
        SyscallFuelParams::None | SyscallFuelParams::Const(_) => 0,
        SyscallFuelParams::LinearFuel(_) => 2,
        SyscallFuelParams::QuadraticFuel(_) => 4,
    }
}

/// Emits syscall metering and returns its peak temporary stack usage in 32-bit slots.
pub(crate) fn compile_block_params(
    isa: &mut InstructionSet,
    syscall_fuel_param: SyscallFuelParams,
    params: &[ValType],
) -> Result<u32, CompilationError> {
    let temporary_slots = syscall_fuel_temporary_slots(&syscall_fuel_param);
    match syscall_fuel_param {
        SyscallFuelParams::None => {}
        // the bytecode carries the constant as a `ConsumeFuel(u32)` immediate; a larger one
        // used to be cut to its low 32 bits and charged as that
        SyscallFuelParams::Const(base) => isa.op_consume_fuel(
            u32::try_from(base).map_err(|_| CompilationError::SyscallFuelOutOfBounds)?,
        ),
        SyscallFuelParams::LinearFuel(fuel_params) => {
            let depth = param_slot_depth(params, fuel_params.param_index)?;
            isa.op_local_get(depth);
            isa.op_i32_const(UntypedValue::from(FUEL_MAX_LINEAR_X));
            isa.op_i32_gt_u();
            isa.op_br_if_eqz(BranchOffset::from(2));
            isa.op_trap(TrapCode::IntegerOverflow);
            isa.op_local_get(depth);
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
            let depth = param_slot_depth(params, fuel_params.local_depth)?;
            instruction_set_internal! {
                isa,
                 // Runtime overflow check
                LocalGet(depth)
                I32Const(FUEL_MAX_QUADRATIC_X)
                I32GtU
                BrIfEqz(2)
                Trap(TrapCode::IntegerOverflow)
                // Linear part: word_cost × words
                LocalGet(depth)
                I32Const(31)
                I32Add
                I32Const(32)
                I32DivU
                I32Const(fuel_params.word_cost)
                I32Mul
                // Quadratic part: words² / divisor
                LocalGet(depth + 1) // linear part left words on stack
                I32Const(31)
                I32Add
                I32Const(32)
                I32DivU
                LocalGet(depth + 2) // linear and first words on stack
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
    debug_assert!(
        temporary_slots as usize <= crate::N_STACK_TRAMPOLINE_HEADROOM,
        "the value stack headroom must cover the trampoline temporaries"
    );
    Ok(temporary_slots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_slot_depth_counts_two_slots_per_i64_above_the_metered_param() {
        use ValType::*;
        assert_eq!(param_slot_depth(&[I32], 1).unwrap(), 1);
        assert_eq!(param_slot_depth(&[I32, I32, I32], 1).unwrap(), 1);
        assert_eq!(param_slot_depth(&[I32, I32, I32], 3).unwrap(), 3);
        assert_eq!(param_slot_depth(&[I32, I64], 2).unwrap(), 3);
        assert_eq!(param_slot_depth(&[I64, I32], 1).unwrap(), 1);
        assert_eq!(param_slot_depth(&[I32, F64, I64, I32], 4).unwrap(), 6);
    }

    #[test]
    fn param_slot_depth_rejects_out_of_range_positions() {
        use ValType::*;
        assert!(param_slot_depth(&[I32, I32], 0).is_err());
        assert!(param_slot_depth(&[I32, I32], 3).is_err());
        assert!(param_slot_depth(&[], 1).is_err());
    }

    /// A wide metered parameter has no single slot to read; the rwasm trampoline used to meter
    /// its high word while the Wasmtime host trampoline metered the whole value.
    #[test]
    fn param_slot_depth_rejects_wide_metered_parameters() {
        use ValType::*;
        for (params, index) in [
            (&[I32, I64][..], 1),
            (&[I64, I32][..], 2),
            (&[I64, I64][..], 1),
            (&[I32, F64][..], 1),
            (&[F32][..], 1),
        ] {
            let error = param_slot_depth(params, index).unwrap_err();
            assert!(
                matches!(error, CompilationError::InvalidSyscallFuelParam),
                "{params:?} at {index}: {error:?}"
            );
        }
    }
}
