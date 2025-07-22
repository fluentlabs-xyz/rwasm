use crate::{
    compiler::translator::{FuncTranslatorAllocations, InstructionTranslator, ReusableAllocations},
    CompilationError, FuelCosts, FuncIdx,
};
use wasmparser::{BinaryReaderError, FuncValidator, FunctionBody, Operator, ValType, ValidatorResources, VisitOperator};
use gas_meter::{DefaultCostModel, GasMeter, ShouldInject};

pub struct FuncBuilder<'a> {
    pub(crate) func_body: FunctionBody<'a>,
    pub(crate) validator: FuncValidator<ValidatorResources>,
    pub(crate) func_idx: FuncIdx,
    pub(crate) translator: InstructionTranslator,
    pub(crate) pos: usize,
    pub(crate) gas_meter: GasMeter<DefaultCostModel>,
}

impl<'a> FuncBuilder<'a> {
    pub fn new(
        func_body: FunctionBody<'a>,
        validator: FuncValidator<ValidatorResources>,
        func_idx: FuncIdx,
        allocations: FuncTranslatorAllocations,
        with_consume_fuel: bool,
    ) -> Self {
        Self {
            func_body,
            validator,
            func_idx,
            translator: InstructionTranslator::new(allocations, with_consume_fuel),
            pos: 0,
            gas_meter: GasMeter::new(),
        }
    }

    pub fn translate(mut self) -> Result<ReusableAllocations, CompilationError> {
        self.translator.prepare(self.func_idx)?;
        // emit special opcodes before the beginning of the function
        self.translate_stack_alloc();
        self.translate_locals()?;
        let offset = self.translate_operators()?;
        self.translator.finish()?;
        Ok(ReusableAllocations {
            translation: self.translator.alloc,
            validation: self.validator.into_allocations(),
        })
    }

    fn translate_locals(&mut self) -> Result<(), CompilationError> {
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
                ValType::Ref(_) => {}
                _ => return Err(CompilationError::NotSupportedLocalType),
            }

            for _ in 0..amount as usize {
                self.translator.alloc.instruction_set.op_i32_const(0);
                // for i64 type, we need to push 2 values on the stack
                if value_type == ValType::I64 || value_type == ValType::F64 {
                    self.translator.alloc.instruction_set.op_i32_const(0);
                }
                self.translator.alloc.stack_types.push(value_type);
            }

            self.translator.stack_height.push_n(amount);
            if value_type == ValType::I64 || value_type == ValType::F64 {
                self.translator.stack_height.push_n(amount);
            }
        }

        // we exclude i64 locals from this check to satisfy wasm fuel calculation policy
        let validated_locals = self.validator.len_locals();
        self.translator
            .bump_fuel_consumption(|| FuelCosts::fuel_for_locals(validated_locals))?;
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
            let operator = reader.visit_operator(self)??;
            if let ShouldInject::InjectCost(gas_spent) = self.gas_meter.charge_gas_for(&operator) {
                self.translator.alloc.instruction_set.op_consume_fuel(gas_spent);
            }

        }
        reader.finish()?;
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
    ($( @$proposal:ident $op:ident $({ $($arg:ident: $argty:ty),* })? => $visit:ident $ann:tt)*) => {
        $(
            fn $visit(&mut self $($(, $arg: $argty)*)?) -> Self::Output {
                let offset = self.pos;
                self.validator.visitor(offset).$visit($($($arg.clone()),*)?).unwrap();//.map_err(::core::convert::Into::into)?;

                Ok(Operator::$op $({ $($arg),* })?)
            }
        )*
    };
}

impl<'a> VisitOperator<'a> for FuncBuilder<'a> {
    type Output = Result<Operator<'a>, CompilationError>;

    wasmparser::for_each_visit_operator!(impl_visit_operator);
}
