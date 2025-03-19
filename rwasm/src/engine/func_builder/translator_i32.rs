use super::{
    control_frame::{
        BlockControlFrame,
        ControlFrame,
        IfControlFrame,
        LoopControlFrame,
        UnreachableControlFrame,
    },
    labels::LabelRef,
    locals_registry::LocalsRegistry,
    value_stack::ValueStackHeight,
    ControlFlowStack,
    InstructionsBuilder,
    TranslationError,
};
use crate::{
    arena::ArenaIndex,
    core::{Pages, UntypedValue, ValueType, F32, N_MAX_TABLE_ELEMENTS},
    engine::{
        bytecode::{
            self,
            AddressOffset,
            BranchOffset,
            BranchTableTargets,
            DataSegmentIdx,
            ElementSegmentIdx,
            Instruction,
            SignatureIdx,
            TableIdx,
        },
        config::FuelCosts,
        func_builder::control_frame::ControlFrameKind,
        CompiledFunc,
        DropKeep,
        Instr,
        RelativeDepth,
    },
    module::{
        BlockType,
        ConstExpr,
        FuncIdx,
        FuncTypeIdx,
        GlobalIdx,
        MemoryIdx,
        ModuleResources,
        DEFAULT_MEMORY_INDEX,
    },
    rwasm::{translate_drop_keep, SegmentBuilder},
    Engine,
    FuncType,
    GlobalType,
    Mutability,
};
use alloc::vec::Vec;
use num_traits::CheckedSub;
use wasmparser::VisitOperator;
use crate::engine::bytecode::LocalDepth;
use crate::engine::FuncTranslatorAllocations;
use crate::value::split_i64_to_i32;


/// Type concerned with translating from Wasm bytecode to `wasmi` bytecode.
pub struct FuncTranslatorI32<'parser> {
    /// The reference to the Wasm module function under construction.
    pub(crate) func: FuncIdx,
    /// The reference to the compiled func allocated to the [`Engine`].
    compiled_func: CompiledFunc,
    /// The immutable `wasmi` module resources.
    pub(crate) res: ModuleResources<'parser>,
    /// This represents the reachability of the currently translated code.
    ///
    /// - `true`: The currently translated code is reachable.
    /// - `false`: The currently translated code is unreachable and can be skipped.
    ///
    /// # Note
    ///
    /// Visiting the Wasm `Else` or `End` control flow operator resets
    /// reachability to `true` again.
    reachable: bool,
    /// The height of the emulated value stack.
    pub(crate) stack_height: ValueStackHeight,
    /// Stores and resolves local variable types.
    locals: LocalsRegistry,
    /// The reusable data structures of the [`FuncTranslatorI32`].
    pub(crate) alloc: FuncTranslatorAllocations,

    pub(crate) stack_types: Vec<ValueType>,
}

impl<'parser> FuncTranslatorI32<'parser> {
    /// Creates a new [`FuncTranslatorI32`].
    pub fn new(
        func: FuncIdx,
        compiled_func: CompiledFunc,
        res: ModuleResources<'parser>,
        alloc: FuncTranslatorAllocations,
    ) -> Self {
        Self {
            func,
            compiled_func,
            res,
            reachable: true,
            stack_height: ValueStackHeight::default(),
            locals: LocalsRegistry::default(),
            alloc,
            stack_types: vec![]
        }
        .init()
    }

    /// Returns a shared reference to the underlying [`Engine`].
    pub(crate) fn engine(&self) -> &Engine {
        self.res.engine()
    }

    /// Initializes a newly constructed [`FuncTranslatorI32`].
    fn init(mut self) -> Self {
        self.alloc.reset();
        self.init_func_body_block();
        self.init_func_params();
        self.init_stack_types();
        self
    }

    /// Registers the `block` control frame surrounding the entire function body.
    fn init_func_body_block(&mut self) {
        let func_type = self.res.get_type_of_func(self.func);
        let block_type = BlockType::func_type(func_type);
        let end_label = self.alloc.inst_builder.new_label();
        let consume_fuel = self.is_fuel_metering_enabled().then(|| {
            self.alloc
                .inst_builder
                .push_inst(self.make_consume_fuel_base())
        });
        let block_frame = BlockControlFrame::new(block_type, end_label, 0, consume_fuel);
        self.alloc.control_frames.push_frame(block_frame);
    }

    /// Registers the function parameters in the emulated value stack.
    fn init_func_params(&mut self) {
        for param_type in self.func_type().origin_params() {
            match param_type {
                ValueType::I64 => {
                    self.locals.register_locals(2);
                }
                _ => {
                    self.locals.register_locals(1);
                }
            }
            self.locals.register_types(1);

        }
    }

    fn init_stack_types(&mut self) {
        let func_type = self.func_type();
        self.stack_types = func_type.origin_params().iter().cloned().collect();
    }

    fn get_expressed_depth(&self, local_depth: u32) -> u32 {
        self.stack_types.iter().rev().take(local_depth as usize).map(|t| {
            if t == &ValueType::I64 {
                2
            } else {
                1
            }
        }).sum()
    }

    /// Registers an `amount` of local variables.
    ///
    /// # Panics
    ///
    /// If too many local variables have been registered.
    pub fn register_locals(&mut self, amount: u32) {
        self.locals.register_locals(amount);
    }

    pub fn register_opcode_metadata(&mut self, pos: usize, opcode: u16) {
        self.alloc.inst_builder.register_meta(pos, opcode);
    }

    /// This informs the [`FuncTranslatorI32`] that the function header translation is finished.
    ///
    /// # Note
    ///
    /// This was introduced to properly calculate the fuel costs for all local variables
    /// and function parameters. After this function call no more locals and parameters may
    /// be added to this function translation.
    pub fn finish_translate_locals(&mut self) -> Result<(), TranslationError> {
        self.bump_fuel_consumption(
            self.fuel_costs()
                .fuel_for_locals(u64::from(self.locals.len_registered())),
        )?;
        Ok(())
    }

    /// Finishes constructing the function and returns its [`CompiledFunc`].
    pub fn finish(&mut self) -> Result<(), TranslationError> {
        self.alloc.inst_builder.finish(
            self.res.engine(),
            self.compiled_func,
            self.len_locals(),
            self.stack_height.max_stack_height() as usize,
        )
    }

    /// Consumes `self` and returns the underlying reusable [`FuncTranslatorAllocations`].
    pub fn into_allocations(self) -> FuncTranslatorAllocations {
        self.alloc
    }

    /// Returns `true` if fuel metering is enabled for the [`Engine`].
    ///
    /// # Note
    ///
    /// This is important for the [`FuncTranslatorI32`] to know since it
    /// has to create [`Instruction::ConsumeFuel`] instructions on the start
    /// of basic blocks such as Wasm `block`, `if` and `loop` that account
    /// for all the instructions that are going to be executed within their
    /// respective scope.
    fn is_fuel_metering_enabled(&self) -> bool {
        self.engine().config().get_consume_fuel()
    }

    /// Creates an [`Instruction::ConsumeFuel`] with base costs.
    fn make_consume_fuel_base(&self) -> Instruction {
        Instruction::consume_fuel(self.fuel_costs().base).expect("base fuel costs must be valid")
    }

    /// Returns the configured [`FuelCosts`] of the [`Engine`].
    fn fuel_costs(&self) -> &FuelCosts {
        self.engine().config().fuel_costs()
    }

    /// Returns the most recent [`ConsumeFuel`] instruction in the translation process.
    ///
    /// Returns `None` if gas metering is disabled.
    ///
    /// [`ConsumeFuel`]: enum.Instruction.html#variant.ConsumeFuel
    fn consume_fuel_instr(&self) -> Option<Instr> {
        self.alloc.control_frames.last().consume_fuel_instr()
    }

    /// Adds fuel to the most recent [`ConsumeFuel`] instruction in the translation process.
    ///
    /// Does nothing if gas metering is disabled.
    ///
    /// [`ConsumeFuel`]: enum.Instruction.html#variant.ConsumeFuel
    fn bump_fuel_consumption(&mut self, delta: u64) -> Result<(), TranslationError> {
        if let Some(instr) = self.consume_fuel_instr() {
            self.alloc
                .inst_builder
                .bump_fuel_consumption(instr, delta)?;
        }
        Ok(())
    }

    /// Returns the [`FuncType`] of the function that is currently translated.
    fn func_type(&self) -> FuncType {
        let dedup_func_type = self.res.get_type_of_func(self.func);
        self.engine()
            .resolve_func_type(dedup_func_type, Clone::clone)
    }

    /// Resolves the [`FuncType`] of the given [`FuncTypeIdx`].
    fn func_type_at(&self, func_type_index: SignatureIdx) -> FuncType {
        let func_type_index = FuncTypeIdx::from(func_type_index.to_u32()); // TODO: use the same type
        let dedup_func_type = self.res.get_func_type(func_type_index);
        self.res
            .engine()
            .resolve_func_type(dedup_func_type, Clone::clone)
    }

    /// Resolves the [`FuncType`] of the given [`FuncIdx`].
    fn func_type_of(&self, func_index: FuncIdx) -> FuncType {
        let dedup_func_type = self.res.get_type_of_func(func_index);
        self.res
            .engine()
            .resolve_func_type(dedup_func_type, Clone::clone)
    }

    /// Returns the number of local variables of the function under construction.
    fn len_locals(&self) -> usize {
        let len_params_locals = self.locals.types_registered() as usize;
        let len_params = self.func_type().origin_params().len();
        if len_params_locals < len_params {
            println!("This");
        }
        debug_assert!(len_params_locals >= len_params);
        len_params_locals - len_params
    }

    /// Returns `true` if the code at the current translation position is reachable.
    fn is_reachable(&self) -> bool {
        self.reachable
    }

    /// Translates into `wasmi` bytecode if the current code path is reachable.
    ///
    /// # Note
    ///
    /// Ignores the `translator` closure if the current code path is unreachable.
    fn translate_if_reachable<F>(&mut self, translator: F) -> Result<(), TranslationError>
    where
        F: FnOnce(&mut Self) -> Result<(), TranslationError>,
    {
        if self.is_reachable() {
            translator(self)?;
        }
        Ok(())
    }

    /// Return the value stack height difference to the height at the given `depth`.
    ///
    /// # Panics
    ///
    /// - If the current code is unreachable.
    fn height_diff(&self, depth: u32) -> u32 {
        debug_assert!(self.is_reachable());
        let current_height = self.stack_height.height();
        let frame = self.alloc.control_frames.nth_back(depth);
        let origin_height = frame.stack_height().expect("frame is reachable");
        assert!(
            origin_height <= current_height,
            "encountered value stack underflow: \
            current height {current_height}, original height {origin_height}",
        );
        current_height - origin_height
    }

    /// Computes how many values should be dropped and kept for the specific branch.
    ///
    /// # Panics
    ///
    /// If underflow of the value stack is detected.
    fn compute_drop_keep(&self, depth: u32) -> Result<DropKeep, TranslationError> {
        debug_assert!(self.is_reachable());
        let frame = self.alloc.control_frames.nth_back(depth);
        // Find out how many values we need to keep (copy to the new stack location after the drop).
        let keep = match frame.kind() {
            ControlFrameKind::Block | ControlFrameKind::If => {
                frame.block_type().len_results(self.res.engine())
            }
            ControlFrameKind::Loop => frame.block_type().len_params(self.res.engine()),
        };
        // Find out how many values we need to drop.
        let height_diff = self.height_diff(depth);
        assert!(
            keep <= height_diff,
            "tried to keep {keep} values while having \
            only {height_diff} values available on the frame",
        );
        let drop = height_diff - keep;
        DropKeep::new(drop as usize, keep as usize).map_err(Into::into)
    }

    /// Returns the maximum control stack depth at the current position in the code.
    fn max_depth(&self) -> u32 {
        self.alloc
            .control_frames
            .len()
            .checked_sub(1)
            .expect("control flow frame stack must not be empty") as u32
    }

    /// Compute [`DropKeep`] for the return statement.
    ///
    /// # Panics
    ///
    /// - If the control flow frame stack is empty.
    /// - If the value stack is underflown.
    fn drop_keep_return(&self) -> Result<DropKeep, TranslationError> {
        debug_assert!(self.is_reachable());
        assert!(
            !self.alloc.control_frames.is_empty(),
            "drop_keep_return cannot be called with the frame stack empty"
        );
        let max_depth = self.max_depth();
        let drop_keep = self.compute_drop_keep(max_depth)?;
        let len_params_locals = self.locals.len_registered() as usize;
        DropKeep::new(
            // Drop all local variables and parameters upon exit.
            drop_keep.drop() as usize + len_params_locals,
            drop_keep.keep() as usize,
        )
        .map_err(Into::into)
    }

