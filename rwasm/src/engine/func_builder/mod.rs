mod control_frame;
mod control_stack;
mod error;
mod inst_builder;
mod labels;
mod locals_registry;
mod translator;
mod value_stack;

use self::{control_frame::ControlFrame, control_stack::ControlFlowStack};
pub use self::{
    error::{TranslationError, TranslationErrorInner},
    inst_builder::{Instr, InstructionsBuilder, RelativeDepth},
    translator::{FuncTranslator, FuncTranslatorAllocations},
};
use super::CompiledFunc;
use crate::{
    arena::ArenaIndex,
    engine::bytecode::Instruction,
    module::{FuncIdx, ModuleResources, ReusableAllocations},
};
use wasmparser::{BinaryReaderError, ValType, VisitOperator};

/// The used function validator type.
type FuncValidator = wasmparser::FuncValidator<wasmparser::ValidatorResources>;

/// The interface to build a `wasmi` bytecode function using Wasm bytecode.
///
/// # Note
///
/// This includes validation of the incoming Wasm bytecode.
pub struct FuncBuilder<'parser> {
    /// The current position in the Wasm binary while parsing operators.
    pos: usize,
    /// The Wasm function validator.
    validator: FuncValidator,
    /// The underlying Wasm to `wasmi` bytecode translator.
    translator: FuncTranslator<'parser>,
    /// If we're in rWASM mode
    pub(crate) is_rwasm: bool,
}

impl<'parser> FuncBuilder<'parser> {
    /// Creates a new [`FuncBuilder`].
    pub fn new(
        func: FuncIdx,
        compiled_func: CompiledFunc,
        res: ModuleResources<'parser>,
        validator: FuncValidator,
        allocations: FuncTranslatorAllocations,
    ) -> Self {
        let is_rwasm = res.res.engine().config().get_rwasm_config().is_some();
        Self {
            pos: 0,
            validator,
            translator: FuncTranslator::new(func, compiled_func, res, allocations),
            is_rwasm,
        }
    }

    pub fn translate_signature_check(&mut self) {
        let func_type = &self.translator.res.res.funcs[self.translator.func.into_u32() as usize];
        let func_type = self
            .translator
            .res
            .res
            .engine()
            .resolve_func_signature(&func_type);
        self.translator
            .alloc
            .inst_builder
            .push_inst(Instruction::SignatureCheck(
                (func_type.into_usize() as u32).into(),
            ));
    }

    /// Translates the given local variables for the translated function.
    pub fn translate_locals(
        &mut self,
        offset: usize,
        amount: u32,
        value_type: wasmparser::ValType,
    ) -> Result<(), TranslationError> {
        self.validator.define_locals(offset, amount, value_type)?;
        // for rWASM we fill locals with zero values
        if self.is_rwasm {
            let instr = match value_type {
                ValType::I32 => Instruction::I32Const(0i32.into()),
                ValType::I64 => Instruction::I64Const(0i64.into()),
                ValType::F32 => Instruction::F32Const(0f32.into()),
                ValType::F64 => Instruction::F64Const(0f64.into()),
                ValType::FuncRef => Instruction::RefFunc(0u32.into()),
                _ => unreachable!("not supported local type ({:?})", value_type),
            };
            (0..amount as usize).for_each(|_| {
                self.translator.alloc.inst_builder.push_inst(instr);
            });
            self.translator.stack_height.push_n(amount);
        } else {
            self.translator.register_locals(amount);
        }
        Ok(())
    }

    /// This informs the [`FuncBuilder`] that the function header translation is finished.
    ///
    /// # Note
    ///
    /// This was introduced to properly calculate the fuel costs for all local variables
    /// and function parameters. After this function call no more locals and parameters may
    /// be added to this function translation.
    pub fn finish_translate_locals(&mut self) -> Result<(), TranslationError> {
        self.translator.finish_translate_locals()
    }

