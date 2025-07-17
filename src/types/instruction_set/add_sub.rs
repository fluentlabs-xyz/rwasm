use crate::InstructionSet;

impl InstructionSet {
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

    /// Max stack height: 8
    pub fn op_i64_sub(&mut self) {
        // TODO(dmitry123): "looks optimizable"
        self.op_local_get(4);
        self.op_local_get(3);
        self.op_i32_sub();
        self.op_local_get(5);
        self.op_local_get(4);
        self.op_i32_lt_u();
        self.op_local_get(5);
        self.op_local_get(4);
        self.op_i32_sub();
        self.op_local_get(2);
        self.op_i32_sub();
        self.op_local_set(5);
        self.op_drop();
        self.op_local_set(4);
        self.op_drop();
        self.op_drop();
    }
}