    /// Returns the relative depth on the stack of the local variable.
    fn relative_local_depth(&self, local_idx: u32) -> u32 {
        debug_assert!(self.is_reachable());
        let stack_height = self.stack_types.len() as u32;
        stack_height.checked_sub(local_idx)
            .unwrap_or_else(|| {
                panic!("cannot convert local index into local depth: {local_idx}")
            })
    }

    /// Creates the [`BranchOffset`] to the `target` instruction for the current instruction.
    fn branch_offset(&mut self, target: LabelRef) -> Result<BranchOffset, TranslationError> {
        self.alloc.inst_builder.try_resolve_label(target)
    }

    /// Calculates the stack height upon entering a control flow frame.
    ///
    /// # Note
    ///
    /// This does not include the parameters of the control flow frame
    /// so that when shrinking the emulated value stack to the control flow
    /// frame's original stack height the control flow frame parameters are
    /// no longer on the emulated value stack.
    ///
    /// # Panics
    ///
    /// When the emulated value stack underflows. This should not happen
    /// since we have already validated the input Wasm prior.
    fn frame_stack_height(&self, block_type: BlockType) -> u32 {
        let len_params = block_type.len_params(self.engine());
        let stack_height = self.stack_height.height();
        stack_height.checked_sub(len_params).unwrap_or_else(|| {
            panic!(
                "encountered emulated value stack underflow with \
                 stack height {stack_height} and {len_params} block parameters",
            )
        })
    }

    /// Adjusts the emulated value stack given the [`FuncType`] of the call.
    fn adjust_value_stack_for_call(&mut self, func_type: &FuncType) {
        let (params, results) = func_type.params_results();
        self.stack_height.pop_n(params.len() as u32);
        for _ in func_type.origin_params() {
            self.stack_types.pop();
        }
        self.stack_height.push_n(results.len() as u32);
        for result in func_type.origin_results() {
            self.stack_types.push(*result);
        }
    }

    /// Returns `Some` equivalent instruction if the `global.get` can be optimzied.
    ///
    /// # Note
    ///
    /// Only internal (non-imported) and constant (non-mutable) globals
    /// have a chance to be optimized to more efficient instructions.
    fn optimize_global_get(
        global_type: &GlobalType,
        init_value: Option<&ConstExpr>,
        engine: &Engine,
    ) -> Result<Option<Instruction>, TranslationError> {
        // don't do global get optimization for rWASM (not needed and misleading)
        if engine.config().get_rwasm_config().is_some() {
            return Ok(None);
        }
        if let (Mutability::Const, Some(init_expr)) = (global_type.mutability(), init_value) {
            if let Some(value) = init_expr.eval_const() {
                // We can optimize `global.get` to the constant value.
                if global_type.content() == ValueType::I32 {
                    return Ok(Some(Instruction::i32_const(i32::from(value))));
                }
                if global_type.content() == ValueType::F32 {
                    return Ok(Some(Instruction::f32_const(F32::from(value))));
                }
                if global_type.content() == ValueType::I64 {
                    return Ok(Some(Instruction::I64Const(value)));
                }
                // No optimized case was applicable, so we have to allocate
                // a constant value in the const pool and reference it.
                let const_ref = engine.alloc_const(value)?;
                return Ok(Some(Instruction::ConstRef(const_ref)));
            }
            if let Some(func_index) = init_expr.funcref() {
                // We can optimize `global.get` to the equivalent `ref.func x` instruction.
                let func_index = bytecode::FuncIdx::from(func_index.into_u32());
                return Ok(Some(Instruction::RefFunc(func_index)));
            }
        }
        Ok(None)
    }

    /// Decompose a [`wasmparser::MemArg`] into its raw parts.
    fn decompose_memarg(memarg: wasmparser::MemArg) -> (MemoryIdx, u32) {
        let memory_idx = MemoryIdx::from(memarg.memory);
        let offset = memarg.offset as u32;
        (memory_idx, offset)
    }

    /// Translate a Wasm `<ty>.load` instruction.
    ///
    /// # Note
    ///
    /// This is used as the translation backend of the following Wasm instructions:
    ///
    /// - `i32.load`
    /// - `i64.load`
    /// - `f32.load`
    /// - `f64.load`
    /// - `i32.load_i8`
    /// - `i32.load_u8`
    /// - `i32.load_i16`
    /// - `i32.load_u16`
    /// - `i64.load_i8`
    /// - `i64.load_u8`
    /// - `i64.load_i16`
    /// - `i64.load_u16`
    /// - `i64.load_i32`
    /// - `i64.load_u32`
    fn translate_load(
        &mut self,
        memarg: wasmparser::MemArg,
        loaded_type: ValueType,
        make_inst: fn(AddressOffset) -> Instruction,
    ) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let (memory_idx, offset) = Self::decompose_memarg(memarg);
            debug_assert_eq!(memory_idx.into_u32(), DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().load)?;
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_types.pop();
            builder.stack_types.push(loaded_type);
            let offset = AddressOffset::from(offset);
            builder.alloc.inst_builder.push_inst(make_inst(offset));
            Ok(())
        })
    }

    /// Translate a Wasm `<ty>.store` instruction.
    ///
    /// # Note
    ///
    /// This is used as the translation backend of the following Wasm instructions:
    ///
    /// - `i32.store`
    /// - `i64.store`
    /// - `f32.store`
    /// - `f64.store`
    /// - `i32.store_i8`
    /// - `i32.store_i16`
    /// - `i64.store_i8`
    /// - `i64.store_i16`
    /// - `i64.store_i32`
    fn translate_store(
        &mut self,
        memarg: wasmparser::MemArg,
        _stored_value: ValueType,
        make_inst: fn(AddressOffset) -> Instruction,
    ) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let (memory_idx, offset) = Self::decompose_memarg(memarg);
            debug_assert_eq!(memory_idx.into_u32(), DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_height.pop2();
            builder.stack_types.pop();
            builder.stack_types.pop();

            let offset = AddressOffset::from(offset);
            builder.alloc.inst_builder.push_inst(make_inst(offset));
            Ok(())
        })
    }

    /// Translate a generic Wasm `<ty>.const` instruction.
    ///
    /// # Note
    ///
    /// This is used as the translation backend of the following Wasm instructions
    /// with constant values not representable by 32-bit values:
    ///
    /// - `i64.const`
    /// - `f64.const`
    #[allow(dead_code)]
    fn translate_const_ref<T>(&mut self, value: T) -> Result<(), TranslationError>
    where
        T: Into<UntypedValue>,
    {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            let value = value.into();
            builder.stack_height.push();
            //TODO: Add stack type
            let cref = builder.engine().alloc_const(value)?;
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::ConstRef(cref));
            Ok(())
        })
    }

    /// Translate a Wasm unary comparison instruction.
    ///
    /// # Note
    ///
    /// This is used to translate the following Wasm instructions:
    ///
    /// - `i32.eqz`
    /// - `i64.eqz`
    fn translate_unary_cmp(
        &mut self,
        _input_type: ValueType,
        inst: Instruction,
    ) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.inst_builder.push_inst(inst);
            Ok(())
        })
    }

    /// Translate a Wasm binary comparison instruction.
    ///
    /// # Note
    ///
    /// This is used to translate the following Wasm instructions:
    ///
    /// - `{i32, i64, f32, f64}.eq`
    /// - `{i32, i64, f32, f64}.ne`
    /// - `{i32, u32, i64, u64, f32, f64}.lt`
    /// - `{i32, u32, i64, u64, f32, f64}.le`
    /// - `{i32, u32, i64, u64, f32, f64}.gt`
    /// - `{i32, u32, i64, u64, f32, f64}.ge`
    fn translate_binary_cmp(
        &mut self,
        input_type: ValueType,
        inst: Instruction,
    ) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(input_type);
            builder.alloc.inst_builder.push_inst(inst);
            Ok(())
        })
    }

    /// Translate a unary Wasm instruction.
    ///
    /// # Note
    ///
    /// This is used to translate the following Wasm instructions:
    ///
    /// - `i32.clz`
    /// - `i32.ctz`
    /// - `i32.popcnt`
    /// - `{f32, f64}.abs`
    /// - `{f32, f64}.neg`
    /// - `{f32, f64}.ceil`
    /// - `{f32, f64}.floor`
    /// - `{f32, f64}.trunc`
    /// - `{f32, f64}.nearest`
    /// - `{f32, f64}.sqrt`
    fn translate_unary_operation(
        &mut self,
        _value_type: ValueType,
        inst: Instruction,
    ) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.inst_builder.push_inst(inst);
            Ok(())
        })
    }

    fn translate_expressed_unary_operation(
        &mut self,
        inst: Instruction,
        additional_opcodes: Option<Vec<Instruction>>,
    ) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.inst_builder.push_inst(inst);
            builder.stack_height.push();
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(inst);
            builder.stack_height.pop1();
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            if let Some(ixs) = additional_opcodes {
                for ix in ixs {
                    builder.alloc.inst_builder.push_inst(ix);
                }
            }

            Ok(())
        })
    }

    /// Translate a binary Wasm instruction.
    ///
    /// - `{i32, i64}.add`
    /// - `{i32, i64}.sub`
    /// - `{i32, i64}.mul`
    /// - `{i32, u32, i64, u64}.div`
    /// - `{i32, u32, i64, u64}.rem`
    /// - `{i32, i64}.and`
    /// - `{i32, i64}.or`
    /// - `{i32, i64}.xor`
    /// - `{i32, i64}.shl`
    /// - `{i32, u32, i64, u64}.shr`
    /// - `{i32, i64}.rotl`
    /// - `{i32, i64}.rotr`
    /// - `{f32, f64}.add`
    /// - `{f32, f64}.sub`
    /// - `{f32, f64}.mul`
    /// - `{f32, f64}.div`
    /// - `{f32, f64}.min`
    /// - `{f32, f64}.max`
    /// - `{f32, f64}.copysign`
    fn translate_binary_operation(
        &mut self,
        value_type: ValueType,
        inst: Instruction,
    ) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(value_type);
            
            builder.alloc.inst_builder.push_inst(inst);
            Ok(())
        })
    }

    fn translate_expressed_binary_operation<F>(
        &mut self,
        inst: Instruction,
        additional_translator: F,
    ) -> Result<(), TranslationError>
    where
        F: FnOnce(&mut Self) -> Result<(), TranslationError>, {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(inst);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(inst);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            additional_translator(builder)?;

            Ok(())
        })
    }


    fn translate_i64_div_u(&mut self) {

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();

        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
        self.alloc.inst_builder.push_inst(Instruction::I32Or);
        self.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(3)));
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
        self.alloc.inst_builder.push_inst(Instruction::I32DivU);

        self.stack_height.push_n(5);

        //Stack: lo1 hi1 lo2 hi2
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(64))); //counter
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0))); //q_lo
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0))); //q_hi
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0))); //r_lo
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0))); //r_hi

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();
        self.stack_height.pop1();

        //set r_hi
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));//r_hi
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
        self.alloc.inst_builder.push_inst(Instruction::I32Shl);
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));//r_lo
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(31)));
        self.alloc.inst_builder.push_inst(Instruction::I32ShrU);
        self.alloc.inst_builder.push_inst(Instruction::I32Or);
        self.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(1)));

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();
        self.stack_height.pop1();

        //set r_lo
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));//r_lo
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
        self.alloc.inst_builder.push_inst(Instruction::I32Shl);
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(9))); //hi1
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(31)));
        self.alloc.inst_builder.push_inst(Instruction::I32ShrU);
        self.alloc.inst_builder.push_inst(Instruction::I32Or);
        self.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();
        self.stack_height.pop1();

        //set hi1
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(8))); //hi1
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
        self.alloc.inst_builder.push_inst(Instruction::I32Shl);
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(10))); //lo1
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(31)));
        self.alloc.inst_builder.push_inst(Instruction::I32ShrU);
        self.alloc.inst_builder.push_inst(Instruction::I32Or);
        self.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(8))); //hi1

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();

        //set lo1
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(9))); //lo1
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
        self.alloc.inst_builder.push_inst(Instruction::I32Shl);
        self.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(9))); //lo1

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();

        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2))); //r_lo
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(8))); //lo2
        self.alloc.inst_builder.push_inst(Instruction::I32Sub);                              //temp_r_lo

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();

        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3))); //r_lo
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(9))); //lo2
        self.alloc.inst_builder.push_inst(Instruction::I32GeU);                              //not l_carry

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();

        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3))); //r_hi
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2))); //not l_carry
        self.alloc.inst_builder.push_inst(Instruction::I32LtU);                              //c_carry

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();

        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4))); //r_hi
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3))); //carry
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
        self.alloc.inst_builder.push_inst(Instruction::I32Xor);
        self.alloc.inst_builder.push_inst(Instruction::I32Sub);
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3))); //not l_carry
        self.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2))); //not l_carry
        self.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2))); //temp_r_hi

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();

        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2))); //temp_r_hi
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(10))); //hi2
        self.alloc.inst_builder.push_inst(Instruction::I32Sub);
        self.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2))); //temp_r_hi

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();

        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4))); //r_hi
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(10))); //hi2
        self.alloc.inst_builder.push_inst(Instruction::I32GeU);                              //not hi_carry

        self.stack_height.pop1();

        self.alloc.inst_builder.push_inst(Instruction::I32And);                               //carry

        self.stack_height.pop1();

        self.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(18)));

        self.stack_height.pop1();
        self.stack_height.pop1();
        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();
        self.stack_height.pop1();

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();

        self.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2))); //r_hi = temp_r_hi
        self.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2))); //r_lo = temp_r_lo;

        //set q_hi
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3))); //q_hi
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
        self.alloc.inst_builder.push_inst(Instruction::I32Shl);
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5))); //q_lo
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(31)));
        self.alloc.inst_builder.push_inst(Instruction::I32ShrU);
        self.alloc.inst_builder.push_inst(Instruction::I32Or);
        self.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3))); //q_hi

        //set q_lo
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4))); //q_lo
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
        self.alloc.inst_builder.push_inst(Instruction::I32Shl);
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
        self.alloc.inst_builder.push_inst(Instruction::I32Or);
        self.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4))); //q_lo

        self.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(15)));

        //set q_hi
        self.alloc.inst_builder.push_inst(Instruction::Drop);
        self.alloc.inst_builder.push_inst(Instruction::Drop);
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3))); //q_hi
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
        self.alloc.inst_builder.push_inst(Instruction::I32Shl);
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5))); //q_lo
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(31)));
        self.alloc.inst_builder.push_inst(Instruction::I32ShrU);
        self.alloc.inst_builder.push_inst(Instruction::I32Or);
        self.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3))); //q_hi

        //set q_lo
        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4))); //q_lo
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
        self.alloc.inst_builder.push_inst(Instruction::I32Shl);
        self.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4))); //q_lo


        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();

        self.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5))); //counter
        self.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
        self.alloc.inst_builder.push_inst(Instruction::I32Sub);
        self.alloc.inst_builder.push_inst(Instruction::LocalTee(LocalDepth::from(6))); //counter
        self.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(-89)));

    }

    /// Translate a Wasm conversion instruction.
    ///
    /// - `i32.wrap_i64`
    /// - `{i32, u32}.trunc_f32
    /// - `{i32, u32}.trunc_f64`
    /// - `{i64, u64}.extend_i32`
    /// - `{i64, u64}.trunc_f32`
    /// - `{i64, u64}.trunc_f64`
    /// - `f32.convert_{i32, u32, i64, u64}`
    /// - `f32.demote_f64`
    /// - `f64.convert_{i32, u32, i64, u64}`
    /// - `f64.promote_f32`
    /// - `i32.reinterpret_f32`
    /// - `i64.reinterpret_f64`
    /// - `f32.reinterpret_i32`
    /// - `f64.reinterpret_i64`
    fn translate_conversion(
        &mut self,
        _input_type: ValueType,
        _output_type: ValueType,
        inst: Instruction,
    ) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.inst_builder.push_inst(inst);
            Ok(())
        })
    }

    /// Returns the target at the given `depth` together with its [`DropKeep`].
    ///
    /// # Panics
    ///
    /// - If the `depth` is greater than the current height of the control frame stack.
    /// - If the value stack underflowed.
    fn acquire_target(&self, relative_depth: u32) -> Result<AcquiredTarget, TranslationError> {
        debug_assert!(self.is_reachable());
        if self.alloc.control_frames.is_root(relative_depth) {
            let drop_keep = self.drop_keep_return()?;
            Ok(AcquiredTarget::Return(drop_keep))
        } else {
            let label = self
                .alloc
                .control_frames
                .nth_back(relative_depth)
                .branch_destination();
            let drop_keep = self.compute_drop_keep(relative_depth)?;
            Ok(AcquiredTarget::Branch(label, drop_keep))
        }
    }

    /// Translates a Wasm reinterpret instruction.
    ///
    /// # Note
    ///
    /// The `wasmi` translation simply ignores reinterpret instructions since
    /// `wasmi` bytecode in itself it untyped.
    fn visit_reinterpret(
        &mut self,
        _input_type: ValueType,
        _output_type: ValueType,
    ) -> Result<(), TranslationError> {
        Ok(())
    }

    /// Called when translating an unsupported Wasm operator.
    ///
    /// # Note
    ///
    /// We panic instead of returning an error because unsupported Wasm
    /// errors should have been filtered out by the validation procedure
    /// already, therefore encountering an unsupported Wasm operator
    /// in the function translation procedure can be considered a bug.
    fn unsupported_operator(&self, name: &str) -> Result<(), TranslationError> {
        panic!("tried to translate an unsupported Wasm operator: {name}")
    }

    /// Computes how many values should be dropped and kept for the return call.
    ///
    /// # Panics
    ///
    /// If underflow of the value stack is detected.
    fn drop_keep_return_call(&self, callee_type: &FuncType) -> Result<DropKeep, TranslationError> {
        debug_assert!(self.is_reachable());
        // For return calls we need to adjust the `keep` value to
        // be equal to the amount of parameters the callee expects.
        let keep = callee_type.params().len() as u32;
        // Find out how many values we need to drop.
        let max_depth = self.max_depth();
        let height_diff = self.height_diff(max_depth);
        assert!(
            keep <= height_diff,
            "tried to keep {keep} values while having \
            only {height_diff} values available on the frame",
        );
        let len_params_locals = self.locals.len_registered();
        let drop = height_diff - keep + len_params_locals;
        DropKeep::new(drop as usize, keep as usize).map_err(Into::into)
    }

    fn resolve_signature_idx(&self, func_type_idx: u32) -> SignatureIdx {
        if !self.engine().config().get_rwasm_config().is_some() {
            return SignatureIdx::from(func_type_idx);
        }
        let func_type = &self.res.res.func_types[func_type_idx as usize];
        let func_type = self.res.res.engine().resolve_func_signature(&func_type);
        SignatureIdx::from(func_type.into_usize() as u32)
    }
}

