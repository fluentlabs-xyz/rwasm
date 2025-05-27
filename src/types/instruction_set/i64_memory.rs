use crate::{split_i64_to_i32, AddressOffset, InstructionSet};

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
        self.op_local_get(1);
        self.op_i32_clz();
        self.op_br_if_eqz(3);
        self.op_i32_const(0);
        self.op_br(2);
        self.op_i32_const(-1);
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
        self.op_local_get(1);
        self.op_i32_clz();
        self.op_br_if_eqz(3);
        self.op_i32_const(0);
        self.op_br(2);
        self.op_i32_const(-1);
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
        self.op_local_get(1);
        self.op_i32_clz();
        self.op_br_if_eqz(3);
        self.op_i32_const(0);
        self.op_br(2);
        self.op_i32_const(-1);
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
}
