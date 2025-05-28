use crate::{
    BlockFuel,
    BranchOffset,
    BranchTableTargets,
    CompiledFunc,
    GlobalIdx,
    InstructionPtr,
    LocalDepth,
    MaxStackHeight,
    RwasmExecutor,
    SignatureIdx,
    SysFuncIdx,
    TrapCode,
    UntypedValue,
    NULL_FUNC_IDX,
    N_MAX_RECURSION_DEPTH,
};
use core::cmp;

impl<'a, T> RwasmExecutor<'a, T> {
    #[inline(always)]
    pub(crate) fn visit_unreachable(&mut self) -> Result<(), TrapCode> {
        Err(TrapCode::UnreachableCodeReached)
    }

    #[inline(always)]
    pub(crate) fn visit_trap_code(&mut self, trap_code: TrapCode) -> Result<(), TrapCode> {
        Err(trap_code)
    }

    #[inline(always)]
    pub(crate) fn visit_local_get(&mut self, local_depth: LocalDepth) {
        let value = self.sp.nth_back(local_depth as usize);
        self.sp.push(value);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_local_set(&mut self, local_depth: LocalDepth) {
        let new_value = self.sp.pop();
        self.sp.set_nth_back(local_depth as usize, new_value);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_local_tee(&mut self, local_depth: LocalDepth) {
        let new_value = self.sp.last();
        self.sp.set_nth_back(local_depth as usize, new_value);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_br(&mut self, branch_offset: BranchOffset) {
        self.ip.offset(branch_offset.to_i32() as isize)
    }

    #[inline(always)]
    pub(crate) fn visit_br_if(&mut self, branch_offset: BranchOffset) {
        let condition = self.sp.pop_as();
        if condition {
            self.ip.add(1);
        } else {
            self.ip.offset(branch_offset.to_i32() as isize);
        }
    }

    #[inline(always)]
    pub(crate) fn visit_br_if_nez(&mut self, branch_offset: BranchOffset) {
        let condition = self.sp.pop_as();
        if condition {
            self.ip.offset(branch_offset.to_i32() as isize);
        } else {
            self.ip.add(1);
        }
    }

    #[inline(always)]
    pub(crate) fn visit_br_table(&mut self, targets: BranchTableTargets) {
        let index: u32 = self.sp.pop_as();
        let max_index = targets as usize - 1;
        let normalized_index = cmp::min(index as usize, max_index);
        self.ip.add(2 * normalized_index + 1);
    }

    #[inline(always)]
    pub(crate) fn visit_consume_fuel(&mut self, block_fuel: BlockFuel) -> Result<(), TrapCode> {
        if self.store.config.fuel_enabled {
            self.try_consume_fuel(block_fuel.to_u64())?;
        }
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_consume_fuel_stack(&mut self) -> Result<(), TrapCode> {
        let block_fuel: u32 = self.sp.pop_as();
        if self.store.config.fuel_enabled {
            self.try_consume_fuel(block_fuel as u64)?;
        }
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_return(&mut self) -> bool {
        self.value_stack.sync_stack_ptr(self.sp);
        match self.call_stack.pop() {
            Some(caller) => {
                self.ip = caller;
                false
            }
            None => true,
        }
    }

    #[inline(always)]
    pub(crate) fn visit_return_call_internal(&mut self, compiled_func: CompiledFunc) {
        self.ip.add(1);
        self.value_stack.sync_stack_ptr(self.sp);
        self.sp = self.value_stack.stack_ptr();
        self.ip = InstructionPtr::new(self.module.code_section.instr.as_ptr());
        self.ip.add(compiled_func as usize);
    }

    #[inline(always)]
    pub(crate) fn visit_return_call(&mut self, sys_func_idx: SysFuncIdx) -> Result<bool, TrapCode> {
        self.value_stack.sync_stack_ptr(self.sp);
        // external call can cause interruption,
        // that is why it's important to increase IP before doing the call
        self.ip.add(1);
        self.invoke_syscall(sys_func_idx)
    }

    #[inline(always)]
    pub(crate) fn visit_return_call_indirect(
        &mut self,
        signature_idx: SignatureIdx,
    ) -> Result<(), TrapCode> {
        let table = self.fetch_table_index(1);
        let func_index: u32 = self.sp.pop_as();
        self.store.last_signature = Some(signature_idx);
        let instr_ref: u32 = self
            .store
            .tables
            .get(&table)
            .expect("rwasm: unresolved table index")
            .get_untyped(func_index)
            .ok_or(TrapCode::TableOutOfBounds)?
            .try_into()
            .unwrap();
        if instr_ref == 0 {
            return Err(TrapCode::IndirectCallToNull.into());
        }
        self.ip.add(2);
        self.value_stack.sync_stack_ptr(self.sp);
        self.sp = self.value_stack.stack_ptr();
        self.ip = InstructionPtr::new(self.module.code_section.instr.as_ptr());
        self.ip.add(instr_ref as usize);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_call_internal(
        &mut self,
        compiled_func: CompiledFunc,
    ) -> Result<(), TrapCode> {
        self.ip.add(1);
        self.value_stack.sync_stack_ptr(self.sp);
        if self.call_stack.len() > N_MAX_RECURSION_DEPTH {
            return Err(TrapCode::StackOverflow);
        }
        self.call_stack.push(self.ip);
        self.sp = self.value_stack.stack_ptr();
        self.ip = InstructionPtr::new(self.module.code_section.instr.as_ptr());
        self.ip.add(compiled_func as usize);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_call(&mut self, sys_func_idx: SysFuncIdx) -> Result<bool, TrapCode> {
        self.value_stack.sync_stack_ptr(self.sp);
        // external call can cause interruption,
        // that is why it's important to increase IP before doing the call
        self.ip.add(1);
        self.invoke_syscall(sys_func_idx)
    }

    #[inline(always)]
    pub(crate) fn visit_call_indirect(
        &mut self,
        signature_idx: SignatureIdx,
    ) -> Result<(), TrapCode> {
        // resolve func index
        let table = self.fetch_table_index(1);
        let func_index: u32 = self.sp.pop_as();
        self.store.last_signature = Some(signature_idx);
        let instr_ref = self
            .store
            .tables
            .get(&table)
            .expect("rwasm: unresolved table index")
            .get_untyped(func_index)
            .map(|v| v.as_u32())
            .ok_or(TrapCode::TableOutOfBounds)?;
        if instr_ref == NULL_FUNC_IDX {
            return Err(TrapCode::IndirectCallToNull);
        }
        // call func
        self.ip.add(2);
        self.value_stack.sync_stack_ptr(self.sp);
        if self.call_stack.len() > N_MAX_RECURSION_DEPTH {
            return Err(TrapCode::StackOverflow);
        }
        self.call_stack.push(self.ip);
        self.sp = self.value_stack.stack_ptr();
        self.ip = InstructionPtr::new(self.module.code_section.instr.as_ptr());
        self.ip.add(instr_ref as usize);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_signature_check(
        &mut self,
        signature_idx: SignatureIdx,
    ) -> Result<(), TrapCode> {
        if let Some(actual_signature) = self.store.last_signature.take() {
            if actual_signature != signature_idx {
                return Err(TrapCode::BadSignature);
            }
        }
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_stack_check(
        &mut self,
        max_stack_height: MaxStackHeight,
    ) -> Result<(), TrapCode> {
        self.value_stack.reserve(max_stack_height as usize)?;
        // we should rewrite SP after reserve because of potential reallocation
        self.sp = self.value_stack.stack_ptr();
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_drop(&mut self) {
        self.sp.drop();
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_select(&mut self) {
        self.sp.eval_top3(|e1, e2, e3| {
            let condition = <bool as From<UntypedValue>>::from(e3);
            if condition {
                e1
            } else {
                e2
            }
        });
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_global_get(&mut self, global_idx: GlobalIdx) {
        let global_value = self
            .store
            .global_variables
            .get(&global_idx)
            .copied()
            .unwrap_or_default();
        self.sp.push(global_value);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_global_set(&mut self, global_idx: GlobalIdx) {
        let new_value = self.sp.pop();
        self.store.global_variables.insert(global_idx, new_value);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_ref_func(&mut self, compiled_func: CompiledFunc) {
        self.sp.push_as(compiled_func);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_i32_const(&mut self, untyped_value: UntypedValue) {
        self.sp.push(untyped_value);
        self.ip.add(1);
    }
}
