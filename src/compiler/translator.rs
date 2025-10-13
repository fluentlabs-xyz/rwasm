use crate::{
    compiler::{
        control_flow::{
            BlockControlFrame, ControlFlowStack, ControlFrame, ControlFrameKind, IfControlFrame,
            LoopControlFrame, UnreachableControlFrame,
        },
        drop_keep::DropKeep,
        error::CompilationError,
        fuel_costs::FuelCosts,
        intrinsic::{Intrinsic, IntrinsicHandler},
        labels::LabelRegistry,
        locals_registry::LocalsRegistry,
        segment_builder::SegmentBuilder,
        snippets::{Snippet, SnippetCall},
        utils::RelativeDepth,
        value_stack::ValueStackHeight,
    },
    AddressOffset, BranchOffset, BranchTableTargets, ConstructorParams, DataSegmentIdx,
    ElementSegmentIdx, FuncIdx, FuncTypeIdx, GlobalVariable, InstrLoc, InstructionSet, LabelRef,
    Opcode, SignatureIdx, TableIdx, BASE_FUEL_COST, DEFAULT_MEMORY_INDEX, N_MAX_MEMORY_PAGES,
    N_MAX_TABLE_SIZE, SNIPPET_FUNC_IDX_UNRESOLVED,
};
use alloc::{boxed::Box, vec, vec::Vec};
use bitvec::macros::internal::funty::Fundamental;
use hashbrown::HashMap;
use wasmparser::{
    BlockType, BrTable, FuncType, FuncValidatorAllocations, GlobalType, Ieee32, Ieee64, MemArg,
    MemoryType, TableType, ValType, VisitOperator, V128,
};

/// Reusable allocations of a [`FuncTranslator`].
#[derive(Debug)]
pub struct FuncTranslatorAllocations {
    /// A stack structure to manage control flow frames within the module.
    ///
    /// The `control_frames` field holds a `ControlFlowStack`, which is used internally
    /// to track control flow constructs (such as loops, conditionals, etc.). This makes
    /// it easier to manage nested control flows and ensures proper functionality
    /// during execution.
    pub(crate) control_frames: ControlFlowStack,
    /// All labels and their uses.
    pub(crate) labels: LabelRegistry,
    /// The instruction builder.
    ///
    /// # Note
    ///
    /// Allows incrementally constructing the instruction of a function.
    pub(crate) instruction_set: InstructionSet,
    /// Buffer for translating `br_table`.
    pub(crate) br_table_branches: InstructionSet,
    /// A vector representing the types of values on a stack.
    ///
    /// This field is used to track the `ValType` of elements in a stack-like structure.
    /// It is defined with `pub(crate)` visibility, meaning that it is accessible
    /// within the current crate but not outside it.
    pub(crate) stack_types: Vec<ValType>,
    /// Module builder for rWASM
    pub(crate) segment_builder: SegmentBuilder,

    pub(crate) original_func_types: Vec<FuncType>,
    pub(crate) func_types: Vec<FuncType>,
    pub(crate) compiled_funcs: Vec<FuncTypeIdx>,
    pub(crate) tables: Vec<TableType>,
    pub(crate) memories: Vec<MemoryType>,
    pub(crate) globals: Vec<GlobalVariable>,
    pub(crate) exported_funcs: HashMap<Box<str>, FuncIdx>,
    pub(crate) start_func: Option<FuncIdx>,
    pub(crate) func_offsets: Vec<u32>,
    pub(crate) constructor_params: ConstructorParams,
    pub(crate) snippet_calls: Vec<SnippetCall>,
    pub(crate) intrinsic_handler: IntrinsicHandler,
}

impl Default for FuncTranslatorAllocations {
    fn default() -> Self {
        Self {
            control_frames: Default::default(),
            labels: Default::default(),
            instruction_set: Default::default(),
            br_table_branches: Default::default(),
            stack_types: vec![],
            segment_builder: Default::default(),
            original_func_types: vec![],
            func_types: vec![],
            compiled_funcs: vec![],
            tables: vec![],
            memories: vec![],
            globals: vec![],
            exported_funcs: Default::default(),
            start_func: None,
            func_offsets: vec![],
            constructor_params: Default::default(),
            snippet_calls: Default::default(),
            intrinsic_handler: Default::default(),
        }
    }
}

impl FuncTranslatorAllocations {
    /// Resets the data structures of the [`rwasm_legacy::engine::FuncTranslatorAllocations`].
    ///
    /// # Note
    ///
    /// This must be called before reusing this [`rwasm_legacy::engine::FuncTranslatorAllocations`]
    /// by another [`FuncTranslator`].
    pub(crate) fn reset(&mut self) {
        self.control_frames.reset();
        self.labels.reset();
        self.br_table_branches.clear();
        self.stack_types.clear();
    }

    pub(crate) fn resolve_func_params_len_type_by_block(&self, block_type: BlockType) -> usize {
        let func_type_index = match block_type {
            BlockType::FuncType(func_type_index) => func_type_index,
            BlockType::Empty | BlockType::Type(_) => return 0,
        };
        self.resolve_func_type_ref(func_type_index, |func_type| func_type.params().len())
    }

    pub(crate) fn resolve_func_results_len_type_by_block(&self, block_type: BlockType) -> usize {
        let func_type_index = match block_type {
            BlockType::FuncType(func_type_index) => func_type_index,
            BlockType::Type(ty) => {
                return match ty {
                    ValType::I64 | ValType::F64 => 2,
                    _ => 1,
                }
            }
            BlockType::Empty => return 0,
        };
        self.resolve_func_type_ref(func_type_index, |func_type| func_type.results().len())
    }

    pub(crate) fn resolve_func_type_ref<R, F: FnOnce(&FuncType) -> R>(
        &self,
        func_type_idx: FuncTypeIdx,
        f: F,
    ) -> R {
        let func_type = self.func_types.get(func_type_idx as usize).unwrap();
        f(func_type)
    }

    pub(crate) fn resolve_func_type_index<I: Into<FuncIdx>>(&self, func_idx: I) -> FuncTypeIdx {
        let func_idx: FuncIdx = func_idx.into();
        self.compiled_funcs.get(func_idx as usize).copied().unwrap()
    }

    pub(crate) fn resolve_func_type_signature(&self, func_type_idx: FuncTypeIdx) -> SignatureIdx {
        let func_type = &self.original_func_types[func_type_idx as usize];
        for (i, x) in self.original_func_types.iter().enumerate() {
            if x == func_type {
                return i as SignatureIdx;
            }
        }
        unreachable!()
    }

    pub(crate) fn emit_function_call(
        &mut self,
        function_index: u32,
        is_entrypoint: bool,
        is_return_call: bool,
    ) {
        let func_type_idx = self.resolve_func_type_index(function_index);
        let func_type = &self.func_types[func_type_idx as usize];

        let is = if is_entrypoint {
            &mut self.segment_builder.entrypoint_bytecode
        } else {
            &mut self.instruction_set
        };

        if let Some((_, intrinsic)) = self
            .intrinsic_handler
            .intrinsics
            .iter()
            .find(|(index, _)| index.as_i32() == function_index as i32)
        {
            match &intrinsic {
                Intrinsic::Replace(ref opcodes) => {
                    for opcode in opcodes {
                        is.push(*opcode);
                    }
                }
                Intrinsic::Remove => {
                    if func_type.results().len() != 0 {
                        unreachable!("remove intrinsic with result not supported")
                    }
                    for _ in 0..func_type.params().len() {
                        is.op_drop();
                    }
                }
            }
        } else if is_return_call {
            is.op_return_call_internal(function_index + 1);
        } else {
            let func_loc = is.loc();
            is.op_call_internal(function_index + 1);
        }
    }
}

/// Reusable heap allocations for function validation and translation.
#[derive(Default)]
pub struct ReusableAllocations {
    pub translation: FuncTranslatorAllocations,
    pub validation: FuncValidatorAllocations,
}

pub struct InstructionTranslator {
    /// This represents the reachability of the currently translated code.
    ///
    /// - `true`: The currently translated code is reachable.
    /// - `false`: The currently translated code is unreachable and can be skipped.
    ///
    /// # Note
    ///
    /// Visiting the Wasm `Else` or `End` control flow operator resets
    /// reachability to `true` again.
    pub(crate) reachable: bool,
    /// The reusable data structures of the [`FuncTranslator`].
    pub(crate) alloc: FuncTranslatorAllocations,
    /// The height of the emulated value stack.
    pub(crate) stack_height: ValueStackHeight,
    /// Do we need to emit consume fuel related opcodes
    pub(crate) with_consume_fuel: bool,
    /// Stores and resolves local variable types.
    pub(crate) locals: LocalsRegistry,
}

impl InstructionTranslator {
    pub fn new(alloc: FuncTranslatorAllocations, with_consume_fuel: bool) -> Self {
        Self {
            reachable: true,
            alloc,
            stack_height: Default::default(),
            with_consume_fuel,
            locals: Default::default(),
        }
    }

    /// Returns `true` if the code at the current translation position is reachable.
    fn is_reachable(&self) -> bool {
        self.reachable
    }

    /// Returns the current instruction pointer as an index.
    pub fn current_pc(&self) -> InstrLoc {
        let instr_loc = u32::try_from(self.alloc.instruction_set.len())
            .unwrap_or_else(|_| panic!("instruction len out of range"));
        instr_loc as InstrLoc
    }

    /// Registers the `block` control frame surrounding the entire function body.
    fn init_func_body_block(&mut self, func_idx: FuncIdx) {
        let func_type_idx = self.alloc.resolve_func_type_index(func_idx);
        let block_type = BlockType::FuncType(func_type_idx);
        let end_label = self.alloc.labels.new_label();
        {
            let func_type_idx = self.alloc.resolve_func_type_index(func_idx);
            let signature_index = self.alloc.resolve_func_type_signature(func_type_idx);
            self.alloc
                .instruction_set
                .op_signature_check(signature_index);
        }
        let consume_fuel = self
            .is_fuel_metering_enabled()
            .then(|| self.push_consume_fuel_base());
        let block_frame = BlockControlFrame::new(block_type, end_label, 0, consume_fuel);
        self.alloc.control_frames.push_frame(block_frame);
        let original_func_types = self
            .alloc
            .original_func_types
            .get(func_type_idx as usize)
            .unwrap();
        self.alloc.stack_types.extend(original_func_types.params());
        let func_type = self.alloc.func_types.get(func_type_idx as usize).unwrap();
        debug_assert_eq!(self.stack_height.height(), 0);
        debug_assert_eq!(self.stack_height.max_stack_height(), 0);
        let func_params_len = func_type.params().len();
        self.locals.register_locals(func_params_len as u32);
    }

