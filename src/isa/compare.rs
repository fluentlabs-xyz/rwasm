use crate::InstructionSet;

impl InstructionSet {
    pub const MSH_I64_EQZ: u32 = 0;
    pub const MSH_I64_EQ: u32 = 1;
    pub const MSH_I64_NE: u32 = 1;
    pub const MSH_I64_LT_S: u32 = 2;
    pub const MSH_I64_LT_U: u32 = 2;
    pub const MSH_I64_GT_S: u32 = 2;
    pub const MSH_I64_GT_U: u32 = 2;
    pub const MSH_I64_LE_S: u32 = 2;
    pub const MSH_I64_LE_U: u32 = 2;
    pub const MSH_I64_GE_S: u32 = 2;
    pub const MSH_I64_GE_U: u32 = 2;

    /// Max stack height: 0
    pub fn op_i64_eqz(&mut self) {
        // [hi, lo] -> [(hi | lo) == 0]
        self.op_i32_or();
        self.op_i32_eqz();
    }

    /// Max stack height: 1
    pub fn op_i64_eq(&mut self) {
        self.op_local_get(3);
        self.op_i32_eq();
        self.op_local_set(2);
        self.op_local_get(3);
        self.op_i32_eq();
        self.op_local_set(2);
        self.op_i32_and();
    }

    /// Max stack height: 1
    pub fn op_i64_ne(&mut self) {
        self.op_local_get(3);
        self.op_i32_ne();
        self.op_local_set(2);
        self.op_local_get(3);
        self.op_i32_ne();
        self.op_local_set(2);
        self.op_i32_or();
    }

    /// Branchless i64 ordered comparison:
    ///
    /// ```text
    /// result = (lhs_hi == rhs_hi) ? (lhs_lo cmp_lo rhs_lo) : (lhs_hi cmp_hi rhs_hi)
    /// ```
    ///
    /// `cmp_hi` must be the strict signed/unsigned compare of the operation and `cmp_lo` the
    /// (possibly non-strict) unsigned compare, e.g. `le_s` passes `i32.lt_s`/`i32.le_u`.
    ///
    /// Entry stack (top first): [rhs_hi, rhs_lo, lhs_hi, lhs_lo]; exit: [result].
    /// Max stack height: 2
    fn op_i64_ordered_cmp(&mut self, cmp_hi: fn(&mut Self), cmp_lo: fn(&mut Self)) {
        self.op_local_get(4); // [lhs_lo, rhs_hi, rhs_lo, lhs_hi, lhs_lo]
        self.op_local_get(3); // [rhs_lo, lhs_lo, rhs_hi, rhs_lo, lhs_hi, lhs_lo]
        cmp_lo(self); // [c_lo, rhs_hi, rhs_lo, lhs_hi, lhs_lo]
        self.op_local_set(4); // [rhs_hi, rhs_lo, lhs_hi, c_lo]
        self.op_local_get(3); // [lhs_hi, rhs_hi, rhs_lo, lhs_hi, c_lo]
        self.op_local_set(2); // [rhs_hi, lhs_hi, lhs_hi, c_lo]
        self.op_local_get(2); // [lhs_hi, rhs_hi, lhs_hi, lhs_hi, c_lo]
        self.op_local_get(2); // [rhs_hi, lhs_hi, rhs_hi, lhs_hi, lhs_hi, c_lo]
        cmp_hi(self); // [c_hi, rhs_hi, lhs_hi, lhs_hi, c_lo]
        self.op_local_set(3); // [rhs_hi, lhs_hi, c_hi, c_lo]
        self.op_i32_eq(); // [eq_hi, c_hi, c_lo]
        self.op_select(); // [eq_hi ? c_lo : c_hi]
    }

    /// Max stack height: 2
    pub fn op_i64_lt_s(&mut self) {
        self.op_i64_ordered_cmp(Self::op_i32_lt_s, Self::op_i32_lt_u);
    }

    /// Max stack height: 2
    pub fn op_i64_lt_u(&mut self) {
        self.op_i64_ordered_cmp(Self::op_i32_lt_u, Self::op_i32_lt_u);
    }

    /// Max stack height: 2
    pub fn op_i64_gt_s(&mut self) {
        self.op_i64_ordered_cmp(Self::op_i32_gt_s, Self::op_i32_gt_u);
    }

    /// Max stack height: 2
    pub fn op_i64_gt_u(&mut self) {
        self.op_i64_ordered_cmp(Self::op_i32_gt_u, Self::op_i32_gt_u);
    }

    /// Max stack height: 2
    pub fn op_i64_le_s(&mut self) {
        self.op_i64_ordered_cmp(Self::op_i32_lt_s, Self::op_i32_le_u);
    }

    /// Max stack height: 2
    pub fn op_i64_le_u(&mut self) {
        self.op_i64_ordered_cmp(Self::op_i32_lt_u, Self::op_i32_le_u);
    }

    /// Max stack height: 2
    pub fn op_i64_ge_s(&mut self) {
        self.op_i64_ordered_cmp(Self::op_i32_gt_s, Self::op_i32_ge_u);
    }

    /// Max stack height: 2
    pub fn op_i64_ge_u(&mut self) {
        self.op_i64_ordered_cmp(Self::op_i32_gt_u, Self::op_i32_ge_u);
    }
}
