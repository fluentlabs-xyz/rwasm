use crate::{types::UntypedValue, vm::executor::RwasmExecutor, TrapCode};

macro_rules! impl_visit_unary {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident(&mut self) {
                self.sp.eval_top(UntypedValue::$untyped_ident);
                self.ip.add(1);
            }
        )*
    }
}

impl<'a, T: Send> RwasmExecutor<'a, T> {
    impl_visit_unary! {
        fn visit_i32_eqz(i32_eqz);

        fn visit_i32_clz(i32_clz);
        fn visit_i32_ctz(i32_ctz);
        fn visit_i32_popcnt(i32_popcnt);

        fn visit_i32_wrap_i64(i32_wrap_i64);

        fn visit_i32_extend8_s(i32_extend8_s);
        fn visit_i32_extend16_s(i32_extend16_s);
    }
}

macro_rules! impl_visit_binary {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident(&mut self) {
                self.sp.eval_top2(UntypedValue::$untyped_ident);
                self.ip.add(1);
            }
        )*
    }
}

impl<'a, T: Send> RwasmExecutor<'a, T> {
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
    }

    pub(crate) fn visit_i32_mul64(&mut self) {
        let (lhs, rhs) = self.sp.pop2();
        let res = lhs.as_i64().wrapping_mul(rhs.as_i64());
        self.sp.push_i64(res);
        self.ip.add(1);
    }

    pub(crate) fn visit_i32_add64(&mut self) {
        let (lhs, rhs) = self.sp.pop2();
        let res = lhs.as_i64().wrapping_add(rhs.as_i64());
        self.sp.push_i64(res);
        self.ip.add(1);
    }
}

macro_rules! impl_visit_fallible_binary {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident(&mut self) -> Result<(), TrapCode> {
                self.sp.try_eval_top2(UntypedValue::$untyped_ident)?;
                self.ip.add(1);
                Ok(())
            }
        )*
    }
}

impl<'a, T: Send> RwasmExecutor<'a, T> {
    impl_visit_fallible_binary! {
        fn visit_i32_div_s(i32_div_s);
        fn visit_i32_div_u(i32_div_u);
        fn visit_i32_rem_s(i32_rem_s);
        fn visit_i32_rem_u(i32_rem_u);
    }
}
