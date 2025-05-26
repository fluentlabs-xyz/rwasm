use crate::InstructionSet;

impl InstructionSet {
    pub fn op_i64_eqz(&mut self) {
        self.op_i32_eqz();
        self.op_local_get(2);
        self.op_i32_eqz();
        self.op_local_set(2);
        self.op_i32_and();
    }

    pub fn op_i64_eq(&mut self) {
        self.op_i32_and();
    }

    pub fn op_i64_ne(&mut self) {
        self.op_i32_or();
    }

    pub fn op_i64_lt_s(&mut self) {
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_eq();
        self.op_br_if_nez(5);
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_lt_s();
        self.op_br(4);
        self.op_local_get(4);
        self.op_local_get(3);
        self.op_i32_lt_u();
        self.op_local_set(4);
        self.op_drop();
        self.op_drop();
        self.op_drop();
    }

    pub fn op_i64_lt_u(&mut self) {
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_eq();
        self.op_br_if_nez(5);
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_lt_u();
        self.op_br(4);
        self.op_local_get(4);
        self.op_local_get(3);
        self.op_i32_lt_u();
        self.op_local_set(4);
        self.op_drop();
        self.op_drop();
        self.op_drop();
    }

    pub fn op_i64_gt_s(&mut self) {
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_eq();
        self.op_br_if_nez(5);
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_gt_s();
        self.op_br(4);
        self.op_local_get(4);
        self.op_local_get(3);
        self.op_i32_gt_u();
        self.op_local_set(4);
        self.op_drop();
        self.op_drop();
        self.op_drop();
    }

    pub fn op_i64_gt_u(&mut self) {
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_eq();
        self.op_br_if_nez(5);
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_gt_u();
        self.op_br(4);
        self.op_local_get(4);
        self.op_local_get(3);
        self.op_i32_gt_u();
        self.op_local_set(4);
        self.op_drop();
        self.op_drop();
        self.op_drop();
    }

    pub fn op_i64_le_s(&mut self) {
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_eq();
        self.op_br_if_nez(5);
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_le_s();
        self.op_br(4);
        self.op_local_get(4);
        self.op_local_get(3);
        self.op_i32_le_u();
        self.op_local_set(4);
        self.op_drop();
        self.op_drop();
        self.op_drop();
    }

    pub fn op_i64_le_u(&mut self) {
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_eq();
        self.op_br_if_nez(5);
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_le_u();
        self.op_br(4);
        self.op_local_get(4);
        self.op_local_get(3);
        self.op_i32_le_u();
        self.op_local_set(4);
        self.op_drop();
        self.op_drop();
        self.op_drop();
    }

    pub fn op_i64_ge_s(&mut self) {
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_eq();
        self.op_br_if_nez(5);
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_ge_s();
        self.op_br(4);
        self.op_local_get(4);
        self.op_local_get(3);
        self.op_i32_ge_u();
        self.op_local_set(4);
        self.op_drop();
        self.op_drop();
        self.op_drop();
    }

    pub fn op_i64_ge_u(&mut self) {
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_eq();
        self.op_br_if_nez(5);
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_ge_u();
        self.op_br(4);
        self.op_local_get(4);
        self.op_local_get(3);
        self.op_i32_ge_u();
        self.op_local_set(4);
        self.op_drop();
        self.op_drop();
        self.op_drop();
    }
}
