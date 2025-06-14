use crate::InstructionSet;

impl InstructionSet {
    /// Max stack height: 0
    pub fn op_i32_wrap_i64(&mut self) {
        self.op_drop(); // drop high
    }

    /// Max stack height: 1
    pub fn op_i64_extend_i32_s(&mut self) {
        self.op_local_get(1); // duplicate for both low and high
        self.op_i32_const(31);
        self.op_i32_shr_s(); // arithmetic shift right → high
    }

    /// Max stack height: 1
    pub fn op_i64_extend_i32_u(&mut self) {
        self.op_i32_const(0); // high = 0
    }

    /// Max stack height: 2
    pub fn op_i64_extend8_s(&mut self) {
        self.op_drop(); // drop old high word
        self.op_i32_extend8_s(); // apply sign-extension to 8-bit low
        self.op_dup(); // copy low → uses to derive high
        self.op_i32_const(31);
        self.op_i32_shr_s(); // high = low >> 31
    }

    /// Max stack height: 2
    pub fn op_i64_extend16_s(&mut self) {
        self.op_drop(); // drop old high word
        self.op_i32_extend16_s(); // apply sign-extension to 8-bit low
        self.op_dup(); // copy low → uses to derive high
        self.op_i32_const(31);
        self.op_i32_shr_s(); // high = low >> 31
    }

    /// Max stack height: 2
    pub fn op_i64_extend32_s(&mut self) {
        self.op_drop(); // drop old high word
        self.op_dup(); // copy low → uses to derive high
        self.op_i32_const(31);
        self.op_i32_shr_s(); // high = low >> 31
    }
}
