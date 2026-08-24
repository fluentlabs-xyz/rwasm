use crate::InstructionSet;

impl InstructionSet {
    pub const MSH_I64_CLZ: u32 = 3;
    pub const MSH_I64_CTZ: u32 = 3;
    pub const MSH_I64_POPCNT: u32 = 1;
    pub const MSH_I64_AND: u32 = 0;
    pub const MSH_I64_OR: u32 = 0;
    pub const MSH_I64_XOR: u32 = 0;
    pub const MSH_I64_SHL: u32 = 3;
    pub const MSH_I64_SHR_S: u32 = 3;
    pub const MSH_I64_SHR_U: u32 = 3;
    pub const MSH_I64_ROTL: u32 = 4;
    pub const MSH_I64_ROTR: u32 = 4;

    /// Max stack height: 3
    pub fn op_i64_clz(&mut self) {
        self.op_i32_clz();
        self.op_local_get(1);
        self.op_i32_const(32);
        self.op_i32_eq();
        self.op_br_if_eqz(4);
        self.op_local_get(2);
        self.op_i32_clz();
        self.op_i32_add();
        self.op_local_set(1);
        self.op_i32_const(0);
    }

    /// Max stack height: 3
    pub fn op_i64_ctz(&mut self) {
        self.op_local_get(2);
        self.op_i32_ctz();
        self.op_local_get(1);
        self.op_i32_const(32);
        self.op_i32_eq();
        self.op_br_if_eqz(5);
        self.op_local_set(2);
        self.op_i32_ctz();
        self.op_i32_add();
        self.op_br(3);
        self.op_local_set(2);
        self.op_drop();
        self.op_i32_const(0);
    }

    /// Max stack height: 1
    pub fn op_i64_popcnt(&mut self) {
        self.op_i32_popcnt();
        self.op_local_get(2);
        self.op_i32_popcnt();
        self.op_local_set(2);
        self.op_i32_add();
        self.op_i32_const(0);
    }

    /// Max stack height: 0
    pub fn op_i64_and(&mut self) {
        self.op_i32_and64();
    }

    /// Max stack height: 0
    pub fn op_i64_or(&mut self) {
        self.op_i32_or64();
    }

    /// Max stack height: 0
    pub fn op_i64_xor(&mut self) {
        self.op_i32_xor64();
    }

    // The i64 shift/rotate bodies below share one convention: entry stack (top first) is
    // [n_hi, n_lo, x_hi, x_lo]; exit stack is [res_hi, res_lo]. Each body first reduces the
    // shift amount to n = n_lo & 63 (n_hi is dropped, wasm ignores it), then branches once on
    // n < 32. Within an arm the code is branchless: i32 shifts mask their amount by 31, so
    // expressions like `x << n` (n in 32..63) and `y >> (31 - n)` (31 - n negative) reduce to
    // the intended `x << (n - 32)` and `y >> (63 - n)` automatically. Cross-limb carries use
    // `(y >> 1) >> (31 - n)` instead of `y >> (32 - n)` so that n = 0 yields 0 instead of a
    // full-width shift.

    /// Max stack height: 3
    pub fn op_i64_shl(&mut self) {
        // [n_hi, n_lo, x_hi, x_lo]
        self.op_drop();
        self.op_i32_const(63);
        self.op_i32_and(); // [n, x_hi, x_lo]
        self.op_local_get(1);
        self.op_i32_const(32);
        self.op_i32_lt_u();
        self.op_br_if_eqz(19); // n in 32..63
                               // n in 0..31: res_hi = (x_hi << n) | ((x_lo >> 1) >> (31 - n)); res_lo = x_lo << n
        self.op_local_get(2); // x_hi
        self.op_local_get(2); // n
        self.op_i32_shl();
        self.op_local_get(4); // x_lo
        self.op_i32_const(1);
        self.op_i32_shr_u();
        self.op_i32_const(31);
        self.op_local_get(4); // n
        self.op_i32_sub();
        self.op_i32_shr_u();
        self.op_i32_or(); // res_hi
        self.op_local_set(2); // [n, res_hi, x_lo]
        self.op_local_get(3); // x_lo
        self.op_local_get(2); // n
        self.op_i32_shl(); // res_lo
        self.op_local_set(3); // [n, res_hi, res_lo]
        self.op_drop();
        self.op_br(8);
        // n in 32..63: res_hi = x_lo << (n - 32); res_lo = 0
        self.op_local_get(3); // x_lo
        self.op_local_get(2); // n
        self.op_i32_shl(); // res_hi
        self.op_local_set(2); // [n, res_hi, x_lo]
        self.op_i32_const(0);
        self.op_local_set(3); // [n, res_hi, 0]
        self.op_drop();
    }

