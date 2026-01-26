use crate::{BlockFuel, GlobalIdx, MaxStackHeight, RwasmExecutor, SignatureIdx, Store, TrapCode};

impl<'a, T: Send + Sync> RwasmExecutor<'a, T> {
    #[inline(always)]
    pub(crate) fn visit_consume_fuel(&mut self, block_fuel: BlockFuel) -> Result<(), TrapCode> {
        self.store.try_consume_fuel(block_fuel as u64)?;
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_consume_fuel_stack(&mut self) -> Result<(), TrapCode> {
        let block_fuel: u32 = self.sp.pop_as();
        self.store.try_consume_fuel(block_fuel as u64)?;
        self.ip.add(1);
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

        #[cfg(feature = "tracing")]
        {
            use crate::{
                mem_index::LAST_SIG_ADDR, InstrStateExtension, SignatureCheckStateExtension,
            };

            let mut instr_state = self.store.tracer.logs.pop().unwrap();

            let last_signature_check_read = self.store.tracer.mr(LAST_SIG_ADDR);

            let state_extension = SignatureCheckStateExtension {
                last_signature_check_read,
            };

            instr_state.extension = Some(InstrStateExtension::SignatureCheck(state_extension));

            self.store.tracer.logs.push(instr_state);
        }

        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_stack_check(
        &mut self,
        max_stack_height: MaxStackHeight,
    ) -> Result<(), TrapCode> {
        self.value_stack.sync_stack_ptr(self.sp);
        self.value_stack.reserve(max_stack_height as usize)?;
        // we should rewrite SP after reserve because of potential reallocation
        self.sp = self.value_stack.stack_ptr();
        self.ip.add(1);
        Ok(())
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
}
