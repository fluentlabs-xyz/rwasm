#![allow(dead_code)]

//! Data structures to represents Wasm constant expressions.
//!
//! This has built-in support for the `extended-const` Wasm proposal.
//! The design of the execution mechanic was inspired by the [`s1vm`]
//! virtual machine architecture.
//!
//! [`s1vm`]: https://github.com/Neopallium/s1vm

use crate::{ExternRef, FuncIdx, FuncRef, GlobalIdx, Value, F32, F64};
use alloc::boxed::Box;
use smallvec::SmallVec;
use wasmparser::ConstExpr;

/// Types that allow evluation given an evaluation context.
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

/// An input parameter to a [`CompiledExpr`] operator.
#[derive(Debug)]
pub enum Op {
    /// A constant value.
    Const(ConstOp),
    /// The value of a global variable.
    Global(GlobalOp),
    /// A Wasm `ref.func index` value.
    FuncRef(FuncRefOp),
    /// An arbitrary expression.
    Expr(ExprOp),
}

impl Clone for Op {
    fn clone(&self) -> Self {
        match self {
            Op::Const(op) => Op::Const(op.clone()),
            Op::Global(op) => Op::Global(*op),
            Op::FuncRef(op) => Op::FuncRef(*op),
            Op::Expr(_) => unreachable!("cloning of expr is not possible"),
        }
    }
}

/// A constant value operator.
///
/// This may represent the following Wasm operators:
///
/// - `i32.const`
/// - `i64.const`
/// - `f32.const`
/// - `f64.const`
/// - `ref.null`
#[derive(Debug, Clone)]
pub struct ConstOp {
    /// The underlying precomputed untyped value.
    value: i64,
}

impl Eval for ConstOp {
    fn eval(&self, _ctx: &dyn EvalContext) -> Option<i64> {
        Some(self.value)
    }
}

/// Represents a Wasm `global.get` operator.

#[derive(Debug, Copy, Clone)]
pub struct GlobalOp {
    /// The index of the global variable.
    global_index: u32,
}

impl Eval for GlobalOp {
    fn eval(&self, ctx: &dyn EvalContext) -> Option<i64> {
        ctx.get_global(self.global_index).map(|v| match v {
            Value::I32(value) => value as i64,
            Value::I64(value) => value,
            Value::F32(value) => value.to_bits() as u64 as i64,
            Value::F64(value) => value.to_bits() as i64,
            Value::FuncRef(value) => value.0 as i32 as i64,
            Value::ExternRef(value) => value.0 as i32 as i64,
        })
    }
}

/// Represents a Wasm `func.ref` operator.

#[derive(Debug, Copy, Clone)]
pub struct FuncRefOp {
    /// The index of the function.
    function_index: u32,
}

impl Eval for FuncRefOp {
    fn eval(&self, ctx: &dyn EvalContext) -> Option<i64> {
        ctx.get_func(self.function_index).map(|v| v.0 as i64)
    }
}

/// A generic Wasm expression operator.
///
/// This may represent one of the following Wasm operators:
///
/// - `i32.add`
/// - `i32.sub`
/// - `i32.mul`
/// - `i64.add`
/// - `i64.sub`
/// - `i64.mul`
#[allow(clippy::type_complexity)]
pub struct ExprOp {
    /// The underlying closure that implements the expression.
    expr: Box<dyn Fn(&dyn EvalContext) -> Option<i64> + Send>,
}

impl core::fmt::Debug for ExprOp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ExprOp").finish()
    }
}

impl Eval for ExprOp {
    fn eval(&self, ctx: &dyn EvalContext) -> Option<i64> {
        (self.expr)(ctx)
    }
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
        Self::Const(ConstOp { value })
    }

    /// Creates a new global operator with the given index.
    pub fn global(global_index: u32) -> Self {
        Self::Global(GlobalOp { global_index })
    }

    /// Creates a new global operator with the given index.
    pub fn funcref(function_index: u32) -> Self {
        Self::FuncRef(FuncRefOp { function_index })
    }

    /// Creates a new expression operator for the given `expr`.
    pub fn expr<T>(expr: T) -> Self
    where
        T: Fn(&dyn EvalContext) -> Option<i64> + Send + 'static,
    {
        Self::Expr(ExprOp {
            expr: Box::new(expr),
        })
    }
}

