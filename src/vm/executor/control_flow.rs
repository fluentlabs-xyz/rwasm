use crate::{
    BranchOffset, BranchTableTargets, CompiledFunc, InstructionPtr, RwasmExecutor, SignatureIdx,
    SysFuncIdx, TrapCode, NULL_FUNC_IDX, N_MAX_RECURSION_DEPTH,
};
use core::cmp;

impl<'a, T: Send + Sync> RwasmExecutor<'a, T> {
    #[inline(always)]
    pub(crate) fn visit_unreachable(&mut self) -> Result<(), TrapCode> {
        Err(TrapCode::UnreachableCodeReached)
    }

    #[inline(always)]
    pub(crate) fn visit_trap_code(&mut self, trap_code: TrapCode) -> Result<(), TrapCode> {
        Err(trap_code)
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
    pub(crate) fn visit_return(&mut self) -> bool {
        self.value_stack.sync_stack_ptr(self.sp);
        match self.call_stack.pop() {
            Some(ip) => {
                #[cfg(feature = "tracing")]
                {
                    use crate::{
                        mem::MemoryRecordEnum, mem_index::TypedAddress, CallStateExtension,
                        InstrStateExtension,
                    };

                    let mut instr_state = self.store.tracer.logs.pop().unwrap();

                    let call_stack_address =
                        TypedAddress::FuncFrame(self.store.tracer.state.call_sp).to_virtual_addr();

                    self.store.tracer.state.call_sp -= 1;

                    let state_extension = CallStateExtension {
                        call_stack_access: MemoryRecordEnum::Read(
                            self.store.tracer.mr(call_stack_address),
                        ),
                        table_idx: 0,
                        table_size_read: None,
                        table_read: None,
                        call_stack_address,
                    };

                    instr_state.extension = Some(InstrStateExtension::Call(state_extension));

                    self.store.tracer.logs.push(instr_state);
                }

                self.ip = ip;
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
        self.ip = InstructionPtr::new(self.module.code_section.as_ptr());
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
        self.ip = InstructionPtr::new(self.module.code_section.as_ptr());
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
        self.ip = InstructionPtr::new(self.module.code_section.as_ptr());
        self.ip.add(compiled_func as usize);

        #[cfg(feature = "tracing")]
        {
            use crate::{
                mem::MemoryRecordEnum, mem_index::TypedAddress, CallStateExtension,
                InstrStateExtension,
            };

            let mut instr_state = self.store.tracer.logs.pop().unwrap();

            self.store.tracer.state.next_cycle();

            let call_stack_address =
                TypedAddress::FuncFrame(self.store.tracer.state.call_sp).to_virtual_addr();

            self.store.tracer.state.call_sp += 1;

            let call_stack_write = self.store.tracer.mw(call_stack_address, instr_state.pc + 1);

            let state_extension = CallStateExtension {
                call_stack_access: MemoryRecordEnum::Write(call_stack_write),
                table_idx: 0,
                table_size_read: None,
                table_read: None,
                call_stack_address,
            };

            instr_state.extension = Some(InstrStateExtension::Call(state_extension));

            self.store.tracer.logs.push(instr_state);
        }

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
        let table_idx = self.fetch_table_index(1);
        let func_index: u32 = self.sp.pop_as();
        self.store.last_signature = Some(signature_idx);
        let instr_ref = self
            .store
            .tables
            .get(&table_idx)
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
        self.ip = InstructionPtr::new(self.module.code_section.as_ptr());
        self.ip.add(instr_ref as usize);

        #[cfg(feature = "tracing")]
        {
            use crate::{
                mem::MemoryRecordEnum, mem_index::TypedAddress, CallStateExtension,
                InstrStateExtension, N_MAX_TABLE_SIZE,
            };

            let mut instr_state = self.store.tracer.logs.pop().unwrap();

            let table_size_read = self
                .store
                .tracer
                .mr(TypedAddress::TableSize(table_idx as u32).to_virtual_addr());

            let addr = TypedAddress::Table(table_idx as u32 * N_MAX_TABLE_SIZE + func_index);
            let table_read = self.store.tracer.mr(addr.to_virtual_addr());

            self.store.tracer.state.next_cycle();

            let call_stack_address =
                TypedAddress::FuncFrame(self.store.tracer.state.call_sp).to_virtual_addr();

            self.store.tracer.state.call_sp += 1;

            let call_stack_write = self.store.tracer.mw(call_stack_address, instr_state.pc + 2);

            let state_extension = CallStateExtension {
                call_stack_access: MemoryRecordEnum::Write(call_stack_write),
                table_idx: 0,
                table_size_read: None,
                table_read: None,
                call_stack_address,
            };

            instr_state.extension = Some(InstrStateExtension::Call(state_extension));

            self.store.tracer.logs.push(instr_state);
        }

        Ok(())
    }
}
