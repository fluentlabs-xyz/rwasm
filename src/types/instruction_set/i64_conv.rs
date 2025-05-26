use crate::InstructionSet;

impl InstructionSet {
    pub fn op_i32_wrap_i64(&mut self) {
        self.op_drop();
    }

    pub fn op_i64_extend_i32_s(&mut self) {
        self.op_local_get(1);
        self.op_i32_clz();
        self.op_br_if_eqz(3);
        self.op_i32_const(0);
        self.op_br(2);
        self.op_i32_const(-1);
    }

    pub fn op_i64_extend_i32_u(&mut self) {
        self.op_i32_const(0);
    }

    pub fn op_i64_extend8_s(&mut self) {
        self.op_drop();
        self.op_i32_extend8_s();
        self.op_local_get(1);
        self.op_i32_const(i32::MIN);
        self.op_i32_and();
        self.op_br_if_eqz(3);
        self.op_i32_const(-1_i32);
        self.op_br(2);
        self.op_i32_const(0);
    }

    pub fn op_i64_extend16_s(&mut self) {
        self.op_drop();
        self.op_i32_extend16_s();
        self.op_local_get(1);
        self.op_i32_const(i32::MIN);
        self.op_i32_and();
        self.op_br_if_eqz(3);
        self.op_i32_const(-1_i32);
        self.op_br(2);
        self.op_i32_const(0);
    }

    pub fn op_i64_extend32_s(&mut self) {
        self.op_drop();
        self.op_local_get(1);
        self.op_i32_const(i32::MIN);
        self.op_i32_and();
        self.op_br_if_eqz(3);
        self.op_i32_const(-1_i32);
        self.op_br(2);
        self.op_i32_const(0);
    }
}
