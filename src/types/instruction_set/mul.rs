use crate::InstructionSet;

impl InstructionSet {
    pub const MSH_I64_MUL: u32 = 5;

    /// Max stack height: 5
    pub fn op_i64_mul(&mut self) {
        self.op_local_get(2);
        self.op_local_get(5);
        self.op_i32_mul64();
        self.op_local_get(3);
        self.op_local_get(7);
        self.op_i32_mul();
        self.op_local_get(5);
        self.op_local_get(7);
        self.op_i32_mul();
        self.op_i32_add();
        self.op_i32_add();
        self.op_local_set(4);
        self.op_local_set(4);
        self.op_drop();
        self.op_drop();
    }
}