impl Eval for Op {
    fn eval(&self, ctx: &dyn EvalContext) -> Option<i64> {
        match self {
            Op::Const(op) => op.eval(ctx),
            Op::Global(op) => op.eval(ctx),
            Op::FuncRef(op) => op.eval(ctx),
            Op::Expr(op) => op.eval(ctx),
        }
    }
}

/// A Wasm constant expression.
///
/// These are used to determine the offsets of memory data
/// and table element segments as well as the initial value
/// of global variables.
#[derive(Debug, Clone)]
pub struct CompiledExpr {
    /// The root operator of the [`CompiledExpr`].
    pub(crate) op: Op,
}

impl Eval for CompiledExpr {
    fn eval(&self, ctx: &dyn EvalContext) -> Option<i64> {
        self.op.eval(ctx)
    }
}

macro_rules! def_expr {
    ($lhs:ident, $rhs:ident, $expr:expr) => {{
        Op::expr(move |ctx: &dyn EvalContext| -> Option<i64> {
            let lhs = $lhs.eval(ctx)?;
            let rhs = $rhs.eval(ctx)?;
            Some($expr(lhs, rhs))
        })
    }};
}

impl CompiledExpr {
    pub fn zero() -> Self {
        Self {
            op: Op::Const(ConstOp {
                value: Default::default(),
            }),
        }
    }

    pub fn from_const(value: i64) -> Self {
        Self {
            op: Op::Const(ConstOp { value }),
        }
    }

