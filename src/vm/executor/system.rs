use crate::{
    BlockFuel, GlobalIdx, MaxStackHeight, RwasmExecutor, SignatureIdx, Store, TrapCode,
    UntypedValue,
};

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
            .get(global_idx as usize)
            .copied()
            .unwrap_or_default();
        self.sp.push(global_value);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_global_set(&mut self, global_idx: GlobalIdx) {
        let new_value: UntypedValue = self.sp.pop();
        let expected_cap = global_idx as usize + 1;
        let len = self.store.global_variables.len();
        if expected_cap > len {
            self.store.global_variables.reserve(expected_cap - len);
            let default_elements_count = global_idx as usize - len;
            self.store
                .global_variables
                .extend(core::iter::repeat(UntypedValue::default()).take(default_elements_count));
            self.store.global_variables.push(new_value);
        } else {
            self.store.global_variables[global_idx as usize] = new_value;
        };
        self.ip.add(1);
    }
}
