#![allow(dead_code)]

//! Data structures to represent Wasm constant expressions.
//!
//! This has built-in support for the `extended-const` Wasm proposal.
//!
//! A [`CompiledExpr`] keeps its operators in postfix order and evaluates them with an explicit
//! operand stack, so constructing, evaluating and dropping an expression never recurses. With
//! `extended-const` a validated expression may be an arbitrarily long operator chain, and a tree of
//! nested closures (one per operator) overflowed the compiler's native stack on such input long
//! before any fuel could be charged.

use crate::{CompilationError, ExternRef, FuncIdx, FuncRef, GlobalIdx, Value, F32, F64};
use smallvec::SmallVec;
use wasmparser::ConstExpr;

/// Types that allow evaluation given an evaluation context.
pub trait Eval {
    /// Evaluates `self` given an [`EvalContext`].
    fn eval(&self, ctx: &dyn EvalContext) -> Option<i64>;
}

/// A [`CompiledExpr`] evaluation context.
///
/// Required for evaluating a [`CompiledExpr`].
pub trait EvalContext {
    /// Returns the [`Value`] of the global value at `index` if any.
    fn get_global(&self, index: u32) -> Option<Value>;
    /// Returns the [`FuncRef`] of the function at `index` if any.
    fn get_func(&self, index: u32) -> Option<FuncRef>;
}

/// An empty evaluation context.
pub struct EmptyEvalContext;

impl EvalContext for EmptyEvalContext {
    fn get_global(&self, _index: u32) -> Option<Value> {
        None
    }

    fn get_func(&self, _index: u32) -> Option<FuncRef> {
        None
    }
}

/// A binary operator of the `extended-const` proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// `i32.add`
    I32Add,
    /// `i32.sub`
    I32Sub,
    /// `i32.mul`
    I32Mul,
    /// `i64.add`
    I64Add,
    /// `i64.sub`
    I64Sub,
    /// `i64.mul`
    I64Mul,
}

impl BinaryOp {
    /// Applies the operator to its two operands.
    fn apply(self, lhs: i64, rhs: i64) -> i64 {
        match self {
            Self::I32Add => i32::wrapping_add(lhs as i32, rhs as i32) as i64,
            Self::I32Sub => i32::wrapping_sub(lhs as i32, rhs as i32) as i64,
            Self::I32Mul => i32::wrapping_mul(lhs as i32, rhs as i32) as i64,
            Self::I64Add => lhs.wrapping_add(rhs),
            Self::I64Sub => lhs.wrapping_sub(rhs),
            Self::I64Mul => lhs.wrapping_mul(rhs),
        }
    }
}

/// One operator of a [`CompiledExpr`], stored in evaluation (postfix) order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// A constant value: `i32.const`, `i64.const`, `f32.const`, `f64.const` or `ref.null`.
    Const(i64),
    /// The value of a global variable: `global.get index`.
    Global(u32),
    /// A Wasm `ref.func index` value.
    FuncRef(u32),
    /// A binary operator over the two topmost operands.
    Binary(BinaryOp),
}

impl Op {
    /// Creates a new constant operator for the given `value`.
    pub fn constant<T>(value: T) -> Self
    where
        T: Into<Value>,
    {
        let value: Value = value.into();
        let value = match value {
            Value::I32(value) => value as i64,
            Value::I64(value) => value,
            Value::F32(value) => value.to_bits() as i32 as i64,
            Value::F64(value) => value.to_bits() as i64,
            Value::FuncRef(value) => value.0 as i64,
            Value::ExternRef(value) => value.0 as i64,
        };
        Self::Const(value)
    }

    /// Creates a new global operator with the given index.
    pub fn global(global_index: u32) -> Self {
        Self::Global(global_index)
    }

    /// Creates a new `ref.func` operator with the given index.
    pub fn funcref(function_index: u32) -> Self {
        Self::FuncRef(function_index)
    }
}