    /// Creates a new [`CompiledExpr`] from the given Wasm [`CompiledExpr`].
    ///
    /// # Note
    ///
    /// The constructor assumes that Wasm validation already succeeded
    /// on the input Wasm [`CompiledExpr`].
    pub fn new(expr: ConstExpr<'_>) -> Self {
        /// A buffer required for translation of Wasm const expressions.
        type TranslationBuffer = SmallVec<[Op; 3]>;
        /// Convenience function to create the various expression operators.
        fn expr_op(stack: &mut TranslationBuffer, expr: fn(i64, i64) -> i64) {
            let rhs = stack
                .pop()
                .expect("must have rhs operator on the stack due to Wasm validation");
            let lhs = stack
                .pop()
                .expect("must have lhs operator on the stack due to Wasm validation");
            let op = match (lhs, rhs) {
                (Op::Const(lhs), Op::Const(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::Const(lhs), Op::Global(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::Const(lhs), Op::FuncRef(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::Const(lhs), Op::Expr(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::Global(lhs), Op::Const(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::Global(lhs), Op::Global(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::Global(lhs), Op::FuncRef(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::Global(lhs), Op::Expr(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::FuncRef(lhs), Op::Const(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::FuncRef(lhs), Op::Global(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::FuncRef(lhs), Op::FuncRef(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::FuncRef(lhs), Op::Expr(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::Expr(lhs), Op::Const(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::Expr(lhs), Op::Global(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::Expr(lhs), Op::FuncRef(rhs)) => def_expr!(lhs, rhs, expr),
                (Op::Expr(lhs), Op::Expr(rhs)) => def_expr!(lhs, rhs, expr),
            };
            stack.push(op);
        }

        let mut reader = expr.get_operators_reader();
        // TODO: we might want to avoid heap allocation in the simple cases that
        //       only have one operator via the small vector data structure.
        let mut stack = TranslationBuffer::new();
        loop {
            let op = reader.read().unwrap_or_else(|error| {
                panic!("unexpectedly encountered invalid const expression operator: {error}")
            });
            match op {
                wasmparser::Operator::I32Const { value } => {
                    stack.push(Op::constant(value));
                }
                wasmparser::Operator::I64Const { value } => {
                    stack.push(Op::constant(value));
                }
                wasmparser::Operator::F32Const { value } => {
                    stack.push(Op::constant(F32::from(value.bits())));
                }
                wasmparser::Operator::F64Const { value } => {
                    stack.push(Op::constant(F64::from(value.bits())));
                }
                wasmparser::Operator::GlobalGet { global_index } => {
                    stack.push(Op::global(global_index));
                }
                wasmparser::Operator::RefNull { ty } => {
                    let value = match ty {
                        wasmparser::ValType::FuncRef => Value::from(FuncRef::null()),
                        wasmparser::ValType::ExternRef => Value::from(ExternRef::null()),
                        ty => panic!("encountered an invalid value type for RefNull: {ty:?}"),
                    };
                    stack.push(Op::constant(value));
                }
                wasmparser::Operator::RefFunc { function_index } => {
                    stack.push(Op::funcref(function_index));
                }
                wasmparser::Operator::I32Add => expr_op(&mut stack, |lhs, rhs| {
                    i32::wrapping_add(lhs as i32, rhs as i32) as i64
                }),
                wasmparser::Operator::I32Sub => expr_op(&mut stack, |lhs, rhs| {
                    i32::wrapping_sub(lhs as i32, rhs as i32) as i64
                }),
                wasmparser::Operator::I32Mul => expr_op(&mut stack, |lhs, rhs| {
                    i32::wrapping_mul(lhs as i32, rhs as i32) as i64
                }),
                wasmparser::Operator::I64Add => {
                    expr_op(&mut stack, |lhs, rhs| lhs.wrapping_add(rhs))
                }
                wasmparser::Operator::I64Sub => {
                    expr_op(&mut stack, |lhs, rhs| lhs.wrapping_sub(rhs))
                }
                wasmparser::Operator::I64Mul => {
                    expr_op(&mut stack, |lhs, rhs| lhs.wrapping_mul(rhs))
                }
                wasmparser::Operator::End => break,
                op => panic!("encountered invalid Wasm const expression operator: {op:?}"),
            };
        }
        reader
            .ensure_end()
            .expect("due to Wasm validation, this is guaranteed to succeed");
        let op = stack
            .pop()
            .expect("due to Wasm validation must have one operator on the stack");
        assert!(
            stack.is_empty(),
            "due to Wasm validation operator stack must be empty now"
        );
        Self { op }
    }

    /// Create a new `ref.func x` [`CompiledExpr`].
    ///
    /// # Note
    ///
    /// Required for setting up table elements.
    pub fn new_funcref(function_index: u32) -> Self {
        Self {
            op: Op::FuncRef(FuncRefOp { function_index }),
        }
    }

    /// Returns `Some(index)` if the [`CompiledExpr`] is a `funcref(index)`.
    ///
    /// Otherwise returns `None`.
    pub fn funcref(&self) -> Option<FuncIdx> {
        if let Op::FuncRef(op) = &self.op {
            return Some(FuncIdx::from(op.function_index));
        }
        None
    }

    pub fn global(&self) -> Option<GlobalIdx> {
        if let Op::Global(op) = &self.op {
            return Some(GlobalIdx::from(op.global_index));
        }
        None
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

    #[test]
    fn empty_eval_context_always_none() {
        let ctx = EmptyEvalContext;
        assert!(ctx.get_global(0).is_none());
        assert!(ctx.get_global(123).is_none());
        assert!(ctx.get_func(0).is_none());
        assert!(ctx.get_func(456).is_none());
    }

    #[test]
    fn constop_eval_returns_value() {
        let ctx = EmptyEvalContext;
        let op = ConstOp { value: 42 };
        assert_eq!(op.eval(&ctx), Some(42));
    }

    #[test]
    fn op_constant_encodes_i32_i64() {
        let ctx = EmptyEvalContext;

        let op_i32 = Op::constant(Value::I32(-7));
        assert_eq!(op_i32.eval(&ctx), Some(-7));

        let op_i64 = Op::constant(Value::I64(-9));
        assert_eq!(op_i64.eval(&ctx), Some(-9));
    }

    #[test]
    fn op_constant_encodes_f32_f64_bits() {
        let ctx = EmptyEvalContext;

        // Use raw bit patterns that are stable.
        let f32v = f32::from_bits(0x7FC0_0001); // a NaN payload
        let f64v = f64::from_bits(0x7FF8_0000_0000_0001); // a NaN payload

        let op_f32 = Op::constant(Value::F32(F32::from(f32v.to_bits())));
        assert_eq!(op_f32.eval(&ctx), Some(bits_f32(f32v)));

        let op_f64 = Op::constant(Value::F64(F64::from(f64v.to_bits())));
        assert_eq!(op_f64.eval(&ctx), Some(bits_f64(f64v)));
    }

    #[test]
    fn op_constant_encodes_funcref_externref_ids() {
        let ctx = EmptyEvalContext;

        // This assumes your FuncRef/ExternRef are tuple structs with .0 as shown in your code.
        let fr = FuncRef(123);
        let er = FuncRef(456);

        let op_fr = Op::constant(Value::FuncRef(fr));
        assert_eq!(op_fr.eval(&ctx), Some(123));

        let op_er = Op::constant(Value::ExternRef(er));
        assert_eq!(op_er.eval(&ctx), Some(456));
    }

    #[test]
    fn globalop_maps_value_kinds_correctly() {
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

        assert_eq!(GlobalOp { global_index: 0 }.eval(&ctx), Some(-1));
        assert_eq!(GlobalOp { global_index: 1 }.eval(&ctx), Some(-2));
        assert_eq!(
            GlobalOp { global_index: 2 }.eval(&ctx),
            Some(bits_f32(nan32))
        );
        assert_eq!(
            GlobalOp { global_index: 3 }.eval(&ctx),
            Some(bits_f64(nan64))
        );
        assert_eq!(GlobalOp { global_index: 4 }.eval(&ctx), Some(7));
        assert_eq!(GlobalOp { global_index: 5 }.eval(&ctx), Some(9));

        // None from context -> None from eval.
        assert_eq!(GlobalOp { global_index: 6 }.eval(&ctx), None);

        // Out of range -> None
        assert_eq!(GlobalOp { global_index: 999 }.eval(&ctx), None);
    }

    #[test]
    fn funcrefop_reads_from_context() {
        let ctx = TestCtx::new().with_funcs(alloc::vec![Some(FuncRef(1)), None, Some(FuncRef(3)),]);

        assert_eq!(FuncRefOp { function_index: 0 }.eval(&ctx), Some(1));
        assert_eq!(FuncRefOp { function_index: 1 }.eval(&ctx), None);
        assert_eq!(FuncRefOp { function_index: 2 }.eval(&ctx), Some(3));

        // Out of range -> None
        assert_eq!(
            FuncRefOp {
                function_index: 999
            }
            .eval(&ctx),
            None
        );
    }

    #[test]
    fn expr_op_combines_operands_and_propagates_none() {
        // expr: global(0) + const(5)
        let expr = Op::expr(|ctx: &dyn EvalContext| {
            let a = Op::global(0).eval(ctx)?;
            let b = Op::constant(Value::I32(5)).eval(ctx)?;
            Some(i32::wrapping_add(a as i32, b as i32) as i64)
        });

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

        let e_g = CompiledExpr { op: Op::global(7) };
        assert_eq!(e_g.global(), Some(GlobalIdx::from(7u32)));
        assert_eq!(e_g.funcref(), None);
    }

    #[test]
    fn eval_with_context_reads_globals_and_funcs() {
        let e_global = CompiledExpr { op: Op::global(0) };
        let e_func = CompiledExpr { op: Op::funcref(1) };

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
    fn op_clone_works_for_non_expr_variants() {
        let c = Op::constant(Value::I64(1));
        let g = Op::global(2);
        let f = Op::funcref(3);

        // Should not panic:
        let _ = c.clone();
        let _ = g.clone();
        let _ = f.clone();
    }

    #[test]
    #[should_panic(expected = "cloning of expr is not possible")]
    fn op_clone_panics_for_expr_variant() {
        let e = Op::expr(|_| Some(0));
        let _ = e.clone();
    }

    fn parse_const_expr(bytes: &[u8]) -> ConstExpr<'_> {
        ConstExpr::new(bytes, 0)
    }

    #[test]
    fn compiledexpr_new_i32_const() {
        // i32.const 7; end
        let expr = parse_const_expr(&[0x41, 0x07, 0x0b]);
        let c = CompiledExpr::new(expr);
        assert_eq!(c.eval_const(), Some(7));
    }

    #[test]
    fn compiledexpr_new_i64_const() {
        // i64.const -1; end
        // signed LEB128 for -1 is 0x7f
        let expr = parse_const_expr(&[0x42, 0x7f, 0x0b]);
        let c = CompiledExpr::new(expr);
        assert_eq!(c.eval_const(), Some(-1));
    }

    #[test]
    fn compiledexpr_new_i32_add_wraps() {
        // i32.const 0x7fffffff; i32.const 1; i32.add; end
        let expr = parse_const_expr(&[
            0x41, 0xff, 0xff, 0xff, 0xff, 0x07, // 2147483647
            0x41, 0x01, // 1
            0x6a, // i32.add
            0x0b, // end
        ]);
        let c = CompiledExpr::new(expr);
        assert_eq!(c.eval_const(), Some(i32::MIN as i64));
    }

    #[test]
    fn compiledexpr_new_i32_sub_wraps() {
        // i32.const i32::MIN; i32.const 1; i32.sub; end
        // i32::MIN = -2147483648 -> signed LEB128: 0x80 0x80 0x80 0x80 0x78
        let expr = parse_const_expr(&[
            0x41, 0x80, 0x80, 0x80, 0x80, 0x78, // -2147483648
            0x41, 0x01, // 1
            0x6b, // i32.sub
            0x0b,
        ]);
        let c = CompiledExpr::new(expr);
        assert_eq!(c.eval_const(), Some(i32::MAX as i64));
    }

    #[test]
    fn compiledexpr_new_i64_mul_wraps() {
        // i64.const i64::MAX; i64.const 2; i64.mul; end
        // i64::MAX LEB128 is a bit long; easiest is:
        // i64.const -1; i64.const 2; i64.mul => -2
        let expr = parse_const_expr(&[
            0x42, 0x7f, // i64.const -1
            0x42, 0x02, // i64.const 2
            0x7e, // i64.mul
            0x0b,
        ]);
        let c = CompiledExpr::new(expr);
        assert_eq!(c.eval_const(), Some(-2));
    }

    #[test]
    fn compiledexpr_new_global_get_uses_context() {
        // global.get 0; end
        let expr = parse_const_expr(&[0x23, 0x00, 0x0b]);
        let c = CompiledExpr::new(expr);

        let ctx = TestCtx::new().with_globals(alloc::vec![Some(Value::I32(99))]);
        assert_eq!(c.eval(&ctx), Some(99));

        let ctx_none = TestCtx::new().with_globals(alloc::vec![None]);
        assert_eq!(c.eval(&ctx_none), None);
    }

    #[test]
    fn compiledexpr_new_ref_func_uses_context() {
        // ref.func 2; end
        let expr = parse_const_expr(&[0xd2, 0x02, 0x0b]);
        let c = CompiledExpr::new(expr);

        let ctx = TestCtx::new().with_funcs(alloc::vec![None, None, Some(FuncRef(555))]);
        assert_eq!(c.eval(&ctx), Some(555));
    }

    #[test]
    fn compiledexpr_new_i32_add_mixed_const_and_global() {
        // global.get 0; i32.const 5; i32.add; end
        let expr = parse_const_expr(&[0x23, 0x00, 0x41, 0x05, 0x6a, 0x0b]);
        let c = CompiledExpr::new(expr);

        let ctx = TestCtx::new().with_globals(alloc::vec![Some(Value::I32(10))]);
        assert_eq!(c.eval(&ctx), Some(15));
    }

    #[test]
    fn compiledexpr_new_i32_add_mixed_global_and_funcref() {
        // global.get 0; ref.func 1; i32.add; end
        //
        // This isn't a valid *typed* Wasm const expr in a real module (i32.add expects i32),
        // but your translator assumes validation already ran. This test ensures the
        // translation/eval plumbing works for mixed operands (it will treat funcref as i64).
        let expr = parse_const_expr(&[0x23, 0x00, 0xd2, 0x01, 0x6a, 0x0b]);
        let c = CompiledExpr::new(expr);

        let ctx = TestCtx::new()
            .with_globals(alloc::vec![Some(Value::I32(10))])
            .with_funcs(alloc::vec![None, Some(FuncRef(7))]);

        assert_eq!(c.eval(&ctx), Some(17));
    }

    #[test]
    fn compiledexpr_eval_const_returns_none_for_global_or_funcref() {
        let e_g = CompiledExpr { op: Op::global(0) };
        let e_f = CompiledExpr { op: Op::funcref(0) };

        assert_eq!(e_g.eval_const(), None);
        assert_eq!(e_f.eval_const(), None);
    }
}
