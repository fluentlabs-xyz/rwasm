use crate::{
    AddressOffset,
    ArithmeticOps,
    ExtendInto,
    Float,
    Opcode,
    RwasmExecutor,
    TrapCode,
    TruncateSaturateInto,
    TryTruncateInto,
    UntypedValue,
    WrapInto,
    F32,
    F64,
};
use core::ops::Neg;

#[inline(always)]
pub(crate) fn visit_i32_trunc_f64_s<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f64();
    let res = <F64 as TryTruncateInto<i32, TrapCode>>::try_truncate_into(value)?;
    vm.sp.push_i32(res);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i32_trunc_f64_u<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f64();
    let res = <F64 as TryTruncateInto<u32, TrapCode>>::try_truncate_into(value)?;
    vm.sp.push_i32(res as i32);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i64_trunc_f32_s<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f32();
    let res = <F32 as TryTruncateInto<i64, TrapCode>>::try_truncate_into(value)?;
    vm.sp.push_i64(res);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i64_trunc_f32_u<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f32();
    let res = <F32 as TryTruncateInto<u64, TrapCode>>::try_truncate_into(value)?;
    vm.sp.push_i64(res as i64);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_f32_load<T>(
    vm: &mut RwasmExecutor<T>,
    address_offset: AddressOffset,
) -> Result<(), TrapCode> {
    vm.execute_load_extend(address_offset, UntypedValue::f32_load)
}

#[inline(always)]
pub(crate) fn visit_f64_load<T>(
    vm: &mut RwasmExecutor<T>,
    address_offset: AddressOffset,
) -> Result<(), TrapCode> {
    let address = vm.sp.pop_i32();
    let memory = vm.global_memory.data();
    let value =
        UntypedValue::load_typed::<F64>(memory, address as u32, address_offset.into_inner())?;
    vm.sp.push_f64(value);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_f32_store<T>(
    vm: &mut RwasmExecutor<T>,
    address_offset: AddressOffset,
) -> Result<(), TrapCode> {
    vm.execute_store_wrap(address_offset, UntypedValue::f32_store, 4)
}

#[inline(always)]
pub(crate) fn visit_f64_store<T>(
    vm: &mut RwasmExecutor<T>,
    address_offset: AddressOffset,
) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f64();
    let address = vm.sp.pop_i32();
    let memory = vm.global_memory.data_mut();
    UntypedValue::store_typed(memory, address as u32, address_offset.into_inner(), value)?;
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i32_trunc_f32_s<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f32();
    let res = <F32 as TryTruncateInto<i32, TrapCode>>::try_truncate_into(value)?;
    vm.sp.push_i32(res);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i32_trunc_f32_u<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f32();
    let res = <F32 as TryTruncateInto<u32, TrapCode>>::try_truncate_into(value)?;
    vm.sp.push_i32(res as i32);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
fn execute_unary_32x32<T, O, I>(
    vm: &mut RwasmExecutor<T>,
    f: fn(F32) -> F32,
) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f32();
    let output_bits = f(value);
    vm.sp.push_f32(output_bits);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_f32_abs<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_unary_32x32::<T, F32, F32>(vm, <F32 as Float<F32>>::abs)
}

#[inline(always)]
pub(crate) fn visit_f32_neg<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_unary_32x32::<T, F32, F32>(vm, <F32 as Neg>::neg)
}

#[inline(always)]
pub(crate) fn visit_f32_ceil<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_unary_32x32::<T, F32, F32>(vm, <F32 as Float<F32>>::ceil)
}

#[inline(always)]
pub(crate) fn visit_f32_floor<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_unary_32x32::<T, F32, F32>(vm, <F32 as Float<F32>>::floor)
}

#[inline(always)]
pub(crate) fn visit_f32_trunc<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_unary_32x32::<T, F32, F32>(vm, <F32 as Float<F32>>::trunc)
}

#[inline(always)]
pub(crate) fn visit_f32_nearest<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_unary_32x32::<T, F32, F32>(vm, <F32 as Float<F32>>::nearest)
}

#[inline(always)]
pub(crate) fn visit_f32_sqrt<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_unary_32x32::<T, F32, F32>(vm, <F32 as Float<F32>>::sqrt)
}

#[inline(always)]
pub(crate) fn visit_f32_convert_i32_s<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_i32();
    let output_bits: F32 = value.extend_into();
    vm.sp.push_i32(output_bits.to_bits() as i32);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_f32_convert_i32_u<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_i32() as u32;
    let output_bits: F32 = value.extend_into();
    vm.sp.push_i32(output_bits.to_bits() as i32);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i32_trunc_sat_f32_s<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f32();
    let output_bits: i32 = value.truncate_saturate_into();
    vm.sp.push_i32(output_bits);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i32_trunc_sat_f32_u<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f32();
    let output_bits: u32 = value.truncate_saturate_into();
    vm.sp.push_i32(output_bits as i32);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i64_trunc_f64_s<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f64();
    let res: i64 = value.try_truncate_into()?;
    vm.sp.push_i64(res);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i64_trunc_f64_u<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f64();
    let res: u64 = value.try_truncate_into()?;
    vm.sp.push_i64(res as i64);
    vm.ip.add(1);
    Ok(())
}

