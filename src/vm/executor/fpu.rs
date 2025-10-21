use crate::{
    AddressOffset, ArithmeticOps, ExtendInto, Float, IGlobalMemory, Opcode, RwasmExecutor,
    TrapCode, TruncateSaturateInto, TryTruncateInto, UntypedValue, WrapInto, F32, F64,
};
use core::ops::Neg;

macro_rules! impl_visit_binary_f32 {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident(&mut self) -> Result<(), TrapCode> {
                self.sp.eval_top2(UntypedValue::$untyped_ident);
                self.ip.add(1);
                Ok(())
            }
        )*
    }
}

macro_rules! op {
    ( $operator:tt ) => {{
        |lhs, rhs| lhs $operator rhs
    }};
}

impl<'a, T: Send + Sync> RwasmExecutor<'a, T> {
    #[inline(always)]
    pub(crate) fn visit_i32_trunc_f64_s(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f64();
        let res = <F64 as TryTruncateInto<i32, TrapCode>>::try_truncate_into(value)?;
        self.sp.push_i32(res);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i32_trunc_f64_u(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f64();
        let res = <F64 as TryTruncateInto<u32, TrapCode>>::try_truncate_into(value)?;
        self.sp.push_i32(res as i32);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i64_trunc_f32_s(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f32();
        let res = <F32 as TryTruncateInto<i64, TrapCode>>::try_truncate_into(value)?;
        self.sp.push_i64(res);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i64_trunc_f32_u(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f32();
        let res = <F32 as TryTruncateInto<u64, TrapCode>>::try_truncate_into(value)?;
        self.sp.push_i64(res as i64);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f32_load(&mut self, address_offset: AddressOffset) -> Result<(), TrapCode> {
        self.sp.try_eval_top(|address| {
            let memory = self.store.global_memory().data();
            let value = UntypedValue::f32_load(memory, address, address_offset)?;
            Ok(value)
        })?;
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f64_load(&mut self, address_offset: AddressOffset) -> Result<(), TrapCode> {
        let address = self.sp.pop_i32();
        let memory = self.store.global_memory().data();
        let value = UntypedValue::load_typed::<F64>(memory, address as u32, address_offset)?;
        self.sp.push_f64(value);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f32_store(
        &mut self,
        address_offset: AddressOffset,
    ) -> Result<(), TrapCode> {
        let (address, value) = self.sp.pop2();
        let memory = self.store.global_memory().data_mut();
        UntypedValue::f32_store(memory, address, address_offset, value)?;
        #[cfg(feature = "tracing")]
        {
            let base_address = address_offset + u32::from(address);
            self.store.tracer.memory_change(
                base_address,
                4,
                &memory[base_address as usize..(base_address + 4) as usize],
            );
        }
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f64_store(
        &mut self,
        address_offset: AddressOffset,
    ) -> Result<(), TrapCode> {
        let value = self.sp.pop_f64();
        let address = self.sp.pop_i32();
        let memory = self.store.global_memory().data_mut();
        UntypedValue::store_typed(memory, address as u32, address_offset, value)?;
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i32_trunc_f32_s(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f32();
        let res = <F32 as TryTruncateInto<i32, TrapCode>>::try_truncate_into(value)?;
        self.sp.push_i32(res);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i32_trunc_f32_u(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f32();
        let res = <F32 as TryTruncateInto<u32, TrapCode>>::try_truncate_into(value)?;
        self.sp.push_i32(res as i32);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    fn execute_unary_32x32<O, I>(&mut self, f: fn(F32) -> F32) -> Result<(), TrapCode> {
        let value = self.sp.pop_f32();
        let output_bits = f(value);
        self.sp.push_f32(output_bits);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f32_abs(&mut self) -> Result<(), TrapCode> {
        self.execute_unary_32x32::<F32, F32>(<F32 as Float<F32>>::abs)
    }

    #[inline(always)]
    pub(crate) fn visit_f32_neg(&mut self) -> Result<(), TrapCode> {
        self.execute_unary_32x32::<F32, F32>(<F32 as Neg>::neg)
    }

    #[inline(always)]
    pub(crate) fn visit_f32_ceil(&mut self) -> Result<(), TrapCode> {
        self.execute_unary_32x32::<F32, F32>(<F32 as Float<F32>>::ceil)
    }

    #[inline(always)]
    pub(crate) fn visit_f32_floor(&mut self) -> Result<(), TrapCode> {
        self.execute_unary_32x32::<F32, F32>(<F32 as Float<F32>>::floor)
    }

    #[inline(always)]
    pub(crate) fn visit_f32_trunc(&mut self) -> Result<(), TrapCode> {
        self.execute_unary_32x32::<F32, F32>(<F32 as Float<F32>>::trunc)
    }

    #[inline(always)]
    pub(crate) fn visit_f32_nearest(&mut self) -> Result<(), TrapCode> {
        self.execute_unary_32x32::<F32, F32>(<F32 as Float<F32>>::nearest)
    }

    #[inline(always)]
    pub(crate) fn visit_f32_sqrt(&mut self) -> Result<(), TrapCode> {
        self.execute_unary_32x32::<F32, F32>(<F32 as Float<F32>>::sqrt)
    }

    #[inline(always)]
    pub(crate) fn visit_f32_convert_i32_s(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_i32();
        let output_bits: F32 = value.extend_into();
        self.sp.push_i32(output_bits.to_bits() as i32);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f32_convert_i32_u(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_i32() as u32;
        let output_bits: F32 = value.extend_into();
        self.sp.push_i32(output_bits.to_bits() as i32);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i32_trunc_sat_f32_s(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f32();
        let output_bits: i32 = value.truncate_saturate_into();
        self.sp.push_i32(output_bits);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i32_trunc_sat_f32_u(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f32();
        let output_bits: u32 = value.truncate_saturate_into();
        self.sp.push_i32(output_bits as i32);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i64_trunc_f64_s(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f64();
        let res: i64 = value.try_truncate_into()?;
        self.sp.push_i64(res);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i64_trunc_f64_u(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f64();
        let res: u64 = value.try_truncate_into()?;
        self.sp.push_i64(res as i64);
        self.ip.add(1);
        Ok(())
    }

    fn execute_unary_64x64(&mut self, f: fn(F64) -> F64) -> Result<(), TrapCode> {
        let value = self.sp.pop_f64();
        let output_bits = f(value);
        self.sp.push_f64(output_bits);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f64_abs(&mut self) -> Result<(), TrapCode> {
        self.execute_unary_64x64(<F64 as Float<F64>>::abs)
    }

    #[inline(always)]
    pub(crate) fn visit_f64_neg(&mut self) -> Result<(), TrapCode> {
        self.execute_unary_64x64(<F64 as Neg>::neg)
    }

    #[inline(always)]
    pub(crate) fn visit_f64_ceil(&mut self) -> Result<(), TrapCode> {
        self.execute_unary_64x64(<F64 as Float<F64>>::ceil)
    }

    #[inline(always)]
    pub(crate) fn visit_f64_floor(&mut self) -> Result<(), TrapCode> {
        self.execute_unary_64x64(<F64 as Float<F64>>::floor)
    }

    #[inline(always)]
    pub(crate) fn visit_f64_trunc(&mut self) -> Result<(), TrapCode> {
        self.execute_unary_64x64(<F64 as Float<F64>>::trunc)
    }

    #[inline(always)]
    pub(crate) fn visit_f64_nearest(&mut self) -> Result<(), TrapCode> {
        self.execute_unary_64x64(<F64 as Float<F64>>::nearest)
    }

    #[inline(always)]
    pub(crate) fn visit_f64_sqrt(&mut self) -> Result<(), TrapCode> {
        self.execute_unary_64x64(<F64 as Float<F64>>::sqrt)
    }

    #[inline(always)]
    pub(crate) fn visit_f64_convert_i64_s(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_i64();
        let output_bits: F64 = value.extend_into();
        self.sp.push_f64(output_bits);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f64_convert_i64_u(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_i64() as u64;
        let output_bits: F64 = value.extend_into();
        self.sp.push_f64(output_bits);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i64_trunc_sat_f64_s(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f64();
        let output_bits: i64 = value.truncate_saturate_into();
        self.sp.push_i64(output_bits);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i64_trunc_sat_f64_u(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f64();
        let output_bits: u64 = value.truncate_saturate_into();
        self.sp.push_i64(output_bits as i64);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f32_convert_i64_s(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_i64();
        let output_bits: F32 = value.wrap_into();
        self.sp.push_f32(output_bits);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f32_convert_i64_u(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_i64() as u64;
        let output_bits: F32 = value.wrap_into();
        self.sp.push_f32(output_bits);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f32_demote_f64(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f64();
        let output_bits: F32 = value.wrap_into();
        self.sp.push_f32(output_bits);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i32_trunc_sat_f64_s(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f64();
        let output_bits: i32 = value.truncate_saturate_into();
        self.sp.push_i32(output_bits);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i32_trunc_sat_f64_u(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f64();
        let output_bits: u32 = value.truncate_saturate_into();
        self.sp.push_i32(output_bits as i32);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f64_convert_i32_s(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_i32();
        let output_bits: F64 = value.extend_into();
        self.sp.push_f64(output_bits.into());
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f64_convert_i32_u(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_i32() as u32;
        let output_bits: F64 = value.extend_into();
        self.sp.push_f64(output_bits.into());
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f64_promote_f32(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f32();
        let output_bits: F64 = value.extend_into();
        self.sp.push_f64(output_bits.into());
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i64_trunc_sat_f32_s(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f32();
        let output_bits: i64 = value.truncate_saturate_into();
        self.sp.push_i64(output_bits);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_i64_trunc_sat_f32_u(&mut self) -> Result<(), TrapCode> {
        let value = self.sp.pop_f32();
        let output_bits: u64 = value.truncate_saturate_into();
        self.sp.push_i64(output_bits as i64);
        self.ip.add(1);
        Ok(())
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
    fn execute_binary_cmp(&mut self, f: fn(F64, F64) -> bool) -> Result<(), TrapCode> {
        let rhs = self.sp.pop_f64();
        let lhs = self.sp.pop_f64();
        let result = f(lhs, rhs);
        self.sp.push_i32(result.into());
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f64_eq(&mut self) -> Result<(), TrapCode> {
        self.execute_binary_cmp(op!(==))
    }

    #[inline(always)]
    pub(crate) fn visit_f64_ne(&mut self) -> Result<(), TrapCode> {
        self.execute_binary_cmp(op!(!=))
    }

    #[inline(always)]
    pub(crate) fn visit_f64_lt(&mut self) -> Result<(), TrapCode> {
        self.execute_binary_cmp(op!(<))
    }

    #[inline(always)]
    pub(crate) fn visit_f64_gt(&mut self) -> Result<(), TrapCode> {
        self.execute_binary_cmp(op!(>))
    }

    #[inline(always)]
    pub(crate) fn visit_f64_le(&mut self) -> Result<(), TrapCode> {
        self.execute_binary_cmp(op!(<=))
    }

    #[inline(always)]
    pub(crate) fn visit_f64_ge(&mut self) -> Result<(), TrapCode> {
        self.execute_binary_cmp(op!(>=))
    }

    #[inline(always)]
    fn execute_binary(&mut self, f: fn(F64, F64) -> F64) -> Result<(), TrapCode> {
        let rhs = self.sp.pop_f64();
        let lhs = self.sp.pop_f64();
        let result = f(lhs, rhs);
        self.sp.push_f64(result.into());
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_f64_add(&mut self) -> Result<(), TrapCode> {
        self.execute_binary(<F64 as ArithmeticOps<F64>>::add)
    }

    #[inline(always)]
    pub(crate) fn visit_f64_sub(&mut self) -> Result<(), TrapCode> {
        self.execute_binary(<F64 as ArithmeticOps<F64>>::sub)
    }

    #[inline(always)]
    pub(crate) fn visit_f64_mul(&mut self) -> Result<(), TrapCode> {
        self.execute_binary(<F64 as ArithmeticOps<F64>>::mul)
    }

    #[inline(always)]
    pub(crate) fn visit_f64_div(&mut self) -> Result<(), TrapCode> {
        self.execute_binary(<F64 as Float<F64>>::div)
    }

    #[inline(always)]
    pub(crate) fn visit_f64_min(&mut self) -> Result<(), TrapCode> {
        self.execute_binary(<F64 as Float<F64>>::min)
    }

    #[inline(always)]
    pub(crate) fn visit_f64_max(&mut self) -> Result<(), TrapCode> {
        self.execute_binary(<F64 as Float<F64>>::max)
    }

    #[inline(always)]
    pub(crate) fn visit_f64_copysign(&mut self) -> Result<(), TrapCode> {
        self.execute_binary(<F64 as Float<F64>>::copysign)
    }

    pub(crate) fn exec_fpu_opcode(&mut self, opcode: Opcode) -> Result<(), TrapCode> {
        use Opcode::*;
        match opcode {
            F32Load(imm) => self.visit_f32_load(imm),
            F64Load(imm) => self.visit_f64_load(imm),
            F32Store(imm) => self.visit_f32_store(imm),
            F64Store(imm) => self.visit_f64_store(imm),
            F32Eq => self.visit_f32_eq(),
            F32Ne => self.visit_f32_ne(),
            F32Lt => self.visit_f32_lt(),
            F32Gt => self.visit_f32_gt(),
            F32Le => self.visit_f32_le(),
            F32Ge => self.visit_f32_ge(),
            F64Eq => self.visit_f64_eq(),
            F64Ne => self.visit_f64_ne(),
            F64Lt => self.visit_f64_lt(),
            F64Gt => self.visit_f64_gt(),
            F64Le => self.visit_f64_le(),
            F64Ge => self.visit_f64_ge(),
            F32Abs => self.visit_f32_abs(),
            F32Neg => self.visit_f32_neg(),
            F32Ceil => self.visit_f32_ceil(),
            F32Floor => self.visit_f32_floor(),
            F32Trunc => self.visit_f32_trunc(),
            F32Nearest => self.visit_f32_nearest(),
            F32Sqrt => self.visit_f32_sqrt(),
            F32Add => self.visit_f32_add(),
            F32Sub => self.visit_f32_sub(),
            F32Mul => self.visit_f32_mul(),
            F32Div => self.visit_f32_div(),
            F32Min => self.visit_f32_min(),
            F32Max => self.visit_f32_max(),
            F32Copysign => self.visit_f32_copysign(),
            F64Abs => self.visit_f64_abs(),
            F64Neg => self.visit_f64_neg(),
            F64Ceil => self.visit_f64_ceil(),
            F64Floor => self.visit_f64_floor(),
            F64Trunc => self.visit_f64_trunc(),
            F64Nearest => self.visit_f64_nearest(),
            F64Sqrt => self.visit_f64_sqrt(),
            F64Add => self.visit_f64_add(),
            F64Sub => self.visit_f64_sub(),
            F64Mul => self.visit_f64_mul(),
            F64Div => self.visit_f64_div(),
            F64Min => self.visit_f64_min(),
            F64Max => self.visit_f64_max(),
            F64Copysign => self.visit_f64_copysign(),
            I32TruncF32S => self.visit_i32_trunc_f32_s(),
            I32TruncF32U => self.visit_i32_trunc_f32_u(),
            I32TruncF64S => self.visit_i32_trunc_f64_s(),
            I32TruncF64U => self.visit_i32_trunc_f64_u(),
            I64TruncF32S => self.visit_i64_trunc_f32_s(),
            I64TruncF32U => self.visit_i64_trunc_f32_u(),
            I64TruncF64S => self.visit_i64_trunc_f64_s(),
            I64TruncF64U => self.visit_i64_trunc_f64_u(),
            F32ConvertI32S => self.visit_f32_convert_i32_s(),
            F32ConvertI32U => self.visit_f32_convert_i32_u(),
            F32ConvertI64S => self.visit_f32_convert_i64_s(),
            F32ConvertI64U => self.visit_f32_convert_i64_u(),
            F32DemoteF64 => self.visit_f32_demote_f64(),
            F64ConvertI32S => self.visit_f64_convert_i32_s(),
            F64ConvertI32U => self.visit_f64_convert_i32_u(),
            F64ConvertI64S => self.visit_f64_convert_i64_s(),
            F64ConvertI64U => self.visit_f64_convert_i64_u(),
            F64PromoteF32 => self.visit_f64_promote_f32(),
            I32TruncSatF32S => self.visit_i32_trunc_sat_f32_s(),
            I32TruncSatF32U => self.visit_i32_trunc_sat_f32_u(),
            I32TruncSatF64S => self.visit_i32_trunc_sat_f64_s(),
            I32TruncSatF64U => self.visit_i32_trunc_sat_f64_u(),
            I64TruncSatF32S => self.visit_i64_trunc_sat_f32_s(),
            I64TruncSatF32U => self.visit_i64_trunc_sat_f32_u(),
            I64TruncSatF64S => self.visit_i64_trunc_sat_f64_s(),
            I64TruncSatF64U => self.visit_i64_trunc_sat_f64_u(),
            _ => unreachable!(),
        }
    }
}
