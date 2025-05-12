use crate::{Opcode, OpcodeData, , RwasmExecutor, UntypedValue};

#[cold]
pub(crate) fn exec_fpu_opcode<T>(
    vm: &mut RwasmExecutor<T>,
    opcode: Opcode,
    data: OpcodeData,
) -> Result<(), RwasmError> {
    use Opcode::*;
    if !vm.config.floats_enabled {
        return Err(RwasmError::FloatsAreDisabled);
    }
    match opcode {
        F32Const | F64Const => {
            let untyped_value = match data {
                OpcodeData::UntypedValue(value) => value,
                _ => unreachable!(),
            };
            vm.sp.push(untyped_value);
            vm.ip.add(1);
        }
        F32Load | F64Load => {
            let offset = match data {
                OpcodeData::AddressOffset(value) => value,
                _ => unreachable!("rwasm: missing instr data"),
            };
            vm.sp.try_eval_top(|address| {
                let memory = vm.global_memory.data();
                let func = match opcode {
                    F32Load => UntypedValue::f32_load,
                    F64Load => UntypedValue::f64_load,
                    _ => unreachable!(),
                };
                func(memory, address, offset.into_inner())
            })?;
            vm.ip.add(1);
        }
        F32Store | F64Store => {
            let offset = match data {
                OpcodeData::AddressOffset(value) => value,
                _ => unreachable!("rwasm: missing instr data"),
            };
            let (address, value) = vm.sp.pop2();
            let memory = vm.global_memory.data_mut();
            let func = match opcode {
                F32Store => UntypedValue::f32_store,
                F64Store => UntypedValue::f64_store,
                _ => unreachable!(),
            };
            func(memory, address, offset.into_inner(), value)?;
            vm.ip.add(1);
        }

        F32Eq | F32Ne | F32Lt | F32Gt | F32Le | F32Ge | F64Eq | F64Ne | F64Lt | F64Gt | F64Le
        | F64Ge => {
            vm.sp.eval_top2(match opcode {
                F32Eq => UntypedValue::f32_eq,
                F32Ne => UntypedValue::f32_ne,
                F32Lt => UntypedValue::f32_lt,
                F32Gt => UntypedValue::f32_gt,
                F32Le => UntypedValue::f32_le,
                F32Ge => UntypedValue::f32_ge,
                F64Eq => UntypedValue::f64_eq,
                F64Ne => UntypedValue::f64_ne,
                F64Lt => UntypedValue::f64_lt,
                F64Gt => UntypedValue::f64_gt,
                F64Le => UntypedValue::f64_le,
                F64Ge => UntypedValue::f64_ge,
                _ => unreachable!(),
            });
            vm.ip.add(1);
        }

        F32Abs | F32Neg | F32Ceil | F32Floor | F32Trunc | F32Nearest | F32Sqrt | F64Abs
        | F64Neg | F64Ceil | F64Floor | F64Trunc | F64Nearest | F64Sqrt => {
            vm.sp.eval_top(match opcode {
                F32Abs => UntypedValue::f32_abs,
                F32Neg => UntypedValue::f32_neg,
                F32Ceil => UntypedValue::f32_ceil,
                F32Floor => UntypedValue::f32_floor,
                F32Trunc => UntypedValue::f32_trunc,
                F32Nearest => UntypedValue::f32_nearest,
                F32Sqrt => UntypedValue::f32_sqrt,
                F64Abs => UntypedValue::f64_abs,
                F64Neg => UntypedValue::f64_neg,
                F64Ceil => UntypedValue::f64_ceil,
                F64Floor => UntypedValue::f64_floor,
                F64Trunc => UntypedValue::f64_trunc,
                F64Nearest => UntypedValue::f64_nearest,
                F64Sqrt => UntypedValue::f64_sqrt,
                _ => unreachable!(),
            });
            vm.ip.add(1);
        }

        F32Add => visit_f32_add(vm),
        F32Sub => visit_f32_sub(vm),
        F32Mul => visit_f32_mul(vm),
        F32Div => visit_f32_div(vm),
        F32Min => visit_f32_min(vm),
        F32Max => visit_f32_max(vm),
        F32Copysign => visit_f32_copysign(vm),
        F64Add => visit_f64_add(vm),
        F64Sub => visit_f64_sub(vm),
        F64Mul => visit_f64_mul(vm),
        F64Div => visit_f64_div(vm),
        F64Min => visit_f64_min(vm),
        F64Max => visit_f64_max(vm),
        F64Copysign => visit_f64_copysign(vm),

        I32TruncF32S => visit_i32_trunc_f32_s(vm)?,
        I32TruncF32U => visit_i32_trunc_f32_u(vm)?,
        I32TruncF64S => visit_i32_trunc_f64_s(vm)?,
        I32TruncF64U => visit_i32_trunc_f64_u(vm)?,
        I64TruncF32S => visit_i64_trunc_f32_s(vm)?,
        I64TruncF32U => visit_i64_trunc_f32_u(vm)?,
        I64TruncF64S => visit_i64_trunc_f64_s(vm)?,
        I64TruncF64U => visit_i64_trunc_f64_u(vm)?,

        F32ConvertI32S => visit_f32_convert_i32_s(vm),
        F32ConvertI32U => visit_f32_convert_i32_u(vm),
        F32ConvertI64S => visit_f32_convert_i64_s(vm),
        F32ConvertI64U => visit_f32_convert_i64_u(vm),
        F32DemoteF64 => visit_f32_demote_f64(vm),
        F64ConvertI32S => visit_f64_convert_i32_s(vm),
        F64ConvertI32U => visit_f64_convert_i32_u(vm),
        F64ConvertI64S => visit_f64_convert_i64_s(vm),
        F64ConvertI64U => visit_f64_convert_i64_u(vm),
        F64PromoteF32 => visit_f64_promote_f32(vm),
        I32TruncSatF32S => visit_i32_trunc_sat_f32_s(vm),
        I32TruncSatF32U => visit_i32_trunc_sat_f32_u(vm),
        I32TruncSatF64S => visit_i32_trunc_sat_f64_s(vm),
        I32TruncSatF64U => visit_i32_trunc_sat_f64_u(vm),
        I64TruncSatF32S => visit_i64_trunc_sat_f32_s(vm),
        I64TruncSatF32U => visit_i64_trunc_sat_f32_u(vm),
        I64TruncSatF64S => visit_i64_trunc_sat_f64_s(vm),
        I64TruncSatF64U => visit_i64_trunc_sat_f64_u(vm),
        _ => unreachable!(),
    }
    Ok(())
}