/// Returns the bits of a global variable's value as seen by a constant expression.
fn global_bits(value: Value) -> i64 {
    match value {
        Value::I32(value) => value as i64,
        Value::I64(value) => value,
        Value::F32(value) => value.to_bits() as u64 as i64,
        Value::F64(value) => value.to_bits() as i64,
        Value::FuncRef(value) => value.0 as i32 as i64,
        Value::ExternRef(value) => value.0 as i32 as i64,
    }
}

/// A Wasm constant expression.
///
/// These are used to determine the offsets of memory data
/// and table element segments as well as the initial value
/// of global variables.
#[derive(Debug, Clone)]
pub struct CompiledExpr {
    /// The operators in postfix order.
    ///
    /// Wasm validation guarantees that evaluating them leaves exactly one value on the operand
    /// stack; [`CompiledExpr::new`] re-checks that so [`Eval::eval`] is total.
    ops: SmallVec<[Op; 1]>,
}

impl Eval for CompiledExpr {
    fn eval(&self, ctx: &dyn EvalContext) -> Option<i64> {
        let mut stack = SmallVec::<[i64; 4]>::new();
        for op in &self.ops {
            let value = match *op {
                Op::Const(value) => value,
                Op::Global(global_index) => global_bits(ctx.get_global(global_index)?),
                Op::FuncRef(function_index) => ctx.get_func(function_index)?.0 as i64,
                Op::Binary(op) => {
                    let rhs = stack.pop()?;
                    let lhs = stack.pop()?;
                    op.apply(lhs, rhs)
                }
            };
            stack.push(value);
        }
        let result = stack.pop()?;
        stack.is_empty().then_some(result)
    }
}

impl CompiledExpr {
    pub fn zero() -> Self {
        Self::from_const(0)
    }

    pub fn from_const(value: i64) -> Self {
        Self::from_op(Op::Const(value))
    }

    fn from_op(op: Op) -> Self {
        let mut ops = SmallVec::new();
        ops.push(op);
        Self { ops }
    }

    /// Creates a new [`CompiledExpr`] from the given Wasm [`ConstExpr`].
    ///
    /// # Note
    ///
    /// The constructor assumes that Wasm validation already succeeded on the input, and it reads
    /// the operators in one pass: the operator count is bounded only by the module size, so it is
    /// never used as a recursion depth.
    ///
    /// # Errors
    ///
    /// - [`CompilationError::MalformedWasmBinary`] if the operator encoding is malformed.
    /// - [`CompilationError::NotSupportedOpcode`] for an operator outside the constant subset.
    /// - [`CompilationError::ConstEvaluationFailed`] if the operators do not leave exactly one
    ///   value on the operand stack.
    ///
    /// Wasm validation rules all three out, so hitting one means the validator and the translator
    /// disagree on the accepted language; the error keeps that a rejected module rather than a
    /// panic.
    pub fn new(expr: ConstExpr<'_>) -> Result<Self, CompilationError> {
        let mut reader = expr.get_operators_reader();
        let mut ops = SmallVec::new();
        // the operand-stack height a stack machine would have after the operators read so far
        let mut height: usize = 0;
        loop {
            let op = match reader.read()? {
                wasmparser::Operator::I32Const { value } => Op::constant(value),
                wasmparser::Operator::I64Const { value } => Op::constant(value),
                wasmparser::Operator::F32Const { value } => Op::constant(F32::from(value.bits())),
                wasmparser::Operator::F64Const { value } => Op::constant(F64::from(value.bits())),
                wasmparser::Operator::GlobalGet { global_index } => Op::global(global_index),
                wasmparser::Operator::RefNull { ty } => match ty {
                    wasmparser::ValType::FuncRef => Op::constant(Value::from(FuncRef::null())),
                    wasmparser::ValType::ExternRef => Op::constant(Value::from(ExternRef::null())),
                    _ => return Err(CompilationError::NotSupportedOpcode),
                },
                wasmparser::Operator::RefFunc { function_index } => Op::funcref(function_index),
                wasmparser::Operator::I32Add => Op::Binary(BinaryOp::I32Add),
                wasmparser::Operator::I32Sub => Op::Binary(BinaryOp::I32Sub),
                wasmparser::Operator::I32Mul => Op::Binary(BinaryOp::I32Mul),
                wasmparser::Operator::I64Add => Op::Binary(BinaryOp::I64Add),
                wasmparser::Operator::I64Sub => Op::Binary(BinaryOp::I64Sub),
                wasmparser::Operator::I64Mul => Op::Binary(BinaryOp::I64Mul),
                wasmparser::Operator::End => break,
                _ => return Err(CompilationError::NotSupportedOpcode),
            };
            height = match op {
                // a binary operator consumes two values and produces one
                Op::Binary(_) => {
                    height
                        .checked_sub(2)
                        .ok_or(CompilationError::ConstEvaluationFailed)?
                        + 1
                }
                _ => height + 1,
            };
            ops.push(op);
        }
        reader.ensure_end()?;
        if height != 1 {
            return Err(CompilationError::ConstEvaluationFailed);
        }
        Ok(Self { ops })
    }

