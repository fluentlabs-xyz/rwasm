use crate::{
    compiler::{
        control_flow::{
            BlockControlFrame,
            ControlFlowStack,
            ControlFrame,
            ControlFrameKind,
            IfControlFrame,
            LoopControlFrame,
            UnreachableControlFrame,
        },
        drop_keep::translate_drop_keep,
        error::CompilationError,
        instr_loc::InstrLoc,
        labels::{LabelRef, LabelRegistry},
        locals_registry::LocalsRegistry,
        segment_builder::SegmentBuilder,
        utils::RelativeDepth,
        value_stack::ValueStackHeight,
    },
    split_i64_to_i32,
    AddressOffset,
    BranchOffset,
    BranchTableTargets,
    DataSegmentIdx,
    DropKeep,
    ElementSegmentIdx,
    FuelCosts,
    FuncIdx,
    FuncTypeIdx,
    GlobalVariable,
    ImportLinkerEntity,
    InstructionSet,
    Opcode,
    OpcodeData,
    DEFAULT_MEMORY_INDEX,
    F32,
    N_MAX_MEMORY_PAGES,
    N_MAX_TABLE_ELEMENTS,
};
use hashbrown::HashMap;
use wasmparser::{
    BlockType,
    BrTable,
    FuncType,
    FuncValidatorAllocations,
    GlobalType,
    Ieee32,
    Ieee64,
    MemArg,
    MemoryType,
    TableType,
    ValType,
    VisitOperator,
    V128,
};