/// An acquired target.
///
/// Returned by [`FuncTranslatorI32::acquire_target`].
#[derive(Debug)]
pub enum AcquiredTarget {
    /// The branch jumps to the label.
    Branch(LabelRef, DropKeep),
    /// The branch returns to the caller.
    ///
    /// # Note
    ///
    /// This is returned if the `relative_depth` points to the outmost
    /// function body `block`. WebAssembly defines branches to this control
    /// flow frame as equivalent to returning from the function.
    Return(DropKeep),
}

macro_rules! impl_visit_operator {
    ( @mvp $($rest:tt)* ) => {
        impl_visit_operator!(@@skipped $($rest)*);
    };
    ( @sign_extension $($rest:tt)* ) => {
        impl_visit_operator!(@@skipped $($rest)*);
    };
    ( @saturating_float_to_int $($rest:tt)* ) => {
        impl_visit_operator!(@@skipped $($rest)*);
    };
    ( @bulk_memory $($rest:tt)* ) => {
        impl_visit_operator!(@@skipped $($rest)*);
    };
    ( @reference_types $($rest:tt)* ) => {
        impl_visit_operator!(@@skipped $($rest)*);
    };
    ( @tail_call $($rest:tt)* ) => {
        impl_visit_operator!(@@skipped $($rest)*);
    };
    ( @@skipped $op:ident $({ $($arg:ident: $argty:ty),* })? => $visit:ident $($rest:tt)* ) => {
        // We skip Wasm operators that we already implement manually.
        impl_visit_operator!($($rest)*);
    };
    ( @$proposal:ident $op:ident $({ $($arg:ident: $argty:ty),* })? => $visit:ident $($rest:tt)* ) => {
        // Wildcard match arm for all the other (yet) unsupported Wasm proposals.
        fn $visit(&mut self $($(, $arg: $argty)*)?) -> Self::Output {
            self.unsupported_operator(stringify!($op))
        }
        impl_visit_operator!($($rest)*);
    };
    () => {};
}