    /// Max stack height: 3
    pub fn op_i64_shr_s(&mut self) {
        // [n_hi, n_lo, x_hi, x_lo]
        self.op_drop();
        self.op_i32_const(63);
        self.op_i32_and(); // [n, x_hi, x_lo]
        self.op_local_get(1);
        self.op_i32_const(32);
        self.op_i32_lt_u();
        self.op_br_if_eqz(19); // n in 32..63
                               // n in 0..31: res_lo = (x_lo >> n) | ((x_hi << 1) << (31 - n)); res_hi = x_hi >>s n
        self.op_local_get(3); // x_lo
        self.op_local_get(2); // n
        self.op_i32_shr_u();
        self.op_local_get(3); // x_hi
        self.op_i32_const(1);
        self.op_i32_shl();
        self.op_i32_const(31);
        self.op_local_get(4); // n
        self.op_i32_sub();
        self.op_i32_shl();
        self.op_i32_or(); // res_lo
        self.op_local_set(3); // [n, x_hi, res_lo]
        self.op_local_get(2); // x_hi
        self.op_local_get(2); // n
        self.op_i32_shr_s(); // res_hi
        self.op_local_set(2); // [n, res_hi, res_lo]
        self.op_drop();
        self.op_br(10);
        // n in 32..63: res_lo = x_hi >>s (n - 32); res_hi = x_hi >>s 31
        self.op_local_get(2); // x_hi
        self.op_local_get(2); // n
        self.op_i32_shr_s(); // res_lo
        self.op_local_set(3); // [n, x_hi, res_lo]
        self.op_local_get(2); // x_hi
        self.op_i32_const(31);
        self.op_i32_shr_s(); // res_hi
        self.op_local_set(2); // [n, res_hi, res_lo]
        self.op_drop();
    }

    /// Max stack height: 3
    pub fn op_i64_shr_u(&mut self) {
        // [n_hi, n_lo, x_hi, x_lo]
        self.op_drop();
        self.op_i32_const(63);
        self.op_i32_and(); // [n, x_hi, x_lo]
        self.op_local_get(1);
        self.op_i32_const(32);
        self.op_i32_lt_u();
        self.op_br_if_eqz(19); // n in 32..63
                               // n in 0..31: res_lo = (x_lo >> n) | ((x_hi << 1) << (31 - n)); res_hi = x_hi >> n
        self.op_local_get(3); // x_lo
        self.op_local_get(2); // n
        self.op_i32_shr_u();
        self.op_local_get(3); // x_hi
        self.op_i32_const(1);
        self.op_i32_shl();
        self.op_i32_const(31);
        self.op_local_get(4); // n
        self.op_i32_sub();
        self.op_i32_shl();
        self.op_i32_or(); // res_lo
        self.op_local_set(3); // [n, x_hi, res_lo]
        self.op_local_get(2); // x_hi
        self.op_local_get(2); // n
        self.op_i32_shr_u(); // res_hi
        self.op_local_set(2); // [n, res_hi, res_lo]
        self.op_drop();
        self.op_br(8);
        // n in 32..63: res_lo = x_hi >> (n - 32); res_hi = 0
        self.op_local_get(2); // x_hi
        self.op_local_get(2); // n
        self.op_i32_shr_u(); // res_lo
        self.op_local_set(3); // [n, x_hi, res_lo]
        self.op_i32_const(0);
        self.op_local_set(2); // [n, 0, res_lo]
        self.op_drop();
    }