macro_rules! impl_visit_fallible_unary {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
                vm.sp.try_eval_top(UntypedValue::$untyped_ident)?;
                vm.ip.add(1);
                Ok(())
            }
        )*
    }
}

impl_visit_fallible_unary! {
    fn visit_i32_trunc_f32_s(i32_trunc_f32_s);
    fn visit_i32_trunc_f32_u(i32_trunc_f32_u);
    fn visit_i32_trunc_f64_s(i32_trunc_f64_s);
    fn visit_i32_trunc_f64_u(i32_trunc_f64_u);

    fn visit_i64_trunc_f32_s(i64_trunc_f32_s);
    fn visit_i64_trunc_f32_u(i64_trunc_f32_u);
    fn visit_i64_trunc_f64_s(i64_trunc_f64_s);
    fn visit_i64_trunc_f64_u(i64_trunc_f64_u);
}

macro_rules! impl_visit_binary {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(

            pub(crate) fn $visit_ident<T>(vm: &mut RwasmExecutor<T>) {
                vm.sp.eval_top2(UntypedValue::$untyped_ident);
                vm.ip.add(1);
            }
        )*
    }
}

impl_visit_binary! {
    fn visit_i32_eq(i32_eq);
    fn visit_i32_ne(i32_ne);
    fn visit_i32_lt_s(i32_lt_s);
    fn visit_i32_lt_u(i32_lt_u);
    fn visit_i32_gt_s(i32_gt_s);
    fn visit_i32_gt_u(i32_gt_u);
    fn visit_i32_le_s(i32_le_s);
    fn visit_i32_le_u(i32_le_u);
    fn visit_i32_ge_s(i32_ge_s);
    fn visit_i32_ge_u(i32_ge_u);

    fn visit_i64_eq(i64_eq);
    fn visit_i64_ne(i64_ne);
    fn visit_i64_lt_s(i64_lt_s);
    fn visit_i64_lt_u(i64_lt_u);
    fn visit_i64_gt_s(i64_gt_s);
    fn visit_i64_gt_u(i64_gt_u);
    fn visit_i64_le_s(i64_le_s);
    fn visit_i64_le_u(i64_le_u);
    fn visit_i64_ge_s(i64_ge_s);
    fn visit_i64_ge_u(i64_ge_u);

    fn visit_f32_eq(f32_eq);
    fn visit_f32_ne(f32_ne);
    fn visit_f32_lt(f32_lt);
    fn visit_f32_gt(f32_gt);
    fn visit_f32_le(f32_le);
    fn visit_f32_ge(f32_ge);

    fn visit_f64_eq(f64_eq);
    fn visit_f64_ne(f64_ne);
    fn visit_f64_lt(f64_lt);
    fn visit_f64_gt(f64_gt);
    fn visit_f64_le(f64_le);
    fn visit_f64_ge(f64_ge);

    fn visit_i32_add(i32_add);
    fn visit_i32_sub(i32_sub);
    fn visit_i32_mul(i32_mul);
    fn visit_i32_and(i32_and);
    fn visit_i32_or(i32_or);
    fn visit_i32_xor(i32_xor);
    fn visit_i32_shl(i32_shl);
    fn visit_i32_shr_s(i32_shr_s);
    fn visit_i32_shr_u(i32_shr_u);
    fn visit_i32_rotl(i32_rotl);
    fn visit_i32_rotr(i32_rotr);

    fn visit_i64_add(i64_add);
    fn visit_i64_sub(i64_sub);
    fn visit_i64_mul(i64_mul);
    fn visit_i64_and(i64_and);
    fn visit_i64_or(i64_or);
    fn visit_i64_xor(i64_xor);
    fn visit_i64_shl(i64_shl);
    fn visit_i64_shr_s(i64_shr_s);
    fn visit_i64_shr_u(i64_shr_u);
    fn visit_i64_rotl(i64_rotl);
    fn visit_i64_rotr(i64_rotr);

    fn visit_f32_add(f32_add);
    fn visit_f32_sub(f32_sub);
    fn visit_f32_mul(f32_mul);
    fn visit_f32_div(f32_div);
    fn visit_f32_min(f32_min);
    fn visit_f32_max(f32_max);
    fn visit_f32_copysign(f32_copysign);

    fn visit_f64_add(f64_add);
    fn visit_f64_sub(f64_sub);
    fn visit_f64_mul(f64_mul);
    fn visit_f64_div(f64_div);
    fn visit_f64_min(f64_min);
    fn visit_f64_max(f64_max);
    fn visit_f64_copysign(f64_copysign);
}