    /// Resolve the label at the current instruction position.
    ///
    /// # Note
    ///
    /// This is used at a position of the Wasm bytecode where it is clear that
    /// the given label can be resolved properly.
    /// This usually takes place when encountering the Wasm `End` operand, for example.
    ///
    /// # Panics
    ///
    /// If the label has already been resolved.
    pub fn pin_label(&mut self, label: LabelRef) {
        self.alloc
            .labels
            .pin_label(label, self.current_pc())
            .unwrap_or_else(|err| panic!("failed to pin label: {err}"));
    }

    /// Pushes an instruction to consume the base fuel cost into the instruction sequence.
    ///
    /// This method calculates the base fuel cost for the associated operation, converts it
    /// into a format suitable for the instruction builder, and appends this instruction
    /// to the sequence of instructions. The finalized location of this instruction in
    /// the instruction sequence is then returned.
    pub fn push_consume_fuel_base(&mut self) -> InstrLoc {
        let instr_loc = self.alloc.instruction_set.loc();
        self.alloc.instruction_set.op_consume_fuel(BASE_FUEL_COST);
        instr_loc as InstrLoc
    }

    pub fn push_consume_fuel_empty(&mut self) -> InstrLoc {
        let instr_loc = self.alloc.instruction_set.loc();
        self.alloc.instruction_set.op_consume_fuel(0);
        instr_loc as InstrLoc
    }

    /// Returns the most recent [`ConsumeFuel`] instruction in the translation process.
    ///
    /// Returns `None` if gas metering is disabled.
    ///
    /// [`ConsumeFuel`]: enum.Instruction.html#variant.ConsumeFuel
    fn consume_fuel_instr(&self) -> Option<InstrLoc> {
        self.alloc.control_frames.last().consume_fuel_instr()
    }

    /// Adds fuel to the most recent [`ConsumeFuel`] instruction in the translation process.
    ///
    /// Does nothing if gas metering is disabled.
    ///
    /// [`ConsumeFuel`]: enum.Instruction.html#variant.ConsumeFuel
    pub(crate) fn bump_fuel_consumption<F: FnOnce() -> u32>(
        &mut self,
        delta: F,
    ) -> Result<(), CompilationError> {
        if let Some(instr) = self.consume_fuel_instr() {
            if !self.with_consume_fuel {
                return Ok(());
            };
            let delta = delta();
            self.alloc
                .instruction_set
                .bump_fuel_consumption(instr, delta)?;
        }
        Ok(())
    }

    /// Calculates the stack height upon entering a control flow frame.
    ///
    /// # Note
    ///
    /// This does not include the parameters of the control flow frame,
    /// so that when shrinking the emulated value stack to the control flow
    /// frame's original stack height, the control flow frame parameters are
    /// no longer on the emulated value stack.
    ///
    /// # Panics
    ///
    /// When the emulated value stack underflows. This should not happen
    /// since we have already validated the input Wasm prior.
    fn frame_stack_height(&self, block_type: BlockType) -> u32 {
        let len_params = self.alloc.resolve_func_params_len_type_by_block(block_type) as u32;
        let stack_height = self.stack_height.height();
        stack_height.checked_sub(len_params).unwrap_or_else(|| {
            panic!(
                "encountered emulated value stack underflow with \
                 stack height {stack_height} and {len_params} block parameters",
            )
        })
    }

    pub fn is_fuel_metering_enabled(&self) -> bool {
        self.with_consume_fuel
    }

    /// Try resolving the `label` for the given `instr`.
    ///
    /// Returns an uninitialized [`BranchOffset`] if the `label` cannot yet
    /// be resolved and defers resolution to later.
    pub fn try_resolve_label_for(
        &mut self,
        label: LabelRef,
        instr: InstrLoc,
    ) -> Result<BranchOffset, CompilationError> {
        self.alloc.labels.try_resolve_label(label, instr)
    }

    /// Try resolving the `label` for the currently constructed instruction.
    ///
    /// Returns an uninitialized [`BranchOffset`] if the `label` cannot yet
    /// be resolved and defers resolution to later.
    pub fn try_resolve_label(&mut self, label: LabelRef) -> Result<BranchOffset, CompilationError> {
        let user = self.current_pc();
        self.try_resolve_label_for(label, user)
    }

    /// Creates the [`BranchOffset`] to the `target` instruction for the current instruction.
    fn branch_offset(&mut self, target: LabelRef) -> Result<BranchOffset, CompilationError> {
        self.try_resolve_label(target)
    }

    /// Prepares the internal state for a new function in the instruction set.
    ///
    /// This method performs the following actions:
    /// - Records the current length of the instruction set as the starting offset (in instructions)
    ///   for a new function.
    /// - Updates the internal list of function offsets with the newly calculated offset.
    pub fn prepare(&mut self, func_idx: FuncIdx) -> Result<(), CompilationError> {
        self.alloc.reset();
        let func_offset = self.alloc.instruction_set.len() as u32;
        self.alloc.func_offsets.push(func_offset);
        self.init_func_body_block(func_idx);
        Ok(())
    }

    /// Finishes constructing the function and returns its [`CompiledFunc`].
    pub fn finish(&mut self) -> Result<(), CompilationError> {
        // update branch offsets in `Branch` opcodes
        for (user, offset) in self.alloc.labels.resolved_users() {
            self.alloc.instruction_set[user as usize].update_branch_offset(offset?);
        }
        let last_func_offset = self.alloc.func_offsets.last().copied().unwrap() as usize;
        // update max stack height in `StackAlloc` opcode
        let how_deep_stack_check = if self.with_consume_fuel { 3 } else { 2 };
        let mut iter = self
            .alloc
            .instruction_set
            .iter_mut()
            .skip(last_func_offset)
            .take(how_deep_stack_check);
        while let Some(opcode) = iter.next() {
            match opcode {
                Opcode::ConsumeFuel(_) | Opcode::SignatureCheck(_) => {}
                Opcode::StackCheck(max_stack_height) => {
                    *max_stack_height = self.stack_height.max_stack_height();
                    break;
                }
                _ => unreachable!(),
            }
        }
        Ok(())
    }

    pub(crate) fn resolve_global_type(&self, global_index: u32) -> &GlobalType {
        self.alloc
            .globals
            .get(global_index as usize)
            .map(|v| &v.global_type)
            .unwrap()
    }

    pub(crate) fn resolve_memory_type(&self, memory_index: u32) -> &MemoryType {
        self.alloc.memories.get(memory_index as usize).unwrap()
    }

    pub(crate) fn resolve_table_type(&self, table_index: u32) -> &TableType {
        self.alloc.tables.get(table_index as usize).unwrap()
    }

    fn add_branch(&mut self, relative_depth: u32) {
        match self.alloc.control_frames.nth_back_mut(relative_depth) {
            ControlFrame::Block(frame) => frame.bump_branches(),
            ControlFrame::Loop(frame) => frame.bump_branches(),
            ControlFrame::If(frame) => frame.bump_branches(),
            ControlFrame::Unreachable(frame) => {
                panic!("tried to `bump_branches` on an unreachable control frame: {frame:?}")
            }
        }
    }