    /// Max stack height: 4
    pub fn op_i64_rotl(&mut self) {
        // [n_hi, n_lo, x_hi, x_lo]
        self.op_drop();
        self.op_i32_const(63);
        self.op_i32_and(); // [n, x_hi, x_lo]
        self.op_local_get(1);
        self.op_i32_const(32);
        self.op_i32_lt_u();
        self.op_br_if_nez(5); // n in 0..31: skip the limb swap
                              // rotl by n in 32..63 == swap limbs, then rotl by n - 32 (shifts self-mask below)
        self.op_local_get(3);
        self.op_local_get(3);
        self.op_local_set(4);
        self.op_local_set(2); // [n, X, Y]: X = hi limb source, Y = lo limb source
                              // res_hi = (X << n) | ((Y >> 1) >> (31 - n))
        self.op_local_get(2); // X
        self.op_local_get(2); // n
        self.op_i32_shl();
        self.op_local_get(4); // Y
        self.op_i32_const(1);
        self.op_i32_shr_u();
        self.op_i32_const(31);
        self.op_local_get(4); // n
        self.op_i32_sub();
        self.op_i32_shr_u();
        self.op_i32_or(); // [res_hi, n, X, Y]
                          // res_lo = (Y << n) | ((X >> 1) >> (31 - n))
        self.op_local_get(4); // Y
        self.op_local_get(3); // n
        self.op_i32_shl();
        self.op_local_get(4); // X
        self.op_i32_const(1);
        self.op_i32_shr_u();
        self.op_i32_const(31);
        self.op_local_get(5); // n
        self.op_i32_sub();
        self.op_i32_shr_u();
        self.op_i32_or(); // [res_lo, res_hi, n, X, Y]
        self.op_local_set(4); // [res_hi, n, X, res_lo]
        self.op_local_set(2); // [n, res_hi, res_lo]
        self.op_drop();
    }

    /// Max stack height: 4
    pub fn op_i64_rotr(&mut self) {
        // [n_hi, n_lo, x_hi, x_lo]
        self.op_drop();
        self.op_i32_const(63);
        self.op_i32_and(); // [n, x_hi, x_lo]
        self.op_local_get(1);
        self.op_i32_const(32);
        self.op_i32_lt_u();
        self.op_br_if_nez(5); // n in 0..31: skip the limb swap
                              // rotr by n in 32..63 == swap limbs, then rotr by n - 32 (shifts self-mask below)
        self.op_local_get(3);
        self.op_local_get(3);
        self.op_local_set(4);
        self.op_local_set(2); // [n, X, Y]: X = hi limb source, Y = lo limb source
                              // res_hi = (X >> n) | ((Y << 1) << (31 - n))
        self.op_local_get(2); // X
        self.op_local_get(2); // n
        self.op_i32_shr_u();
        self.op_local_get(4); // Y
        self.op_i32_const(1);
        self.op_i32_shl();
        self.op_i32_const(31);
        self.op_local_get(4); // n
        self.op_i32_sub();
        self.op_i32_shl();
        self.op_i32_or(); // [res_hi, n, X, Y]
                          // res_lo = (Y >> n) | ((X << 1) << (31 - n))
        self.op_local_get(4); // Y
        self.op_local_get(3); // n
        self.op_i32_shr_u();
        self.op_local_get(4); // X
        self.op_i32_const(1);
        self.op_i32_shl();
        self.op_i32_const(31);
        self.op_local_get(5); // n
        self.op_i32_sub();
        self.op_i32_shl();
        self.op_i32_or(); // [res_lo, res_hi, n, X, Y]
        self.op_local_set(4); // [res_hi, n, X, res_lo]
        self.op_local_set(2); // [n, res_hi, res_lo]
        self.op_drop();
    }
}
