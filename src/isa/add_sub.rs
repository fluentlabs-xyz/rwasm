use crate::InstructionSet;

impl InstructionSet {
    pub const MSH_I64_ADD: u32 = 4;
    pub const MSH_I64_SUB: u32 = 4;

    /// Max stack height: 4
    pub fn op_i64_add(&mut self) {
        self.op_local_get(4);
        self.op_local_get(3);
        self.op_i32_add64();
        self.op_local_get(5);
        self.op_local_get(4);
        self.op_i32_add();
        self.op_i32_add();
        self.op_local_set(4);
        self.op_local_set(4);
        self.op_drop();
        self.op_drop();
    }

    /// Max stack height: 4
    pub fn op_i64_sub(&mut self) {
        self.op_local_get(4);
        self.op_local_get(3);
        self.op_i32_sub64();
        self.op_local_get(5);
        self.op_local_get(4);
        self.op_i32_sub();
        self.op_i32_add();
        self.op_local_set(4);
        self.op_local_set(4);
        self.op_drop();
        self.op_drop();
    }
}
