use crate::{
    compiler::translator::{FuncTranslatorAllocations, InstructionTranslator, ReusableAllocations},
    CompilationError, FuncIdx,
};
use rwasm_fuel_policy::FuelCosts;
use wasmparser::{
    BinaryReaderError, FuncValidator, FunctionBody, ValType, ValidatorResources, VisitOperator,
};

pub struct FuncBuilder<'a> {
    pub(crate) func_body: FunctionBody<'a>,
    pub(crate) validator: FuncValidator<ValidatorResources>,
    pub(crate) func_idx: FuncIdx,
    pub(crate) translator: InstructionTranslator,
    pub(crate) pos: usize,
}

impl<'a> FuncBuilder<'a> {
    pub fn new(
        func_body: FunctionBody<'a>,
        validator: FuncValidator<ValidatorResources>,
        func_idx: FuncIdx,
        allocations: FuncTranslatorAllocations,
        with_consume_fuel: bool,
        with_code_snippets: bool,
        consume_fuel_for_params_and_locals: bool,
        max_allowed_memory_pages: u32,
    ) -> Self {
        Self {
            func_body,
            validator,
            func_idx,
            translator: InstructionTranslator::new(
                allocations,
                with_consume_fuel,
                with_code_snippets,
                consume_fuel_for_params_and_locals,
                max_allowed_memory_pages,
            ),
            pos: 0,
        }
    }

    pub fn translate(mut self) -> Result<ReusableAllocations, CompilationError> {
        self.translator.prepare(self.func_idx)?;
        self.translator.bump_fuel_consumption(|| FuelCosts::BASE)?;
        // emit special opcodes before the beginning of the function
        self.translate_stack_alloc();
        self.translate_locals()?;
        let offset = self.translate_operators()?;
        self.validator.finish(offset)?;
        self.translator.finish()?;
        Ok(ReusableAllocations {
            translation: self.translator.alloc,
            validation: self.validator.into_allocations(),
        })
    }

    fn translate_locals(&mut self) -> Result<(), CompilationError> {
        if self.translator.consume_fuel_for_params_and_locals {
            let mut locals_reader = self.func_body.get_locals_reader()?;
            let mut total_body_locals: u32 = 0;
            for _ in 0..locals_reader.get_count() {
                let (amount, _) = locals_reader.read()?;
                total_body_locals = total_body_locals.saturating_add(amount);
            }
            let cost = FuelCosts::fuel_for_locals(total_body_locals);
            if cost > 0 {
                self.translator.bump_fuel_consumption(|| cost)?;
            }
        }
        // translate locals
        let mut locals_reader = self.func_body.get_locals_reader()?;
        let locals_count = locals_reader.get_count() as usize;
        for _ in 0..locals_count {
            let offset = locals_reader.original_position();
            let (amount, value_type) = locals_reader.read()?;

            self.validator.define_locals(offset, amount, value_type)?;
            match value_type {
                ValType::I32 | ValType::I64 => {}
                // TODO(dmitry123): "make sure this type is not allowed with floats disabled"
                ValType::F32 | ValType::F64 => {}
                ValType::V128 => return Err(CompilationError::NotSupportedLocalType),
                ValType::FuncRef | ValType::ExternRef => {}
                #[allow(unreachable_patterns)]
                _ => return Err(CompilationError::NotSupportedLocalType),
            }

            let mut total_locals_required = 0u32;
            for _ in 0..amount as usize {
                // for i64 type, we need to push 2 values on the stack
                if value_type == ValType::I64 || value_type == ValType::F64 {
                    total_locals_required += 2;
                } else {
                    total_locals_required += 1;
                }
                self.translator.alloc.stack_types.push(value_type);
            }
            self.translator
                .alloc
                .instruction_set
                .op_bulk_const(total_locals_required);

            self.translator.stack_height.push_n(amount);
            if value_type == ValType::I64 || value_type == ValType::F64 {
                self.translator.stack_height.push_n(amount);
            }
        }

        Ok(())
    }

    fn translate_stack_alloc(&mut self) {
        // we use `u32::MAX` here because we replace it with
        // the final calculated value later
        self.translator
            .alloc
            .instruction_set
            .op_stack_check(u32::MAX);
    }

    /// Translates the Wasm operators of the Wasm function.
    ///
    /// Returns the offset of the `End` Wasm operator.
    fn translate_operators(&mut self) -> Result<usize, CompilationError> {
        let mut reader = self.func_body.get_operators_reader()?;
        while !reader.eof() {
            // #[cfg(feature = "debug-print")]
            // {
            //     let operator = reader.clone().read()?;
            //     println!("{:?}", operator);
            // }
            self.pos = reader.original_position();
            reader.visit_operator(self)??;
        }
        reader.ensure_end()?;
        Ok(reader.original_position())
    }

    /// Translates into `rwasm` bytecode if the current code path is reachable.
    fn validate_then_translate<V, T>(
        &mut self,
        validate: V,
        translate: T,
    ) -> Result<(), CompilationError>
    where
        V: FnOnce(&mut FuncValidator<ValidatorResources>) -> Result<(), BinaryReaderError>,
        T: FnOnce(&mut InstructionTranslator) -> Result<(), CompilationError>,
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
            let offset = self.pos;
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
            let offset = self.pos;
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
            let offset = self.pos;
            self.validator.visitor(offset).$visit($($($arg),*)?).map_err(::core::convert::Into::into)
        }
        impl_visit_operator!($($rest)*);
    };
    () => {};
}

impl<'a> VisitOperator<'a> for FuncBuilder<'a> {
    type Output = Result<(), CompilationError>;

    wasmparser::for_each_operator!(impl_visit_operator);
}
