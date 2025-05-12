use crate::{Opcode, RwasmExecutor, TrapCode, UntypedValue};

#[inline(always)]
pub(crate) fn exec_arith_unsigned_opcode<T>(
    vm: &mut RwasmExecutor<T>,
    opcode: Opcode,
) -> Result<(), TrapCode> {
    use Opcode::*;
    vm.sp.eval_top2(match opcode {
        I32Add => UntypedValue::i32_add,
        I32Sub => UntypedValue::i32_sub,
        I32Mul => UntypedValue::i32_mul,
        // I64Add => UntypedValue::i64_add,
        // I64Sub => UntypedValue::i64_sub,
        // I64Mul => UntypedValue::i64_mul,
        _ => unreachable!(),
    });
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn exec_arith_signed_opcode<T>(
    vm: &mut RwasmExecutor<T>,
    opcode: Opcode,
) -> Result<(), TrapCode> {
    use Opcode::*;
    vm.sp.try_eval_top2(match opcode {
        I32DivS => UntypedValue::i32_div_s,
        I32DivU => UntypedValue::i32_div_u,
        I32RemS => UntypedValue::i32_rem_s,
        I32RemU => UntypedValue::i32_rem_u,
        // I64DivS => UntypedValue::i64_div_s,
        // I64DivU => UntypedValue::i64_div_u,
        // I64RemS => UntypedValue::i64_rem_s,
        // I64RemU => UntypedValue::i64_rem_u,
        _ => unreachable!(),
    })?;
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn exec_bitwise_unary_opcode<T>(
    vm: &mut RwasmExecutor<T>,
    opcode: Opcode,
) -> Result<(), TrapCode> {
    use Opcode::*;
    vm.sp.eval_top(match opcode {
        I32Clz => UntypedValue::i32_clz,
        I32Ctz => UntypedValue::i32_ctz,
        I32Popcnt => UntypedValue::i32_popcnt,
        // I64Clz => UntypedValue::i64_clz,
        // I64Ctz => UntypedValue::i64_ctz,
        // I64Popcnt => UntypedValue::i64_popcnt,
        _ => unreachable!(),
    });
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn exec_bitwise_binary_opcode<T>(
    vm: &mut RwasmExecutor<T>,
    opcode: Opcode,
) -> Result<(), TrapCode> {
    use Opcode::*;
    vm.sp.eval_top2(match opcode {
        I32And => UntypedValue::i32_and,
        I32Or => UntypedValue::i32_or,
        I32Xor => UntypedValue::i32_xor,
        I32Shl => UntypedValue::i32_shl,
        I32ShrS => UntypedValue::i32_shr_s,
        I32ShrU => UntypedValue::i32_shr_u,
        I32Rotl => UntypedValue::i32_rotl,
        I32Rotr => UntypedValue::i32_rotr,
        // I64And => UntypedValue::i64_and,
        // I64Or => UntypedValue::i64_or,
        // I64Xor => UntypedValue::i64_xor,
        // I64Shl => UntypedValue::i64_shl,
        // I64ShrS => UntypedValue::i64_shr_s,
        // I64ShrU => UntypedValue::i64_shr_u,
        // I64Rotl => UntypedValue::i64_rotl,
        // I64Rotr => UntypedValue::i64_rotr,
        _ => unreachable!(),
    });
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn exec_compare_binary_opcode<T>(
    vm: &mut RwasmExecutor<T>,
    opcode: Opcode,
) -> Result<(), TrapCode> {
    use Opcode::*;
    vm.sp.eval_top2(match opcode {
        I32Eq => UntypedValue::i32_eq,
        I32Ne => UntypedValue::i32_ne,
        I32LtS => UntypedValue::i32_lt_s,
        I32LtU => UntypedValue::i32_lt_u,
        I32GtS => UntypedValue::i32_gt_s,
        I32GtU => UntypedValue::i32_gt_u,
        I32LeS => UntypedValue::i32_le_s,
        I32LeU => UntypedValue::i32_le_u,
        I32GeS => UntypedValue::i32_ge_s,
        I32GeU => UntypedValue::i32_ge_u,
        // I64Eq => UntypedValue::i64_eq,
        // I64Ne => UntypedValue::i64_ne,
        // I64LtS => UntypedValue::i64_lt_s,
        // I64LtU => UntypedValue::i64_lt_u,
        // I64GtS => UntypedValue::i64_gt_s,
        // I64GtU => UntypedValue::i64_gt_u,
        // I64LeS => UntypedValue::i64_le_s,
        // I64LeU => UntypedValue::i64_le_u,
        // I64GeS => UntypedValue::i64_ge_s,
        // I64GeU => UntypedValue::i64_ge_u,
        _ => unreachable!(),
    });
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn exec_compare_unary_opcode<T>(
    vm: &mut RwasmExecutor<T>,
    opcode: Opcode,
) -> Result<(), TrapCode> {
    use Opcode::*;
    vm.sp.eval_top(match opcode {
        I32Eqz => UntypedValue::i32_eqz,
        // I64Eqz => UntypedValue::i64_eqz,
        _ => unreachable!(),
    });
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn exec_convert_unary_opcode<T>(
    vm: &mut RwasmExecutor<T>,
    opcode: Opcode,
) -> Result<(), TrapCode> {
    use Opcode::*;
    vm.sp.eval_top(match opcode {
        // I32WrapI64 => UntypedValue::i32_wrap_i64,
        // I64ExtendI32S => UntypedValue::i64_extend_i32_s,
        // I64ExtendI32U => UntypedValue::i64_extend_i32_u,
        I32Extend8S => UntypedValue::i32_extend8_s,
        I32Extend16S => UntypedValue::i32_extend16_s,
        // I64Extend8S => UntypedValue::i64_extend8_s,
        // I64Extend16S => UntypedValue::i64_extend16_s,
        // I64Extend32S => UntypedValue::i64_extend32_s,
        _ => unreachable!(),
    });
    vm.ip.add(1);
    Ok(())
}