fn execute_unary_64x64<T>(vm: &mut RwasmExecutor<T>, f: fn(F64) -> F64) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f64();
    let output_bits = f(value);
    vm.sp.push_f64(output_bits);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_f64_abs<T>(exec: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_unary_64x64::<T>(exec, <F64 as Float<F64>>::abs)
}

#[inline(always)]
pub(crate) fn visit_f64_neg<T>(exec: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_unary_64x64::<T>(exec, <F64 as Neg>::neg)
}

#[inline(always)]
pub(crate) fn visit_f64_ceil<T>(exec: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_unary_64x64::<T>(exec, <F64 as Float<F64>>::ceil)
}

#[inline(always)]
pub(crate) fn visit_f64_floor<T>(exec: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_unary_64x64::<T>(exec, <F64 as Float<F64>>::floor)
}

#[inline(always)]
pub(crate) fn visit_f64_trunc<T>(exec: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_unary_64x64::<T>(exec, <F64 as Float<F64>>::trunc)
}

#[inline(always)]
pub(crate) fn visit_f64_nearest<T>(exec: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_unary_64x64::<T>(exec, <F64 as Float<F64>>::nearest)
}

#[inline(always)]
pub(crate) fn visit_f64_sqrt<T>(exec: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_unary_64x64::<T>(exec, <F64 as Float<F64>>::sqrt)
}

#[inline(always)]
pub(crate) fn visit_f64_convert_i64_s<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_i64();
    let output_bits: F64 = value.extend_into();
    vm.sp.push_f64(output_bits);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_f64_convert_i64_u<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_i64() as u64;
    let output_bits: F64 = value.extend_into();
    vm.sp.push_f64(output_bits);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i64_trunc_sat_f64_s<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f64();
    let output_bits: i64 = value.truncate_saturate_into();
    vm.sp.push_i64(output_bits);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i64_trunc_sat_f64_u<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f64();
    let output_bits: u64 = value.truncate_saturate_into();
    vm.sp.push_i64(output_bits as i64);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_f32_convert_i64_s<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_i64();
    let output_bits: F32 = value.wrap_into();
    vm.sp.push_f32(output_bits);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_f32_convert_i64_u<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_i64() as u64;
    let output_bits: F32 = value.wrap_into();
    vm.sp.push_f32(output_bits);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_f32_demote_f64<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f64();
    let output_bits: F32 = value.wrap_into();
    vm.sp.push_f32(output_bits);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i32_trunc_sat_f64_s<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f64();
    let output_bits: i32 = value.truncate_saturate_into();
    vm.sp.push_i32(output_bits);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i32_trunc_sat_f64_u<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f64();
    let output_bits: u32 = value.truncate_saturate_into();
    vm.sp.push_i32(output_bits as i32);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_f64_convert_i32_s<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_i32();
    let output_bits: F64 = value.extend_into();
    vm.sp.push_f64(output_bits.into());
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_f64_convert_i32_u<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_i32() as u32;
    let output_bits: F64 = value.extend_into();
    vm.sp.push_f64(output_bits.into());
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_f64_promote_f32<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f32();
    let output_bits: F64 = value.extend_into();
    vm.sp.push_f64(output_bits.into());
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i64_trunc_sat_f32_s<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f32();
    let output_bits: i64 = value.truncate_saturate_into();
    vm.sp.push_i64(output_bits);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_i64_trunc_sat_f32_u<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let value = vm.sp.pop_f32();
    let output_bits: u64 = value.truncate_saturate_into();
    vm.sp.push_i64(output_bits as i64);
    vm.ip.add(1);
    Ok(())
}

macro_rules! impl_visit_binary_f32 {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
                vm.sp.eval_top2(UntypedValue::$untyped_ident);
                vm.ip.add(1);
                Ok(())
            }
        )*
    }
}

impl_visit_binary_f32! {
    fn visit_f32_eq(f32_eq);
    fn visit_f32_ne(f32_ne);
    fn visit_f32_lt(f32_lt);
    fn visit_f32_gt(f32_gt);
    fn visit_f32_le(f32_le);
    fn visit_f32_ge(f32_ge);

    fn visit_f32_add(f32_add);
    fn visit_f32_sub(f32_sub);
    fn visit_f32_mul(f32_mul);
    fn visit_f32_div(f32_div);
    fn visit_f32_min(f32_min);
    fn visit_f32_max(f32_max);
    fn visit_f32_copysign(f32_copysign);
}

#[inline(always)]
fn execute_binary_cmp<T>(
    vm: &mut RwasmExecutor<T>,
    f: fn(F64, F64) -> bool,
) -> Result<(), TrapCode> {
    let rhs = vm.sp.pop_f64();
    let lhs = vm.sp.pop_f64();
    let result = f(lhs, rhs);
    vm.sp.push_i32(result.into());
    vm.ip.add(1);
    Ok(())
}

macro_rules! op {
    ( $operator:tt ) => {{
        |lhs, rhs| lhs $operator rhs
    }};
}