    /// Create a new `ref.func x` [`CompiledExpr`].
    ///
    /// # Note
    ///
    /// Required for setting up table elements.
    pub fn new_funcref(function_index: u32) -> Self {
        Self::from_op(Op::FuncRef(function_index))
    }

    /// Returns `Some(index)` if the [`CompiledExpr`] is a `funcref(index)`.
    ///
    /// Otherwise returns `None`.
    pub fn funcref(&self) -> Option<FuncIdx> {
        match self.ops.as_slice() {
            [Op::FuncRef(function_index)] => Some(FuncIdx::from(*function_index)),
            _ => None,
        }
    }

    /// Returns `Some(index)` if the [`CompiledExpr`] is a `global.get index`.
    ///
    /// Otherwise returns `None`.
    pub fn global(&self) -> Option<GlobalIdx> {
        match self.ops.as_slice() {
            [Op::Global(global_index)] => Some(GlobalIdx::from(*global_index)),
            _ => None,
        }
    }

    /// Evaluates the [`CompiledExpr`] in a constant evaluation context.
    ///
    /// # Note
    ///
    /// This is useful for evaluations during Wasm translation to
    /// perform optimizations on the translated bytecode.
    pub fn eval_const(&self) -> Option<i64> {
        self.eval(&EmptyEvalContext)
    }