impl<'a> VisitOperator<'a> for FuncTranslatorI32<'a> {
    type Output = Result<(), TranslationError>;

    wasmparser::for_each_operator!(impl_visit_operator);

    fn visit_nop(&mut self) -> Result<(), TranslationError> {
        Ok(())
    }

    fn visit_unreachable(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::Unreachable);
            builder.reachable = false;
            Ok(())
        })
    }

    fn visit_block(&mut self, block_type: wasmparser::BlockType) -> Result<(), TranslationError> {
        let block_type = BlockType::new(block_type, self.res);
        if self.is_reachable() {
            // Inherit `ConsumeFuel` instruction from parent control frame.
            // This is an optimization to reduce the number of `ConsumeFuel` instructions
            // and is applicable since Wasm `block` unconditionally executes all its instructions.
            let consume_fuel = self.alloc.control_frames.last().consume_fuel_instr();
            let stack_height = self.frame_stack_height(block_type);
            let end_label = self.alloc.inst_builder.new_label();
            self.alloc.control_frames.push_frame(BlockControlFrame::new(
                block_type,
                end_label,
                stack_height,
                consume_fuel,
            ));
        } else {
            self.alloc
                .control_frames
                .push_frame(UnreachableControlFrame::new(
                    ControlFrameKind::Block,
                    block_type,
                ));
        }
        Ok(())
    }

    fn visit_loop(&mut self, block_type: wasmparser::BlockType) -> Result<(), TranslationError> {
        let block_type = BlockType::new(block_type, self.res);
        if self.is_reachable() {
            let stack_height = self.frame_stack_height(block_type);
            let header = self.alloc.inst_builder.new_label();
            self.alloc.inst_builder.pin_label(header);
            let consume_fuel = self.is_fuel_metering_enabled().then(|| {
                self.alloc
                    .inst_builder
                    .push_inst(self.make_consume_fuel_base())
            });
            self.alloc.control_frames.push_frame(LoopControlFrame::new(
                block_type,
                header,
                stack_height,
                consume_fuel,
            ));
        } else {
            self.alloc
                .control_frames
                .push_frame(UnreachableControlFrame::new(
                    ControlFrameKind::Loop,
                    block_type,
                ));
        }
        Ok(())
    }

    fn visit_if(&mut self, block_type: wasmparser::BlockType) -> Result<(), TranslationError> {
        let block_type = BlockType::new(block_type, self.res);
        if self.is_reachable() {
            self.stack_height.pop1();
            self.stack_types.pop();

            let stack_height = self.frame_stack_height(block_type);
            let else_label = self.alloc.inst_builder.new_label();
            let end_label = self.alloc.inst_builder.new_label();
            self.bump_fuel_consumption(self.fuel_costs().base)?;
            let branch_offset = self.branch_offset(else_label)?;
            self.alloc
                .inst_builder
                .push_inst(Instruction::BrIfEqz(branch_offset));
            let consume_fuel = self.is_fuel_metering_enabled().then(|| {
                self.alloc
                    .inst_builder
                    .push_inst(self.make_consume_fuel_base())
            });
            self.alloc.control_frames.push_frame(IfControlFrame::new(
                block_type,
                end_label,
                else_label,
                stack_height,
                consume_fuel,
            ));
        } else {
            self.alloc
                .control_frames
                .push_frame(UnreachableControlFrame::new(
                    ControlFrameKind::If,
                    block_type,
                ));
        }
        Ok(())
    }

    fn visit_else(&mut self) -> Result<(), TranslationError> {
        let mut if_frame = match self.alloc.control_frames.pop_frame() {
            ControlFrame::If(if_frame) => if_frame,
            ControlFrame::Unreachable(frame) if matches!(frame.kind(), ControlFrameKind::If) => {
                // Encountered `Else` block for unreachable `If` block.
                //
                // In this case we can simply ignore the entire `Else` block
                // since it is unreachable anyways.
                self.alloc.control_frames.push_frame(frame);
                return Ok(());
            }
            unexpected => panic!(
                "expected `if` control flow frame on top \
                for `else` but found: {unexpected:?}",
            ),
        };
        let reachable = self.is_reachable();
        // At this point we know if the end of the `then` block of the paren
        // `if` block is reachable so we update the parent `if` frame.
        //
        // Note: This information is important to decide whether code is
        //       reachable after the `if` block (including `else`) ends.
        if_frame.update_end_of_then_reachability(reachable);
        // Create the jump from the end of the `then` block to the `if`
        // block's end label in case the end of `then` is reachable.
        if reachable {
            self.bump_fuel_consumption(self.fuel_costs().base)?;
            let offset = self.branch_offset(if_frame.end_label())?;
            self.alloc.inst_builder.push_inst(Instruction::Br(offset));
        }
        // Now resolve labels for the instructions of the `else` block
        self.alloc.inst_builder.pin_label(if_frame.else_label());
        // Now we can also update the `ConsumeFuel` function to use the one
        // created for the `else` part of the `if` block. This can be done
        // since the `ConsumeFuel` instruction for the `then` block is no longer
        // used from this point on.
        self.is_fuel_metering_enabled().then(|| {
            let consume_fuel = self
                .alloc
                .inst_builder
                .push_inst(self.make_consume_fuel_base());
            if_frame.update_consume_fuel_instr(consume_fuel);
        });

        let mut old_stack_height = self.stack_height.height();
        // We need to reset the value stack to exactly how it has been
        // when entering the `if` in the first place so that the `else`
        // block has the same parameters on top of the stack.
        self.stack_height.shrink_to(if_frame.stack_height());

        while old_stack_height > self.stack_height.height() {
            match self.stack_types.pop() {
                Some(ValueType::I64) => {old_stack_height -= 2;}
                Some(_) => {old_stack_height -= 1;}
                None => {panic!("Stack corrupted in else block")}
            }
        }

        if_frame
            .block_type()
            .foreach_param(self.res.engine(), |param| {
                self.stack_height.push();
                self.stack_types.push(param);
                if param == ValueType::I64 {
                    self.stack_height.push();
                }
            });
        self.alloc.control_frames.push_frame(if_frame);
        // We can reset reachability now since the parent `if` block was reachable.
        self.reachable = true;
        Ok(())
    }

    fn visit_end(&mut self) -> Result<(), TranslationError> {
        let frame = self.alloc.control_frames.last();
        if let ControlFrame::If(if_frame) = &frame {
            // At this point we can resolve the `Else` label.
            //
            // Note: The `Else` label might have already been resolved
            //       in case there was an `Else` block.
            self.alloc
                .inst_builder
                .pin_label_if_unpinned(if_frame.else_label());
        }
        if frame.is_reachable() && !matches!(frame.kind(), ControlFrameKind::Loop) {
            // At this point we can resolve the `End` labels.
            // Note that `loop` control frames do not have an `End` label.
            self.alloc.inst_builder.pin_label(frame.end_label());
        }
        // These bindings are required because of borrowing issues.
        let frame_reachable = frame.is_reachable();
        let frame_stack_height = frame.stack_height();
        if self.alloc.control_frames.len() == 1 {
            // If the control flow frames stack is empty after this point
            // we know that we are ending the function body `block`
            // frame and therefore we have to return from the function.
            self.visit_return()?;
        } else {
            // The following code is only reachable if the ended control flow
            // frame was reachable upon entering to begin with.
            self.reachable = frame_reachable;
        }
        if let Some(frame_stack_height) = frame_stack_height {
            let mut old_stack_height = self.stack_height.height();

            self.stack_height.shrink_to(frame_stack_height);

            while old_stack_height > self.stack_height.height() {
                println!("stack: {} {} {:?}", old_stack_height, self.stack_height.height(), self.stack_types);
                match self.stack_types.pop() {
                    Some(ValueType::I64) => {old_stack_height -= 2;}
                    Some(_) => {old_stack_height -= 1;}
                    None => {panic!("Type stack corrupted")}
                }
            }
        }
        let frame = self.alloc.control_frames.pop_frame();
        frame
            .block_type()
            .foreach_result(self.res.engine(), |result| {
                self.stack_height.push();
                self.stack_types.push(result);
                if result == ValueType::I64 {
                    self.stack_height.push();
                }
            });
        Ok(())
    }

    fn visit_br(&mut self, relative_depth: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let rwasm_drop_keep = builder
                .engine()
                .config()
                .get_rwasm_config()
                .map(|v| v.translate_drop_keep)
                .unwrap_or_default();
            match builder.acquire_target(relative_depth)? {
                AcquiredTarget::Branch(end_label, drop_keep) => {
                    builder.bump_fuel_consumption(builder.fuel_costs().base)?;
                    let drop_keep = if rwasm_drop_keep {
                        translate_drop_keep(&mut builder.alloc.inst_builder, drop_keep);
                        DropKeep::none()
                    } else {
                        drop_keep
                    };
                    let offset = builder.branch_offset(end_label)?;
                    if drop_keep.is_noop() {
                        builder
                            .alloc
                            .inst_builder
                            .push_inst(Instruction::Br(offset));
                    } else {
                        builder.bump_fuel_consumption(
                            builder.fuel_costs().fuel_for_drop_keep(drop_keep),
                        )?;
                        builder
                            .alloc
                            .inst_builder
                            .push_br_adjust_instr(offset, drop_keep);
                    }
                }
                AcquiredTarget::Return(_) => {
                    // In this case the `br` can be directly translated as `return`.
                    builder.visit_return()?;
                }
            }
            builder.reachable = false;
            Ok(())
        })
    }

    fn visit_br_if(&mut self, relative_depth: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let rwasm_drop_keep = builder
                .engine()
                .config()
                .get_rwasm_config()
                .map(|v| v.translate_drop_keep)
                .unwrap_or_default();
            builder.stack_height.pop1();
            builder.stack_types.pop();

            match builder.acquire_target(relative_depth)? {
                AcquiredTarget::Branch(end_label, drop_keep) => {
                    builder.bump_fuel_consumption(builder.fuel_costs().base)?;
                    let offset = builder.branch_offset(end_label)?;
                    if drop_keep.is_noop() {
                        builder
                            .alloc
                            .inst_builder
                            .push_inst(Instruction::BrIfNez(offset));
                    } else {
                        builder.bump_fuel_consumption(
                            builder.fuel_costs().fuel_for_drop_keep(drop_keep),
                        )?;
                        if rwasm_drop_keep {
                            builder
                                .alloc
                                .inst_builder
                                .push_inst(Instruction::BrIfEqz(BranchOffset::uninit()));
                            let drop_keep_length =
                                translate_drop_keep(&mut builder.alloc.inst_builder, drop_keep);
                            builder
                                .alloc
                                .inst_builder
                                .last_nth_mut(drop_keep_length)
                                .unwrap()
                                .update_branch_offset(drop_keep_length as i32 + 1);
                            builder
                                .alloc
                                .inst_builder
                                .push_inst(Instruction::Br(offset));
                        } else {
                            builder
                                .alloc
                                .inst_builder
                                .push_br_adjust_nez_instr(offset, drop_keep);
                        }
                    }
                }
                AcquiredTarget::Return(drop_keep) => {
                    if rwasm_drop_keep {
                        builder
                            .alloc
                            .inst_builder
                            .push_inst(Instruction::BrIfEqz(BranchOffset::uninit()));
                        let drop_keep_length =
                            translate_drop_keep(&mut builder.alloc.inst_builder, drop_keep);
                        builder
                            .alloc
                            .inst_builder
                            .last_nth_mut(drop_keep_length)
                            .unwrap()
                            .update_branch_offset(drop_keep_length as i32 + 1);
                        builder
                            .alloc
                            .inst_builder
                            .push_inst(Instruction::Return(DropKeep::none()));
                    } else {
                        builder
                            .alloc
                            .inst_builder
                            .push_inst(Instruction::ReturnIfNez(drop_keep));
                    }
                }
            }
            Ok(())
        })
    }

    fn visit_br_table(&mut self, table: wasmparser::BrTable<'a>) -> Result<(), TranslationError> {
        #[derive(Debug, Copy, Clone)]
        enum BrTableTarget {
            Br(BranchOffset, DropKeep),
            Return(DropKeep),
        }

        self.translate_if_reachable(|builder| {
            fn offset_instr(base: Instr, offset: usize) -> Instr {
                Instr::from_u32(base.into_u32() + offset as u32)
            }

            fn compute_instr(
                builder: &mut FuncTranslatorI32,
                n: usize,
                depth: RelativeDepth,
                max_drop_keep_fuel: &mut u64,
            ) -> Result<BrTableTarget, TranslationError> {
                match builder.acquire_target(depth.into_u32())? {
                    AcquiredTarget::Branch(label, drop_keep) => {
                        *max_drop_keep_fuel = (*max_drop_keep_fuel)
                            .max(builder.fuel_costs().fuel_for_drop_keep(drop_keep));
                        let base = builder.alloc.inst_builder.current_pc();
                        let instr = offset_instr(base, 2 * n + 1);
                        let offset = builder
                            .alloc
                            .inst_builder
                            .try_resolve_label_for(label, instr)?;
                        Ok(BrTableTarget::Br(offset, drop_keep))
                    }
                    AcquiredTarget::Return(drop_keep) => {
                        *max_drop_keep_fuel = (*max_drop_keep_fuel)
                            .max(builder.fuel_costs().fuel_for_drop_keep(drop_keep));
                        Ok(BrTableTarget::Return(drop_keep))
                    }
                }
            }

            /// Encodes the [`BrTableTarget`] into the given [`Instruction`] stream.
            fn encode_br_table_target(stream: &mut Vec<Instruction>, target: BrTableTarget) {
                match target {
                    BrTableTarget::Br(offset, drop_keep) => {
                        // Case: We push a `Br` followed by a `Return` as usual.
                        stream.push(Instruction::BrAdjust(offset));
                        stream.push(Instruction::Return(drop_keep));
                    }
                    BrTableTarget::Return(drop_keep) => {
                        // Case: We push `Return` two times to make all branch targets use 2
                        // instruction words.       This is important to
                        // make `br_table` dispatch efficient.
                        stream.push(Instruction::Return(drop_keep));
                        stream.push(Instruction::Return(drop_keep));
                    }
                }
            }

            let default = RelativeDepth::from_u32(table.default());
            let targets = table
                .targets()
                .map(|relative_depth| {
                    relative_depth.unwrap_or_else(|error| {
                        panic!(
                            "encountered unexpected invalid relative depth \
                            for `br_table` target: {error}",
                        )
                    })
                })
                .map(RelativeDepth::from_u32);

            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            // The maximum fuel costs among all `br_table` arms.
            // We use this to charge fuel once at the entry of a `br_table`
            // for the most expensive arm of all of its arms.
            let mut max_drop_keep_fuel = 0;

            builder.stack_height.pop1();
            builder.stack_types.pop();

            builder.alloc.br_table_branches.clear();
            for (n, depth) in targets.into_iter().enumerate() {
                let target = compute_instr(builder, n, depth, &mut max_drop_keep_fuel)?;
                encode_br_table_target(&mut builder.alloc.br_table_branches, target)
            }

            // We include the default target in `len_branches`. Each branch takes up 2 instruction
            // words.
            let len_branches = builder.alloc.br_table_branches.len() / 2;
            let default_branch =
                compute_instr(builder, len_branches, default, &mut max_drop_keep_fuel)?;
            let len_targets = BranchTableTargets::try_from(len_branches + 1)?;
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::BrTable(len_targets));
            encode_br_table_target(&mut builder.alloc.br_table_branches, default_branch);
            for branch in builder.alloc.br_table_branches.drain(..) {
                builder.alloc.inst_builder.push_inst(branch);
            }
            builder.bump_fuel_consumption(max_drop_keep_fuel)?;
            builder.reachable = false;
            Ok(())
        })
    }

    fn visit_return(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let rwasm_drop_keep = builder
                .engine()
                .config()
                .get_rwasm_config()
                .map(|v| v.translate_drop_keep)
                .unwrap_or_default();
            let drop_keep = builder.drop_keep_return()?;
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.bump_fuel_consumption(builder.fuel_costs().fuel_for_drop_keep(drop_keep))?;
            if rwasm_drop_keep {
                translate_drop_keep(&mut builder.alloc.inst_builder, drop_keep);
                builder
                    .alloc
                    .inst_builder
                    .push_inst(Instruction::Return(DropKeep::none()));
            } else {
                builder
                    .alloc
                    .inst_builder
                    .push_inst(Instruction::Return(drop_keep));
            }
            builder.reachable = false;
            Ok(())
        })
    }

    fn visit_return_call(&mut self, func_idx: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let func_type = builder.func_type_of(func_idx.into());
            let drop_keep = builder.drop_keep_return_call(&func_type)?;
            builder.bump_fuel_consumption(builder.fuel_costs().call)?;
            builder.bump_fuel_consumption(builder.fuel_costs().fuel_for_drop_keep(drop_keep))?;
            match builder.res.get_compiled_func(func_idx.into()) {
                Some(compiled_func) => {
                    // Case: We are calling an internal function and can optimize
                    //       this case by using the special instruction for it.
                    builder
                        .alloc
                        .inst_builder
                        .push_inst(Instruction::ReturnCallInternal(compiled_func));
                }
                None => {
                    // Case: We are calling an imported function and must use the
                    //       general calling operator for it.
                    let func = bytecode::FuncIdx::from(func_idx);
                    builder
                        .alloc
                        .inst_builder
                        .push_inst(Instruction::ReturnCall(func));
                }
            }
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::Return(drop_keep));
            builder.reachable = false;
            Ok(())
        })
    }

    fn visit_return_call_indirect(
        &mut self,
        func_type_index: u32,
        table_index: u32,
    ) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let signature = SignatureIdx::from(func_type_index);
            let func_type = builder.func_type_at(signature);
            let table = TableIdx::from(table_index);
            builder.stack_height.pop1();
            builder.stack_types.pop();

            let drop_keep = builder.drop_keep_return_call(&func_type)?;
            builder.bump_fuel_consumption(builder.fuel_costs().call)?;
            builder.bump_fuel_consumption(builder.fuel_costs().fuel_for_drop_keep(drop_keep))?;
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::ReturnCallIndirect(
                    builder.resolve_signature_idx(func_type_index),
                ));
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::Return(drop_keep));
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::TableGet(table));
            builder.reachable = false;
            Ok(())
        })
    }

    fn visit_call(&mut self, func_idx: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().call)?;
            let func_idx = FuncIdx::from(func_idx);
            let func_type = builder.func_type_of(func_idx);
            builder.adjust_value_stack_for_call(&func_type);
            match builder.res.get_compiled_func(func_idx) {
                Some(compiled_func) => {
                    // Case: We are calling an internal function and can optimize
                    //       this case by using the special instruction for it.
                    builder
                        .alloc
                        .inst_builder
                        .push_inst(Instruction::CallInternal(compiled_func));
                }
                None => {
                    // Case: We are calling an imported function and must use the
                    //       general calling operator for it.
                    let func_idx = bytecode::FuncIdx::from(func_idx.into_u32());
                    builder
                        .alloc
                        .inst_builder
                        .push_inst(Instruction::Call(func_idx));
                }
            }
            Ok(())
        })
    }

    fn visit_call_indirect(
        &mut self,
        func_type_index: u32,
        table_index: u32,
        _table_byte: u8,
    ) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().call)?;
            let func_type = SignatureIdx::from(func_type_index);
            let table = TableIdx::from(table_index);
            builder.stack_height.pop1();
            builder.stack_types.pop();

            builder.adjust_value_stack_for_call(&builder.func_type_at(func_type));
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::CallIndirect(
                    builder.resolve_signature_idx(func_type_index),
                ));
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::TableGet(table));
            Ok(())
        })
    }

    fn visit_drop(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.pop1();
            let item_type = builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            if item_type == Some(ValueType::I64) {
                builder.stack_height.pop1();
                builder.alloc.inst_builder.push_inst(Instruction::Drop);
            }

            Ok(())
        })
    }

    fn visit_select(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.pop3();
            builder.stack_height.push();

            builder.stack_types.pop();
            let item = builder.stack_types.pop().unwrap();

            if item == ValueType::I64 {
                builder.stack_height.pop1();

                builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(4)));
                builder.alloc.inst_builder.push_inst(Instruction::Drop);
                builder.alloc.inst_builder.push_inst(Instruction::Drop);
                builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(3)));
                builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));
                builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            } else {
                builder.alloc.inst_builder.push_inst(Instruction::Select);
            }
            Ok(())
        })
    }

    fn visit_typed_select(&mut self, _ty: wasmparser::ValType) -> Result<(), TranslationError> {
        // The `ty` parameter is only important for Wasm validation.
        // Since `wasmi` bytecode is untyped we are not interested in this additional information.
        self.visit_select()
    }

    fn visit_ref_null(&mut self, _ty: wasmparser::ValType) -> Result<(), TranslationError> {
        // Since `wasmi` bytecode is untyped we have no special `null` instructions
        // but simply reuse the `constant` instruction with an immediate value of 0.
        // Note that `FuncRef` and `ExternRef` are encoded as 64-bit values in `wasmi`.
        self.visit_i32_const(0i32)
    }

    fn visit_ref_is_null(&mut self) -> Result<(), TranslationError> {
        // Since `wasmi` bytecode is untyped we have no special `null` instructions
        // but simply reuse the `i64.eqz` instruction with an immediate value of 0.
        // Note that `FuncRef` and `ExternRef` are encoded as 64-bit values in `wasmi`.
        self.visit_i32_eqz()
    }

    fn visit_ref_func(&mut self, func_index: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            let func_index = bytecode::FuncIdx::from(func_index);
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::RefFunc(func_index));
            builder.stack_height.push();
            builder.stack_types.push(ValueType::FuncRef);
            Ok(())
        })
    }

    fn visit_local_get(&mut self, local_idx: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            let local_depth = builder.relative_local_depth(local_idx);

            let value = builder.stack_types[builder.stack_types.len() - local_depth as usize];

            let expressed_depth = builder.get_expressed_depth(local_depth);

            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::local_get(expressed_depth)?);
            builder.stack_height.push();

            if ValueType::I64 == value {
                builder
                    .alloc
                    .inst_builder
                    .push_inst(Instruction::local_get(expressed_depth)?);
                builder.stack_height.push();

            }
            builder.stack_types.push(value);

            Ok(())
        })
    }

    fn visit_local_set(&mut self, local_idx: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.pop1();
            let value_type = builder.stack_types.pop().unwrap();

            let local_depth = builder.relative_local_depth(local_idx);
            let expressed_depth = builder.get_expressed_depth(local_depth);

            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::local_set(expressed_depth)?);

            if ValueType::I64 == value_type {
                builder
                    .alloc
                    .inst_builder
                    .push_inst(Instruction::local_set(expressed_depth)?);
                builder.stack_height.pop1();

            }

            Ok(())
        })
    }

    fn visit_local_tee(&mut self, local_idx: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            let local_depth = builder.relative_local_depth(local_idx);
            let expressed_depth = builder.get_expressed_depth(local_depth);
            let value_type = builder.stack_types.last().unwrap();

            if ValueType::I64 == *value_type {
                builder.stack_height.push();
                builder.stack_height.pop1();

                builder
                    .alloc
                    .inst_builder
                    .push_inst(Instruction::local_tee(expressed_depth-1)?);
                builder
                    .alloc
                    .inst_builder
                    .push_inst(Instruction::local_get(2)?);
                builder
                    .alloc
                    .inst_builder
                    .push_inst(Instruction::local_set(expressed_depth)?);
            } else {
                builder
                    .alloc
                    .inst_builder
                    .push_inst(Instruction::local_tee(expressed_depth)?);
            }
            Ok(())
        })
    }

    fn visit_global_get(&mut self, global_idx: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            let global_idx = GlobalIdx::from(global_idx);
            builder.stack_height.push();

            let (global_type, init_value) = builder.res.get_global(global_idx);
            let global_idx = bytecode::GlobalIdx::from(global_idx.into_u32());
            let engine = builder.engine();
            let instr = Self::optimize_global_get(&global_type, init_value, engine)?.unwrap_or({
                // No optimization took place in this case.
                Instruction::GlobalGet((global_idx.to_u32() * 2).into())
            });

            builder.stack_types.push(global_type.content());

            if global_type.content() == ValueType::I64 {
                builder.alloc.inst_builder.push_inst(Instruction::GlobalGet((global_idx.to_u32() * 2 + 1).into()));

                builder.stack_height.push();
            }

            builder.alloc.inst_builder.push_inst(instr);
            Ok(())
        })
    }

    fn visit_global_set(&mut self, global_idx: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            let global_idx = GlobalIdx::from(global_idx);
            let global_type = builder.res.get_type_of_global(global_idx);
            debug_assert_eq!(global_type.mutability(), Mutability::Var);
            builder.stack_height.pop1();
            builder.stack_types.pop();

            let idx = global_idx.into_u32() * 2;
            let global_idx = bytecode::GlobalIdx::from(idx);

            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::GlobalSet(global_idx));

            if global_type.content() == ValueType::I64 {
                builder
                    .alloc
                    .inst_builder
                    .push_inst(Instruction::GlobalSet(bytecode::GlobalIdx::from(idx + 1)));

                builder.stack_height.pop1();
            }

            Ok(())
        })
    }

    fn visit_i32_load(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_load(memarg, ValueType::I32, Instruction::I32Load)
    }

    fn visit_i64_load(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let (memory_idx, offset) = Self::decompose_memarg(memarg);
            debug_assert_eq!(memory_idx.into_u32(), DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I64);

            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            let offset = AddressOffset::from(offset);

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Load(AddressOffset::from(offset.into_inner().checked_add(4).unwrap_or(u32::MAX))));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Load(AddressOffset::from(offset.into_inner())));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            Ok(())
        })
    }

    fn visit_f32_load(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_load(memarg, ValueType::F32, Instruction::F32Load)
    }

    fn visit_f64_load(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_load(memarg, ValueType::F64, Instruction::F64Load)
    }

    fn visit_i32_load8_s(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_load(memarg, ValueType::I32, Instruction::I32Load8S)
    }

    fn visit_i32_load8_u(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_load(memarg, ValueType::I32, Instruction::I32Load8U)
    }

    fn visit_i32_load16_s(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_load(memarg, ValueType::I32, Instruction::I32Load16S)
    }

    fn visit_i32_load16_u(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_load(memarg, ValueType::I32, Instruction::I32Load16U)
    }

    fn visit_i64_load8_s(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let (memory_idx, offset) = Self::decompose_memarg(memarg);
            debug_assert_eq!(memory_idx.into_u32(), DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I64);

            let offset = AddressOffset::from(offset);

            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();

            builder.alloc.inst_builder.push_inst(Instruction::I32Load8S(AddressOffset::from(offset.into_inner())));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Clz);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));

            Ok(())
        })
    }

    fn visit_i64_load8_u(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let (memory_idx, offset) = Self::decompose_memarg(memarg);
            debug_assert_eq!(memory_idx.into_u32(), DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_height.push();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I64);

            let offset = AddressOffset::from(offset);

            builder.alloc.inst_builder.push_inst(Instruction::I32Load8U(AddressOffset::from(offset.into_inner())));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));

            Ok(())
        })
    }

    fn visit_i64_load16_s(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let (memory_idx, offset) = Self::decompose_memarg(memarg);
            debug_assert_eq!(memory_idx.into_u32(), DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I64);

            let offset = AddressOffset::from(offset);

            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();

            builder.alloc.inst_builder.push_inst(Instruction::I32Load16S(AddressOffset::from(offset.into_inner())));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Clz);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));

            Ok(())
        })
    }

    fn visit_i64_load16_u(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let (memory_idx, offset) = Self::decompose_memarg(memarg);
            debug_assert_eq!(memory_idx.into_u32(), DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_height.push();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I64);

            let offset = AddressOffset::from(offset);

            builder.alloc.inst_builder.push_inst(Instruction::I32Load16U(AddressOffset::from(offset.into_inner())));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));


            Ok(())
        })
    }

    fn visit_i64_load32_s(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let (memory_idx, offset) = Self::decompose_memarg(memarg);
            debug_assert_eq!(memory_idx.into_u32(), DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I64);

            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();

            let offset = AddressOffset::from(offset);

            builder.alloc.inst_builder.push_inst(Instruction::I32Load(AddressOffset::from(offset.into_inner())));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Clz);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));

            Ok(())
        })
    }

    fn visit_i64_load32_u(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let (memory_idx, offset) = Self::decompose_memarg(memarg);
            debug_assert_eq!(memory_idx.into_u32(), DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I64);

            let offset = AddressOffset::from(offset);

            builder.alloc.inst_builder.push_inst(Instruction::I32Load(AddressOffset::from(offset.into_inner())));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));

            Ok(())
        })
    }

    fn visit_i32_store(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_store(memarg, ValueType::I32, Instruction::I32Store)
    }

    fn visit_i64_store(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let (memory_idx, offset) = Self::decompose_memarg(memarg);
            debug_assert_eq!(memory_idx.into_u32(), DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_types.pop();
            builder.stack_types.pop();

            let offset = AddressOffset::from(offset);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.pop1();
            builder.stack_height.pop2();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Store(AddressOffset::from(offset.into_inner().checked_add(4).unwrap_or(u32::MAX))));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::I32Store(offset));


            Ok(())
        })

    }

    fn visit_f32_store(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_store(memarg, ValueType::F32, Instruction::F32Store)
    }

    fn visit_f64_store(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_store(memarg, ValueType::F64, Instruction::F64Store)
    }

    fn visit_i32_store8(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_store(memarg, ValueType::I32, Instruction::I32Store8)
    }

    fn visit_i32_store16(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_store(memarg, ValueType::I32, Instruction::I32Store16)
    }

    fn visit_i64_store8(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let (memory_idx, offset) = Self::decompose_memarg(memarg);
            debug_assert_eq!(memory_idx.into_u32(), DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_height.pop3();
            builder.stack_types.pop();
            builder.stack_types.pop();

            let offset = AddressOffset::from(offset);

            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::I32Store8(AddressOffset::from(offset.into_inner())));

            Ok(())
        })
    }

    fn visit_i64_store16(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let (memory_idx, offset) = Self::decompose_memarg(memarg);
            debug_assert_eq!(memory_idx.into_u32(), DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_height.pop3();
            builder.stack_types.pop();
            builder.stack_types.pop();

            let offset = AddressOffset::from(offset);

            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::I32Store16(AddressOffset::from(offset.into_inner())));

            Ok(())
        })
    }

    fn visit_i64_store32(&mut self, memarg: wasmparser::MemArg) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let (memory_idx, offset) = Self::decompose_memarg(memarg);
            debug_assert_eq!(memory_idx.into_u32(), DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_height.pop3();
            builder.stack_types.pop();
            builder.stack_types.pop();

            let offset = AddressOffset::from(offset);

            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::I32Store(AddressOffset::from(offset.into_inner())));

            Ok(())
        })
    }

    fn visit_memory_size(
        &mut self,
        memory_idx: u32,
        _mem_byte: u8,
    ) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            let memory_idx = MemoryIdx::from(memory_idx);
            debug_assert_eq!(memory_idx.into_u32(), DEFAULT_MEMORY_INDEX);
            builder.stack_height.push();
            builder.stack_types.push(ValueType::I32);
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::MemorySize);
            Ok(())
        })
    }

    fn visit_memory_grow(
        &mut self,
        memory_index: u32,
        _mem_byte: u8,
    ) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let is_rwasm = builder.engine().config().get_rwasm_config().is_some();
            debug_assert_eq!(memory_index, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            let ib = &mut builder.alloc.inst_builder;
            // for rWASM, we inject memory limit error check, if we exceed the number of allowed
            // pages, then we push `u32::MAX` value on the stack that is equal to memory grow
            // overflow error
            if is_rwasm {
                assert!(!self.res.res.memories.is_empty(), "memory must be provided");
                let max_pages = self.res.res.memories[0]
                    .maximum_pages()
                    .unwrap_or(Pages::max())
                    .into_inner();
                builder.stack_height.push();
                builder.stack_height.push();
                builder.stack_height.pop1();
                builder.stack_height.push();
                builder.stack_height.pop1();
                builder.stack_height.pop1();

                ib.push_inst(Instruction::LocalGet(1.into()));
                ib.push_inst(Instruction::MemorySize);
                ib.push_inst(Instruction::I32Add);
                ib.push_inst(Instruction::I32Const(max_pages.into()));
                ib.push_inst(Instruction::I32GtS);
                ib.push_inst(Instruction::BrIfEqz(4.into()));
                ib.push_inst(Instruction::Drop);
                ib.push_inst(Instruction::I32Const(u32::MAX.into()));
                ib.push_inst(Instruction::Br(2.into()));

                builder.stack_height.pop1();
                builder.stack_height.push();
                ib.push_inst(Instruction::MemoryGrow);
            } else {
                builder.stack_height.pop1();
                builder.stack_height.push();
                ib.push_inst(Instruction::MemoryGrow);
            }
            Ok(())
        })
    }

    /// MemoryInit opcode reads 3 elements from stack (dst, src, len), where:
    /// - dst - Memory destination of copied data
    /// - src - Data source of copied data (in the passive section)
    /// - len - Length of copied data
    ///
    /// In the `passive_sections` field we store info about all passive sections
    /// that are presented in the WebAssembly binary. When passive section is activated
    /// though `memory.init` opcode we find modified offsets in the data section
    /// and put the on the stack by removing previous values.
    ///
    /// Here is the stack structure for `memory.init` call:
    /// - ... some other stack elements
    /// - dst
    /// - src
    /// - len
    /// - ... call of `memory.init` happens here
    ///
    /// Here we need to replace `src` field with our modified, but since we don't know
    /// how stack was structured then we can achieve it by replacing stack element using `local.set`
    /// opcode.
    ///
    /// - dst
    /// - src  <----+
    /// - len       |
    /// - new_src --+
    /// - ... call `local.set` to replace prev offset
    ///
    /// Here we use 1 offset because we pop `new_src`, and then count from the top, `len`
    /// has 0 offset and `src` has offset 1.
    ///
    /// Before doing these ops we must ensure that specified length of copied data
    /// doesn't exceed original section size. We also inject GT check to make sure that
    /// there is no data section overflow.
    fn visit_memory_init(
        &mut self,
        segment_index: u32,
        memory_index: u32,
    ) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let is_rwasm = builder.engine().config().get_rwasm_config().is_some();
            debug_assert_eq!(memory_index, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.pop();

            if is_rwasm {
                let (ib, rb) = (
                    &mut builder.alloc.inst_builder,
                    &mut builder.alloc.segment_builder,
                );
                let data_segment_index: DataSegmentIdx = segment_index.into();
                let (offset, length) = rb
                    .memory_sections
                    .get(&data_segment_index)
                    .copied()
                    .expect("can't resolve passive segment by index");
                // do an overflow check
                if length > 0 {
                    builder.stack_height.push();
                    builder.stack_height.push();
                    builder.stack_height.pop1();
                    builder.stack_height.push();
                    builder.stack_height.pop1();
                    builder.stack_height.pop1();
                    builder.stack_height.push();
                    builder.stack_height.pop1();
                    ib.push_inst(Instruction::LocalGet(1u32.into()));
                    ib.push_inst(Instruction::LocalGet(3u32.into()));
                    ib.push_inst(Instruction::I32Add);
                    ib.push_inst(Instruction::I32Const((length as i32).into()));
                    ib.push_inst(Instruction::I32GtS);
                    ib.push_inst(Instruction::BrIfEqz(3.into()));
                    // we can't manually emit "out of bounds table access" error required
                    // by WebAssembly standards, so we put impossible number of tables to trigger
                    // overflow by rewriting the number of elements to be copied
                    ib.push_inst(Instruction::I32Const(u32::MAX.into()));
                    ib.push_inst(Instruction::LocalSet(1.into()));
                }
                // we need to replace offset on the stack with the new value
                if offset > 0 {
                    builder.stack_height.push();
                    builder.stack_height.push();
                    builder.stack_height.pop1();
                    builder.stack_height.pop1();
                    ib.push_inst(Instruction::I32Const((offset as i32).into()));
                    ib.push_inst(Instruction::LocalGet(3.into()));
                    ib.push_inst(Instruction::I32Add);
                    ib.push_inst(Instruction::LocalSet(2.into()));
                }
                builder.stack_height.pop3();
                // since we store all data sections in the one segment then index is always 0
                ib.push_inst(Instruction::MemoryInit(DataSegmentIdx::from(
                    segment_index + 1,
                )));
            } else {
                builder.stack_height.pop3();
                builder
                    .alloc
                    .inst_builder
                    .push_inst(Instruction::MemoryInit(DataSegmentIdx::from(segment_index)));
            }
            Ok(())
        })
    }

    fn visit_memory_fill(&mut self, memory_index: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memory_index, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            builder.stack_height.pop3();
            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.pop();

            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::MemoryFill);
            Ok(())
        })
    }

    fn visit_memory_copy(&mut self, dst_mem: u32, src_mem: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(dst_mem, DEFAULT_MEMORY_INDEX);
            debug_assert_eq!(src_mem, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            builder.stack_height.pop3();
            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.pop();

            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::MemoryCopy);
            Ok(())
        })
    }

    fn visit_data_drop(&mut self, segment_index: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            let segment_index = if builder.engine().config().get_rwasm_config().is_some() {
                DataSegmentIdx::from(segment_index + 1)
            } else {
                DataSegmentIdx::from(segment_index)
            };
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::DataDrop(segment_index));
            Ok(())
        })
    }

    fn visit_table_size(&mut self, table_index: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            let table = TableIdx::from(table_index);
            builder.stack_height.push();
            builder.stack_types.push(ValueType::I32);

            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::TableSize(table));
            Ok(())
        })
    }

    fn visit_table_grow(&mut self, table_index: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let is_rwasm = builder.engine().config().get_rwasm_config().is_some();
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            let table = TableIdx::from(table_index);

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I32);

            let ib = &mut builder.alloc.inst_builder;
            // for rWASM we inject table limit error check, if we exceed number of allowed elements
            // then we push `u32::MAX` on the stack that is equal to table grow overflow error
            if is_rwasm {
                let max_table_elements = builder
                    .res
                    .res
                    .tables
                    .get(table_index as usize)
                    .unwrap()
                    .maximum()
                    .unwrap_or(N_MAX_TABLE_ELEMENTS);
                builder.stack_height.push();
                builder.stack_height.push();
                builder.stack_height.pop1();
                builder.stack_height.push();
                builder.stack_height.pop1();
                builder.stack_height.pop1();

                ib.push_inst(Instruction::LocalGet(1.into()));
                ib.push_inst(Instruction::TableSize(table));
                ib.push_inst(Instruction::I32Add);
                ib.push_inst(Instruction::I32Const(max_table_elements.into()));
                ib.push_inst(Instruction::I32GtS);
                ib.push_inst(Instruction::BrIfEqz(5.into()));
                ib.push_inst(Instruction::Drop);
                ib.push_inst(Instruction::Drop);
                ib.push_inst(Instruction::I32Const(u32::MAX.into()));
                ib.push_inst(Instruction::Br(2.into()));

                builder.stack_height.pop1();
                builder.stack_height.pop1();
                builder.stack_height.push();
                ib.push_inst(Instruction::TableGrow(table));
            } else {
                builder.stack_height.pop1();
                builder.stack_height.pop1();
                builder.stack_height.push();
                ib.push_inst(Instruction::TableGrow(table));
            }
            Ok(())
        })
    }

    fn visit_table_copy(&mut self, dst_table: u32, src_table: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            let dst = TableIdx::from(dst_table);
            let src = TableIdx::from(src_table);
            builder.stack_height.pop3();
            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.pop();

            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::TableCopy(dst));
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::TableGet(src));
            Ok(())
        })
    }

    fn visit_table_fill(&mut self, table_index: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            let table = TableIdx::from(table_index);
            builder.stack_height.pop3();
            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.pop();

            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::TableFill(table));
            Ok(())
        })
    }

    fn visit_table_get(&mut self, table_index: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            let table = TableIdx::from(table_index);
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::TableGet(table));
            Ok(())
        })
    }

    fn visit_table_set(&mut self, table_index: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            let table = TableIdx::from(table_index);
            builder.stack_height.pop2();
            //TODO: Do set and get for i32 x2 as i64
            builder.stack_types.pop();
            builder.stack_types.pop();

            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::TableSet(table));
            Ok(())
        })
    }

    fn visit_table_init(
        &mut self,
        segment_index: u32,
        table_index: u32,
    ) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            let is_rwasm = builder.engine().config().get_rwasm_config().is_some();
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.pop();


            if is_rwasm {
                let (ib, rb) = (
                    &mut builder.alloc.inst_builder,
                    &mut builder.alloc.segment_builder,
                );
                let elem_segment_index: ElementSegmentIdx = segment_index.into();
                let (offset, length) = rb
                    .element_sections
                    .get(&elem_segment_index)
                    .copied()
                    .expect("can't resolve element segment by index");
                // do an overflow check
                builder.stack_height.push();
                builder.stack_height.push();
                builder.stack_height.pop1();
                builder.stack_height.push();
                builder.stack_height.pop1();
                builder.stack_height.pop1();
                builder.stack_height.push();
                builder.stack_height.pop1();

                ib.push_inst(Instruction::LocalGet(1u32.into()));
                ib.push_inst(Instruction::LocalGet(3u32.into()));
                ib.push_inst(Instruction::I32Add);
                ib.push_inst(Instruction::I32Const((length as i32).into()));
                ib.push_inst(Instruction::I32GtS);
                ib.push_inst(Instruction::BrIfEqz(3.into()));
                // we can't manually emit "out of bounds table access" error that is required
                // by WebAssembly standards, so we put impossible number of tables to trigger
                // overflow by rewriting number of elements to be copied
                ib.push_inst(Instruction::I32Const(u32::MAX.into()));
                ib.push_inst(Instruction::LocalSet(1.into()));
                // we need to replace offset on the stack with the new value
                if offset > 0 {
                    builder.stack_height.push();
                    builder.stack_height.push();
                    builder.stack_height.pop1();
                    builder.stack_height.pop1();
                    // replace offset with an adjusted value
                    ib.push_inst(Instruction::I32Const((offset as i32).into()));
                    ib.push_inst(Instruction::LocalGet(3.into()));
                    ib.push_inst(Instruction::I32Add);
                    ib.push_inst(Instruction::LocalSet(2.into()));
                }
                builder.stack_height.pop3();
                // since we store all element sections in the one segment then index is always 0
                ib.push_inst(Instruction::TableInit((segment_index + 1).into()));
                ib.push_inst(Instruction::TableGet(table_index.into()));
            } else {
                let table = TableIdx::from(table_index);
                let elem = ElementSegmentIdx::from(segment_index);

                builder.stack_height.pop3();
                builder
                    .alloc
                    .inst_builder
                    .push_inst(Instruction::TableInit(elem));
                builder
                    .alloc
                    .inst_builder
                    .push_inst(Instruction::TableGet(table));
            }
            Ok(())
        })
    }

    fn visit_elem_drop(&mut self, segment_index: u32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            let segment_index = if builder.engine().config().get_rwasm_config().is_some() {
                ElementSegmentIdx::from(segment_index + 1)
            } else {
                ElementSegmentIdx::from(segment_index)
            };
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::ElemDrop(segment_index));
            Ok(())
        })
    }

    fn visit_i32_const(&mut self, value: i32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.push();
            builder.stack_types.push(ValueType::I32);
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::i32_const(value));
            Ok(())
        })
    }

    fn visit_i64_const(&mut self, value: i64) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_types.push(ValueType::I64);

            let [expected_low, expected_high] = split_i64_to_i32(value);
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::I32Const(UntypedValue::from(expected_low)));
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::I32Const(UntypedValue::from(expected_high)));
            Ok(())
        })
    }

    fn visit_f32_const(&mut self, value: wasmparser::Ieee32) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.push();
            builder.stack_types.push(ValueType::F32);
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::f32_const(F32::from(value.bits())));
            Ok(())
        })
    }

    fn visit_f64_const(&mut self, value: wasmparser::Ieee64) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.push();
            builder.stack_types.push(ValueType::F64);
            builder
                .alloc
                .inst_builder
                .push_inst(Instruction::F64Const(UntypedValue::from_bits(value.bits())));
            Ok(())
        })
    }

    fn visit_i32_eqz(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_cmp(ValueType::I32, Instruction::I32Eqz)
    }

    fn visit_i32_eq(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::I32, Instruction::I32Eq)
    }

    fn visit_i32_ne(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::I32, Instruction::I32Ne)
    }

    fn visit_i32_lt_s(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::I32, Instruction::I32LtS)
    }

    fn visit_i32_lt_u(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::I32, Instruction::I32LtU)
    }

    fn visit_i32_gt_s(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::I32, Instruction::I32GtS)
    }

    fn visit_i32_gt_u(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::I32, Instruction::I32GtU)
    }

    fn visit_i32_le_s(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::I32, Instruction::I32LeS)
    }

    fn visit_i32_le_u(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::I32, Instruction::I32LeU)
    }

    fn visit_i32_ge_s(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::I32, Instruction::I32GeS)
    }

    fn visit_i32_ge_u(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::I32, Instruction::I32GeU)
    }

    fn visit_i64_eqz(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.inst_builder.push_inst(Instruction::I32Eqz);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eqz);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);

            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I32);

            Ok(())
        })
    }

    fn visit_i64_eq(&mut self) -> Result<(), TranslationError> {
        self.translate_expressed_binary_operation(Instruction::I32Eq, |builder|{
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.stack_height.pop1();

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I32);

            Ok(())
        })
    }

    fn visit_i64_ne(&mut self) -> Result<(), TranslationError> {
        self.translate_expressed_binary_operation(Instruction::I32Ne, |builder|{
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);
            builder.stack_height.pop1();

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I32);
            Ok(())
        })
    }

    fn visit_i64_lt_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I32);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop_n(4);

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32LtS);
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32LtU);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_lt_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I32);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop_n(4);

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32LtU);
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32LtU);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_gt_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I32);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop_n(4);

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32GtS);
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32GtU);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_gt_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I32);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop_n(4);


            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32GtU);
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32GtU);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_le_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I32);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop_n(4);


            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32LeS);
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32LeU);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_le_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I32);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop_n(4);

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32LeU);
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32LeU);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_ge_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I32);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop_n(4);

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32GeS);
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32GeU);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_ge_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I32);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop_n(4);

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32GeU);
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32GeU);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_f32_eq(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::F32, Instruction::F32Eq)
    }

    fn visit_f32_ne(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::F32, Instruction::F32Ne)
    }

    fn visit_f32_lt(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::F32, Instruction::F32Lt)
    }

    fn visit_f32_gt(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::F32, Instruction::F32Gt)
    }

    fn visit_f32_le(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::F32, Instruction::F32Le)
    }

    fn visit_f32_ge(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::F32, Instruction::F32Ge)
    }

    fn visit_f64_eq(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::F64, Instruction::F64Eq)
    }

    fn visit_f64_ne(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::F64, Instruction::F64Ne)
    }

    fn visit_f64_lt(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::F64, Instruction::F64Lt)
    }

    fn visit_f64_gt(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::F64, Instruction::F64Gt)
    }

    fn visit_f64_le(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::F64, Instruction::F64Le)
    }

    fn visit_f64_ge(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_cmp(ValueType::F64, Instruction::F64Ge)
    }

    fn visit_i32_clz(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::I32, Instruction::I32Clz)
    }

    fn visit_i32_ctz(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::I32, Instruction::I32Ctz)
    }

    fn visit_i32_popcnt(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::I32, Instruction::I32Popcnt)
    }

    fn visit_i32_add(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32Add)
    }

    fn visit_i32_sub(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32Sub)
    }

    fn visit_i32_mul(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32Mul)
    }

    fn visit_i32_div_s(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32DivS)
    }

    fn visit_i32_div_u(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32DivU)
    }

    fn visit_i32_rem_s(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32RemS)
    }

    fn visit_i32_rem_u(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32RemU)
    }

    fn visit_i32_and(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32And)
    }

    fn visit_i32_or(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32Or)
    }

    fn visit_i32_xor(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32Xor)
    }

    fn visit_i32_shl(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32Shl)
    }

    fn visit_i32_shr_s(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32ShrS)
    }

    fn visit_i32_shr_u(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32ShrU)
    }

    fn visit_i32_rotl(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32Rotl)
    }

    fn visit_i32_rotr(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::I32, Instruction::I32Rotr)
    }

    fn visit_i64_clz(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();

            builder.alloc.inst_builder.push_inst(Instruction::I32Clz);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Clz);
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));

            Ok(())
        })
    }

    fn visit_i64_ctz(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Ctz);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Ctz);
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));

            Ok(())
        })
    }

    fn visit_i64_popcnt(&mut self) -> Result<(), TranslationError> {
        self.translate_expressed_unary_operation(Instruction::I32Popcnt, Some(vec![
            Instruction::I32Add,
            Instruction::I32Const(UntypedValue::from(0))])
        )
    }

    fn visit_i64_add(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I64);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::I32Clz);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(6)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::I32Clz);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32GtU);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_sub(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I64);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();


            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32LtU);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_mul(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I64);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0x0000ffff)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(16)));
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0x0000ffff)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(6)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(16)));
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(6)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0x0000ffff)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(7)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(16)));
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();


            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(7)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0x0000ffff)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(8)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(16)));
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            //a0 * b0
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(8)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Mul);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            //a1 * b0 + a0 * b1
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(9)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Mul);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(9)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(7)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Mul);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            //carry for c3
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::I32Clz);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::I32Clz);
            builder.alloc.inst_builder.push_inst(Instruction::I32GtU);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(1)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            //a2 * b0 + a1 * b1 + a1 * b2
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(11)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(6)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Mul);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(11)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(8)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Mul);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(11)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(10)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Mul);
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            //a3 * b0 + a2 * b1 + a1 * b2 + a0 * b3
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(1)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(12)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(6)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Mul);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(12)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(8)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Mul);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(12)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(10)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Mul);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(12)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(12)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Mul);
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);

            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(8)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(8)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(8)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(8)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();

            //Calculate first i32 with carry
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(16)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(16)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::I32Clz);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::I32Clz);
            builder.alloc.inst_builder.push_inst(Instruction::I32GtU);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();


            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(16)));
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(16)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);

            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(8)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(8)));

            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_div_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I64);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            // divide by zero check
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.alloc.inst_builder.push_inst(Instruction::I32DivU);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            // integer overflow check
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(14)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-2147483648)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(10)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-2147483648)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32DivS);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            // zero division check
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(186 + 4 + 4 - 6)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.alloc.inst_builder.push_inst(Instruction::I32LtS);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(11 + 4)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(7)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(7)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.alloc.inst_builder.push_inst(Instruction::I32LtS);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(11 + 4)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::Select);

            builder.stack_height.pop2();

            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(10)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            builder.stack_height.pop3();
            builder.stack_height.push();
            builder.stack_height.pop2();

            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(6)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(6)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(1)));

            builder.translate_i64_div_u();

            builder.stack_height.pop_n(7);

            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(5)));

            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(22 - 6)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop2();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_div_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I64);

            builder.translate_i64_div_u();

            builder.stack_height.pop_n(7);

            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(5)));

            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);


            Ok(())
        })
    }

    fn visit_i64_rem_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I64);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.alloc.inst_builder.push_inst(Instruction::I32DivU);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(158 + 4 + 4 - 2 + 15 + 7)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.alloc.inst_builder.push_inst(Instruction::I32LtS);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(11 + 4)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(7)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(7)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.alloc.inst_builder.push_inst(Instruction::I32LtS);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(11 + 4)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();

            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::Select);

            builder.stack_height.pop2();

            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(10)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            builder.stack_height.pop3();
            builder.stack_height.push();
            builder.stack_height.pop2();

            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(6)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(6)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(1)));

            builder.translate_i64_div_u();

            builder.stack_height.pop_n(7);

            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(7)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(7)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Eq);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(16)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop2();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Xor);
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfNez(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);


            Ok(())
        })
    }

    fn visit_i64_rem_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();
            builder.stack_types.pop();
            builder.stack_types.push(ValueType::I64);

            builder.translate_i64_div_u();

            builder.stack_height.pop_n(7);

            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(7)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(7)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);


            Ok(())
        })
    }

    fn visit_i64_and(&mut self) -> Result<(), TranslationError> {
        self.translate_expressed_binary_operation(Instruction::I32And, |builder|{
            builder.stack_types.pop();
            Ok(())
        })
    }

    fn visit_i64_or(&mut self) -> Result<(), TranslationError> {
        self.translate_expressed_binary_operation(Instruction::I32Or, |builder|{
            builder.stack_types.pop();
            Ok(())
        })
    }

    fn visit_i64_xor(&mut self) -> Result<(), TranslationError> {
        self.translate_expressed_binary_operation(Instruction::I32Xor, |builder|{
            builder.stack_types.pop();
            Ok(())
        })
    }

    fn visit_i64_shl(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(63)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(32)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(31)));
            builder.alloc.inst_builder.push_inst(Instruction::I32GtU);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(10)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(19)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(31)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_shr_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(63)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(34)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(31)));
            builder.alloc.inst_builder.push_inst(Instruction::I32GtU);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(12)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrS);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(31)));
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrS);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(19)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrS);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrS);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(31)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_shr_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(63)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(32)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();


            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(31)));
            builder.alloc.inst_builder.push_inst(Instruction::I32GtU);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(10)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(19)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(31)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_rotl(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(63)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(32 + 10 + 2 + 6)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(31)));
            builder.alloc.inst_builder.push_inst(Instruction::I32GtU);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(26)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(64)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(64)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);

            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(19 + 2)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);

            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));

            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_i64_rotr(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(63)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(32 + 10 + 2 + 6)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(31)));
            builder.alloc.inst_builder.push_inst(Instruction::I32GtU);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(26)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(64)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(64)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);

            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(19 + 2)));

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Sub);
            builder.alloc.inst_builder.push_inst(Instruction::I32Shl);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(5)));
            builder.alloc.inst_builder.push_inst(Instruction::I32ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::I32Or);

            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(4)));

            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::Drop);

            Ok(())
        })
    }

    fn visit_f32_abs(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::F32, Instruction::F32Abs)
    }

    fn visit_f32_neg(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::F32, Instruction::F32Neg)
    }

    fn visit_f32_ceil(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::F32, Instruction::F32Ceil)
    }

    fn visit_f32_floor(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::F32, Instruction::F32Floor)
    }

    fn visit_f32_trunc(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::F32, Instruction::F32Trunc)
    }

    fn visit_f32_nearest(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::F32, Instruction::F32Nearest)
    }

    fn visit_f32_sqrt(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::F32, Instruction::F32Sqrt)
    }

    fn visit_f32_add(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::F32, Instruction::F32Add)
    }

    fn visit_f32_sub(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::F32, Instruction::F32Sub)
    }

    fn visit_f32_mul(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::F32, Instruction::F32Mul)
    }

    fn visit_f32_div(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::F32, Instruction::F32Div)
    }

    fn visit_f32_min(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::F32, Instruction::F32Min)
    }

    fn visit_f32_max(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::F32, Instruction::F32Max)
    }

    fn visit_f32_copysign(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::F32, Instruction::F32Copysign)
    }

    fn visit_f64_abs(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::F64, Instruction::F64Abs)
    }

    fn visit_f64_neg(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::F64, Instruction::F64Neg)
    }

    fn visit_f64_ceil(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::F64, Instruction::F64Ceil)
    }

    fn visit_f64_floor(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::F64, Instruction::F64Floor)
    }

    fn visit_f64_trunc(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::F64, Instruction::F64Trunc)
    }

    fn visit_f64_nearest(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::F64, Instruction::F64Nearest)
    }

    fn visit_f64_sqrt(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::F64, Instruction::F64Sqrt)
    }

    fn visit_f64_add(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::F64, Instruction::F64Add)
    }

    fn visit_f64_sub(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::F64, Instruction::F64Sub)
    }

    fn visit_f64_mul(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::F64, Instruction::F64Mul)
    }

    fn visit_f64_div(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::F64, Instruction::F64Div)
    }

    fn visit_f64_min(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::F64, Instruction::F64Min)
    }

    fn visit_f64_max(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::F64, Instruction::F64Max)
    }

    fn visit_f64_copysign(&mut self) -> Result<(), TranslationError> {
        self.translate_binary_operation(ValueType::F64, Instruction::F64Copysign)
    }

    fn visit_i32_wrap_i64(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();

            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.stack_types.push(ValueType::I32);
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_i32_trunc_f32_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I32TruncF32S);
            builder.stack_types.push(ValueType::F32);
            Ok(())
        })
    }

    fn visit_i32_trunc_f32_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I32TruncF32U);
            builder.stack_types.push(ValueType::F32);
            Ok(())
        })
    }

    fn visit_i32_trunc_f64_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I32TruncF64S);
            builder.stack_types.push(ValueType::F32);
            Ok(())
        })
    }

    fn visit_i32_trunc_f64_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I32TruncF64U);
            builder.stack_types.push(ValueType::F32);
            Ok(())
        })
    }

    fn visit_i64_extend_i32_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();

            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Clz);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1)));
            builder.stack_types.push(ValueType::I64);

            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();

            Ok(())
        })
    }

    fn visit_i64_extend_i32_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();

            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));
            builder.stack_types.push(ValueType::I64);
            builder.stack_height.push();

            Ok(())
        })
    }

    fn visit_i64_trunc_f32_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I64TruncF32S);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I64Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I64ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            builder.stack_types.push(ValueType::I64);
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_i64_trunc_f32_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I64TruncF32U);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I64Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I64ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            builder.stack_types.push(ValueType::I64);
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();


            Ok(())
        })
    }

    fn visit_i64_trunc_f64_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I64TruncF64S);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I64Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I64ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            builder.stack_types.push(ValueType::I64);
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();


            Ok(())
        })
    }

    fn visit_i64_trunc_f64_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I64TruncF64U);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I64Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I64ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            builder.stack_types.push(ValueType::I64);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_f32_convert_i32_s(&mut self) -> Result<(), TranslationError> {
        self.translate_conversion(ValueType::I32, ValueType::F32, Instruction::F32ConvertI32S)
    }

    fn visit_f32_convert_i32_u(&mut self) -> Result<(), TranslationError> {
        self.translate_conversion(ValueType::I32, ValueType::F32, Instruction::F32ConvertI32U)
    }

    fn visit_f32_convert_i64_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I64ExtendI32U);
            builder.alloc.inst_builder.push_inst(Instruction::I64Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I64Shl);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I64ExtendI32U);
            builder.alloc.inst_builder.push_inst(Instruction::I64Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::F32ConvertI64S);

            builder.stack_types.push(ValueType::F32);
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_f32_convert_i64_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I64ExtendI32U);
            builder.alloc.inst_builder.push_inst(Instruction::I64Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I64Shl);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I64ExtendI32U);
            builder.alloc.inst_builder.push_inst(Instruction::I64Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::F32ConvertI64U);

            builder.stack_types.push(ValueType::F32);

            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_f32_demote_f64(&mut self) -> Result<(), TranslationError> {
        self.translate_conversion(ValueType::F64, ValueType::F32, Instruction::F32DemoteF64)
    }

    fn visit_f64_convert_i32_s(&mut self) -> Result<(), TranslationError> {
        self.translate_conversion(ValueType::I32, ValueType::F64, Instruction::F64ConvertI32S)
    }

    fn visit_f64_convert_i32_u(&mut self) -> Result<(), TranslationError> {
        self.translate_conversion(ValueType::I32, ValueType::F64, Instruction::F64ConvertI32U)
    }

    fn visit_f64_convert_i64_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I64ExtendI32U);
            builder.alloc.inst_builder.push_inst(Instruction::I64Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I64Shl);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I64ExtendI32U);
            builder.alloc.inst_builder.push_inst(Instruction::I64Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::F64ConvertI64S);

            builder.stack_types.push(ValueType::F64);
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            Ok(())
        })
    }

    fn visit_f64_convert_i64_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I64ExtendI32U);
            builder.alloc.inst_builder.push_inst(Instruction::I64Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I64Shl);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I64ExtendI32U);
            builder.alloc.inst_builder.push_inst(Instruction::I64Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::F64ConvertI64U);

            builder.stack_types.push(ValueType::F64);
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_f64_promote_f32(&mut self) -> Result<(), TranslationError> {
        self.translate_conversion(ValueType::F32, ValueType::F64, Instruction::F64PromoteF32)
    }

    fn visit_i32_reinterpret_f32(&mut self) -> Result<(), TranslationError> {
        self.visit_reinterpret(ValueType::F32, ValueType::I32)
    }

    fn visit_i64_reinterpret_f64(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I64Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I64ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            builder.stack_types.push(ValueType::I64);
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();


            Ok(())
        })

    }

    fn visit_f32_reinterpret_i32(&mut self) -> Result<(), TranslationError> {
        self.visit_reinterpret(ValueType::I32, ValueType::F32)
    }

    fn visit_f64_reinterpret_i64(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I64ExtendI32U);
            builder.alloc.inst_builder.push_inst(Instruction::I64Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I64Shl);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I64ExtendI32U);
            builder.alloc.inst_builder.push_inst(Instruction::I64Add);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(1)));

            builder.stack_types.push(ValueType::F64);
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            Ok(())
        })
    }

    fn visit_i32_extend8_s(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::I32, Instruction::I32Extend8S)
    }

    fn visit_i32_extend16_s(&mut self) -> Result<(), TranslationError> {
        self.translate_unary_operation(ValueType::I32, Instruction::I32Extend16S)
    }

    fn visit_i64_extend8_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::I32Extend8S);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(i32::MIN)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1_i32)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));

            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();


            Ok(())
        })
    }

    fn visit_i64_extend16_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::I32Extend16S);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(i32::MIN)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1_i32)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));

            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();

            Ok(())
        })
    }

    fn visit_i64_extend32_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.inst_builder.push_inst(Instruction::Drop);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(i32::MIN)));
            builder.alloc.inst_builder.push_inst(Instruction::I32And);
            builder.alloc.inst_builder.push_inst(Instruction::BrIfEqz(BranchOffset::from(3)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(-1_i32)));
            builder.alloc.inst_builder.push_inst(Instruction::Br(BranchOffset::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32Const(UntypedValue::from(0)));

            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();

            Ok(())
        })
    }

    fn visit_i32_trunc_sat_f32_s(&mut self) -> Result<(), TranslationError> {
        self.translate_conversion(ValueType::F32, ValueType::I32, Instruction::I32TruncSatF32S)
    }

    fn visit_i32_trunc_sat_f32_u(&mut self) -> Result<(), TranslationError> {
        self.translate_conversion(ValueType::F32, ValueType::I32, Instruction::I32TruncSatF32U)
    }

    fn visit_i32_trunc_sat_f64_s(&mut self) -> Result<(), TranslationError> {
        self.translate_conversion(ValueType::F64, ValueType::I32, Instruction::I32TruncSatF64S)
    }

    fn visit_i32_trunc_sat_f64_u(&mut self) -> Result<(), TranslationError> {
        self.translate_conversion(ValueType::F64, ValueType::I32, Instruction::I32TruncSatF64U)
    }

    fn visit_i64_trunc_sat_f32_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I64TruncSatF32S);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I64Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I64ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            builder.stack_types.push(ValueType::I64);
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_i64_trunc_sat_f32_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I64TruncSatF32U);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I64Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I64ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            builder.stack_types.push(ValueType::I64);
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_i64_trunc_sat_f64_s(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_types.pop();
            builder.alloc.inst_builder.push_inst(Instruction::I64TruncSatF64S);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I64Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I64ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            builder.stack_types.push(ValueType::I64);
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_i64_trunc_sat_f64_u(&mut self) -> Result<(), TranslationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.stack_types.pop();

            builder.alloc.inst_builder.push_inst(Instruction::I64TruncSatF64U);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(1)));
            builder.alloc.inst_builder.push_inst(Instruction::I64Const(UntypedValue::from(32)));
            builder.alloc.inst_builder.push_inst(Instruction::I64ShrU);
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalGet(LocalDepth::from(2)));
            builder.alloc.inst_builder.push_inst(Instruction::I32WrapI64);
            builder.alloc.inst_builder.push_inst(Instruction::LocalSet(LocalDepth::from(2)));

            builder.stack_types.push(ValueType::I64);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }
}