#[inline(always)]
pub(crate) fn visit_f64_eq<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_binary_cmp::<T>(vm, op!(==))
}

#[inline(always)]
pub(crate) fn visit_f64_ne<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_binary_cmp::<T>(vm, op!(!=))
}

#[inline(always)]
pub(crate) fn visit_f64_lt<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_binary_cmp::<T>(vm, op!(<))
}

#[inline(always)]
pub(crate) fn visit_f64_gt<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_binary_cmp::<T>(vm, op!(>))
}

#[inline(always)]
pub(crate) fn visit_f64_le<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_binary_cmp::<T>(vm, op!(<=))
}

#[inline(always)]
pub(crate) fn visit_f64_ge<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_binary_cmp::<T>(vm, op!(>=))
}

#[inline(always)]
fn execute_binary<T>(vm: &mut RwasmExecutor<T>, f: fn(F64, F64) -> F64) -> Result<(), TrapCode> {
    let rhs = vm.sp.pop_f64();
    let lhs = vm.sp.pop_f64();
    let result = f(lhs, rhs);
    vm.sp.push_f64(result.into());
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_f64_add<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_binary::<T>(vm, <F64 as ArithmeticOps<F64>>::add)
}

#[inline(always)]
pub(crate) fn visit_f64_sub<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_binary::<T>(vm, <F64 as ArithmeticOps<F64>>::sub)
}

#[inline(always)]
pub(crate) fn visit_f64_mul<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_binary::<T>(vm, <F64 as ArithmeticOps<F64>>::mul)
}

#[inline(always)]
pub(crate) fn visit_f64_div<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_binary::<T>(vm, <F64 as Float<F64>>::div)
}

#[inline(always)]
pub(crate) fn visit_f64_min<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_binary::<T>(vm, <F64 as Float<F64>>::min)
}

#[inline(always)]
pub(crate) fn visit_f64_max<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_binary::<T>(vm, <F64 as Float<F64>>::max)
}

#[inline(always)]
pub(crate) fn visit_f64_copysign<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    execute_binary::<T>(vm, <F64 as Float<F64>>::copysign)
}

pub(crate) fn exec_fpu_opcode<T>(
    vm: &mut RwasmExecutor<T>,
    opcode: Opcode,
) -> Result<(), TrapCode> {
    use Opcode::*;
    match opcode {
        F32Load(imm) => visit_f32_load(vm, imm),
        F64Load(imm) => visit_f64_load(vm, imm),
        F32Store(imm) => visit_f32_store(vm, imm),
        F64Store(imm) => visit_f64_store(vm, imm),
        F32Eq => visit_f32_eq(vm),
        F32Ne => visit_f32_ne(vm),
        F32Lt => visit_f32_lt(vm),
        F32Gt => visit_f32_gt(vm),
        F32Le => visit_f32_le(vm),
        F32Ge => visit_f32_ge(vm),
        F64Eq => visit_f64_eq(vm),
        F64Ne => visit_f64_ne(vm),
        F64Lt => visit_f64_lt(vm),
        F64Gt => visit_f64_gt(vm),
        F64Le => visit_f64_le(vm),
        F64Ge => visit_f64_ge(vm),
        F32Abs => visit_f32_abs(vm),
        F32Neg => visit_f32_neg(vm),
        F32Ceil => visit_f32_ceil(vm),
        F32Floor => visit_f32_floor(vm),
        F32Trunc => visit_f32_trunc(vm),
        F32Nearest => visit_f32_nearest(vm),
        F32Sqrt => visit_f32_sqrt(vm),
        F32Add => visit_f32_add(vm),
        F32Sub => visit_f32_sub(vm),
        F32Mul => visit_f32_mul(vm),
        F32Div => visit_f32_div(vm),
        F32Min => visit_f32_min(vm),
        F32Max => visit_f32_max(vm),
        F32Copysign => visit_f32_copysign(vm),
        F64Abs => visit_f64_abs(vm),
        F64Neg => visit_f64_neg(vm),
        F64Ceil => visit_f64_ceil(vm),
        F64Floor => visit_f64_floor(vm),
        F64Trunc => visit_f64_trunc(vm),
        F64Nearest => visit_f64_nearest(vm),
        F64Sqrt => visit_f64_sqrt(vm),
        F64Add => visit_f64_add(vm),
        F64Sub => visit_f64_sub(vm),
        F64Mul => visit_f64_mul(vm),
        F64Div => visit_f64_div(vm),
        F64Min => visit_f64_min(vm),
        F64Max => visit_f64_max(vm),
        F64Copysign => visit_f64_copysign(vm),
        I32TruncF32S => visit_i32_trunc_f32_s(vm),
        I32TruncF32U => visit_i32_trunc_f32_u(vm),
        I32TruncF64S => visit_i32_trunc_f64_s(vm),
        I32TruncF64U => visit_i32_trunc_f64_u(vm),
        I64TruncF32S => visit_i64_trunc_f32_s(vm),
        I64TruncF32U => visit_i64_trunc_f32_u(vm),
        I64TruncF64S => visit_i64_trunc_f64_s(vm),
        I64TruncF64U => visit_i64_trunc_f64_u(vm),
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
}