macro_rules! impl_visit_unary {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident<T>(exec: &mut RwasmExecutor<T>) {
                exec.sp.eval_top(UntypedValue::$untyped_ident);
                exec.ip.add(1);
            }
        )*
    }
}

impl_visit_unary! {
    fn visit_i32_eqz(i32_eqz);
    fn visit_i64_eqz(i64_eqz);

    fn visit_i32_clz(i32_clz);
    fn visit_i32_ctz(i32_ctz);
    fn visit_i32_popcnt(i32_popcnt);

    fn visit_i64_clz(i64_clz);
    fn visit_i64_ctz(i64_ctz);
    fn visit_i64_popcnt(i64_popcnt);

    fn visit_f32_abs(f32_abs);
    fn visit_f32_neg(f32_neg);
    fn visit_f32_ceil(f32_ceil);
    fn visit_f32_floor(f32_floor);
    fn visit_f32_trunc(f32_trunc);
    fn visit_f32_nearest(f32_nearest);
    fn visit_f32_sqrt(f32_sqrt);

    fn visit_f64_abs(f64_abs);
    fn visit_f64_neg(f64_neg);
    fn visit_f64_ceil(f64_ceil);
    fn visit_f64_floor(f64_floor);
    fn visit_f64_trunc(f64_trunc);
    fn visit_f64_nearest(f64_nearest);
    fn visit_f64_sqrt(f64_sqrt);

    fn visit_i32_wrap_i64(i32_wrap_i64);
    fn visit_i64_extend_i32_s(i64_extend_i32_s);
    fn visit_i64_extend_i32_u(i64_extend_i32_u);

    fn visit_f32_convert_i32_s(f32_convert_i32_s);
    fn visit_f32_convert_i32_u(f32_convert_i32_u);
    fn visit_f32_convert_i64_s(f32_convert_i64_s);
    fn visit_f32_convert_i64_u(f32_convert_i64_u);
    fn visit_f32_demote_f64(f32_demote_f64);
    fn visit_f64_convert_i32_s(f64_convert_i32_s);
    fn visit_f64_convert_i32_u(f64_convert_i32_u);
    fn visit_f64_convert_i64_s(f64_convert_i64_s);
    fn visit_f64_convert_i64_u(f64_convert_i64_u);
    fn visit_f64_promote_f32(f64_promote_f32);

    fn visit_i32_extend8_s(i32_extend8_s);
    fn visit_i32_extend16_s(i32_extend16_s);
    fn visit_i64_extend8_s(i64_extend8_s);
    fn visit_i64_extend16_s(i64_extend16_s);
    fn visit_i64_extend32_s(i64_extend32_s);

    fn visit_i32_trunc_sat_f32_s(i32_trunc_sat_f32_s);
    fn visit_i32_trunc_sat_f32_u(i32_trunc_sat_f32_u);
    fn visit_i32_trunc_sat_f64_s(i32_trunc_sat_f64_s);
    fn visit_i32_trunc_sat_f64_u(i32_trunc_sat_f64_u);
    fn visit_i64_trunc_sat_f32_s(i64_trunc_sat_f32_s);
    fn visit_i64_trunc_sat_f32_u(i64_trunc_sat_f32_u);
    fn visit_i64_trunc_sat_f64_s(i64_trunc_sat_f64_s);
    fn visit_i64_trunc_sat_f64_u(i64_trunc_sat_f64_u);
}