/// Reusable allocations of a [`FuncTranslator`].
#[derive(Debug, Default)]
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

    pub(crate) func_types: Vec<FuncType>,
    pub(crate) imported_funcs: Vec<(ImportLinkerEntity, FuncTypeIdx)>,
    pub(crate) compiled_funcs: Vec<FuncTypeIdx>,
    pub(crate) tables: Vec<TableType>,
    pub(crate) memories: Vec<MemoryType>,
    pub(crate) globals: Vec<GlobalVariable>,
    pub(crate) exported_funcs: HashMap<Box<str>, FuncIdx>,
    pub(crate) start_func: Option<FuncIdx>,
    pub(crate) func_offsets: Vec<u32>,
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
        let func_index = match block_type {
            BlockType::FuncType(func_index) => func_index,
            BlockType::Empty | BlockType::Type(_) => return 0,
        };
        self.resolve_func_type_ref(func_index, |func_type| func_type.params().len())
    }

    pub(crate) fn resolve_func_results_len_type_by_block(&self, block_type: BlockType) -> usize {
        let func_index = match block_type {
            BlockType::FuncType(func_index) => func_index,
            BlockType::Empty | BlockType::Type(_) => return 0,
        };
        self.resolve_func_type_ref(func_index, |func_type| func_type.results().len())
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
        let is_imported_func = func_idx.to_u32() < self.imported_funcs.len() as u32;
        if is_imported_func {
            self.imported_funcs
                .get(func_idx.to_u32() as usize)
                .map(|v| v.1)
                .unwrap()
        } else {
            let len_imports = self.imported_funcs.len();
            self.compiled_funcs
                .get(func_idx.to_u32() as usize - len_imports)
                .copied()
                .unwrap()
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
    /// The `fuel_costs` field represents an instance of the `FuelCosts` structure.
    /// This field is used to store and manage data related to the costs associated
    /// with fuel consumption, such as pricing or expenditure for a particular use case.
    pub(crate) fuel_costs: FuelCosts,
    /// Stores and resolves local variable types.
    pub(crate) locals: LocalsRegistry,
}

impl InstructionTranslator {
    pub fn new(alloc: FuncTranslatorAllocations) -> Self {
        Self {
            reachable: true,
            alloc,
            stack_height: Default::default(),
            fuel_costs: Default::default(),
            locals: Default::default(),
        }
    }

    /// Returns `true` if the code at the current translation position is reachable.
    fn is_reachable(&self) -> bool {
        self.reachable
    }

    /// Returns the current instruction pointer as an index.
    pub fn current_pc(&self) -> InstrLoc {
        InstrLoc::from_usize(self.alloc.instruction_set.len())
    }

    /// Registers the `block` control frame surrounding the entire function body.
    fn init_func_body_block(&mut self, func_idx: FuncIdx) {
        let block_type = BlockType::FuncType(func_idx.to_u32());
        let end_label = self.alloc.labels.new_label();
        let consume_fuel = self
            .is_fuel_metering_enabled()
            .then(|| self.push_consume_fuel_base());
        let block_frame = BlockControlFrame::new(block_type, end_label, 0, consume_fuel);
        self.alloc.control_frames.push_frame(block_frame);
        let func_type_idx = self.alloc.resolve_func_type_index(func_idx);
        let func_type = self.alloc.func_types.get(func_type_idx as usize).unwrap();
        // TODO(dmitry123): "use original params here?"
        self.alloc.stack_types.extend(func_type.params());
        let func_params_len = func_type.params().len();
        for _ in 0..func_params_len {
            self.locals.register_locals(1);
        }
    }

    /// Resolve the label at the current instruction position.
    ///
    /// Does nothing if the label has already been resolved.
    ///
    /// # Note
    ///
    /// This is used at a position of the Wasm bytecode where it is clear that
    /// the given label can be resolved properly.
    /// This usually takes place when encountering the Wasm `End` operand, for example.
    pub fn pin_label_if_unpinned(&mut self, label: LabelRef) {
        self.alloc.labels.try_pin_label(label, self.current_pc())
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
        let base_fuel = u32::try_from(self.fuel_costs.base).expect("base fuel exceeds u32 size");
        let instr_loc = self.alloc.instruction_set.op_consume_fuel(base_fuel);
        InstrLoc::from_u32(instr_loc)
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
    pub(crate) fn bump_fuel_consumption(&mut self, delta: u64) -> Result<(), CompilationError> {
        if let Some(instr) = self.consume_fuel_instr() {
            self.alloc
                .instruction_set
                .bump_fuel_consumption(instr.into_u32(), delta)?;
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
        true
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
        // if func offsets are empty, then let's reserve the first element under the entrypoint,
        // but since we don't know the length of entrypoint yet then we will have to
        // recalculate all offsets later
        if self.alloc.func_offsets.is_empty() {
            self.alloc.func_offsets.push(0);
        }
        self.alloc.func_offsets.push(func_offset);
        self.init_func_body_block(func_idx);
        Ok(())
    }

    /// Finishes constructing the function and returns its [`CompiledFunc`].
    pub fn finish(&mut self) -> Result<(), CompilationError> {
        // update branch offsets in `Branch` opcodes
        for (user, offset) in self.alloc.labels.resolved_users() {
            self.alloc.instruction_set.instr[user.into_usize()]
                .1
                .update_branch_offset(offset?);
        }
        // update max stack height in `StackAlloc` opcode
        let mut iter = self.alloc.instruction_set.instr.iter_mut().take(3);
        loop {
            let opcode = iter.next().unwrap();
            match opcode {
                (Opcode::ConsumeFuel, _) | (Opcode::SignatureCheck, _) => {}
                (Opcode::StackCheck, OpcodeData::StackAlloc(stack_alloc)) => {
                    stack_alloc.max_stack_height = self.stack_height.max_stack_height();
                    break;
                }
                _ => unreachable!("rwasm: not allowed opcode"),
            }
        }
        Ok(())
    }

    pub(crate) fn resolve_global_type(&self, _global_index: u32) -> GlobalType {
        todo!()
    }

    pub(crate) fn resolve_memory_type(&self, _memory_index: u32) -> MemoryType {
        todo!()
    }

    pub(crate) fn resolve_table_type(&self, _table_index: u32) -> TableType {
        todo!()
    }

    pub(crate) fn get_compiled_func(&self, _function_index: u32) -> Option<u32> {
        todo!()
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

    fn fuel_costs(&self) -> &FuelCosts {
        &self.fuel_costs
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
            .map(|t| if t == &ValType::I64 { 2 } else { 1 })
            .sum()
    }

    /// Adjusts the emulated value stack given the [`rwasm_legacy::FuncType`] of the call.
    fn adjust_value_stack_for_call(&mut self, func_type_idx: FuncTypeIdx) {
        let func_type = self.alloc.func_types.get(func_type_idx as usize).unwrap();
        let params = func_type.params();
        self.stack_height.pop_n(params.len() as u32);
        for _ in params {
            self.alloc.stack_types.pop();
        }
        let results = func_type.results();
        self.stack_height.push_n(results.len() as u32);
        for result in results {
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
        self.alloc.instruction_set.op_unreachable();
        Ok(())
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
                .then(|| self.push_consume_fuel_base());
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
            self.bump_fuel_consumption(self.fuel_costs.base)?;
            let branch_offset = self.branch_offset(else_label)?;
            self.alloc.instruction_set.op_br_if_eqz(branch_offset);
            let consume_fuel = self
                .is_fuel_metering_enabled()
                .then(|| self.push_consume_fuel_base());
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
            self.bump_fuel_consumption(self.fuel_costs.base)?;
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
                Some(_) => old_stack_height -= 1,
                None => panic!("Stack corrupted in else block"),
            }
        }
        match if_frame.block_type() {
            BlockType::FuncType(func_type_idx) => {
                let func_type = self.alloc.func_types.get(func_type_idx as usize).unwrap();
                func_type.params().iter().for_each(|param| {
                    if *param == ValType::I64 {
                        self.stack_height.push_n(2);
                    } else {
                        self.stack_height.push();
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
        // These bindings are required because of borrowing issues.
        let frame_reachable = frame.is_reachable();
        let frame_stack_height = frame.stack_height();
        if self.alloc.control_frames.len() == 1 {
            // If the control flow frames stack is empty after this point,
            // we know that we are ending the function body `block`
            // frame, and therefore we have to return from the function.
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
                match self.alloc.stack_types.pop() {
                    Some(ValType::I64) => old_stack_height -= 2,
                    Some(_) => old_stack_height -= 1,
                    None => panic!("type stack corrupted"),
                }
            }
        }
        let frame = self.alloc.control_frames.pop_frame();
        match frame.block_type() {
            BlockType::FuncType(func_type_idx) => {
                let func_type = self.alloc.func_types.get(func_type_idx as usize).unwrap();
                func_type.params().iter().for_each(|param| {
                    if *param == ValType::I64 {
                        self.stack_height.push_n(2);
                    } else {
                        self.stack_height.push();
                    }
                    self.alloc.stack_types.push(*param);
                });
            }
            _ => {}
        }
        Ok(())
    }

    fn visit_br(&mut self, relative_depth: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            match builder.acquire_target(relative_depth)? {
                AcquiredTarget::Branch(end_label, drop_keep) => {
                    builder.bump_fuel_consumption(builder.fuel_costs().base)?;
                    translate_drop_keep(
                        &mut builder.alloc.instruction_set,
                        drop_keep,
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
            builder.alloc.stack_types.pop();
            match builder.acquire_target(relative_depth)? {
                AcquiredTarget::Branch(end_label, drop_keep) => {
                    builder.bump_fuel_consumption(builder.fuel_costs().base)?;
                    if drop_keep.is_noop() {
                        let offset = builder.branch_offset(end_label)?;
                        builder.alloc.instruction_set.op_br_if_nez(offset);
                    } else {
                        builder.bump_fuel_consumption(
                            builder.fuel_costs().fuel_for_drop_keep(drop_keep),
                        )?;
                        builder
                            .alloc
                            .instruction_set
                            .op_br_if_eqz(BranchOffset::uninit());
                        let drop_keep_length = translate_drop_keep(
                            &mut builder.alloc.instruction_set,
                            drop_keep,
                            &mut builder.stack_height,
                        );
                        builder
                            .alloc
                            .instruction_set
                            .last_nth_mut(drop_keep_length)
                            .map(|v| &mut v.1)
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
                    let drop_keep_length = translate_drop_keep(
                        &mut builder.alloc.instruction_set,
                        drop_keep,
                        &mut builder.stack_height,
                    );
                    builder
                        .alloc
                        .instruction_set
                        .last_nth_mut(drop_keep_length)
                        .map(|v| &mut v.1)
                        .unwrap()
                        .update_branch_offset(drop_keep_length as i32 + 2);
                    builder.alloc.instruction_set.op_return(DropKeep::none());
                }
            }
            Ok(())
        })
    }

    fn visit_br_table(&mut self, targets: BrTable<'a>) -> Self::Output {
        #[derive(Debug, Copy, Clone)]
        enum BrTableTarget {
            Br(BranchOffset, DropKeep),
            Return(DropKeep),
        }

        self.translate_if_reachable(|builder| {
            fn offset_instr(base: InstrLoc, offset: usize) -> InstrLoc {
                InstrLoc::from_u32(base.into_u32() + offset as u32)
            }

            fn compute_instr(
                builder: &mut InstructionTranslator,
                n: usize,
                depth: RelativeDepth,
                max_drop_keep_fuel: &mut u64,
            ) -> Result<BrTableTarget, CompilationError> {
                match builder.acquire_target(depth.into_u32())? {
                    AcquiredTarget::Branch(label, drop_keep) => {
                        *max_drop_keep_fuel = (*max_drop_keep_fuel)
                            .max(builder.fuel_costs().fuel_for_drop_keep(drop_keep));
                        let base = builder.current_pc();
                        let instr = offset_instr(base, 2 * n + 1);
                        let offset = builder.try_resolve_label_for(label, instr)?;
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
            fn encode_br_table_target(stream: &mut InstructionSet, target: BrTableTarget) {
                match target {
                    BrTableTarget::Br(offset, drop_keep) => {
                        // Case: We push a `Br` followed by a `Return` as usual.
                        stream.op_br_adjust(offset);
                        stream.op_return(drop_keep);
                    }
                    BrTableTarget::Return(drop_keep) => {
                        // Case: We push `Return` two times to make all branch targets use 2
                        // instruction words.       This is important to
                        // make `br_table` dispatch efficient.
                        stream.op_return(drop_keep);
                        stream.op_return(drop_keep);
                    }
                }
            }

            let default = RelativeDepth::from_u32(targets.default());
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

            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            // The maximum fuel costs among all `br_table` arms.
            // We use this to charge fuel once at the entry of a `br_table`
            // for the most expensive arm of all of its arms.
            let mut max_drop_keep_fuel = 0;

            builder.stack_height.pop1();
            builder.alloc.stack_types.pop();

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
            builder.alloc.instruction_set.op_br_table(len_targets);
            encode_br_table_target(&mut builder.alloc.br_table_branches, default_branch);
            for branch in builder.alloc.br_table_branches.drain(..) {
                builder.alloc.instruction_set.push(branch.0, branch.1);
            }
            builder.bump_fuel_consumption(max_drop_keep_fuel)?;
            builder.reachable = false;
            Ok(())
        })
    }

    fn visit_return(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            let drop_keep = builder.drop_keep_return()?;
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.bump_fuel_consumption(builder.fuel_costs().fuel_for_drop_keep(drop_keep))?;
            translate_drop_keep(
                &mut builder.alloc.instruction_set,
                drop_keep,
                &mut builder.stack_height,
            );
            builder.alloc.instruction_set.op_return(DropKeep::none());
            builder.reachable = false;
            Ok(())
        })
    }

    fn visit_call(&mut self, function_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().call)?;
            let func_type_idx = builder.alloc.resolve_func_type_index(function_index);
            builder.adjust_value_stack_for_call(func_type_idx);
            match builder.get_compiled_func(function_index) {
                Some(compiled_func) => {
                    // Case: We are calling an internal function and can optimize
                    //       this case by using the special instruction for it.
                    builder
                        .alloc
                        .instruction_set
                        .op_call_internal(compiled_func);
                }
                None => {
                    // Case: We are calling an imported function and must use the
                    //       general calling operator for it.
                    builder.alloc.instruction_set.op_call(function_index);
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
    ) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().call)?;
            builder.stack_height.pop1();
            builder.alloc.stack_types.pop();
            builder.adjust_value_stack_for_call(func_type_index as FuncTypeIdx);
            builder
                .alloc
                .instruction_set
                .op_call_indirect(func_type_index);
            builder.alloc.instruction_set.op_table_get(table_index);
            Ok(())
        })
    }

    fn visit_return_call(&mut self, _function_index: u32) -> Self::Output {
        unimplemented!()
    }

    fn visit_return_call_indirect(
        &mut self,
        _func_type_index: u32,
        _table_index: u32,
    ) -> Self::Output {
        unimplemented!()
    }

    fn visit_delegate(&mut self, _relative_depth: u32) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_catch_all(&mut self) -> Self::Output {
        Err(CompilationError::NotSupportedExtension)
    }

    fn visit_drop(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.pop1();
            let item_type = builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_drop();
            if item_type == Some(ValType::I64) {
                builder.stack_height.pop1();
                builder.alloc.instruction_set.op_drop();
            }
            Ok(())
        })
    }

    fn visit_select(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.pop3();
            builder.stack_height.push();
            builder.alloc.stack_types.pop();
            let item = builder.alloc.stack_types.pop().unwrap();
            if item == ValType::I64 {
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
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            let local_depth = builder.relative_local_depth(local_index);
            let value =
                builder.alloc.stack_types[builder.alloc.stack_types.len() - local_depth as usize];
            let expressed_depth = builder.get_expressed_depth(local_depth);
            builder.alloc.instruction_set.op_local_get(expressed_depth);
            builder.stack_height.push();
            if value == ValType::I64 {
                builder.alloc.instruction_set.op_local_get(expressed_depth);
                builder.stack_height.push();
            }
            builder.alloc.stack_types.push(value);
            Ok(())
        })
    }

    fn visit_local_set(&mut self, local_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.pop1();
            let value_type = builder.alloc.stack_types.pop().unwrap();
            let local_depth = builder.relative_local_depth(local_index);
            let expressed_depth = builder.get_expressed_depth(local_depth);
            builder.alloc.instruction_set.op_local_set(expressed_depth);
            if value_type == ValType::I64 {
                builder.alloc.instruction_set.op_local_set(expressed_depth);
                builder.stack_height.pop1();
            }
            Ok(())
        })
    }

    fn visit_local_tee(&mut self, local_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            let local_depth = builder.relative_local_depth(local_index);
            let expressed_depth = builder.get_expressed_depth(local_depth);
            let value_type = builder.alloc.stack_types.last().unwrap();
            if *value_type == ValType::I64 {
                builder.stack_height.push();
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
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            builder.stack_height.push();
            let global_type = builder.resolve_global_type(global_index);
            builder.alloc.stack_types.push(global_type.content_type);
            if global_type.content_type == ValType::I64 {
                builder
                    .alloc
                    .instruction_set
                    .op_global_get(global_index * 2 + 1);
            } else {
                builder
                    .alloc
                    .instruction_set
                    .op_global_get(global_index * 2);
            }
            builder.stack_height.push();
            Ok(())
        })
    }

    fn visit_global_set(&mut self, global_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            let global_type = builder.resolve_global_type(global_index);
            debug_assert!(global_type.mutable);
            builder.stack_height.pop1();
            builder.alloc.stack_types.pop();
            builder
                .alloc
                .instruction_set
                .op_global_set(global_index * 2);
            if global_type.content_type == ValType::I64 {
                builder
                    .alloc
                    .instruction_set
                    .op_local_set(global_index * 2 + 1);
                builder.stack_height.pop1();
            }
            Ok(())
        })
    }

    fn visit_i32_load(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(memarg, ValType::I32, Opcode::I32Load)
    }

    fn visit_i64_load(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I64);
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            let offset = AddressOffset::from(memarg.offset as u32);
            builder.alloc.instruction_set.op_local_get(1);
            builder
                .alloc
                .instruction_set
                .op_i32_load(AddressOffset::from(
                    offset.into_inner().checked_add(4).unwrap_or(u32::MAX),
                ));
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_load(offset);
            builder.alloc.instruction_set.op_local_set(2);
            Ok(())
        })
    }

    fn visit_f32_load(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(memarg, ValType::F32, Opcode::F32Load)
    }

    fn visit_f64_load(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(memarg, ValType::F64, Opcode::F64Load)
    }

    fn visit_i32_load8_s(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(memarg, ValType::I32, Opcode::I32Load8S)
    }

    fn visit_i32_load8_u(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(memarg, ValType::I32, Opcode::I32Load8U)
    }

    fn visit_i32_load16_s(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(memarg, ValType::I32, Opcode::I32Load16S)
    }

    fn visit_i32_load16_u(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_load(memarg, ValType::I32, Opcode::I32Load16U)
    }

    fn visit_i64_load8_s(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I64);
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder
                .alloc
                .instruction_set
                .op_i32_load8_s(memarg.offset as u32);
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_clz();
            builder.alloc.instruction_set.op_br_if_eqz(3);
            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.instruction_set.op_br(2);
            builder.alloc.instruction_set.op_i32_const(-1);
            Ok(())
        })
    }

    fn visit_i64_load8_u(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_height.push();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I64);
            let offset = AddressOffset::from(memarg.offset as u32);
            builder.alloc.instruction_set.op_i32_load8_u(offset);
            builder.alloc.instruction_set.op_i32_const(0);
            Ok(())
        })
    }

    fn visit_i64_load16_s(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I64);
            let offset = AddressOffset::from(memarg.offset as u32);
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.alloc.instruction_set.op_i32_load16_s(offset);
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_clz();
            builder.alloc.instruction_set.op_br_if_eqz(3);
            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.instruction_set.op_br(2);
            builder.alloc.instruction_set.op_i32_const(-1);
            Ok(())
        })
    }

    fn visit_i64_load16_u(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_height.push();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I64);
            let offset = AddressOffset::from(memarg.offset as u32);
            builder.alloc.instruction_set.op_i32_load16_u(offset);
            builder.alloc.instruction_set.op_i32_const(0);
            Ok(())
        })
    }

    fn visit_i64_load32_s(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I64);
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder
                .alloc
                .instruction_set
                .op_i32_load(memarg.offset as u32);
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_clz();
            builder.alloc.instruction_set.op_br_if_eqz(3);
            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.instruction_set.op_br(2);
            builder.alloc.instruction_set.op_i32_const(-1);
            Ok(())
        })
    }

    fn visit_i64_load32_u(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I64);
            let offset = AddressOffset::from(memarg.offset as u32);
            builder.alloc.instruction_set.op_i32_load(offset);
            builder.alloc.instruction_set.op_i32_const(0);
            Ok(())
        })
    }

    fn visit_i32_store(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_store(memarg, ValType::I32, Opcode::I32Store)
    }

    fn visit_i64_store(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            let offset = AddressOffset::from(memarg.offset as u32);
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.pop1();
            builder.stack_height.pop2();
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder
                .alloc
                .instruction_set
                .op_i32_store(offset.into_inner().checked_add(4).unwrap_or(u32::MAX));
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_i32_store(offset);
            Ok(())
        })
    }

    fn visit_f32_store(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_store(memarg, ValType::F32, Opcode::F32Store)
    }

    fn visit_f64_store(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_store(memarg, ValType::F64, Opcode::F64Store)
    }

    fn visit_i32_store8(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_store(memarg, ValType::I32, Opcode::I32Store8)
    }

    fn visit_i32_store16(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_store(memarg, ValType::I32, Opcode::I32Store16)
    }

    fn visit_i64_store8(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_height.pop3();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            let offset = AddressOffset::from(memarg.offset as u32);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_i32_store8(offset);
            Ok(())
        })
    }

    fn visit_i64_store16(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_height.pop3();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            let offset = AddressOffset::from(memarg.offset as u32);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_i32_store16(offset);
            Ok(())
        })
    }

    fn visit_i64_store32(&mut self, memarg: MemArg) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_height.pop3();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            let offset = AddressOffset::from(memarg.offset as u32);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_i32_store(offset);
            Ok(())
        })
    }

    fn visit_memory_size(&mut self, memory_index: u32, _mem_byte: u8) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            debug_assert_eq!(memory_index, DEFAULT_MEMORY_INDEX);
            builder.stack_height.push();
            builder.alloc.stack_types.push(ValType::I32);
            builder.alloc.instruction_set.op_memory_size();
            Ok(())
        })
    }

    fn visit_memory_grow(&mut self, memory_index: u32, _mem_byte: u8) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memory_index, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            // for rWASM, we inject memory limit error check, if we exceed the number of allowed
            // pages, then we push `u32::MAX` value on the stack that is equal to memory grow
            // overflow error
            let memory_type = builder.resolve_memory_type(memory_index);
            let max_pages = memory_type
                .maximum
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(N_MAX_MEMORY_PAGES);
            builder.alloc.instruction_set.op_local_get(1);
            builder.stack_height.push();
            builder.alloc.instruction_set.op_memory_size();
            builder.stack_height.push();
            builder.alloc.instruction_set.op_i32_add();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.alloc.instruction_set.op_i32_const(max_pages);
            builder.stack_height.push();
            builder.alloc.instruction_set.op_i32_gt_s();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.alloc.instruction_set.op_br_if_eqz(4);
            builder.stack_height.pop1();
            builder.alloc.instruction_set.op_drop();
            builder.stack_height.pop1();
            builder.alloc.instruction_set.op_i32_const(u32::MAX);
            builder.stack_height.push();
            builder.alloc.instruction_set.op_br(2);
            builder.alloc.instruction_set.op_memory_grow();
            builder.stack_height.pop1();
            builder.stack_height.push();
            Ok(())
        })
    }

    fn visit_i32_const(&mut self, value: i32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.push();
            builder.alloc.stack_types.push(ValType::I32);
            builder.alloc.instruction_set.op_i32_const(value);
            Ok(())
        })
    }

    fn visit_i64_const(&mut self, value: i64) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.push();
            builder.stack_height.push();
            builder.alloc.stack_types.push(ValType::I64);
            let (expected_low, expected_high) = split_i64_to_i32(value);
            builder.alloc.instruction_set.op_i32_const(expected_low);
            builder.alloc.instruction_set.op_i32_const(expected_high);
            Ok(())
        })
    }

    fn visit_f32_const(&mut self, value: Ieee32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.push();
            builder.alloc.stack_types.push(ValType::F32);
            builder
                .alloc
                .instruction_set
                .op_f32_const(F32::from(value.bits()));
            Ok(())
        })
    }

    fn visit_f64_const(&mut self, value: Ieee64) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.push();
            builder.alloc.stack_types.push(ValType::F64);
            builder.alloc.instruction_set.op_f64_const(value.bits());
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
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.instruction_set.op_ref_func(function_index);
            builder.stack_height.push();
            builder.alloc.stack_types.push(ValType::FuncRef);
            Ok(())
        })
    }

    fn visit_i32_eqz(&mut self) -> Self::Output {
        self.translate_unary_cmp(ValType::I32, Opcode::I32Eqz)
    }

    fn visit_i32_eq(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::I32, Opcode::I32Eq)
    }

    fn visit_i32_ne(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::I32, Opcode::I32Ne)
    }

    fn visit_i32_lt_s(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::I32, Opcode::I32LtS)
    }

    fn visit_i32_lt_u(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::I32, Opcode::I32LtU)
    }

    fn visit_i32_gt_s(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::I32, Opcode::I32GtS)
    }

    fn visit_i32_gt_u(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::I32, Opcode::I32GtU)
    }

    fn visit_i32_le_s(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::I32, Opcode::I32LeS)
    }

    fn visit_i32_le_u(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::I32, Opcode::I32LeU)
    }

    fn visit_i32_ge_s(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::I32, Opcode::I32GeS)
    }

    fn visit_i32_ge_u(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::I32, Opcode::I32GeU)
    }

    fn visit_i64_eqz(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.instruction_set.op_i32_eqz();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.alloc.instruction_set.op_local_get(2);
            builder.stack_height.push();
            builder.alloc.instruction_set.op_i32_eqz();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.alloc.instruction_set.op_local_set(2);
            builder.stack_height.pop2();
            builder.alloc.instruction_set.op_i32_add();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I32);
            Ok(())
        })
    }

    fn visit_i64_eq(&mut self) -> Self::Output {
        self.translate_expressed_binary_operation(Opcode::I32Eq, |builder| {
            builder.alloc.instruction_set.op_i32_and();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I32);
            Ok(())
        })
    }

    fn visit_i64_ne(&mut self) -> Self::Output {
        self.translate_expressed_binary_operation(Opcode::I32Ne, |builder| {
            builder.alloc.instruction_set.op_i32_or();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I32);
            Ok(())
        })
    }

    fn visit_i64_lt_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I32);

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

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_nez(5);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_lt_s();
            builder.alloc.instruction_set.op_br(4);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_lt_u();
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            Ok(())
        })
    }

    fn visit_i64_lt_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I32);

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

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_nez(5);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_lt_u();
            builder.alloc.instruction_set.op_br(4);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_lt_u();
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            Ok(())
        })
    }

    fn visit_i64_gt_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I32);

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

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_nez(5);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_gt_s();
            builder.alloc.instruction_set.op_br(4);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_gt_u();
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            Ok(())
        })
    }

    fn visit_i64_gt_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I32);

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

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_nez(5);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_gt_u();
            builder.alloc.instruction_set.op_br(4);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_gt_u();
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            Ok(())
        })
    }

    fn visit_i64_le_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I32);

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

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_nez(5);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_le_s();
            builder.alloc.instruction_set.op_br(4);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_le_u();
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            Ok(())
        })
    }

    fn visit_i64_le_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I32);

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

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_nez(5);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_le_u();
            builder.alloc.instruction_set.op_br(4);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_le_u();
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            Ok(())
        })
    }

    fn visit_i64_ge_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I32);

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

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_nez(5);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_ge_s();
            builder.alloc.instruction_set.op_br(4);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_ge_u();
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            Ok(())
        })
    }

    fn visit_i64_ge_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I32);

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

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_nez(5);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_ge_u();
            builder.alloc.instruction_set.op_br(4);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_ge_u();
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            Ok(())
        })
    }

    fn visit_f32_eq(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::F32, Opcode::F32Eq)
    }

    fn visit_f32_ne(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::F32, Opcode::F32Ne)
    }

    fn visit_f32_lt(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::F32, Opcode::F32Lt)
    }

    fn visit_f32_gt(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::F32, Opcode::F32Gt)
    }

    fn visit_f32_le(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::F32, Opcode::F32Le)
    }

    fn visit_f32_ge(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::F32, Opcode::F32Ge)
    }

    fn visit_f64_eq(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::F64, Opcode::F64Eq)
    }

    fn visit_f64_ne(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::F64, Opcode::F64Ne)
    }

    fn visit_f64_lt(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::F64, Opcode::F64Lt)
    }

    fn visit_f64_gt(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::F64, Opcode::F64Gt)
    }

    fn visit_f64_le(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::F64, Opcode::F64Le)
    }

    fn visit_f64_ge(&mut self) -> Self::Output {
        self.translate_binary_cmp(ValType::F64, Opcode::F64Ge)
    }

    fn visit_i32_clz(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::I32, Opcode::I32Clz)
    }

    fn visit_i32_ctz(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::I32, Opcode::I32Ctz)
    }

    fn visit_i32_popcnt(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::I32, Opcode::I32Popcnt)
    }

    fn visit_i32_add(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32Add)
    }

    fn visit_i32_sub(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32Sub)
    }

    fn visit_i32_mul(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32Mul)
    }

    fn visit_i32_div_s(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32DivS)
    }

    fn visit_i32_div_u(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32DivU)
    }

    fn visit_i32_rem_s(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32RemS)
    }

    fn visit_i32_rem_u(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32RemU)
    }

    fn visit_i32_and(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32And)
    }

    fn visit_i32_or(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32Or)
    }

    fn visit_i32_xor(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32Xor)
    }

    fn visit_i32_shl(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32Shl)
    }

    fn visit_i32_shr_s(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32ShrS)
    }

    fn visit_i32_shr_u(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32ShrU)
    }

    fn visit_i32_rotl(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32Rotl)
    }

    fn visit_i32_rotr(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::I32, Opcode::I32Rotr)
    }

    fn visit_i64_clz(&mut self) -> Self::Output {
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

            builder.alloc.instruction_set.op_i32_clz();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_eqz(4);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_clz();
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_set(1);
            builder.alloc.instruction_set.op_i32_const(0);
            Ok(())
        })
    }

    fn visit_i64_ctz(&mut self) -> Self::Output {
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

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_ctz();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_eqz(5);
            builder.alloc.instruction_set.op_local_set(2);
            builder.alloc.instruction_set.op_i32_ctz();
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_br(3);
            builder.alloc.instruction_set.op_local_set(2);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_i32_const(0);
            Ok(())
        })
    }

    fn visit_i64_popcnt(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.instruction_set.op_i32_popcnt();
            builder.stack_height.push();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_popcnt();
            builder.stack_height.pop1();
            builder.alloc.instruction_set.op_local_set(2);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_i32_const(0);
            Ok(())
        })
    }

    fn visit_i64_add(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I64);

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

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_or();
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_i32_clz();
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_local_set(6);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_i32_clz();
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_gt_u();
            builder.alloc.instruction_set.op_br_if_eqz(3);
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_set(5);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            Ok(())
        })
    }

    fn visit_i64_sub(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I64);

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

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_lt_u();
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_local_set(5);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_br_if_eqz(3);
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            Ok(())
        })
    }

    fn visit_i64_mul(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I64);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(0x0000ffff);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_const(16);
            builder.alloc.instruction_set.op_i32_shr_u();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_const(0x0000ffff);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_local_get(6);
            builder.alloc.instruction_set.op_i32_const(16);
            builder.alloc.instruction_set.op_i32_shr_u();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(6);
            builder.alloc.instruction_set.op_i32_const(0x0000ffff);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_local_get(7);
            builder.alloc.instruction_set.op_i32_const(16);
            builder.alloc.instruction_set.op_i32_shr_u();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(7);
            builder.alloc.instruction_set.op_i32_const(0x0000ffff);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_local_get(8);
            builder.alloc.instruction_set.op_i32_const(16);
            builder.alloc.instruction_set.op_i32_shr_u();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            //a0 * b0
            builder.alloc.instruction_set.op_local_get(8);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_mul();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            //a1 * b0 + a0 * b1
            builder.alloc.instruction_set.op_local_get(9);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_mul();
            builder.alloc.instruction_set.op_local_get(9);
            builder.alloc.instruction_set.op_local_get(7);
            builder.alloc.instruction_set.op_i32_mul();

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
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_or();
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_i32_clz();
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_i32_clz();
            builder.alloc.instruction_set.op_i32_gt_u();
            builder.alloc.instruction_set.op_br_if_eqz(3);
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_br(2);
            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.instruction_set.op_local_set(1);

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
            builder.alloc.instruction_set.op_local_get(11);
            builder.alloc.instruction_set.op_local_get(6);
            builder.alloc.instruction_set.op_i32_mul();
            builder.alloc.instruction_set.op_local_get(11);
            builder.alloc.instruction_set.op_local_get(8);
            builder.alloc.instruction_set.op_i32_mul();
            builder.alloc.instruction_set.op_local_get(11);
            builder.alloc.instruction_set.op_local_get(10);
            builder.alloc.instruction_set.op_i32_mul();
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_i32_add();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            //a3 * b0 + a2 * b1 + a1 * b2 + a0 * b3
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_local_set(3);
            builder.alloc.instruction_set.op_local_set(1);

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

            builder.alloc.instruction_set.op_local_get(12);
            builder.alloc.instruction_set.op_local_get(6);
            builder.alloc.instruction_set.op_i32_mul();
            builder.alloc.instruction_set.op_local_get(12);
            builder.alloc.instruction_set.op_local_get(8);
            builder.alloc.instruction_set.op_i32_mul();
            builder.alloc.instruction_set.op_local_get(12);
            builder.alloc.instruction_set.op_local_get(10);
            builder.alloc.instruction_set.op_i32_mul();
            builder.alloc.instruction_set.op_local_get(12);
            builder.alloc.instruction_set.op_local_get(12);
            builder.alloc.instruction_set.op_i32_mul();
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_i32_add();

            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_set(8);
            builder.alloc.instruction_set.op_local_set(8);
            builder.alloc.instruction_set.op_local_set(8);
            builder.alloc.instruction_set.op_local_set(8);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();

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
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(16);
            builder.alloc.instruction_set.op_i32_shl();
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_const(16);
            builder.alloc.instruction_set.op_i32_shl();
            builder.alloc.instruction_set.op_i32_or();
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_i32_clz();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_i32_clz();
            builder.alloc.instruction_set.op_i32_gt_u();
            builder.alloc.instruction_set.op_br_if_eqz(3);
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_br(2);
            builder.alloc.instruction_set.op_i32_const(0);

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

            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_const(16);
            builder.alloc.instruction_set.op_i32_shr_u();
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_const(16);
            builder.alloc.instruction_set.op_i32_shl();
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_i32_add();

            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_set(8);
            builder.alloc.instruction_set.op_local_set(8);

            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();

            Ok(())
        })
    }

    fn visit_i64_div_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I64);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            // divide by zero check
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_or();
            builder.alloc.instruction_set.op_br_if_nez(3);
            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.instruction_set.op_i32_div_u();

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
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_eqz(14);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_const(-2147483648);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_eqz(10);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_eqz(4);
            builder.alloc.instruction_set.op_i32_const(-2147483648);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_div_s();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            // zero division check
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_or();
            builder.alloc.instruction_set.op_br_if_nez(4);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_br(186 + 4 + 4 - 6);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.instruction_set.op_i32_lt_s();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_br_if_eqz(11 + 4);

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

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_local_set(5);
            builder.alloc.instruction_set.op_br_if_nez(3);
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_set(2);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(7);
            builder.alloc.instruction_set.op_local_get(7);
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.instruction_set.op_i32_lt_s();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_br_if_eqz(11 + 4);

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

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_local_set(5);
            builder.alloc.instruction_set.op_br_if_nez(3);
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_set(2);

            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_select();

            builder.stack_height.pop2();

            builder.alloc.instruction_set.op_local_set(10);
            builder.alloc.instruction_set.op_drop();

            builder.stack_height.pop3();
            builder.stack_height.push();
            builder.stack_height.pop2();

            builder.alloc.instruction_set.op_local_set(6);
            builder.alloc.instruction_set.op_local_set(6);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_local_set(3);
            builder.alloc.instruction_set.op_local_set(1);

            builder.translate_i64_div_u();

            builder.stack_height.pop_n(7);

            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();

            builder.alloc.instruction_set.op_local_set(5);
            builder.alloc.instruction_set.op_local_set(5);

            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_eqz(5);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_local_set(3);
            builder.alloc.instruction_set.op_local_set(1);
            builder.alloc.instruction_set.op_br(22 - 6);

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

            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_local_set(5);
            builder.alloc.instruction_set.op_br_if_nez(3);
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_set(2);
            builder.alloc.instruction_set.op_drop();

            Ok(())
        })
    }

    fn visit_i64_div_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I64);

            builder.translate_i64_div_u();

            builder.stack_height.pop_n(7);

            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();

            builder.alloc.instruction_set.op_local_set(5);
            builder.alloc.instruction_set.op_local_set(5);

            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();

            Ok(())
        })
    }

    fn visit_i64_rem_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I64);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_or();
            builder.alloc.instruction_set.op_br_if_nez(3);
            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.instruction_set.op_i32_div_u();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_or();
            builder.alloc.instruction_set.op_br_if_nez(4);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder
                .alloc
                .instruction_set
                .op_br(158 + 4 + 4 - 2 + 15 + 7);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.instruction_set.op_i32_lt_s();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_br_if_eqz(11 + 4);

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

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_local_set(5);
            builder.alloc.instruction_set.op_br_if_nez(3);
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_set(2);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(7);
            builder.alloc.instruction_set.op_local_get(7);
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.instruction_set.op_i32_lt_s();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_br_if_eqz(11 + 4);

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

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_local_set(5);
            builder.alloc.instruction_set.op_br_if_nez(3);
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_set(2);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();

            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_select();

            builder.stack_height.pop2();

            builder.alloc.instruction_set.op_local_set(10);
            builder.alloc.instruction_set.op_drop();

            builder.stack_height.pop3();
            builder.stack_height.push();
            builder.stack_height.pop2();

            builder.alloc.instruction_set.op_local_set(6);
            builder.alloc.instruction_set.op_local_set(6);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_local_set(3);
            builder.alloc.instruction_set.op_local_set(1);

            builder.translate_i64_div_u();

            builder.stack_height.pop_n(7);

            builder.alloc.instruction_set.op_local_set(7);
            builder.alloc.instruction_set.op_local_set(7);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_eq();
            builder.alloc.instruction_set.op_br_if_eqz(5);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_local_set(3);
            builder.alloc.instruction_set.op_local_set(1);
            builder.alloc.instruction_set.op_br(16);

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

            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.instruction_set.op_i32_xor();
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_local_set(5);
            builder.alloc.instruction_set.op_br_if_nez(3);
            builder.alloc.instruction_set.op_i32_const(1);
            builder.alloc.instruction_set.op_i32_add();
            builder.alloc.instruction_set.op_local_set(2);
            builder.alloc.instruction_set.op_drop();

            Ok(())
        })
    }

    fn visit_i64_rem_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I64);

            builder.translate_i64_div_u();

            builder.stack_height.pop_n(7);

            builder.alloc.instruction_set.op_local_set(7);
            builder.alloc.instruction_set.op_local_set(7);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();

            Ok(())
        })
    }

    fn visit_i64_and(&mut self) -> Self::Output {
        self.translate_expressed_binary_operation(Opcode::I32And, |builder| {
            builder.alloc.stack_types.pop();
            Ok(())
        })
    }

    fn visit_i64_or(&mut self) -> Self::Output {
        self.translate_expressed_binary_operation(Opcode::I32Or, |builder| {
            builder.alloc.stack_types.pop();
            Ok(())
        })
    }

    fn visit_i64_xor(&mut self) -> Self::Output {
        self.translate_expressed_binary_operation(Opcode::I32Xor, |builder| {
            builder.alloc.stack_types.pop();
            Ok(())
        })
    }

    fn visit_i64_shl(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(63);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_local_set(2);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_br_if_eqz(32);

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

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(31);
            builder.alloc.instruction_set.op_i32_gt_u();
            builder.alloc.instruction_set.op_br_if_eqz(10);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shl();
            builder.alloc.instruction_set.op_local_set(3);
            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_br(19);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_shl();
            builder.alloc.instruction_set.op_local_set(4);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_shl();
            builder.alloc.instruction_set.op_local_set(3);

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

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(31);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shr_u();
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_or();
            builder.alloc.instruction_set.op_local_set(3);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();

            Ok(())
        })
    }

    fn visit_i64_shr_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(63);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_local_set(2);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_br_if_eqz(34);

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

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(31);
            builder.alloc.instruction_set.op_i32_gt_u();
            builder.alloc.instruction_set.op_br_if_eqz(12);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shr_s();
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_const(31);
            builder.alloc.instruction_set.op_i32_shr_s();
            builder.alloc.instruction_set.op_local_set(3);
            builder.alloc.instruction_set.op_br(19);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_shr_s();
            builder.alloc.instruction_set.op_local_set(3);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_shr_s();
            builder.alloc.instruction_set.op_local_set(4);

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

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(31);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shl();
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_or();
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();

            Ok(())
        })
    }

    fn visit_i64_shr_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(63);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_local_set(2);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_br_if_eqz(32);

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

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(31);
            builder.alloc.instruction_set.op_i32_gt_u();
            builder.alloc.instruction_set.op_br_if_eqz(10);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shr_u();
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.instruction_set.op_local_set(3);
            builder.alloc.instruction_set.op_br(19);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_shr_u();
            builder.alloc.instruction_set.op_local_set(3);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_shr_u();
            builder.alloc.instruction_set.op_local_set(4);

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

            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(31);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shl();
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_or();
            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();

            Ok(())
        })
    }

    fn visit_i64_rotl(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(63);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_local_set(2);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_br_if_eqz(32 + 10 + 2 + 6);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(31);
            builder.alloc.instruction_set.op_i32_gt_u();
            builder.alloc.instruction_set.op_br_if_eqz(26);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shl();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(64);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shr_u();
            builder.alloc.instruction_set.op_i32_or();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_const(64);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shr_u();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shl();
            builder.alloc.instruction_set.op_i32_or();

            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_set(5);
            builder.alloc.instruction_set.op_local_set(3);
            builder.alloc.instruction_set.op_br(19 + 2);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_shl();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shr_u();
            builder.alloc.instruction_set.op_i32_or();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shr_u();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_shl();
            builder.alloc.instruction_set.op_i32_or();

            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_local_set(4);

            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();

            Ok(())
        })
    }

    fn visit_i64_rotr(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(63);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_local_set(2);
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_br_if_eqz(32 + 10 + 2 + 6);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_const(31);
            builder.alloc.instruction_set.op_i32_gt_u();
            builder.alloc.instruction_set.op_br_if_eqz(26);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shr_u();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(64);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shl();
            builder.alloc.instruction_set.op_i32_or();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_const(64);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shl();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shr_u();
            builder.alloc.instruction_set.op_i32_or();

            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_set(5);
            builder.alloc.instruction_set.op_local_set(3);
            builder.alloc.instruction_set.op_br(19 + 2);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_local_get(3);
            builder.alloc.instruction_set.op_i32_shr_u();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(4);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shl();
            builder.alloc.instruction_set.op_i32_or();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_const(32);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_sub();
            builder.alloc.instruction_set.op_i32_shl();

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_local_get(5);
            builder.alloc.instruction_set.op_i32_shr_u();
            builder.alloc.instruction_set.op_i32_or();

            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_local_set(4);
            builder.alloc.instruction_set.op_local_set(4);

            builder.stack_height.pop1();
            builder.stack_height.pop1();

            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_drop();

            Ok(())
        })
    }

    fn visit_f32_abs(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::F32, Opcode::F32Abs)
    }

    fn visit_f32_neg(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::F32, Opcode::F32Neg)
    }

    fn visit_f32_ceil(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::F32, Opcode::F32Ceil)
    }

    fn visit_f32_floor(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::F32, Opcode::F32Floor)
    }

    fn visit_f32_trunc(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::F32, Opcode::F32Trunc)
    }

    fn visit_f32_nearest(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::F32, Opcode::F32Nearest)
    }

    fn visit_f32_sqrt(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::F32, Opcode::F32Sqrt)
    }

    fn visit_f32_add(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::F32, Opcode::F32Add)
    }

    fn visit_f32_sub(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::F32, Opcode::F32Sub)
    }

    fn visit_f32_mul(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::F32, Opcode::F32Mul)
    }

    fn visit_f32_div(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::F32, Opcode::F32Div)
    }

    fn visit_f32_min(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::F32, Opcode::F32Min)
    }

    fn visit_f32_max(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::F32, Opcode::F32Max)
    }

    fn visit_f32_copysign(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::F32, Opcode::F32Copysign)
    }

    fn visit_f64_abs(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::F64, Opcode::F64Abs)
    }

    fn visit_f64_neg(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::F64, Opcode::F64Neg)
    }

    fn visit_f64_ceil(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::F64, Opcode::F64Ceil)
    }

    fn visit_f64_floor(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::F64, Opcode::F64Floor)
    }

    fn visit_f64_trunc(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::F64, Opcode::F64Trunc)
    }

    fn visit_f64_nearest(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::F64, Opcode::F64Nearest)
    }

    fn visit_f64_sqrt(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::F64, Opcode::F64Sqrt)
    }

    fn visit_f64_add(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::F64, Opcode::F64Add)
    }

    fn visit_f64_sub(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::F64, Opcode::F64Sub)
    }

    fn visit_f64_mul(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::F64, Opcode::F64Mul)
    }

    fn visit_f64_div(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::F64, Opcode::F64Div)
    }

    fn visit_f64_min(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::F64, Opcode::F64Min)
    }

    fn visit_f64_max(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::F64, Opcode::F64Max)
    }

    fn visit_f64_copysign(&mut self) -> Self::Output {
        self.translate_binary_operation(ValType::F64, Opcode::F64Copysign)
    }
    fn visit_i32_wrap_i64(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();

            builder.alloc.instruction_set.op_drop();
            builder.alloc.stack_types.push(ValType::I32);
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_i32_trunc_f32_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i32_trunc_f32_s();
            builder.alloc.stack_types.push(ValType::F32);
            Ok(())
        })
    }

    fn visit_i32_trunc_f32_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i32_trunc_f32_u();
            builder.alloc.stack_types.push(ValType::F32);
            Ok(())
        })
    }

    fn visit_i32_trunc_f64_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i32_trunc_f64_s();
            builder.alloc.stack_types.push(ValType::F32);
            Ok(())
        })
    }

    fn visit_i32_trunc_f64_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i32_trunc_f64_u();
            builder.alloc.stack_types.push(ValType::F32);
            Ok(())
        })
    }

    fn visit_i64_extend_i32_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();

            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_clz();
            builder.alloc.instruction_set.op_br_if_eqz(3);
            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.instruction_set.op_br(2);
            builder.alloc.instruction_set.op_i32_const(-1);
            builder.alloc.stack_types.push(ValType::I64);

            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();

            Ok(())
        })
    }

    fn visit_i64_extend_i32_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();

            builder.alloc.instruction_set.op_i32_const(0);
            builder.alloc.stack_types.push(ValType::I64);
            builder.stack_height.push();

            Ok(())
        })
    }

    fn visit_i64_trunc_f32_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i64_trunc_f32_s();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i64_const(32);
            builder.alloc.instruction_set.op_i64_shr_u();
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_set(2);

            builder.alloc.stack_types.push(ValType::I64);
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_i64_trunc_f32_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i64_trunc_f32_u();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i64_const(32);
            builder.alloc.instruction_set.op_i64_shr_u();
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_set(2);

            builder.alloc.stack_types.push(ValType::I64);
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_i64_trunc_f64_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i64_trunc_f64_s();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i64_const(32);
            builder.alloc.instruction_set.op_i64_shr_u();
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_set(2);

            builder.alloc.stack_types.push(ValType::I64);
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_i64_trunc_f64_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i64_trunc_f64_u();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i64_const(32);
            builder.alloc.instruction_set.op_i64_shr_u();
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_set(2);

            builder.alloc.stack_types.push(ValType::I64);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_f32_convert_i32_s(&mut self) -> Self::Output {
        self.translate_conversion(ValType::I32, ValType::F32, Opcode::F32ConvertI32S)
    }

    fn visit_f32_convert_i32_u(&mut self) -> Self::Output {
        self.translate_conversion(ValType::I32, ValType::F32, Opcode::F32ConvertI32U)
    }

    fn visit_f32_convert_i64_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i64_extend_i32_u();
            builder.alloc.instruction_set.op_i64_const(32);
            builder.alloc.instruction_set.op_i64_shl();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i64_extend_i32_u();
            builder.alloc.instruction_set.op_i64_add();
            builder.alloc.instruction_set.op_local_set(1);
            builder.alloc.instruction_set.op_f32_convert_i64_s();

            builder.alloc.stack_types.push(ValType::F32);
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

    fn visit_f32_convert_i64_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i64_extend_i32_u();
            builder.alloc.instruction_set.op_i64_const(32);
            builder.alloc.instruction_set.op_i64_shl();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i64_extend_i32_u();
            builder.alloc.instruction_set.op_i64_add();
            builder.alloc.instruction_set.op_local_set(1);
            builder.alloc.instruction_set.op_f32_convert_i64_u();

            builder.alloc.stack_types.push(ValType::F32);

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

    fn visit_f32_demote_f64(&mut self) -> Self::Output {
        self.translate_conversion(ValType::F64, ValType::F32, Opcode::F32DemoteF64)
    }

    fn visit_f64_convert_i32_s(&mut self) -> Self::Output {
        self.translate_conversion(ValType::I32, ValType::F64, Opcode::F64ConvertI32S)
    }

    fn visit_f64_convert_i32_u(&mut self) -> Self::Output {
        self.translate_conversion(ValType::I32, ValType::F64, Opcode::F64ConvertI32U)
    }

    fn visit_f64_convert_i64_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i64_extend_i32_u();
            builder.alloc.instruction_set.op_i64_const(32);
            builder.alloc.instruction_set.op_i64_shl();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i64_extend_i32_u();
            builder.alloc.instruction_set.op_i64_add();
            builder.alloc.instruction_set.op_local_set(1);
            builder.alloc.instruction_set.op_f64_convert_i64_s();

            builder.alloc.stack_types.push(ValType::F64);
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            Ok(())
        })
    }

    fn visit_f64_convert_i64_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i64_extend_i32_u();
            builder.alloc.instruction_set.op_i64_const(32);
            builder.alloc.instruction_set.op_i64_shl();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i64_extend_i32_u();
            builder.alloc.instruction_set.op_i64_add();
            builder.alloc.instruction_set.op_local_set(1);
            builder.alloc.instruction_set.op_f64_convert_i64_u();

            builder.alloc.stack_types.push(ValType::F64);
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_f64_promote_f32(&mut self) -> Self::Output {
        self.translate_conversion(ValType::F32, ValType::F64, Opcode::F64PromoteF32)
    }

    fn visit_i32_reinterpret_f32(&mut self) -> Self::Output {
        self.visit_reinterpret(ValType::F32, ValType::I32)
    }

    fn visit_i64_reinterpret_f64(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i64_const(32);
            builder.alloc.instruction_set.op_i64_shr_u();
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_set(2);

            builder.alloc.stack_types.push(ValType::I64);
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

    fn visit_f32_reinterpret_i32(&mut self) -> Self::Output {
        self.visit_reinterpret(ValType::I32, ValType::F32)
    }

    fn visit_f64_reinterpret_i64(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i64_extend_i32_u();
            builder.alloc.instruction_set.op_i64_const(32);
            builder.alloc.instruction_set.op_i64_shl();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i64_extend_i32_u();
            builder.alloc.instruction_set.op_i64_add();
            builder.alloc.instruction_set.op_local_set(1);

            builder.alloc.stack_types.push(ValType::F64);
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            Ok(())
        })
    }

    fn visit_i32_extend8_s(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::I32, Opcode::I32Extend8S)
    }

    fn visit_i32_extend16_s(&mut self) -> Self::Output {
        self.translate_unary_operation(ValType::I32, Opcode::I32Extend16S)
    }

    fn visit_i64_extend8_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_i32_extend8_s();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_const(i32::MIN);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_br_if_eqz(3);
            builder.alloc.instruction_set.op_i32_const(-1_i32);
            builder.alloc.instruction_set.op_br(2);
            builder.alloc.instruction_set.op_i32_const(0);

            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();

            Ok(())
        })
    }

    fn visit_i64_extend16_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_i32_extend16_s();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_const(i32::MIN);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_br_if_eqz(3);
            builder.alloc.instruction_set.op_i32_const(-1_i32);
            builder.alloc.instruction_set.op_br(2);
            builder.alloc.instruction_set.op_i32_const(0);

            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();

            Ok(())
        })
    }

    fn visit_i64_extend32_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.instruction_set.op_drop();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i32_const(i32::MIN);
            builder.alloc.instruction_set.op_i32_and();
            builder.alloc.instruction_set.op_br_if_eqz(3);
            builder.alloc.instruction_set.op_i32_const(-1_i32);
            builder.alloc.instruction_set.op_br(2);
            builder.alloc.instruction_set.op_i32_const(0);

            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();

            Ok(())
        })
    }

    fn visit_i32_trunc_sat_f32_s(&mut self) -> Self::Output {
        self.translate_conversion(ValType::F32, ValType::I32, Opcode::I32TruncSatF32S)
    }

    fn visit_i32_trunc_sat_f32_u(&mut self) -> Self::Output {
        self.translate_conversion(ValType::F32, ValType::I32, Opcode::I32TruncSatF32U)
    }

    fn visit_i32_trunc_sat_f64_s(&mut self) -> Self::Output {
        self.translate_conversion(ValType::F64, ValType::I32, Opcode::I32TruncSatF64S)
    }

    fn visit_i32_trunc_sat_f64_u(&mut self) -> Self::Output {
        self.translate_conversion(ValType::F64, ValType::I32, Opcode::I32TruncSatF64U)
    }

    fn visit_i64_trunc_sat_f32_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i64_trunc_sat_f32_s();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i64_const(32);
            builder.alloc.instruction_set.op_i64_shr_u();
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_set(2);

            builder.alloc.stack_types.push(ValType::I64);
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_i64_trunc_sat_f32_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i64_trunc_sat_f32_u();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i64_const(32);
            builder.alloc.instruction_set.op_i64_shr_u();
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_set(2);

            builder.alloc.stack_types.push(ValType::I64);
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_i64_trunc_sat_f64_s(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_i64_trunc_sat_f64_s();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i64_const(32);
            builder.alloc.instruction_set.op_i64_shr_u();
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_set(2);

            builder.alloc.stack_types.push(ValType::I64);
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    fn visit_i64_trunc_sat_f64_u(&mut self) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;

            builder.alloc.stack_types.pop();

            builder.alloc.instruction_set.op_i64_trunc_sat_f64_u();
            builder.alloc.instruction_set.op_local_get(1);
            builder.alloc.instruction_set.op_i64_const(32);
            builder.alloc.instruction_set.op_i64_shr_u();
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_get(2);
            builder.alloc.instruction_set.op_i32_wrap_i64();
            builder.alloc.instruction_set.op_local_set(2);

            builder.alloc.stack_types.push(ValType::I64);

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            Ok(())
        })
    }

    /// MemoryInit opcode reads 3 elements from the stack (dst, src, len), where:
    /// - dst - Memory destination of copied data
    /// - src - Data source of copied data (in the passive section)
    /// - len - Length of copied data
    ///
    /// In the `passive_sections` field, we store info about all passive sections
    /// that are presented in the WebAssembly binary. When a passive section is activated
    /// though `memory.init` opcode, we find modified offsets in the data section
    /// and put on the stack by removing previous values.
    ///
    /// Here is the stack structure for `memory.init` call:
    /// - ... some other stack elements
    /// - dst
    /// - src
    /// - len
    /// - ... call of `memory.init` happens here
    ///
    /// Here we need to replace the `src` field with our modified, but since we don't know
    /// how the stack was structured, then we can achieve it by replacing a stack element using
    /// `local.set` opcode.
    ///
    /// - dst
    /// - src <-----+
    /// - len       |
    /// - new_src --+
    /// - ... call `local.set` to replace prev offset
    ///
    /// Here we use 1 offset because we pop `new_src`, and then count from the top, `len`
    /// has 0 offset and `src` has offset 1.
    ///
    /// Before doing these ops, we must ensure that the specified length of copied data
    /// doesn't exceed the original section size. We also inject GT check to make sure that
    /// there is no data section overflow.
    fn visit_memory_init(&mut self, data_segment_index: u32, memory_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memory_index, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();

            let data_segment_index: DataSegmentIdx = data_segment_index.into();

            let (ib, rb) = (
                &mut builder.alloc.instruction_set,
                &mut builder.alloc.segment_builder,
            );
            let (offset, length) = rb
                .memory_sections
                .get(&data_segment_index)
                .copied()
                .expect("can't resolve a passive segment by index");
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
                ib.op_local_get(1);
                ib.op_local_get(3);
                ib.op_i32_add();
                ib.op_i32_const(length);
                ib.op_i32_gt_s();
                ib.op_br_if_eqz(3);
                // we can't manually emit the "out-of-bounds table access" error required
                // by WebAssembly standards, so we put an impossible number of tables to trigger
                // overflow by rewriting the number of elements to be copied
                ib.op_i32_const(u32::MAX);
                ib.op_local_set(1);
            }
            // we need to replace the offset on the stack with the new value
            if offset > 0 {
                builder.stack_height.push();
                builder.stack_height.push();
                builder.stack_height.pop1();
                builder.stack_height.pop1();
                ib.op_i32_const(offset);
                ib.op_local_get(3);
                ib.op_i32_add();
                ib.op_local_set(2);
            }
            builder.stack_height.pop3();
            // since we store all data sections in the one segment, then the index is always 0
            ib.op_memory_init(data_segment_index.to_u32() + 1);
            Ok(())
        })
    }

    fn visit_data_drop(&mut self, data_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
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
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            builder.stack_height.pop3();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_memory_copy();
            Ok(())
        })
    }

    fn visit_memory_fill(&mut self, memory_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memory_index, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            builder.stack_height.pop3();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_memory_fill();
            Ok(())
        })
    }

    fn visit_table_init(&mut self, segment_index: u32, table_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();

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
            // do an overflow check
            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();

            ib.op_local_get(1);
            ib.op_local_get(3);
            ib.op_i32_add();
            ib.op_i32_const(length);
            ib.op_i32_gt_s();
            ib.op_br_if_eqz(3);
            // we can't manually emit the "out-of-bounds table access" error required
            // by WebAssembly standards, so we put an impossible number of tables to trigger
            // overflow by rewriting the number of elements to be copied
            ib.op_i32_const(u32::MAX);
            ib.op_local_set(1);
            // we need to replace the offset on the stack with the new value
            if offset > 0 {
                builder.stack_height.push();
                builder.stack_height.push();
                builder.stack_height.pop1();
                builder.stack_height.pop1();
                // replace offset with an adjusted value
                ib.op_i32_const(offset);
                ib.op_local_get(3);
                ib.op_i32_add();
                ib.op_local_set(2);
            }
            builder.stack_height.pop3();
            // since we store all element sections in the one segment, then the index is always 0
            ib.op_table_init(segment_index + 1);
            ib.op_table_get(table_index);

            Ok(())
        })
    }

    fn visit_elem_drop(&mut self, segment_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            builder
                .alloc
                .instruction_set
                .op_elem_drop(segment_index + 1);
            Ok(())
        })
    }

    fn visit_table_copy(&mut self, dst_table: u32, src_table: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            builder.stack_height.pop3();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_table_copy(dst_table);
            builder.alloc.instruction_set.op_table_get(src_table);
            Ok(())
        })
    }

    fn visit_table_fill(&mut self, table_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            builder.stack_height.pop3();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_table_fill(table_index);
            Ok(())
        })
    }

    fn visit_table_get(&mut self, table_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            builder.alloc.instruction_set.op_table_get(table_index);
            Ok(())
        })
    }

    fn visit_table_set(&mut self, table_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            builder.stack_height.pop2();
            //TODO: Do set and get for i32 x2 as i64
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.instruction_set.op_table_set(table_index);
            Ok(())
        })
    }

    fn visit_table_grow(&mut self, table_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;

            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(ValType::I32);

            // for rWASM we inject table limit error check, if we exceed the number of allowed
            // elements, then we push `u32::MAX` on the stack that is equal to table
            // grow overflow error
            let table_type = builder.resolve_table_type(table_index);
            let max_table_elements = table_type.maximum.unwrap_or(N_MAX_TABLE_ELEMENTS as u32);
            let ib = &mut builder.alloc.instruction_set;

            builder.stack_height.push();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();

            ib.op_local_get(1);
            ib.op_table_size(table_index);
            ib.op_i32_add();
            ib.op_i32_const(max_table_elements);
            ib.op_i32_gt_s();
            ib.op_br_if_eqz(5);
            ib.op_drop();
            ib.op_drop();
            ib.op_i32_const(u32::MAX);
            ib.op_br(2);

            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            ib.op_table_grow(table_index);
            Ok(())
        })
    }

    fn visit_table_size(&mut self, table_index: u32) -> Self::Output {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().entity)?;
            builder.stack_height.push();
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
        memarg: MemArg,
        loaded_type: ValType,
        opcode: Opcode,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().load)?;
            builder.alloc.stack_types.pop();
            builder.stack_height.pop1();
            builder.alloc.stack_types.push(loaded_type);
            builder.stack_height.push();
            let offset = AddressOffset::from(memarg.offset as u32);
            builder
                .alloc
                .instruction_set
                .push(opcode, OpcodeData::AddressOffset(offset));
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
        memarg: MemArg,
        _stored_value: ValType,
        opcode: Opcode,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            debug_assert_eq!(memarg.memory, DEFAULT_MEMORY_INDEX);
            builder.bump_fuel_consumption(builder.fuel_costs().store)?;
            builder.stack_height.pop2();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            let offset = AddressOffset::from(memarg.offset as u32);
            builder
                .alloc
                .instruction_set
                .push(opcode, OpcodeData::AddressOffset(offset));
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
        _input_type: ValType,
        inst: Opcode,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder
                .alloc
                .instruction_set
                .push(inst, OpcodeData::EmptyData);
            Ok(())
        })
    }

    fn translate_expressed_binary_operation<F>(
        &mut self,
        opcode: Opcode,
        additional_translator: F,
    ) -> Result<(), CompilationError>
    where
        F: FnOnce(&mut Self) -> Result<(), CompilationError>,
    {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.stack_height.push();
            builder.stack_height.pop1();
            builder.stack_height.pop1();
            builder.alloc.instruction_set.op_local_get(3);
            builder
                .alloc
                .instruction_set
                .push(opcode, OpcodeData::EmptyData);
            builder.alloc.instruction_set.op_local_set(2);
            builder.alloc.instruction_set.op_local_get(3);
            builder
                .alloc
                .instruction_set
                .push(opcode, OpcodeData::EmptyData);
            builder.alloc.instruction_set.op_local_set(2);
            additional_translator(builder)?;
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
        input_type: ValType,
        opcode: Opcode,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(input_type);
            builder
                .alloc
                .instruction_set
                .push(opcode, OpcodeData::EmptyData);
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
        _value_type: ValType,
        opcode: Opcode,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder
                .alloc
                .instruction_set
                .push(opcode, OpcodeData::EmptyData);
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
        value_type: ValType,
        opcode: Opcode,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder.stack_height.pop2();
            builder.stack_height.push();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.pop();
            builder.alloc.stack_types.push(value_type);
            builder
                .alloc
                .instruction_set
                .push(opcode, OpcodeData::EmptyData);
            Ok(())
        })
    }

    /// Translates a 64-bit unsigned integer division operation into a sequence of WebAssembly
    /// (Wasm) instructions.
    ///
    /// This function implements the logic for dividing two 64-bit integers (treated as unsigned) by
    /// breaking them into 32-bit components and simulating the division operation. The function
    /// generates the corresponding WebAssembly operations using an instruction builder
    /// (`inst_builder`) and maintains the stack height for proper stack-based Wasm semantics.
    ///
    /// # Implementation Details
    /// - The function works with the high (`hi`) and low (`lo`) 32-bit parts of two 64-bit
    ///   integers.
    /// - It simulates bitwise shifts, arithmetic operations, and comparisons to calculate the
    ///   quotient and remainder.
    /// - The function uses a counter to iterate through the 64 bits of the dividend, updating
    ///   relevant intermediate results for the quotient and remainder at each step.
    ///
    /// # Stack Usage
    /// The function involves meticulous stack operations:
    /// - Push and pop operations are tracked using `self.stack_height` to manage the stack state.
    /// - Custom stack height tracking ensures adherence to Wasm stack rules during dynamic
    ///   instruction generation.
    ///
    /// # Generated Operations
    /// - Loads local variables using `op_local_get`.
    /// - Executes arithmetic and logical operations such as `op_i32_add`, `op_i32_sub`,
    ///   `op_i32_or`, `op_i32_shl`, and `op_i32_shr_u`.
    /// - Performs conditional branching using `op_br_if_nez` and `op_br_if_eqz`.
    /// - Updates locals with `op_local_set` and `op_local_tee`.
    /// - Handles division-related edge cases such as carry propagation and overflow checks.
    ///
    /// # Example Workflow
    /// 1. Decompose the high and low 32-bit parts of the two 64-bit operands.
    /// 2. Simulate division through a loop where each iteration:
    ///    - Shifts bits and updates intermediate results.
    ///    - Computes carries and propagates bit results for the quotient and remainder.
    /// 3. Rebuild the final results from the calculated quotient and remainder.
    ///
    /// # Errors
    /// - The function assumes correct initialization of local variables and proper input setup for
    ///   the operands.
    /// - Overflow, division by zero, or other exceptional arithmetic conditions should be handled
    ///   as part of higher-level logic or exception management.
    ///
    /// # Notes
    /// - This implementation is tailored for environments where 64-bit integer operations are not
    ///   natively available in Wasm, making it necessary to simulate these operations through
    ///   32-bit arithmetic.
    /// - The function might be updated in future versions for optimizations or support of
    ///   additional architectures.
    fn translate_i64_div_u(&mut self) {
        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();

        self.alloc.instruction_set.op_local_get(2);
        self.alloc.instruction_set.op_local_get(2);
        self.alloc.instruction_set.op_i32_or();
        self.alloc.instruction_set.op_br_if_nez(3);
        self.alloc.instruction_set.op_i32_const(0);
        self.alloc.instruction_set.op_i32_div_u();

        self.stack_height.push_n(5);

        //Stack: lo1 hi1 lo2 hi2
        self.alloc.instruction_set.op_i32_const(64); //counter
        self.alloc.instruction_set.op_i32_const(0); //q_lo
        self.alloc.instruction_set.op_i32_const(0); //q_hi
        self.alloc.instruction_set.op_i32_const(0); //r_lo
        self.alloc.instruction_set.op_i32_const(0); //r_hi

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();
        self.stack_height.pop1();

        //set r_hi
        self.alloc.instruction_set.op_local_get(1); //ri
        self.alloc.instruction_set.op_i32_const(1);
        self.alloc.instruction_set.op_i32_shl();
        self.alloc.instruction_set.op_local_get(3); //ro
        self.alloc.instruction_set.op_i32_const(31);
        self.alloc.instruction_set.op_i32_shr_u();
        self.alloc.instruction_set.op_i32_or();
        self.alloc.instruction_set.op_local_set(1);

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();
        self.stack_height.pop1();

        //set r_lo
        self.alloc.instruction_set.op_local_get(2); //ro
        self.alloc.instruction_set.op_i32_const(1);
        self.alloc.instruction_set.op_i32_shl();
        self.alloc.instruction_set.op_local_get(9); //1
        self.alloc.instruction_set.op_i32_const(31);
        self.alloc.instruction_set.op_i32_shr_u();
        self.alloc.instruction_set.op_i32_or();
        self.alloc.instruction_set.op_local_set(2);

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();
        self.stack_height.pop1();

        //set hi1
        self.alloc.instruction_set.op_local_get(8); //1
        self.alloc.instruction_set.op_i32_const(1);
        self.alloc.instruction_set.op_i32_shl();
        self.alloc.instruction_set.op_local_get(10); //1
        self.alloc.instruction_set.op_i32_const(31);
        self.alloc.instruction_set.op_i32_shr_u();
        self.alloc.instruction_set.op_i32_or();
        self.alloc.instruction_set.op_local_set(8); //1

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();

        //set lo1
        self.alloc.instruction_set.op_local_get(9); //1
        self.alloc.instruction_set.op_i32_const(1);
        self.alloc.instruction_set.op_i32_shl();
        self.alloc.instruction_set.op_local_set(9); //1

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();

        self.alloc.instruction_set.op_local_get(2); //ro
        self.alloc.instruction_set.op_local_get(8); //2
        self.alloc.instruction_set.op_i32_sub(); //temp_r_lo

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();

        self.alloc.instruction_set.op_local_get(3); //ro
        self.alloc.instruction_set.op_local_get(9); //2
        self.alloc.instruction_set.op_i32_ge_u(); //not l_carry

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();

        self.alloc.instruction_set.op_local_get(3); //ri
        self.alloc.instruction_set.op_local_get(2); //not l_cay
        self.alloc.instruction_set.op_i32_lt_u(); //c_carry

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();

        self.alloc.instruction_set.op_local_get(4); //ri
        self.alloc.instruction_set.op_local_get(3); //cay
        self.alloc.instruction_set.op_i32_const(1);
        self.alloc.instruction_set.op_i32_xor();
        self.alloc.instruction_set.op_i32_sub();
        self.alloc.instruction_set.op_local_get(3); //not l_cay
        self.alloc.instruction_set.op_local_set(2); //not l_cay
        self.alloc.instruction_set.op_local_set(2); //temp_ri

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();

        self.alloc.instruction_set.op_local_get(2); //temp_ri
        self.alloc.instruction_set.op_local_get(10); //2
        self.alloc.instruction_set.op_i32_sub();
        self.alloc.instruction_set.op_local_set(2); //temp_ri

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();

        self.alloc.instruction_set.op_local_get(4); //ri
        self.alloc.instruction_set.op_local_get(10); //2
        self.alloc.instruction_set.op_i32_ge_u(); //not hi_carry

        self.stack_height.pop1();

        self.alloc.instruction_set.op_i32_and(); //carry

        self.stack_height.pop1();

        self.alloc.instruction_set.op_br_if_eqz(18);

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

        self.alloc.instruction_set.op_local_set(2); //r_hi = temp_ri
        self.alloc.instruction_set.op_local_set(2); //r_lo = temp_r_;

        //set q_hi
        self.alloc.instruction_set.op_local_get(3); //qi
        self.alloc.instruction_set.op_i32_const(1);
        self.alloc.instruction_set.op_i32_shl();
        self.alloc.instruction_set.op_local_get(5); //qo
        self.alloc.instruction_set.op_i32_const(31);
        self.alloc.instruction_set.op_i32_shr_u();
        self.alloc.instruction_set.op_i32_or();
        self.alloc.instruction_set.op_local_set(3); //qi

        //set q_lo
        self.alloc.instruction_set.op_local_get(4); //qo
        self.alloc.instruction_set.op_i32_const(1);
        self.alloc.instruction_set.op_i32_shl();
        self.alloc.instruction_set.op_i32_const(1);
        self.alloc.instruction_set.op_i32_or();
        self.alloc.instruction_set.op_local_set(4); //qo

        self.alloc.instruction_set.op_br(15);

        //set q_hi
        self.alloc.instruction_set.op_drop();
        self.alloc.instruction_set.op_drop();
        self.alloc.instruction_set.op_local_get(3); //qi
        self.alloc.instruction_set.op_i32_const(1);
        self.alloc.instruction_set.op_i32_shl();
        self.alloc.instruction_set.op_local_get(5); //qo
        self.alloc.instruction_set.op_i32_const(31);
        self.alloc.instruction_set.op_i32_shr_u();
        self.alloc.instruction_set.op_i32_or();
        self.alloc.instruction_set.op_local_set(3); //qi

        //set q_lo
        self.alloc.instruction_set.op_local_get(4); //qo
        self.alloc.instruction_set.op_i32_const(1);
        self.alloc.instruction_set.op_i32_shl();
        self.alloc.instruction_set.op_local_set(4); //qo

        self.stack_height.push();
        self.stack_height.push();
        self.stack_height.pop1();
        self.stack_height.pop1();

        self.alloc.instruction_set.op_local_get(5); //counr
        self.alloc.instruction_set.op_i32_const(1);
        self.alloc.instruction_set.op_i32_sub();
        self.alloc.instruction_set.op_local_tee(6); //counr
        self.alloc.instruction_set.op_br_if_nez(-89);
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
        _input_type: ValType,
        _output_type: ValType,
        opcode: Opcode,
    ) -> Result<(), CompilationError> {
        self.translate_if_reachable(|builder| {
            builder.bump_fuel_consumption(builder.fuel_costs().base)?;
            builder
                .alloc
                .instruction_set
                .push(opcode, OpcodeData::EmptyData);
            Ok(())
        })
    }

    /// Translates a Wasm reinterpret instruction.
    ///
    /// # Note
    ///
    /// The `rwasm` translation simply ignores reinterpret instructions since
    /// `rwasm` bytecode in itself is untyped.
    fn visit_reinterpret(
        &mut self,
        _input_type: ValType,
        _output_type: ValType,
    ) -> Result<(), CompilationError> {
        Ok(())
    }
}