    /// Returns the target at the given `depth` together with its [`DropKeep`].
    ///
    /// # Panics
    ///
    /// - If the `depth` is greater than the current height of the control frame stack.
    /// - If the value stack underflow's.
    fn acquire_target(&self, relative_depth: u32) -> Result<AcquiredTarget, CompilationError> {
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

    /// Computes how many values should be dropped and kept for the specific branch.
    ///
    /// # Panics
    ///
    /// If underflow of the value stack is detected.
    fn compute_drop_keep(&self, depth: u32) -> Result<DropKeep, CompilationError> {
        debug_assert!(self.is_reachable());
        let frame = self.alloc.control_frames.nth_back(depth);
        // Find out how many values we need to keep (copy to the new stack location after the drop).
        let keep = match frame.kind() {
            ControlFrameKind::Block | ControlFrameKind::If => self
                .alloc
                .resolve_func_results_len_type_by_block(frame.block_type()),
            ControlFrameKind::Loop => self
                .alloc
                .resolve_func_params_len_type_by_block(frame.block_type()),
        } as u32;
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

    /// Compute [`DropKeep`] for the return statement.
    ///
    /// # Panics
    ///
    /// - If the control flow frame stack is empty.
    /// - If the value stack is underflow.
    fn drop_keep_return(&self) -> Result<DropKeep, CompilationError> {
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

    /// Computes how many values should be dropped and kept for the return call.
    ///
    /// # Panics
    ///
    /// If underflow of the value stack is detected.
    fn drop_keep_return_call(&self, callee_type: &FuncType) -> Result<DropKeep, CompilationError> {
        debug_assert!(self.is_reachable());
        // For return calls we need to adjust the `keep` value to
        // be equal to the number of parameters the callee expects.
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

    /// Returns the maximum control stack depth at the current position in the code.
    fn max_depth(&self) -> u32 {
        self.alloc
            .control_frames
            .len()
            .checked_sub(1)
            .expect("the control flow frame stack must not be empty") as u32
    }

    /// Translates into `rwasm` bytecode if the current code path is reachable.
    ///
    /// # Note
    ///
    /// Ignore the `translator` closure if the current code path is unreachable.
    fn translate_if_reachable<F>(&mut self, translator: F) -> Result<(), CompilationError>
    where
        F: FnOnce(&mut Self) -> Result<(), CompilationError>,
    {
        if self.is_reachable() {
            translator(self)?;
        }
        Ok(())
    }

    /// Returns the relative depth on the stack of the local variable.
    fn relative_local_depth(&self, local_idx: u32) -> u32 {
        debug_assert!(self.is_reachable());
        let stack_height = self.alloc.stack_types.len() as u32;
        stack_height
            .checked_sub(local_idx)
            .unwrap_or_else(|| panic!("cannot convert a local index into local depth: {local_idx}"))
    }

    fn get_expressed_depth(&self, local_depth: u32) -> u32 {
        self.alloc
            .stack_types
            .iter()
            .rev()
            .take(local_depth as usize)
            .map(|t| {
                if t == &ValType::I64 || t == &ValType::F64 {
                    2
                } else {
                    1
                }
            })
            .sum()
    }

    /// Adjusts the emulated value stack given the [`rwasm_legacy::FuncType`] of the call.
    fn adjust_value_stack_for_call(&mut self, func_type_idx: FuncTypeIdx) {
        let func_type = &self.alloc.func_types[func_type_idx as usize];
        self.stack_height.pop_n(func_type.params().len() as u32);
        self.stack_height.push_n(func_type.results().len() as u32);
        let func_type = &self.alloc.original_func_types[func_type_idx as usize];
        for func_type in func_type.params().iter().rev() {
            let popped_type = self.alloc.stack_types.pop().unwrap();
            assert_eq!(*func_type, popped_type)
        }
        for result in func_type.results() {
            self.alloc.stack_types.push(*result);
        }
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

impl<'a> VisitOperator<'a> for InstructionTranslator {
    type Output = Result<(), CompilationError>;

    fn visit_unreachable(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.alloc.instruction_set.op_unreachable();
            builder.reachable = false;
            Ok(())
        })
    }

    fn visit_nop(&mut self) -> Self::Output {
        Ok(())
    }

    fn visit_block(&mut self, block_type: BlockType) -> Self::Output {
        if self.is_reachable() {
            // Inherit `ConsumeFuel` instruction from the parent control frame.
            // This is an optimization to reduce the number of `ConsumeFuel` instructions
            // and is applicable since Wasm `block` unconditionally executes all its instructions.
            let consume_fuel = self.alloc.control_frames.last().consume_fuel_instr();
            let stack_height = self.frame_stack_height(block_type);
            let end_label = self.alloc.labels.new_label();
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

    fn visit_loop(&mut self, block_type: BlockType) -> Self::Output {
        if self.is_reachable() {
            let stack_height = self.frame_stack_height(block_type);
            let header = self.alloc.labels.new_label();
            self.pin_label(header);
            let consume_fuel = self
                .is_fuel_metering_enabled()
                .then(|| self.push_consume_fuel_empty());
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

    fn visit_if(&mut self, block_type: BlockType) -> Self::Output {
        if self.is_reachable() {
            self.stack_height.pop1();
            self.alloc.stack_types.pop();
            let stack_height = self.frame_stack_height(block_type);
            let else_label = self.alloc.labels.new_label();
            let end_label = self.alloc.labels.new_label();
            self.bump_fuel_consumption(|| FuelCosts::BASE)?;

            let branch_offset = self.branch_offset(else_label)?;
            self.alloc.instruction_set.op_br_if_eqz(branch_offset);
            let consume_fuel = self
                .is_fuel_metering_enabled()
                .then(|| self.push_consume_fuel_empty());
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

    fn visit_else(&mut self) -> Self::Output {
        let mut if_frame = match self.alloc.control_frames.pop_frame() {
            ControlFrame::If(if_frame) => if_frame,
            ControlFrame::Unreachable(frame) if matches!(frame.kind(), ControlFrameKind::If) => {
                // Encountered `Else` block for unreachable `If` block.
                //
                // In this case, we can simply ignore the entire `Else` block
                // since it is unreachable anyway.
                self.alloc.control_frames.push_frame(frame);
                return Ok(());
            }
            unexpected => panic!(
                "expected `if` control flow frame on top \
                for `else` but found: {unexpected:?}",
            ),
        };
        let reachable = self.is_reachable();
        // At this point, we know if the end of the `then` block of the paren
        // `if` block is reachable, so we update the parent `if` frame.
        //
        // Note: This information is important to decide whether code is
        //       reachable after the `if` block (including `else`) ends.
        if_frame.update_end_of_then_reachability(reachable);
        // Create the jump from the end of the `then` block to the `if`
        // block's end label in case the end of `then` is reachable.
        if reachable {
            let offset = self.branch_offset(if_frame.end_label())?;
            self.alloc.instruction_set.op_br(offset);
        }
        // Now resolve labels for the instructions of the `else` block
        self.pin_label(if_frame.else_label());
        // Now we can also update the `ConsumeFuel` function to use the one
        // created for the `else` part of the `if` block. This can be done
        // since the `ConsumeFuel` instruction for the `then` block is no longer
        // used from this point on.
        self.is_fuel_metering_enabled().then(|| {
            let consume_fuel = self.push_consume_fuel_base();
            if_frame.update_consume_fuel_instr(consume_fuel);
        });
        let mut old_stack_height = self.stack_height.height();
        // We need to reset the value stack to exactly how it has been
        // when entering the `if` in the first place so that the `else`
        // block has the same parameters on top of the stack.
        self.stack_height.shrink_to(if_frame.stack_height());
        // Adjust the stack height for the `else` block.
        while old_stack_height > self.stack_height.height() {
            match self.alloc.stack_types.pop() {
                Some(ValType::I64) => old_stack_height -= 2,
                Some(ValType::F64) => old_stack_height -= 2,
                Some(_) => old_stack_height -= 1,
                None => panic!("stack corrupted in else block"),
            }
        }
        match if_frame.block_type() {
            BlockType::FuncType(func_type_idx) => {
                let func_type = self.alloc.func_types.get(func_type_idx as usize).unwrap();
                func_type.params().iter().for_each(|param| {
                    if *param == ValType::I64 || *param == ValType::F64 {
                        self.stack_height.push_n(2);
                    } else {
                        self.stack_height.push1();
                    }
                    self.alloc.stack_types.push(*param);
                });
            }
            _ => {}
        }
        self.alloc.control_frames.push_frame(if_frame);
        // We can reset reachability now since the parent `if` block was reachable.
        self.reachable = true;
        Ok(())
    }

    fn visit_try(&mut self, _block_type: BlockType) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_catch(&mut self, _tag_index: u32) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_throw(&mut self, _tag_index: u32) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_rethrow(&mut self, _relative_depth: u32) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_end(&mut self) -> Self::Output {
        let frame = self.alloc.control_frames.last();
        if let ControlFrame::If(if_frame) = frame {
            // At this point, we can resolve the `Else` label.
            //
            // Note: The `Else` label might have already been resolved
            //       in case there was an `Else` block.
            self.alloc
                .labels
                .try_pin_label(if_frame.else_label(), self.current_pc())
        }
        if frame.is_reachable() && !matches!(frame.kind(), ControlFrameKind::Loop) {
            // At this point, we can resolve the `End` labels.
            // Note that `loop` control frames do not have an `End` label.
            self.alloc
                .labels
                .pin_label(frame.end_label(), self.current_pc())
                .unwrap_or_else(|err| panic!("failed to pin label: {err}"));
        }
        let is_branches = self.reachable && frame.is_branched_to();
        self.reachable = self.reachable | frame.is_branched_to();

        // These bindings are required because of borrowing issues.
        let frame_stack_height = frame.stack_height();
        let block_type = frame.block_type();
        if self.alloc.control_frames.len() == 1 {
            // If the control flow frames stack is empty after this point,
            // we know that we are ending the function body `block`
            // frame, and therefore we have to return from the function.
            self.visit_return()?;
        }
        if let Some(frame_stack_height) = frame_stack_height {
            let mut old_stack_height = self.stack_height.height();
            self.stack_height.shrink_to(frame_stack_height);
            while old_stack_height > self.stack_height.height() {
                match self.alloc.stack_types.pop() {
                    Some(ValType::I64) => old_stack_height -= 2,
                    Some(ValType::F64) => old_stack_height -= 2,
                    Some(_) => old_stack_height -= 1,
                    None => panic!("type stack corrupted"),
                }
            }
        }
        let frame = self.alloc.control_frames.pop_frame();
        match frame.block_type() {
            BlockType::FuncType(func_type_idx) => {
                let func_type = self
                    .alloc
                    .original_func_types
                    .get(func_type_idx as usize)
                    .unwrap();
                func_type.results().iter().for_each(|param| {
                    if *param == ValType::I64 || *param == ValType::F64 {
                        self.stack_height.push_n(2);
                    } else {
                        self.stack_height.push1();
                    }
                    self.alloc.stack_types.push(*param);
                });
            }
            BlockType::Type(val_type) => {
                if val_type == ValType::I64 || val_type == ValType::F64 {
                    self.stack_height.push_n(2);
                } else {
                    self.stack_height.push1();
                }
                self.alloc.stack_types.push(val_type);
            }
            _ => {}
        }
        Ok(())
    }

    fn visit_br(&mut self, relative_depth: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            println!("Visit br depth: {}", relative_depth);
            builder.add_branch(relative_depth);

            match builder.acquire_target(relative_depth)? {
                AcquiredTarget::Branch(end_label, drop_keep) => {
                    builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
                    drop_keep.translate_drop_keep(
                        &mut builder.alloc.instruction_set,
                        &mut builder.stack_height,
                    );

                    let offset = builder.branch_offset(end_label)?;
                    builder.alloc.instruction_set.op_br(offset);
                }
                AcquiredTarget::Return(_) => {
                    // In this case, the `br` can be directly translated as `return`.
                    builder.visit_return()?;
                }
            }
            builder.reachable = false;
            Ok(())
        })
    }

    fn visit_br_if(&mut self, relative_depth: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.stack_height.pop1();
            builder.alloc.stack_types.pop().unwrap();
            builder.add_branch(relative_depth);
            match builder.acquire_target(relative_depth)? {
                AcquiredTarget::Branch(end_label, drop_keep) => {
                    builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
                    if drop_keep.is_noop() {
                        let offset = builder.branch_offset(end_label)?;
                        builder.alloc.instruction_set.op_br_if_nez(offset);
                    } else {
                        builder
                            .bump_fuel_consumption(|| FuelCosts::fuel_for_drop_keep(drop_keep))?;
                        builder
                            .alloc
                            .instruction_set
                            .op_br_if_eqz(BranchOffset::uninit());
                        let drop_keep_length = drop_keep.translate_drop_keep(
                            &mut builder.alloc.instruction_set,
                            &mut builder.stack_height,
                        );
                        builder
                            .alloc
                            .instruction_set
                            .last_nth_mut(drop_keep_length)
                            .unwrap()
                            .update_branch_offset(drop_keep_length as i32 + 2);
                        let offset = builder.branch_offset(end_label)?;
                        builder.alloc.instruction_set.op_br(offset);
                    }
                }
                AcquiredTarget::Return(drop_keep) => {
                    builder
                        .alloc
                        .instruction_set
                        .op_br_if_eqz(BranchOffset::uninit());
                    let drop_keep_length = drop_keep.translate_drop_keep(
                        &mut builder.alloc.instruction_set,
                        &mut builder.stack_height,
                    );
                    builder
                        .alloc
                        .instruction_set
                        .last_nth_mut(drop_keep_length)
                        .unwrap()
                        .update_branch_offset(drop_keep_length as i32 + 2);
                    builder.alloc.instruction_set.op_return();
                }
            }
            Ok(())
        })
    }

    fn visit_br_table(&mut self, targets: BrTable<'a>) -> Self::Output {
        #[derive(Debug, Copy, Clone)]
        enum BrTableTarget {
            Return(DropKeep),
            Label(LabelRef, DropKeep),
        }

        self.translate_if_reachable(|builder| {
            fn offset_instr(base: InstrLoc, offset: usize) -> InstrLoc {
                (base + offset as u32) as InstrLoc
            }

            fn fuel_for_drop_keep(builder: &mut InstructionTranslator, drop_keep: DropKeep) -> u32 {
                if builder.with_consume_fuel {
                    FuelCosts::fuel_for_drop_keep(drop_keep)
                } else {
                    0
                }
            }

            fn compute_instr(
                builder: &mut InstructionTranslator,
                n: usize,
                depth: RelativeDepth,
                max_drop_keep_fuel: &mut u32,
            ) -> Result<BrTableTarget, CompilationError> {
                match builder.acquire_target(depth.into_u32())? {
                    AcquiredTarget::Branch(label, drop_keep) => {
                        *max_drop_keep_fuel =
                            (*max_drop_keep_fuel).max(fuel_for_drop_keep(builder, drop_keep));

                        if drop_keep.keep > 0 && *max_drop_keep_fuel == 0 {
                            *max_drop_keep_fuel = 1;
                        }
                        Ok(BrTableTarget::Label(label, drop_keep))
                    }
                    AcquiredTarget::Return(drop_keep) => {
                        *max_drop_keep_fuel =
                            (*max_drop_keep_fuel).max(fuel_for_drop_keep(builder, drop_keep));

                        if drop_keep.keep > 0 && *max_drop_keep_fuel == 0 {
                            *max_drop_keep_fuel = 1;
                        }
                        Ok(BrTableTarget::Return(drop_keep))
                    }
                }
            }

            /// Encodes the [`BrTableTarget`] into the given [`Instruction`] stream.
            fn encode_br_table_target(
                builder: &mut InstructionTranslator,
                target: BrTableTarget,
                trampoline_ixs: &mut InstructionSet,
                final_len: usize,
            ) -> Result<(), CompilationError> {
                match target {
                    BrTableTarget::Return(drop_keep) => {
                        // Case: We push `Return` two times to make all branch targets use 2
                        // instruction words.       This is important to
                        // make `br_table` dispatch efficient.
                        if drop_keep.is_noop() {
                            builder.alloc.br_table_branches.op_return();
                            builder.alloc.br_table_branches.op_return();
                        } else {
                            builder.alloc.br_table_branches.op_br(
                                (final_len - builder.alloc.br_table_branches.len()
                                    + trampoline_ixs.len()) as i32,
                            );
                            builder.alloc.br_table_branches.op_return();
                            drop_keep
                                .translate_drop_keep(trampoline_ixs, &mut builder.stack_height);
                            trampoline_ixs.op_return();
                        }
                    }
                    BrTableTarget::Label(label, drop_keep) => {
                        let base = builder.current_pc();
                        if drop_keep.is_noop() {
                            builder.alloc.br_table_branches.op_return();

                            let instr = offset_instr(base, builder.alloc.br_table_branches.len());
                            let offset = builder.try_resolve_label_for(label, instr)?;
                            *builder.alloc.br_table_branches.last_mut().unwrap() =
                                Opcode::Br(BranchOffset::from(offset));

                            builder.alloc.br_table_branches.op_return();
                        } else {
                            let br_offset = (final_len - builder.alloc.br_table_branches.len()
                                + trampoline_ixs.len())
                                as i32;
                            builder
                                .alloc
                                .br_table_branches
                                .op_br(BranchOffset::from(br_offset));
                            builder.alloc.br_table_branches.op_return();

                            drop_keep
                                .translate_drop_keep(trampoline_ixs, &mut builder.stack_height);
                            trampoline_ixs.op_return();

                            let instr = offset_instr(base, final_len + trampoline_ixs.len());
                            let offset = builder.try_resolve_label_for(label, instr)?;
                            *trampoline_ixs.last_mut().unwrap() =
                                Opcode::Br(BranchOffset::from(offset));
                        }
                    }
                }

                Ok(())
            }

            let default = RelativeDepth::from_u32(targets.default());
            let target_len = (targets.len() as usize + 1) * 2; //With default branch
            let targets = targets
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

            builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
            // The maximum fuel costs among all `br_table` arms.
            // We use this to charge fuel once at the entry of a `br_table`
            // for the most expensive arm of all of its arms.
            let mut max_drop_keep_fuel = 0u32;
            builder.stack_height.pop1();
            builder.alloc.stack_types.pop().unwrap();

            builder.alloc.br_table_branches.clear();

            let final_len = builder.alloc.instruction_set.len() + target_len;
            let mut trampoline_ixs = InstructionSet::new();
            for (n, depth) in targets.into_iter().enumerate() {
                builder.add_branch(depth.into_u32());
                let target = compute_instr(builder, n, depth, &mut max_drop_keep_fuel)?;

                encode_br_table_target(builder, target, &mut trampoline_ixs, target_len)?;
            }

            // We include the default target in `len_branches`. Each branch takes up 2 instruction
            // words.
            let len_branches = builder.alloc.br_table_branches.len() / 2;
            builder.add_branch(default.into_u32());
            let default_branch =
                compute_instr(builder, len_branches, default, &mut max_drop_keep_fuel)?;
            let len_targets = BranchTableTargets::try_from(len_branches + 1)
                .map_err(|_| CompilationError::BranchTableTargetsOutOfBounds)?;
            encode_br_table_target(builder, default_branch, &mut trampoline_ixs, target_len)?;
            builder.alloc.instruction_set.op_br_table(len_targets);

            builder.alloc.br_table_branches.append(&mut trampoline_ixs);
            for branch in builder.alloc.br_table_branches.drain(..) {
                builder.alloc.instruction_set.push(branch);
            }
            builder.reachable = false;
            Ok(())
        })
    }

    fn visit_return(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            let drop_keep = builder.drop_keep_return()?;
            builder.bump_fuel_consumption(|| FuelCosts::fuel_for_drop_keep(drop_keep))?;
            drop_keep.translate_drop_keep(
                &mut builder.alloc.instruction_set,
                &mut builder.stack_height,
            );
            builder.alloc.instruction_set.op_return();
            builder.reachable = false;
            Ok(())
        })
    }

    fn visit_call(&mut self, function_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::CALL)?;
            let func_type_idx = builder.alloc.resolve_func_type_index(function_index);
            builder.adjust_value_stack_for_call(func_type_idx);
            builder
                .alloc
                .emit_function_call(function_index, false, false);
            Ok(())
        })
    }

    fn visit_call_indirect(
        &mut self,
        func_type_index: u32,
        table_index: u32,
        _table_byte: u8,
    ) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::CALL)?;
            builder.stack_height.pop1();
            builder.alloc.stack_types.pop().unwrap();
            builder.adjust_value_stack_for_call(func_type_index as FuncTypeIdx);
            let signature_idx = builder.alloc.resolve_func_type_signature(func_type_index);
            builder
                .alloc
                .instruction_set
                .op_call_indirect(signature_idx);
            builder.alloc.instruction_set.op_table_get(table_index);
            Ok(())
        })
    }

    fn visit_return_call(&mut self, function_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            let func_type_idx = builder.alloc.resolve_func_type_index(function_index);
            let func_type = &builder.alloc.func_types[func_type_idx as usize];
            let drop_keep = builder.drop_keep_return_call(&func_type)?;
            builder.bump_fuel_consumption(|| FuelCosts::CALL)?;
            builder.bump_fuel_consumption(|| FuelCosts::fuel_for_drop_keep(drop_keep))?;
            drop_keep.translate_drop_keep(
                &mut builder.alloc.instruction_set,
                &mut builder.stack_height,
            );
            builder
                .alloc
                .emit_function_call(function_index, false, true);
            builder.reachable = false;
            Ok(())
        })
    }

    fn visit_return_call_indirect(
        &mut self,
        func_type_index: u32,
        table_index: u32,
    ) -> Self::Output {
        self.translate_if_reachable(|builder| {
            let func_type = &builder.alloc.func_types[func_type_index as usize];
            builder.stack_height.pop1();
            builder.alloc.stack_types.pop().unwrap();
            let mut drop_keep = builder.drop_keep_return_call(&func_type)?;
            // TODO(dmitry123): "why? is there a bug in [drop_keep_return_call]?"
            drop_keep.keep += 1;
            builder.bump_fuel_consumption(|| FuelCosts::CALL)?;
            builder.bump_fuel_consumption(|| FuelCosts::fuel_for_drop_keep(drop_keep))?;
            drop_keep.translate_drop_keep(
                &mut builder.alloc.instruction_set,
                &mut builder.stack_height,
            );
            let signature_idx = builder.alloc.resolve_func_type_signature(func_type_index);
            builder
                .alloc
                .instruction_set
                .op_return_call_indirect(signature_idx);
            builder.alloc.instruction_set.op_table_get(table_index);
            builder.reachable = false;
            Ok(())
        })
    }

    fn visit_delegate(&mut self, _relative_depth: u32) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_catch_all(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_drop(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.stack_height.pop1();
            let item_type = builder.alloc.stack_types.pop().unwrap();
            builder.alloc.instruction_set.op_drop();
            if item_type == ValType::I64 || item_type == ValType::F64 {
                builder.stack_height.pop1();
                builder.alloc.instruction_set.op_drop();
            }
            Ok(())
        })
    }

    fn visit_select(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
            builder.stack_height.pop3();
            builder.stack_height.push1();
            builder.alloc.stack_types.pop().unwrap();
            let item = builder.alloc.stack_types.pop().unwrap();
            if item == ValType::I64 || item == ValType::F64 {
                builder.stack_height.pop1();
                builder.alloc.instruction_set.op_br_if_eqz(4);
                builder.alloc.instruction_set.op_drop();
                builder.alloc.instruction_set.op_drop();
                builder.alloc.instruction_set.op_br(3);
                builder.alloc.instruction_set.op_local_set(2);
                builder.alloc.instruction_set.op_local_set(2);
            } else {
                builder.alloc.instruction_set.op_select();
            }
            Ok(())
        })
    }

    fn visit_typed_select(&mut self, _ty: ValType) -> Self::Output {
        // The `ty` parameter is only important for Wasm validation.
        // Since `rwasm` bytecode is untyped, we are not interested in this additional information.
        self.visit_select()
    }

    fn visit_local_get(&mut self, local_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
            let local_depth = builder.relative_local_depth(local_index);
            let value =
                builder.alloc.stack_types[builder.alloc.stack_types.len() - local_depth as usize];
            let expressed_depth = builder.get_expressed_depth(local_depth);
            builder.alloc.instruction_set.op_local_get(expressed_depth);
            builder.stack_height.push1();
            if value == ValType::I64 || value == ValType::F64 {
                builder.alloc.instruction_set.op_local_get(expressed_depth);
                builder.stack_height.push1();
            }
            builder.alloc.stack_types.push(value);
            Ok(())
        })
    }

    fn visit_local_set(&mut self, local_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
            builder.stack_height.pop1();
            let value_type = builder.alloc.stack_types.pop().unwrap();
            let local_depth = builder.relative_local_depth(local_index);
            let expressed_depth = builder.get_expressed_depth(local_depth);
            builder.alloc.instruction_set.op_local_set(expressed_depth);
            if value_type == ValType::I64 || value_type == ValType::F64 {
                builder.alloc.instruction_set.op_local_set(expressed_depth);
                builder.stack_height.pop1();
            }
            Ok(())
        })
    }

    fn visit_local_tee(&mut self, local_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
            let local_depth = builder.relative_local_depth(local_index);
            let expressed_depth = builder.get_expressed_depth(local_depth);
            let value_type = builder.alloc.stack_types.last().unwrap();
            if *value_type == ValType::I64 || *value_type == ValType::F64 {
                builder.stack_height.push1();
                builder.stack_height.pop1();
                builder
                    .alloc
                    .instruction_set
                    .op_local_tee(expressed_depth - 1);
                builder.alloc.instruction_set.op_local_get(2);
                builder.alloc.instruction_set.op_local_set(expressed_depth);
            } else {
                builder.alloc.instruction_set.op_local_tee(expressed_depth);
            }
            Ok(())
        })
    }

    fn visit_global_get(&mut self, global_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            let global_type = builder.resolve_global_type(global_index).clone();
            builder.alloc.stack_types.push(global_type.content_type);
            if global_type.content_type == ValType::I64 || global_type.content_type == ValType::F64
            {
                builder
                    .alloc
                    .instruction_set
                    .op_global_get(global_index * 2 + 1);
                builder
                    .alloc
                    .instruction_set
                    .op_global_get(global_index * 2);
                builder.stack_height.push_n(2);
            } else {
                builder
                    .alloc
                    .instruction_set
                    .op_global_get(global_index * 2);
                builder.stack_height.push1();
            }
            Ok(())
        })
    }

    fn visit_global_set(&mut self, global_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            let global_type = builder.resolve_global_type(global_index).clone();
            debug_assert!(global_type.mutable);
            builder.alloc.stack_types.pop().unwrap();
            builder
                .alloc
                .instruction_set
                .op_global_set(global_index * 2);
            builder.stack_height.pop1();
            if global_type.content_type == ValType::I64 || global_type.content_type == ValType::F64
            {
                builder
                    .alloc
                    .instruction_set
                    .op_global_set(global_index * 2 + 1);
                builder.stack_height.pop1();
            }
            Ok(())
        })
    }

    fn visit_i32_load(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(memarg, ValType::I32, InstructionSet::op_i32_load, 0)
    }

    fn visit_i64_load(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(
            memarg,
            ValType::I64,
            InstructionSet::op_i64_load,
            InstructionSet::MSH_I64_LOAD,
        )
    }

    fn visit_f32_load(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(memarg, ValType::F32, InstructionSet::op_f32_load, 0)
    }

    fn visit_f64_load(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(memarg, ValType::F64, InstructionSet::op_f64_load, 0)
    }

    fn visit_i32_load8_s(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(memarg, ValType::I32, InstructionSet::op_i32_load8_s, 0)
    }

    fn visit_i32_load8_u(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(memarg, ValType::I32, InstructionSet::op_i32_load8_u, 0)
    }

    fn visit_i32_load16_s(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(memarg, ValType::I32, InstructionSet::op_i32_load16_s, 0)
    }

    fn visit_i32_load16_u(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(memarg, ValType::I32, InstructionSet::op_i32_load16_u, 0)
    }

    fn visit_i64_load8_s(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(
            memarg,
            ValType::I64,
            InstructionSet::op_i64_load8_s,
            InstructionSet::MSH_I64_LOAD8_S,
        )
    }

    fn visit_i64_load8_u(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(
            memarg,
            ValType::I64,
            InstructionSet::op_i64_load8_u,
            InstructionSet::MSH_I64_LOAD8_U,
        )
    }

    fn visit_i64_load16_s(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(
            memarg,
            ValType::I64,
            InstructionSet::op_i64_load16_s,
            InstructionSet::MSH_I64_LOAD16_S,
        )
    }

    fn visit_i64_load16_u(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(
            memarg,
            ValType::I64,
            InstructionSet::op_i64_load16_u,
            InstructionSet::MSH_I64_LOAD16_U,
        )
    }

    fn visit_i64_load32_s(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(
            memarg,
            ValType::I64,
            InstructionSet::op_i64_load32_s,
            InstructionSet::MSH_I64_LOAD32_S,
        )
    }

    fn visit_i64_load32_u(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(
            memarg,
            ValType::I64,
            InstructionSet::op_i64_load32_u,
            InstructionSet::MSH_I64_LOAD32_U,
        )
    }

    fn visit_i32_store(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_store(memarg, ValType::I32, InstructionSet::op_i32_store, 0)
    }

    fn visit_i64_store(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_store(
            memarg,
            ValType::I64,
            InstructionSet::op_i64_store,
            InstructionSet::MSH_I64_STORE,
        )
    }

    fn visit_f32_store(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_store(memarg, ValType::F32, InstructionSet::op_f32_store, 0)
    }

    fn visit_f64_store(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_store(memarg, ValType::F64, InstructionSet::op_f64_store, 0)
    }

    fn visit_i32_store8(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_store(memarg, ValType::I32, InstructionSet::op_i32_store8, 0)
    }

    fn visit_i32_store16(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_store(memarg, ValType::I32, InstructionSet::op_i32_store16, 0)
    }

    fn visit_i64_store8(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_store(memarg, ValType::I64, InstructionSet::op_i64_store8, 0)
    }

    fn visit_i64_store16(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_store(memarg, ValType::I64, InstructionSet::op_i64_store16, 0)
    }

    fn visit_i64_store32(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_store(memarg, ValType::I64, InstructionSet::op_i64_store32, 0)
    }

    fn visit_memory_size(&mut self, memory_index: u32, _mem_byte: u8) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            debug_assert_eq!(memory_index, DEFAULT_MEMORY_INDEX);
            builder.alloc.stack_types.push(ValType::I32);
            builder.stack_height.push1();
            builder.alloc.instruction_set.op_memory_size();
            Ok(())
        })
    }

    fn visit_memory_grow(&mut self, memory_index: u32, _mem_byte: u8) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memory_index, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            // for rWASM, we inject memory limit error check, if we exceed the number of allowed
            // pages, then we push `u32::MAX` value on the stack that is equal to memory grow
            // overflow error
            let memory_type = builder.resolve_memory_type(memory_index);
            let max_pages = memory_type
                .maximum
                .and_then(|v| u32::try_from(v).ok())
                .filter(|v| *v <= N_MAX_MEMORY_PAGES)
                .unwrap_or(N_MAX_MEMORY_PAGES);
            builder
                .alloc
                .instruction_set
                .op_memory_grow_checked(Some(max_pages), builder.is_fuel_metering_enabled());
            // make sure types are correct
            let popped_type = builder.alloc.stack_types.pop().unwrap();
            debug_assert_eq!(popped_type, ValType::I32);
            builder.alloc.stack_types.push(popped_type);
            // calc stack height
            builder.stack_height.push2();
            builder.stack_height.pop2();
            builder.stack_height.pop_type(ValType::I32);
            builder.stack_height.push_type(ValType::I32);
            Ok(())
        })
    }

    fn visit_i32_const(&mut self, value: i32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
            builder.alloc.stack_types.push(ValType::I32);
            builder.stack_height.push1();
            builder.alloc.instruction_set.op_i32_const(value);
            Ok(())
        })
    }

    fn visit_i64_const(&mut self, value: i64) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
            builder.alloc.stack_types.push(ValType::I64);
            builder.stack_height.push2();
            builder.alloc.instruction_set.op_i64_const(value);
            Ok(())
        })
    }

    fn visit_f32_const(&mut self, value: Ieee32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.alloc.stack_types.push(ValType::F32);
            builder.stack_height.push1();
            use crate::F32;
            let value = F32::from(value.bits());
            builder.alloc.instruction_set.op_i32_const(value);
            Ok(())
        })
    }

    fn visit_f64_const(&mut self, value: Ieee64) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.alloc.stack_types.push(ValType::F64);
            builder.stack_height.push2();
            let value = value.bits() as i64;
            builder.alloc.instruction_set.op_i64_const(value);
            Ok(())
        })
    }

    fn visit_ref_null(&mut self, _ty: ValType) -> Self::Output {
        // Since `rwasm` bytecode is untyped, we have no special `null` instructions
        // but simply reuse the `constant` instruction with an immediate value of 0.
        // Note that `FuncRef` and `ExternRef` are encoded as 64-bit values in `rwasm`.
        self.visit_i32_const(0i32)
    }

    fn visit_ref_is_null(&mut self) -> Self::Output {
        // Since `rwasm` bytecode is untyped, we have no special `null` instructions
        // but simply reuse the `i64.eqz` instruction with an immediate value of 0.
        // Note that `FuncRef` and `ExternRef` are encoded as 64-bit values in `rwasm`.
        self.visit_i32_eqz()
    }

    fn visit_ref_func(&mut self, function_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.alloc.stack_types.push(ValType::FuncRef);
            builder.stack_height.push1();
            // We do +1 here because 0 offset is reserved for `null` value and an entrypoint
            builder
                .alloc
                .instruction_set
                .op_ref_func(function_index + 1);
            Ok(())
        })
    }

    fn visit_i32_eqz(&mut self) -> Self::Output {
        self.translate_unary_compare(InstructionSet::op_i32_eqz, 0)
    }

    fn visit_i32_eq(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_i32_eq, 0)
    }

    fn visit_i32_ne(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_i32_ne, 0)
    }

    fn visit_i32_lt_s(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_i32_lt_s, 0)
    }

    fn visit_i32_lt_u(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_i32_lt_u, 0)
    }

    fn visit_i32_gt_s(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_i32_gt_s, 0)
    }

    fn visit_i32_gt_u(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_i32_gt_u, 0)
    }

    fn visit_i32_le_s(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_i32_le_s, 0)
    }

    fn visit_i32_le_u(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_i32_le_u, 0)
    }

    fn visit_i32_ge_s(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_i32_ge_s, 0)
    }

    fn visit_i32_ge_u(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_i32_ge_u, 0)
    }

    fn visit_i64_eqz(&mut self) -> Self::Output {
        self.translate_unary_compare(InstructionSet::op_i64_eqz, InstructionSet::MSH_I64_EQZ)
    }

    fn visit_i64_eq(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64Eq)
    }

    fn visit_i64_ne(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64Ne)
    }

    fn visit_i64_lt_s(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64LtS)
    }

    fn visit_i64_lt_u(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64LtU)
    }

    fn visit_i64_gt_s(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64GtS)
    }

    fn visit_i64_gt_u(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64GtU)
    }

    fn visit_i64_le_s(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64LeS)
    }

    fn visit_i64_le_u(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64LeU)
    }

    fn visit_i64_ge_s(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64GeS)
    }

    fn visit_i64_ge_u(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64GeU)
    }

    fn visit_f32_eq(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_f32_eq, 0)
    }

    fn visit_f32_ne(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_f32_ne, 0)
    }

    fn visit_f32_lt(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_f32_lt, 0)
    }

    fn visit_f32_gt(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_f32_gt, 0)
    }

    fn visit_f32_le(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_f32_le, 0)
    }

    fn visit_f32_ge(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_f32_ge, 0)
    }

    fn visit_f64_eq(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_f64_eq, 0)
    }

    fn visit_f64_ne(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_f64_ne, 0)
    }

    fn visit_f64_lt(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_f64_lt, 0)
    }

    fn visit_f64_gt(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_f64_gt, 0)
    }

    fn visit_f64_le(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_f64_le, 0)
    }

    fn visit_f64_ge(&mut self) -> Self::Output {
        self.translate_binary_compare(InstructionSet::op_f64_ge, 0)
    }

    fn visit_i32_clz(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_i32_clz, 0)
    }

    fn visit_i32_ctz(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_i32_ctz, 0)
    }

    fn visit_i32_popcnt(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_i32_popcnt, 0)
    }

    fn visit_i32_add(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_add, 0)
    }

    fn visit_i32_sub(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_sub, 0)
    }

    fn visit_i32_mul(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_mul, 0)
    }

    fn visit_i32_div_s(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_div_s, 0)
    }

    fn visit_i32_div_u(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_div_u, 0)
    }

    fn visit_i32_rem_s(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_rem_s, 0)
    }

    fn visit_i32_rem_u(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_rem_u, 0)
    }

    fn visit_i32_and(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_and, 0)
    }

    fn visit_i32_or(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_or, 0)
    }

    fn visit_i32_xor(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_xor, 0)
    }

    fn visit_i32_shl(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_shl, 0)
    }

    fn visit_i32_shr_s(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_shr_s, 0)
    }

    fn visit_i32_shr_u(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_shr_u, 0)
    }

    fn visit_i32_rotl(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_rotl, 0)
    }

    fn visit_i32_rotr(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i32_rotr, 0)
    }

    fn visit_i64_clz(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_i64_clz, InstructionSet::MSH_I64_CLZ)
    }

    fn visit_i64_ctz(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_i64_ctz, InstructionSet::MSH_I64_CTZ)
    }

    fn visit_i64_popcnt(&mut self) -> Self::Output {
        self.translate_unary(
            InstructionSet::op_i64_popcnt,
            InstructionSet::MSH_I64_POPCNT,
        )
    }

    fn visit_i64_add(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64Add)
    }

    fn visit_i64_sub(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64Sub)
    }

    fn visit_i64_mul(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64Mul)
    }

    fn visit_i64_div_s(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64DivS)
    }

    fn visit_i64_div_u(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64DivU)
    }

    fn visit_i64_rem_s(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64RemS)
    }

    fn visit_i64_rem_u(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64RemU)
    }

    fn visit_i64_and(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i64_and, InstructionSet::MSH_I64_AND)
    }

    fn visit_i64_or(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i64_or, InstructionSet::MSH_I64_OR)
    }

    fn visit_i64_xor(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_i64_xor, InstructionSet::MSH_I64_XOR)
    }

    fn visit_i64_shl(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64Shl)
    }

    fn visit_i64_shr_s(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64ShrS)
    }

    fn visit_i64_shr_u(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64ShrU)
    }

    fn visit_i64_rotl(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64RotL)
    }

    fn visit_i64_rotr(&mut self) -> Self::Output {
        self.translate_to_snippet_call(Snippet::I64RotR)
    }

    fn visit_f32_abs(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_f32_abs, 0)
    }

    fn visit_f32_neg(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_f32_neg, 0)
    }

    fn visit_f32_ceil(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_f32_ceil, 0)
    }

    fn visit_f32_floor(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_f32_floor, 0)
    }

    fn visit_f32_trunc(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_f32_trunc, 0)
    }

    fn visit_f32_nearest(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_f32_nearest, 0)
    }

    fn visit_f32_sqrt(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_f32_sqrt, 0)
    }

    fn visit_f32_add(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_f32_add, 0)
    }

    fn visit_f32_sub(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_f32_sub, 0)
    }

    fn visit_f32_mul(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_f32_mul, 0)
    }

    fn visit_f32_div(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_f32_div, 0)
    }

    fn visit_f32_min(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_f32_min, 0)
    }

    fn visit_f32_max(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_f32_max, 0)
    }

    fn visit_f32_copysign(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_f32_copysign, 0)
    }

    fn visit_f64_abs(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_f64_abs, 0)
    }

    fn visit_f64_neg(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_f64_neg, 0)
    }

    fn visit_f64_ceil(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_f64_ceil, 0)
    }

    fn visit_f64_floor(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_f64_floor, 0)
    }

    fn visit_f64_trunc(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_f64_trunc, 0)
    }

    fn visit_f64_nearest(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_f64_nearest, 0)
    }

    fn visit_f64_sqrt(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_f64_sqrt, 0)
    }

    fn visit_f64_add(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_f64_add, 0)
    }

    fn visit_f64_sub(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_f64_sub, 0)
    }

    fn visit_f64_mul(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_f64_mul, 0)
    }

    fn visit_f64_div(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_f64_div, 0)
    }

    fn visit_f64_min(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_f64_min, 0)
    }

    fn visit_f64_max(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_f64_max, 0)
    }

    fn visit_f64_copysign(&mut self) -> Self::Output {
        self.translate_binary(InstructionSet::op_f64_copysign, 0)
    }

    fn visit_i32_wrap_i64(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            //TODO: maybe add for registry
            // builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
            let popped_value = builder.alloc.stack_types.pop().unwrap();
            debug_assert_eq!(popped_value, ValType::I64);
            builder.alloc.stack_types.push(ValType::I32);
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.stack_height.pop1();
            Ok(())
        })
    }

    fn visit_i32_trunc_f32_s(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F32,
            ValType::I32,
            InstructionSet::op_i32_trunc_f32_s,
            0,
        )
    }

    fn visit_i32_trunc_f32_u(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F32,
            ValType::I32,
            InstructionSet::op_i32_trunc_f32_u,
            0,
        )
    }

    fn visit_i32_trunc_f64_s(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F64,
            ValType::I32,
            InstructionSet::op_i32_trunc_f64_s,
            0,
        )
    }

    fn visit_i32_trunc_f64_u(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F64,
            ValType::I32,
            InstructionSet::op_i32_trunc_f64_u,
            0,
        )
    }

    fn visit_i64_extend_i32_s(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::I32,
            ValType::I64,
            InstructionSet::op_i64_extend_i32_s,
            InstructionSet::MSH_I64_EXTEND_I32_S,
        )
    }

    fn visit_i64_extend_i32_u(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::I32,
            ValType::I64,
            InstructionSet::op_i64_extend_i32_u,
            InstructionSet::MSH_I64_EXTEND_I32_U,
        )
    }

    fn visit_i64_trunc_f32_s(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F32,
            ValType::I64,
            InstructionSet::op_i64_trunc_f32_s,
            0,
        )
    }

    fn visit_i64_trunc_f32_u(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F32,
            ValType::I64,
            InstructionSet::op_i64_trunc_f32_u,
            0,
        )
    }

    fn visit_i64_trunc_f64_s(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F64,
            ValType::I64,
            InstructionSet::op_i64_trunc_f64_s,
            0,
        )
    }

    fn visit_i64_trunc_f64_u(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F64,
            ValType::I64,
            InstructionSet::op_i64_trunc_f64_u,
            0,
        )
    }

    fn visit_f32_convert_i32_s(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::I32,
            ValType::F32,
            InstructionSet::op_f32_convert_i32_s,
            0,
        )
    }

    fn visit_f32_convert_i32_u(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::I32,
            ValType::F32,
            InstructionSet::op_f32_convert_i32_u,
            0,
        )
    }

    fn visit_f32_convert_i64_s(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::I64,
            ValType::F32,
            InstructionSet::op_f32_convert_i64_s,
            0,
        )
    }

    fn visit_f32_convert_i64_u(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::I64,
            ValType::F32,
            InstructionSet::op_f32_convert_i64_u,
            0,
        )
    }

    fn visit_f32_demote_f64(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F64,
            ValType::F32,
            InstructionSet::op_f32_demote_f64,
            0,
        )
    }

    fn visit_f64_convert_i32_s(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::I32,
            ValType::F64,
            InstructionSet::op_f64_convert_i32_s,
            0,
        )
    }

    fn visit_f64_convert_i32_u(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::I32,
            ValType::F64,
            InstructionSet::op_f64_convert_i32_u,
            0,
        )
    }

    fn visit_f64_convert_i64_s(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::I64,
            ValType::F64,
            InstructionSet::op_f64_convert_i64_s,
            0,
        )
    }

    fn visit_f64_convert_i64_u(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::I64,
            ValType::F64,
            InstructionSet::op_f64_convert_i64_u,
            0,
        )
    }

    fn visit_f64_promote_f32(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F32,
            ValType::F64,
            InstructionSet::op_f64_promote_f32,
            0,
        )
    }

    fn visit_i32_reinterpret_f32(&mut self) -> Self::Output {
        self.visit_reinterpret(ValType::F32, ValType::I32)
    }

    fn visit_i64_reinterpret_f64(&mut self) -> Self::Output {
        self.visit_reinterpret(ValType::F64, ValType::I64)
    }

    fn visit_f32_reinterpret_i32(&mut self) -> Self::Output {
        self.visit_reinterpret(ValType::I32, ValType::F32)
    }

    fn visit_f64_reinterpret_i64(&mut self) -> Self::Output {
        self.visit_reinterpret(ValType::I64, ValType::F64)
    }

    fn visit_i32_extend8_s(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_i32_extend8_s, 0)
    }

    fn visit_i32_extend16_s(&mut self) -> Self::Output {
        self.translate_unary(InstructionSet::op_i32_extend16_s, 0)
    }

    fn visit_i64_extend8_s(&mut self) -> Self::Output {
        self.translate_unary(
            InstructionSet::op_i64_extend8_s,
            InstructionSet::MSH_I64_EXTEND8_S,
        )
    }

    fn visit_i64_extend16_s(&mut self) -> Self::Output {
        self.translate_unary(
            InstructionSet::op_i64_extend16_s,
            InstructionSet::MSH_I64_EXTEND16_S,
        )
    }

    fn visit_i64_extend32_s(&mut self) -> Self::Output {
        self.translate_unary(
            InstructionSet::op_i64_extend32_s,
            InstructionSet::MSH_I64_EXTEND32_S,
        )
    }

    fn visit_i32_trunc_sat_f32_s(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F32,
            ValType::I32,
            InstructionSet::op_i32_trunc_sat_f32_s,
            0,
        )
    }

    fn visit_i32_trunc_sat_f32_u(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F32,
            ValType::I32,
            InstructionSet::op_i32_trunc_sat_f32_u,
            0,
        )
    }

    fn visit_i32_trunc_sat_f64_s(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F64,
            ValType::I32,
            InstructionSet::op_i32_trunc_sat_f64_s,
            0,
        )
    }

    fn visit_i32_trunc_sat_f64_u(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F64,
            ValType::I32,
            InstructionSet::op_i32_trunc_sat_f64_u,
            0,
        )
    }

    fn visit_i64_trunc_sat_f32_s(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F32,
            ValType::I64,
            InstructionSet::op_i64_trunc_sat_f32_s,
            0,
        )
    }

    fn visit_i64_trunc_sat_f32_u(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F32,
            ValType::I64,
            InstructionSet::op_i64_trunc_sat_f32_u,
            0,
        )
    }

    fn visit_i64_trunc_sat_f64_s(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F64,
            ValType::I64,
            InstructionSet::op_i64_trunc_sat_f64_s,
            0,
        )
    }

    fn visit_i64_trunc_sat_f64_u(&mut self) -> Self::Output {
        self.translate_conversion(
            ValType::F64,
            ValType::I64,
            InstructionSet::op_i64_trunc_sat_f64_u,
            0,
        )
    }

    fn visit_memory_init(&mut self, data_segment_index: u32, memory_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memory_index, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.pop().unwrap();
            let data_segment_index: DataSegmentIdx = data_segment_index.into();
            let is_fuel_metering_enabled = builder.is_fuel_metering_enabled();
            let (ib, rb) = (
                &mut builder.alloc.instruction_set,
                &mut builder.alloc.segment_builder,
            );
            let (offset, length) = rb
                .memory_sections
                .get(&data_segment_index)
                .copied()
                .expect("can't resolve a passive segment by index");
            builder.stack_height.push2();
            builder.stack_height.pop2();
            builder.stack_height.pop3();
            // since we store all data sections in the one segment, then the index is always 0
            ib.op_memory_init_checked(
                Some(offset),
                Some(length),
                data_segment_index + 1,
                is_fuel_metering_enabled,
            );
            Ok(())
        })
    }

    fn visit_data_drop(&mut self, data_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            // We do +1 here because we store all data sections in the one segment,
            // and we use 0 for a default memory segment.
            builder.alloc.instruction_set.op_data_drop(data_index + 1);
            Ok(())
        })
    }

    fn visit_memory_copy(&mut self, dst_memory_index: u32, src_memory_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(dst_memory_index, DEFAULT_MEMORY_INDEX);
            debug_assert_eq!(src_memory_index, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            builder.stack_height.push2();
            builder.stack_height.pop2();
            builder.stack_height.pop3();
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.pop().unwrap();
            builder
                .alloc
                .instruction_set
                .op_memory_copy_checked(builder.is_fuel_metering_enabled());
            Ok(())
        })
    }

    fn visit_memory_fill(&mut self, memory_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memory_index, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            builder.stack_height.push2();
            builder.stack_height.pop2();
            builder.stack_height.pop3();
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.pop().unwrap();
            builder
                .alloc
                .instruction_set
                .op_memory_fill_checked(builder.is_fuel_metering_enabled());
            Ok(())
        })
    }

    fn visit_table_init(&mut self, segment_index: u32, table_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            let is_fuel_metering_enabled = builder.is_fuel_metering_enabled();
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.pop().unwrap();
            let (ib, rb) = (
                &mut builder.alloc.instruction_set,
                &mut builder.alloc.segment_builder,
            );
            let elem_segment_index: ElementSegmentIdx = segment_index.into();
            let (offset, length) = rb
                .element_sections
                .get(&elem_segment_index)
                .copied()
                .expect("can't resolve an element segment by index");
            ib.op_table_init_checked(
                segment_index + 1,
                TableIdx::try_from(table_index).unwrap(),
                length,
                offset,
                is_fuel_metering_enabled,
            );
            builder
                .stack_height
                .push_n(InstructionSet::MSH_TABLE_INIT_CHECKED);
            builder
                .stack_height
                .pop_n(InstructionSet::MSH_TABLE_INIT_CHECKED);
            builder.stack_height.pop3();
            Ok(())
        })
    }

    fn visit_elem_drop(&mut self, segment_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            builder
                .alloc
                .instruction_set
                .op_elem_drop(segment_index + 1);
            Ok(())
        })
    }

    fn visit_table_copy(&mut self, dst_table: u32, src_table: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            let is_fuel_metering_enabled = builder.is_fuel_metering_enabled();
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            builder
                .stack_height
                .push_n(InstructionSet::MSH_TABLE_COPY_CHECKED);
            builder
                .stack_height
                .pop_n(InstructionSet::MSH_TABLE_COPY_CHECKED);
            builder.stack_height.pop3();
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.instruction_set.op_table_copy_checked(
                TableIdx::try_from(dst_table).unwrap(),
                TableIdx::try_from(src_table).unwrap(),
                is_fuel_metering_enabled,
            );
            Ok(())
        })
    }

    fn visit_table_fill(&mut self, table_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            let is_fuel_metering_enabled = builder.is_fuel_metering_enabled();
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            builder
                .stack_height
                .push_n(InstructionSet::MSH_TABLE_FILL_CHECKED);
            builder
                .stack_height
                .pop_n(InstructionSet::MSH_TABLE_FILL_CHECKED);
            builder.stack_height.pop3();
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.instruction_set.op_table_fill_checked(
                TableIdx::try_from(table_index).unwrap(),
                is_fuel_metering_enabled,
            );
            Ok(())
        })
    }

    fn visit_table_get(&mut self, table_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            let popped_type = builder.alloc.stack_types.pop().unwrap();
            debug_assert_eq!(popped_type, ValType::I32);
            builder.alloc.instruction_set.op_table_get(table_index);
            let table_type = builder.resolve_table_type(table_index);
            builder.alloc.stack_types.push(table_type.element_type);
            Ok(())
        })
    }

    fn visit_table_set(&mut self, table_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            builder.stack_height.pop2();
            //TODO: Do set and get for i32 x2 as i64
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.instruction_set.op_table_set(table_index);
            Ok(())
        })
    }

    fn visit_table_grow(&mut self, table_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            let is_fuel_metering_enabled = builder.is_fuel_metering_enabled();
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.push(ValType::I32);
            // for rWASM we inject table limit error check, if we exceed the number of allowed
            // elements, then we push `u32::MAX` on the stack that is equal to table
            // grow overflow error
            let table_type = builder.resolve_table_type(table_index);
            // TODO(dmitry123): "is this construction correct?"
            let max_table_elements = table_type.maximum.unwrap_or(N_MAX_TABLE_SIZE);
            let ib = &mut builder.alloc.instruction_set;
            ib.op_table_grow_checked(
                TableIdx::try_from(table_index).unwrap(),
                Some(max_table_elements),
                is_fuel_metering_enabled,
            );
            builder
                .stack_height
                .push_n(InstructionSet::MSH_TABLE_GROW_CHECKED);
            builder
                .stack_height
                .pop_n(InstructionSet::MSH_TABLE_GROW_CHECKED);
            builder.stack_height.pop2();
            builder.stack_height.push1();
            Ok(())
        })
    }

    fn visit_table_size(&mut self, table_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::ENTITY)?;
            builder.stack_height.push1();
            builder.alloc.stack_types.push(ValType::I32);
            builder.alloc.instruction_set.op_table_size(table_index);
            Ok(())
        })
    }

    fn visit_memory_discard(&mut self, _mem: u32) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_memory_atomic_notify(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_memory_atomic_wait32(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_memory_atomic_wait64(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_atomic_fence(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_load(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_load(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_load8_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_load16_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_load8_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_load16_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_load32_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_store(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_store(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_store8(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_store16(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_store8(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_store16(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_store32(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw_add(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw_add(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw8_add_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw16_add_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw8_add_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw16_add_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw32_add_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw_sub(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw_sub(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw8_sub_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw16_sub_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw8_sub_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw16_sub_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw32_sub_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw_and(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw_and(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw8_and_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw16_and_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw8_and_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw16_and_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw32_and_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw_or(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw_or(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw8_or_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw16_or_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw8_or_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw16_or_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw32_or_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw_xor(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw_xor(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw8_xor_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw16_xor_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw8_xor_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw16_xor_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw32_xor_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw_xchg(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw_xchg(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw8_xchg_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw16_xchg_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw8_xchg_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw16_xchg_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw32_xchg_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw_cmpxchg(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw_cmpxchg(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw8_cmpxchg_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32_atomic_rmw16_cmpxchg_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw8_cmpxchg_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw16_cmpxchg_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64_atomic_rmw32_cmpxchg_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load8x8_s(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load8x8_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load16x4_s(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load16x4_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load32x2_s(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load32x2_u(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load8_splat(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load16_splat(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load32_splat(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load64_splat(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load32_zero(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load64_zero(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_store(&mut self, _memarg: MemArg) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load8_lane(&mut self, _memarg: MemArg, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load16_lane(&mut self, _memarg: MemArg, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load32_lane(&mut self, _memarg: MemArg, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_load64_lane(&mut self, _memarg: MemArg, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_store8_lane(&mut self, _memarg: MemArg, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_store16_lane(&mut self, _memarg: MemArg, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_store32_lane(&mut self, _memarg: MemArg, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_store64_lane(&mut self, _memarg: MemArg, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_const(&mut self, _value: V128) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_shuffle(&mut self, _value: [u8; 16]) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_extract_lane_s(&mut self, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_extract_lane_u(&mut self, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_replace_lane(&mut self, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_extract_lane_s(&mut self, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_extract_lane_u(&mut self, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_replace_lane(&mut self, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_extract_lane(&mut self, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_replace_lane(&mut self, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_extract_lane(&mut self, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_replace_lane(&mut self, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_extract_lane(&mut self, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_replace_lane(&mut self, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_extract_lane(&mut self, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_replace_lane(&mut self, _lane: u8) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_swizzle(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_splat(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_splat(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_splat(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_splat(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_splat(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_splat(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_eq(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_ne(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_lt_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_lt_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_gt_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_gt_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_le_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_le_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_ge_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_ge_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_eq(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_ne(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_lt_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_lt_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_gt_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_gt_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_le_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_le_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_ge_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_ge_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_eq(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_ne(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_lt_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_lt_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_gt_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_gt_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_le_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_le_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_ge_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_ge_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_eq(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_ne(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_lt_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_gt_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_le_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_ge_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_eq(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_ne(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_lt(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_gt(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_le(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_ge(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_eq(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_ne(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_lt(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_gt(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_le(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_ge(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_not(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_and(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_andnot(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_or(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_xor(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_bitselect(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_v128_any_true(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_abs(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_neg(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_popcnt(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_all_true(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_bitmask(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_narrow_i16x8_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_narrow_i16x8_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_shl(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_shr_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_shr_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_add(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_add_sat_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_add_sat_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_sub(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_sub_sat_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_sub_sat_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_min_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_min_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_max_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_max_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_avgr_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_extadd_pairwise_i8x16_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_extadd_pairwise_i8x16_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_abs(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_neg(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_q15mulr_sat_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_all_true(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_bitmask(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_narrow_i32x4_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_narrow_i32x4_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_extend_low_i8x16_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_extend_high_i8x16_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_extend_low_i8x16_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_extend_high_i8x16_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_shl(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_shr_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_shr_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_add(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_add_sat_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_add_sat_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_sub(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_sub_sat_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_sub_sat_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_mul(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_min_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_min_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_max_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_max_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_avgr_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_extmul_low_i8x16_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_extmul_high_i8x16_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_extmul_low_i8x16_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_extmul_high_i8x16_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_extadd_pairwise_i16x8_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_extadd_pairwise_i16x8_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_abs(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_neg(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_all_true(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_bitmask(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_extend_low_i16x8_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_extend_high_i16x8_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_extend_low_i16x8_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_extend_high_i16x8_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_shl(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_shr_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_shr_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_add(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_sub(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_mul(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_min_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_min_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_max_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_max_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_dot_i16x8_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_extmul_low_i16x8_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_extmul_high_i16x8_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_extmul_low_i16x8_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_extmul_high_i16x8_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_abs(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_neg(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_all_true(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_bitmask(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_extend_low_i32x4_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_extend_high_i32x4_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_extend_low_i32x4_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_extend_high_i32x4_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_shl(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_shr_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_shr_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_add(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_sub(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_mul(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_extmul_low_i32x4_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_extmul_high_i32x4_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_extmul_low_i32x4_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_extmul_high_i32x4_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_ceil(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_floor(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_trunc(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_nearest(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_abs(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_neg(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_sqrt(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_add(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_sub(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_mul(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_div(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_min(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_max(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_pmin(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_pmax(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_ceil(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_floor(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_trunc(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_nearest(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_abs(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_neg(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_sqrt(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_add(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_sub(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_mul(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_div(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_min(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_max(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_pmin(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_pmax(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_trunc_sat_f32x4_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_trunc_sat_f32x4_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_convert_i32x4_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_convert_i32x4_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_trunc_sat_f64x2_s_zero(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_trunc_sat_f64x2_u_zero(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_convert_low_i32x4_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_convert_low_i32x4_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_demote_f64x2_zero(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_promote_low_f32x4(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_relaxed_swizzle(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_relaxed_trunc_sat_f32x4_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_relaxed_trunc_sat_f32x4_u(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_relaxed_trunc_sat_f64x2_s_zero(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_relaxed_trunc_sat_f64x2_u_zero(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_relaxed_fma(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_relaxed_fnma(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_relaxed_fma(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_relaxed_fnma(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i8x16_relaxed_laneselect(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_relaxed_laneselect(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_relaxed_laneselect(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i64x2_relaxed_laneselect(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_relaxed_min(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_relaxed_max(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_relaxed_min(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f64x2_relaxed_max(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_relaxed_q15mulr_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i16x8_dot_i8x16_i7x16_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_i32x4_dot_i8x16_i7x16_add_s(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_f32x4_relaxed_dot_bf16x8_add_f32x4(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }
}

impl InstructionTranslator {
    fn translate_load(
        &mut self,
        memarg: MemArg,
        loaded_type: ValType,
        emitter: fn(&mut InstructionSet, offset: AddressOffset),
        max_stack_height: u32,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(|| FuelCosts::LOAD)?;
            let addr_type = builder.alloc.stack_types.pop().unwrap();
            debug_assert_eq!(addr_type, ValType::I32);
            builder.stack_height.push_n(max_stack_height);
            builder.stack_height.pop_type(addr_type);
            builder.stack_height.push_type(loaded_type);
            builder.stack_height.pop_n(max_stack_height);
            builder.alloc.stack_types.push(loaded_type);
            let offset = AddressOffset::from(memarg.offset as u32);
            emitter(&mut builder.alloc.instruction_set, offset);
            Ok(())
        })
    }

    fn translate_store(
        &mut self,
        memarg: MemArg,
        stored_value: ValType,
        emitter: fn(&mut InstructionSet, offset: AddressOffset),
        max_stack_height: u32,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(|| FuelCosts::STORE)?;
            builder.stack_height.push_n(max_stack_height);
            let value_type = builder.alloc.stack_types.pop().unwrap();
            debug_assert_eq!(value_type, stored_value);
            builder.stack_height.pop_type(value_type);
            let addr_type = builder.alloc.stack_types.pop().unwrap();
            debug_assert_eq!(addr_type, ValType::I32);
            builder.stack_height.pop_type(addr_type);
            builder.stack_height.pop_n(max_stack_height);
            let offset = AddressOffset::from(memarg.offset as u32);
            emitter(&mut builder.alloc.instruction_set, offset);
            Ok(())
        })
    }

    fn translate_conversion(
        &mut self,
        input_type: ValType,
        output_type: ValType,
        emitter: fn(&mut InstructionSet),
        max_stack_height: u32,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
            if max_stack_height > 0 {
                builder.stack_height.push_n(max_stack_height);
            }
            let lhs_type = builder.alloc.stack_types.pop().unwrap();
            debug_assert_eq!(lhs_type, input_type);
            builder.alloc.stack_types.push(output_type);
            builder.stack_height.pop_type(input_type);
            builder.stack_height.push_type(output_type);
            if max_stack_height > 0 {
                builder.stack_height.pop_n(max_stack_height);
            }
            emitter(&mut builder.alloc.instruction_set);
            Ok(())
        })
    }

    #[allow(dead_code)]
    fn visit_reinterpret(
        &mut self,
        input_type: ValType,
        output_type: ValType,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            let lhs_type = builder.alloc.stack_types.pop().unwrap();
            debug_assert_eq!(lhs_type, input_type);
            builder.alloc.stack_types.push(output_type);
            // calc stack height
            builder.stack_height.pop_type(input_type);
            builder.stack_height.push_type(output_type);
            Ok(())
        })
    }

    fn translate_binary(
        &mut self,
        emitter: fn(&mut InstructionSet),
        max_stack_height: u32,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
            // calculate the type stack and make sure params are correct
            let lhs_type = builder.alloc.stack_types.pop().unwrap();
            let rhs_type = builder.alloc.stack_types.pop().unwrap();
            debug_assert_eq!(lhs_type, rhs_type);
            builder.alloc.stack_types.push(lhs_type);
            // calculate max stack height
            if max_stack_height > 0 {
                builder.stack_height.push_n(max_stack_height);
            }
            builder.stack_height.pop_type(lhs_type);
            builder.stack_height.pop_type(lhs_type);
            builder.stack_height.push_type(lhs_type);
            if max_stack_height > 0 {
                builder.stack_height.pop_n(max_stack_height);
            }
            // emit an instruction
            emitter(&mut builder.alloc.instruction_set);
            Ok(())
        })
    }

    fn translate_to_snippet_call(&mut self, snippet: Snippet) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
            builder
                .stack_height
                .pop_n(snippet.func_type().params().len() as u32);
            builder
                .stack_height
                .push_n(snippet.func_type().results().len() as u32);
            for func_type in snippet.orig_func_type().params().iter().rev() {
                let popped_type = builder.alloc.stack_types.pop().unwrap();
                assert_eq!(*func_type, popped_type)
            }
            for result in snippet.orig_func_type().results() {
                builder.alloc.stack_types.push(*result);
            }
            let loc = builder.alloc.instruction_set.loc();
            builder
                .alloc
                .instruction_set
                .op_call_internal(SNIPPET_FUNC_IDX_UNRESOLVED);
            builder
                .alloc
                .snippet_calls
                .push(SnippetCall { loc, snippet });
            Ok(())
        })
    }

    fn translate_binary_compare(
        &mut self,
        emitter: fn(&mut InstructionSet),
        max_stack_height: u32,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
            let lhs_type = builder.alloc.stack_types.pop().unwrap();
            let rhs_type = builder.alloc.stack_types.pop().unwrap();
            debug_assert_eq!(lhs_type, rhs_type);
            builder.alloc.stack_types.push(ValType::I32);
            // do stack height check
            if max_stack_height > 0 {
                builder.stack_height.push_n(max_stack_height);
            }
            builder.stack_height.pop_type(lhs_type);
            builder.stack_height.pop_type(rhs_type);
            builder.stack_height.push_type(ValType::I32);
            if max_stack_height > 0 {
                builder.stack_height.pop_n(max_stack_height);
            }
            // emit an opcode
            emitter(&mut builder.alloc.instruction_set);
            Ok(())
        })
    }

    fn translate_unary(
        &mut self,
        emitter: fn(&mut InstructionSet),
        max_stack_height: u32,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
            // calc the type stack
            let lhs_type = builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.push(lhs_type);
            // calc stack height
            builder.stack_height.push_n(max_stack_height);
            builder.stack_height.pop_type(lhs_type);
            builder.stack_height.push_type(lhs_type);
            builder.stack_height.pop_n(max_stack_height);
            // emit instruction
            emitter(&mut builder.alloc.instruction_set);
            Ok(())
        })
    }

    fn translate_unary_compare(
        &mut self,
        emitter: fn(&mut InstructionSet),
        max_stack_height: u32,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(|| FuelCosts::BASE)?;
            // check the type stack
            let lsh_type = builder.alloc.stack_types.pop().unwrap();
            builder.alloc.stack_types.push(ValType::I32);
            // calc stack height
            builder.stack_height.push_n(max_stack_height);
            builder.stack_height.pop_type(lsh_type);
            builder.stack_height.push_type(ValType::I32);
            builder.stack_height.pop_n(max_stack_height);
            // emit opcode
            emitter(&mut builder.alloc.instruction_set);
            Ok(())
        })
    }
}