    /// Updates the current position within the Wasm binary while parsing operators.
    pub fn update_pos_with_opcode(&mut self, pos: usize, opcode: u16) {
        self.pos = pos;
        self.translator.register_opcode_metadata(pos, opcode);
    }

    /// Updates the current position within the Wasm binary while parsing operators.
    pub fn update_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    /// Returns the current position within the Wasm binary while parsing operators.
    pub fn current_pos(&self) -> usize {
        self.pos
    }

    /// Finishes constructing the function by initializing its [`CompiledFunc`].
    pub fn finish(mut self, offset: usize) -> Result<ReusableAllocations, TranslationError> {
        self.validator.finish(offset)?;
        self.translator.finish()?;
        let allocations = ReusableAllocations {
            translation: self.translator.into_allocations(),
            validation: self.validator.into_allocations(),
        };
        Ok(allocations)
    }

    /// Translates into `wasmi` bytecode if the current code path is reachable.
    fn validate_then_translate<V, T>(
        &mut self,
        validate: V,
        translate: T,
    ) -> Result<(), TranslationError>
    where
        V: FnOnce(&mut FuncValidator) -> Result<(), BinaryReaderError>,
        T: FnOnce(&mut FuncTranslator<'parser>) -> Result<(), TranslationError>,
    {
        validate(&mut self.validator)?;
        translate(&mut self.translator)?;
        Ok(())
    }
}

macro_rules! impl_visit_operator {
    ( @mvp BrTable { $arg:ident: $argty:ty } => $visit:ident $($rest:tt)* ) => {
        // We need to special case the `BrTable` operand since its
        // arguments (a.k.a. `BrTable<'a>`) are not `Copy` which all
        // the other impls make use of.
        fn $visit(&mut self, $arg: $argty) -> Self::Output {
            let offset = self.current_pos();
            let arg_cloned = $arg.clone();
            self.validate_then_translate(
                |validator| validator.visitor(offset).$visit(arg_cloned),
                |translator| translator.$visit($arg),
            )
        }
        impl_visit_operator!($($rest)*);
    };
    ( @mvp $($rest:tt)* ) => {
        impl_visit_operator!(@@supported $($rest)*);
    };
    ( @sign_extension $($rest:tt)* ) => {
        impl_visit_operator!(@@supported $($rest)*);
    };
    ( @saturating_float_to_int $($rest:tt)* ) => {
        impl_visit_operator!(@@supported $($rest)*);
    };
    ( @bulk_memory $($rest:tt)* ) => {
        impl_visit_operator!(@@supported $($rest)*);
    };
    ( @reference_types $($rest:tt)* ) => {
        impl_visit_operator!(@@supported $($rest)*);
    };
    ( @tail_call $($rest:tt)* ) => {
        impl_visit_operator!(@@supported $($rest)*);
    };
    ( @@supported $op:ident $({ $($arg:ident: $argty:ty),* })? => $visit:ident $($rest:tt)* ) => {
        fn $visit(&mut self $($(,$arg: $argty)*)?) -> Self::Output {
            let offset = self.current_pos();
            self.validate_then_translate(
                |v| v.visitor(offset).$visit($($($arg),*)?),
                |t| t.$visit($($($arg),*)?),
            )
        }
        impl_visit_operator!($($rest)*);
    };
    ( @$proposal:ident $op:ident $({ $($arg:ident: $argty:ty),* })? => $visit:ident $($rest:tt)* ) => {
        // Wildcard match arm for all the other (yet) unsupported Wasm proposals.
        fn $visit(&mut self $($(, $arg: $argty)*)?) -> Self::Output {
            let offset = self.current_pos();
            self.validator.visitor(offset).$visit($($($arg),*)?).map_err(::core::convert::Into::into)
        }
        impl_visit_operator!($($rest)*);
    };
    () => {};
}

impl<'a> VisitOperator<'a> for FuncBuilder<'a> {
    type Output = Result<(), TranslationError>;

    wasmparser::for_each_operator!(impl_visit_operator);
}
