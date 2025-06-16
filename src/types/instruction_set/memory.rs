use crate::{
    split_i64_to_i32,
    AddressOffset,
    InstructionSet,
    MEMORY_BYTES_PER_FUEL,
    N_BYTES_PER_MEMORY_PAGE,
};

impl InstructionSet {
    /// Max stack height: 2
    pub fn op_i64_load<I: Into<AddressOffset>>(&mut self, offset: I) {
        let offset: AddressOffset = offset.into();
        self.op_local_get(1);
        self.op_i32_load(offset.checked_add(4).unwrap_or(u32::MAX));
        self.op_local_get(2);
        self.op_i32_load(offset);
        self.op_local_set(2);
    }

    /// Max stack height: 1
    pub fn op_i64_load8_s<I: Into<AddressOffset>>(&mut self, offset: I) {
        let offset: AddressOffset = offset.into();
        self.op_i32_load8_s(offset);
        self.op_dup();
        self.op_i32_const(31);
        self.op_i32_shr_s();
    }

    /// Max stack height: 1
    pub fn op_i64_load8_u<I: Into<AddressOffset>>(&mut self, offset: I) {
        let offset: AddressOffset = offset.into();
        self.op_i32_load8_u(offset);
        self.op_i32_const(0);
    }

    /// Max stack height: 1
    pub fn op_i64_load16_s<I: Into<AddressOffset>>(&mut self, offset: I) {
        let offset: AddressOffset = offset.into();
        self.op_i32_load16_s(offset);
        self.op_dup();
        self.op_i32_const(31);
        self.op_i32_shr_s();
    }

    /// Max stack height: 1
    pub fn op_i64_load16_u<I: Into<AddressOffset>>(&mut self, offset: I) {
        let offset: AddressOffset = offset.into();
        self.op_i32_load16_u(offset);
        self.op_i32_const(0);
    }

    /// Max stack height: 1
    pub fn op_i64_load32_s<I: Into<AddressOffset>>(&mut self, offset: I) {
        let offset: AddressOffset = offset.into();
        self.op_i32_load(offset);
        self.op_dup();
        self.op_i32_const(31);
        self.op_i32_shr_s();
    }

    pub fn op_i64_load32_u<I: Into<AddressOffset>>(&mut self, offset: I) {
        let offset: AddressOffset = offset.into();
        self.op_i32_load(offset);
        self.op_i32_const(0);
    }

    /// Max stack height:
    pub fn op_i64_store<I: Into<AddressOffset>>(&mut self, offset: I) {
        let offset: AddressOffset = offset.into();
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_store(offset.checked_add(4).unwrap_or(u32::MAX));
        self.op_drop();
        self.op_i32_store(offset);
    }

    /// Max stack height: 0
    pub fn op_i64_store8<I: Into<AddressOffset>>(&mut self, offset: I) {
        let offset: AddressOffset = offset.into();
        self.op_drop();
        self.op_i32_store8(offset);
    }

    /// Max stack height: 0
    pub fn op_i64_store16<I: Into<AddressOffset>>(&mut self, offset: I) {
        let offset: AddressOffset = offset.into();
        self.op_drop();
        self.op_i32_store16(offset);
    }

    /// Max stack height: 0
    pub fn op_i64_store32<I: Into<AddressOffset>>(&mut self, offset: I) {
        let offset: AddressOffset = offset.into();
        self.op_drop();
        self.op_i32_store(offset);
    }

    pub fn op_i64_const(&mut self, value: i64) {
        let (expected_low, expected_high) = split_i64_to_i32(value);
        self.op_i32_const(expected_low);
        self.op_i32_const(expected_high);
    }

    /// Max stack height: 2
    pub fn op_memory_grow_checked(&mut self, max_pages: Option<u32>, bytes_per_fuel: Option<u32>) {
        // we must do max memory check before an execution
        if let Some(max_pages) = max_pages {
            self.op_local_get(1);
            self.op_memory_size();
            self.op_i32_add();
            self.op_i32_const(max_pages);
            self.op_i32_gt_s();
            self.op_br_if_eqz(4);
            self.op_drop();
            self.op_i32_const(u32::MAX);
            let jump_to = bytes_per_fuel.map(|_| 8).unwrap_or(2);
            self.op_br(jump_to);
        }
        // now we know that pages can't exceed i32::MAX,
        // so we can safely multiply the num of pages to the page size
        // to calculate fuel required for memory to grow
        if let Some(fuel_per_byte) = bytes_per_fuel {
            // TODO(dmitry123): "fix `shr_u` params"
            debug_assert_eq!(fuel_per_byte, MEMORY_BYTES_PER_FUEL);
            self.op_local_get(1);
            self.op_i32_const(N_BYTES_PER_MEMORY_PAGE); // size of each memory page
            self.op_i32_mul(); // overflow is impossible here
            self.op_i32_const(6); // 2^6=64
            self.op_i32_shr_u(); // delta/64
            self.op_consume_fuel_stack();
        }
        // emit memory grows only if fuel is charged
        self.op_memory_grow();
    }
}