    /// Evaluates the [`CompiledExpr`] given a context for globals and functions.
    ///
    /// Returns `None` if a non-const expression operand is encountered
    /// or the provided globals and functions context returns `None`.
    ///
    /// # Note
    ///
    /// This is useful for evaluation of [`CompiledExpr`] during bytecode execution.
    pub fn eval_with_context<G, F>(&self, global_get: G, func_get: F) -> Option<i64>
    where
        G: Fn(u32) -> Option<Value>,
        F: Fn(u32) -> Option<FuncRef>,
    {
        /// Context that wraps closures representing partial evaluation contexts.
        struct WrappedEvalContext<G, F> {
            /// Wrapped context for global variables.
            global_get: G,
            /// Wrapped context for functions.
            func_get: F,
        }
        impl<G, F> EvalContext for WrappedEvalContext<G, F>
        where
            G: Fn(u32) -> Option<Value>,
            F: Fn(u32) -> Option<FuncRef>,
        {
            fn get_global(&self, index: u32) -> Option<Value> {
                (self.global_get)(index)
            }

            fn get_func(&self, index: u32) -> Option<FuncRef> {
                (self.func_get)(index)
            }
        }
        self.eval(&WrappedEvalContext::<G, F> {
            global_get,
            func_get,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    // A tiny evaluation context for tests.
    struct TestCtx {
        globals: Vec<Option<Value>>,
        funcs: Vec<Option<FuncRef>>,
    }

    impl TestCtx {
        fn new() -> Self {
            Self {
                globals: Vec::new(),
                funcs: Vec::new(),
            }
        }

        fn with_globals(mut self, globals: Vec<Option<Value>>) -> Self {
            self.globals = globals;
            self
        }

        fn with_funcs(mut self, funcs: Vec<Option<FuncRef>>) -> Self {
            self.funcs = funcs;
            self
        }
    }

    impl EvalContext for TestCtx {
        fn get_global(&self, index: u32) -> Option<Value> {
            self.globals.get(index as usize).and_then(|opt| opt.clone())
        }

        fn get_func(&self, index: u32) -> Option<FuncRef> {
            self.funcs.get(index as usize).and_then(|opt| opt.clone())
        }
    }

    fn bits_f32(x: f32) -> i64 {
        x.to_bits() as i32 as i64
    }

    fn bits_f64(x: f64) -> i64 {
        x.to_bits() as i64
    }

    fn parse_const_expr(bytes: &[u8]) -> ConstExpr<'_> {
        ConstExpr::new(bytes, 0)
    }

    fn compile(bytes: &[u8]) -> CompiledExpr {
        CompiledExpr::new(parse_const_expr(bytes)).unwrap()
    }

    #[test]
    fn empty_eval_context_always_none() {
        let ctx = EmptyEvalContext;
        assert!(ctx.get_global(0).is_none());
        assert!(ctx.get_global(123).is_none());
        assert!(ctx.get_func(0).is_none());
        assert!(ctx.get_func(456).is_none());
    }

    #[test]
    fn op_constant_encodes_i32_i64() {
        assert_eq!(Op::constant(Value::I32(-7)), Op::Const(-7));
        assert_eq!(Op::constant(Value::I64(-9)), Op::Const(-9));
    }

    #[test]
    fn op_constant_encodes_f32_f64_bits() {
        // Use raw bit patterns that are stable.
        let f32v = f32::from_bits(0x7FC0_0001); // a NaN payload
        let f64v = f64::from_bits(0x7FF8_0000_0000_0001); // a NaN payload

        let op_f32 = Op::constant(Value::F32(F32::from(f32v.to_bits())));
        assert_eq!(op_f32, Op::Const(bits_f32(f32v)));

        let op_f64 = Op::constant(Value::F64(F64::from(f64v.to_bits())));
        assert_eq!(op_f64, Op::Const(bits_f64(f64v)));
    }

    #[test]
    fn op_constant_encodes_funcref_externref_ids() {
        let fr = FuncRef(123);
        let er = FuncRef(456);
        assert_eq!(Op::constant(Value::FuncRef(fr)), Op::Const(123));
        assert_eq!(Op::constant(Value::ExternRef(er)), Op::Const(456));
    }

    #[test]
    fn global_op_maps_value_kinds_correctly() {
        let nan32 = f32::from_bits(0x7FC0_0001);
        let nan64 = f64::from_bits(0x7FF8_0000_0000_0001);

        let ctx = TestCtx::new().with_globals(alloc::vec![
            Some(Value::I32(-1)),
            Some(Value::I64(-2)),
            Some(Value::F32(F32::from(nan32.to_bits()))),
            Some(Value::F64(F64::from(nan64.to_bits()))),
            Some(Value::FuncRef(FuncRef(7))),
            Some(Value::ExternRef(FuncRef(9))),
            None,
        ]);
        let global = |index| CompiledExpr::from_op(Op::global(index));

        assert_eq!(global(0).eval(&ctx), Some(-1));
        assert_eq!(global(1).eval(&ctx), Some(-2));
        assert_eq!(global(2).eval(&ctx), Some(nan32.to_bits() as u64 as i64));
        assert_eq!(global(3).eval(&ctx), Some(bits_f64(nan64)));
        assert_eq!(global(4).eval(&ctx), Some(7));
        assert_eq!(global(5).eval(&ctx), Some(9));

        // None from context -> None from eval.
        assert_eq!(global(6).eval(&ctx), None);

        // Out of range -> None
        assert_eq!(global(999).eval(&ctx), None);
    }

    #[test]
    fn funcref_op_reads_from_context() {
        let ctx = TestCtx::new().with_funcs(alloc::vec![Some(FuncRef(1)), None, Some(FuncRef(3)),]);
        let funcref = |index| CompiledExpr::new_funcref(index);

        assert_eq!(funcref(0).eval(&ctx), Some(1));
        assert_eq!(funcref(1).eval(&ctx), None);
        assert_eq!(funcref(2).eval(&ctx), Some(3));

        // Out of range -> None
        assert_eq!(funcref(999).eval(&ctx), None);
    }

    #[test]
    fn binary_op_combines_operands_and_propagates_none() {
        // expr: global(0) + const(5)
        let expr = compile(&[0x23, 0x00, 0x41, 0x05, 0x6a, 0x0b]);

        let ctx_some = TestCtx::new().with_globals(alloc::vec![Some(Value::I32(10))]);
        assert_eq!(expr.eval(&ctx_some), Some(15));

        let ctx_none = TestCtx::new().with_globals(alloc::vec![None]);
        assert_eq!(expr.eval(&ctx_none), None);
    }

    #[test]
    fn compiledexpr_zero_is_zero() {
        let e = CompiledExpr::zero();
        assert_eq!(e.eval_const(), Some(0));
    }

    #[test]
    fn compiledexpr_from_const_roundtrips() {
        let e = CompiledExpr::from_const(-123);
        assert_eq!(e.eval_const(), Some(-123));
    }

    #[test]
    fn compiledexpr_funcref_and_global_introspection() {
        let e_fr = CompiledExpr::new_funcref(42);
        assert_eq!(e_fr.funcref(), Some(FuncIdx::from(42u32)));
        assert_eq!(e_fr.global(), None);

        let e_g = CompiledExpr::from_op(Op::global(7));
        assert_eq!(e_g.global(), Some(GlobalIdx::from(7u32)));
        assert_eq!(e_g.funcref(), None);

        // a compound expression is neither, even if it ends in `ref.func`/`global.get`
        let compound = compile(&[0x41, 0x01, 0x23, 0x00, 0x6a, 0x0b]);
        assert_eq!(compound.funcref(), None);
        assert_eq!(compound.global(), None);
    }

    #[test]
    fn eval_with_context_reads_globals_and_funcs() {
        let e_global = CompiledExpr::from_op(Op::global(0));
        let e_func = CompiledExpr::from_op(Op::funcref(1));

        let g = |idx: u32| match idx {
            0 => Some(Value::I64(123)),
            _ => None,
        };
        let f = |idx: u32| match idx {
            1 => Some(FuncRef(77)),
            _ => None,
        };

        assert_eq!(e_global.eval_with_context(g, f), Some(123));
        assert_eq!(e_func.eval_with_context(|_| None, f), Some(77));

        // Missing values -> None
        assert_eq!(e_global.eval_with_context(|_| None, |_| None), None);
        assert_eq!(e_func.eval_with_context(|_| None, |_| None), None);
    }

    #[test]
    fn compiledexpr_new_i32_const() {
        // i32.const 7; end
        let c = compile(&[0x41, 0x07, 0x0b]);
        assert_eq!(c.eval_const(), Some(7));
    }

    #[test]
    fn compiledexpr_new_i64_const() {
        // i64.const -1; end
        // signed LEB128 for -1 is 0x7f
        let c = compile(&[0x42, 0x7f, 0x0b]);
        assert_eq!(c.eval_const(), Some(-1));
    }

    #[test]
    fn compiledexpr_new_f32_f64_const() {
        // f32.const 1.0; end
        let c = compile(&[0x43, 0x00, 0x00, 0x80, 0x3f, 0x0b]);
        assert_eq!(c.eval_const(), Some(bits_f32(1.0)));
        // f64.const 1.0; end
        let c = compile(&[0x44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f, 0x0b]);
        assert_eq!(c.eval_const(), Some(bits_f64(1.0)));
    }

    #[test]
    fn compiledexpr_new_ref_null() {
        // ref.null func; end
        let c = compile(&[0xd0, 0x70, 0x0b]);
        assert_eq!(c.eval_const(), Some(FuncRef::null().0 as i64));
        // ref.null extern; end
        let c = compile(&[0xd0, 0x6f, 0x0b]);
        assert_eq!(c.eval_const(), Some(ExternRef::null().0 as i64));
    }

    #[test]
    fn compiledexpr_new_i32_add_wraps() {
        // i32.const 0x7fffffff; i32.const 1; i32.add; end
        let c = compile(&[
            0x41, 0xff, 0xff, 0xff, 0xff, 0x07, // 2147483647
            0x41, 0x01, // 1
            0x6a, // i32.add
            0x0b, // end
        ]);
        assert_eq!(c.eval_const(), Some(i32::MIN as i64));
    }

    #[test]
    fn compiledexpr_new_i32_sub_wraps() {
        // i32.const i32::MIN; i32.const 1; i32.sub; end
        // i32::MIN = -2147483648 -> signed LEB128: 0x80 0x80 0x80 0x80 0x78
        let c = compile(&[
            0x41, 0x80, 0x80, 0x80, 0x80, 0x78, // -2147483648
            0x41, 0x01, // 1
            0x6b, // i32.sub
            0x0b,
        ]);
        assert_eq!(c.eval_const(), Some(i32::MAX as i64));
    }

    #[test]
    fn compiledexpr_new_i32_mul_wraps() {
        // i32.const 0x40000000; i32.const 4; i32.mul; end
        let c = compile(&[
            0x41, 0x80, 0x80, 0x80, 0x80, 0x04, // 0x40000000
            0x41, 0x04, // 4
            0x6c, // i32.mul
            0x0b,
        ]);
        assert_eq!(c.eval_const(), Some(0));
    }

    #[test]
    fn compiledexpr_new_i64_add_sub_mul_wrap() {
        // i64.const -1; i64.const 2; i64.mul => -2
        let c = compile(&[0x42, 0x7f, 0x42, 0x02, 0x7e, 0x0b]);
        assert_eq!(c.eval_const(), Some(-2));
        // i64.const -1; i64.const 2; i64.add => 1
        let c = compile(&[0x42, 0x7f, 0x42, 0x02, 0x7c, 0x0b]);
        assert_eq!(c.eval_const(), Some(1));
        // i64.const -1; i64.const 2; i64.sub => -3
        let c = compile(&[0x42, 0x7f, 0x42, 0x02, 0x7d, 0x0b]);
        assert_eq!(c.eval_const(), Some(-3));
    }

    #[test]
    fn compiledexpr_new_global_get_uses_context() {
        // global.get 0; end
        let c = compile(&[0x23, 0x00, 0x0b]);

        let ctx = TestCtx::new().with_globals(alloc::vec![Some(Value::I32(99))]);
        assert_eq!(c.eval(&ctx), Some(99));

        let ctx_none = TestCtx::new().with_globals(alloc::vec![None]);
        assert_eq!(c.eval(&ctx_none), None);
    }

    #[test]
    fn compiledexpr_new_ref_func_uses_context() {
        // ref.func 2; end
        let c = compile(&[0xd2, 0x02, 0x0b]);
        assert_eq!(c.funcref(), Some(2));

        let ctx = TestCtx::new().with_funcs(alloc::vec![None, None, Some(FuncRef(555))]);
        assert_eq!(c.eval(&ctx), Some(555));
    }

    #[test]
    fn compiledexpr_new_i32_add_mixed_global_and_funcref() {
        // global.get 0; ref.func 1; i32.add; end
        //
        // This isn't a valid *typed* Wasm const expr in a real module (i32.add expects i32),
        // but the translator assumes validation already ran. This test ensures the
        // translation/eval plumbing works for mixed operands (it will treat funcref as i64).
        let c = compile(&[0x23, 0x00, 0xd2, 0x01, 0x6a, 0x0b]);

        let ctx = TestCtx::new()
            .with_globals(alloc::vec![Some(Value::I32(10))])
            .with_funcs(alloc::vec![None, Some(FuncRef(7))]);

        assert_eq!(c.eval(&ctx), Some(17));
    }

    #[test]
    fn compiledexpr_eval_const_returns_none_for_global_or_funcref() {
        let e_g = CompiledExpr::from_op(Op::global(0));
        let e_f = CompiledExpr::from_op(Op::funcref(0));

        assert_eq!(e_g.eval_const(), None);
        assert_eq!(e_f.eval_const(), None);
    }

    #[test]
    fn compiledexpr_new_rejects_unbalanced_operators() {
        // two values left on the stack: i32.const 1; i32.const 2; end
        let err = CompiledExpr::new(parse_const_expr(&[0x41, 0x01, 0x41, 0x02, 0x0b])).unwrap_err();
        assert!(matches!(err, CompilationError::ConstEvaluationFailed));
        // a binary operator without operands: i32.add; end
        let err = CompiledExpr::new(parse_const_expr(&[0x6a, 0x0b])).unwrap_err();
        assert!(matches!(err, CompilationError::ConstEvaluationFailed));
        // a binary operator with one operand: i32.const 1; i32.add; end
        let err = CompiledExpr::new(parse_const_expr(&[0x41, 0x01, 0x6a, 0x0b])).unwrap_err();
        assert!(matches!(err, CompilationError::ConstEvaluationFailed));
        // no value at all: end
        let err = CompiledExpr::new(parse_const_expr(&[0x0b])).unwrap_err();
        assert!(matches!(err, CompilationError::ConstEvaluationFailed));
    }

    #[test]
    fn compiledexpr_new_rejects_operators_outside_the_constant_subset() {
        // i32.const 1; i32.eqz; end
        let err = CompiledExpr::new(parse_const_expr(&[0x41, 0x01, 0x45, 0x0b])).unwrap_err();
        assert!(matches!(err, CompilationError::NotSupportedOpcode));
        // ref.null with a non-reference heap type
        let err = CompiledExpr::new(parse_const_expr(&[0xd0, 0x7f, 0x0b])).unwrap_err();
        assert!(matches!(
            err,
            CompilationError::NotSupportedOpcode | CompilationError::MalformedWasmBinary(_)
        ));
        // a truncated expression
        let err = CompiledExpr::new(parse_const_expr(&[0x41, 0x01])).unwrap_err();
        assert!(matches!(err, CompilationError::MalformedWasmBinary(_)));
    }

    /// The regression for the nested-closure representation: a validated `extended-const`
    /// expression may chain hundreds of thousands of operators, and translating, evaluating and
    /// dropping it must not recurse.
    #[test]
    fn long_operator_chain_does_not_recurse() {
        const OPERATORS: usize = 300_000;
        // i32.const 0; (i32.const 1; i32.add) x OPERATORS; end
        let mut bytes = alloc::vec![0x41, 0x00];
        for _ in 0..OPERATORS {
            bytes.extend_from_slice(&[0x41, 0x01, 0x6a]);
        }
        bytes.push(0x0b);
        let expr = compile(&bytes);
        assert_eq!(expr.eval_const(), Some(OPERATORS as i64));
        assert_eq!(expr.funcref(), None);
        drop(expr);

        // a chain that builds up a deep operand stack before folding it
        let mut bytes = alloc::vec![];
        for _ in 0..=OPERATORS {
            bytes.extend_from_slice(&[0x42, 0x01]); // i64.const 1
        }
        bytes.extend(core::iter::repeat_n(0x7c, OPERATORS)); // i64.add
        bytes.push(0x0b);
        let expr = compile(&bytes);
        assert_eq!(expr.eval_const(), Some(OPERATORS as i64 + 1));
    }
}
