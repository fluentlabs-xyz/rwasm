use crate::{InstructionSet, TrapCode};

impl InstructionSet {
    pub const MSH_I64_DIV_S: u32 = 17;

    /// Performs a signed 64-bit integer division using only 32-bit arithmetic,
    /// returning the 64-bit quotient as two `u32` limbs (low, high).
    ///
    /// This routine emulates full signed 64-bit division for platforms or VMs
    /// that lack native 64-bit division, such as WebAssembly VMs with 32-bit stack elements.
    ///
    /// # Arguments
    /// - `n_lo`, `n_hi`: Low and high 32 bits of the dividend (`n = (n_hi << 32) | n_lo`,
    ///   interpreted as signed i64)
    /// - `d_lo`, `d_hi`: Low and high 32 bits of the divisor (`d = (d_hi << 32) | d_lo`,
    ///   interpreted as signed i64)
    ///
    /// # Returns
    /// - `(q_lo, q_hi)`: Low and high 32 bits of the signed quotient
    /// (as if casting a result to i64 and splitting)
    ///
    /// # Algorithm
    /// - Checks for division by zero and triggers a trap if detected.
    /// - Checks for signed division overflow (i64::MIN / -1) and traps on this case, matching
    ///   Rust/WASM semantics.
    /// - Computes absolute values of numerator and denominator.
    /// - Performs unsigned 64-bit division using a dedicated routine (`div64_impl`), producing the
    ///   absolute quotient.
    /// - Applies the correct sign to the quotient according to the signs of the operands
    /// (truncates toward zero).
    ///
    /// # Panics / Traps
    /// - Division by zero triggers a trap.
    /// - Division overflow (i64::MIN / -1) triggers a trap, matching language and VM semantics.
    ///
    /// # Note
    /// This function returns only the quotient, not the remainder.
    /// The quotient is returned in the
    /// same two-limb (low/high) format as the inputs.
    /// Both input and output values should be
    /// interpreted as signed i64.
    ///
    /// Max stack height: 17
    pub fn op_i64_div_s(&mut self) {
        self.op_i32_const(0);
        self.op_i32_const(0);
        self.op_i32_const(0);
        self.op_i32_const(0);
        self.op_i32_const(0);
        self.op_i32_const(0);
        self.op_i32_const(0);
        self.op_i32_const(0);
        self.op_i32_const(0);
        self.op_local_get(10);
        self.op_local_get(12);
        self.op_i32_or();
        self.op_br_if_nez(2);
        self.op_trap(TrapCode::IntegerDivisionByZero);
        self.op_local_get(12);
        self.op_i32_const(-2147483648);
        self.op_i32_ne();
        self.op_br_if_nez(12);
        self.op_local_get(10);
        self.op_i32_const(-1);
        self.op_i32_ne();
        self.op_br_if_nez(8);
        self.op_local_get(13);
        self.op_br_if_nez(43);
        self.op_local_get(11);
        self.op_i32_const(-1);
        self.op_i32_ne();
        self.op_br_if_nez(39);
        self.op_trap(TrapCode::IntegerOverflow);
        self.op_local_get(12);
        self.op_i32_const(-1);
        self.op_i32_gt_s();
        self.op_br_if_nez(17);
        self.op_i32_const(0);
        self.op_local_get(14);
        self.op_i32_sub();
        self.op_local_set(9);
        self.op_local_get(13);
        self.op_i32_eqz();
        self.op_local_get(13);
        self.op_i32_const(-1);
        self.op_i32_xor();
        self.op_i32_add();
        self.op_local_set(8);
        self.op_local_get(10);
        self.op_i32_const(0);
        self.op_i32_lt_s();
        self.op_br_if_nez(28);
        self.op_br(13);
        self.op_local_get(13);
        self.op_local_set(9);
        self.op_local_get(12);
        self.op_local_set(8);
        self.op_local_get(10);
        self.op_i32_const(-1);
        self.op_i32_le_s();
        self.op_br_if_nez(19);
        self.op_local_get(13);
        self.op_local_set(9);
        self.op_local_get(12);
        self.op_local_set(8);
        self.op_local_get(10);
        self.op_local_set(7);
        self.op_local_get(11);
        self.op_local_set(6);
        self.op_br(21);
        self.op_i32_const(2147483647);
        self.op_i32_const(-2147483648);
        self.op_local_get(15);
        self.op_select();
        self.op_local_set(8);
        self.op_i32_const(0);
        self.op_local_get(14);
        self.op_i32_sub();
        self.op_local_set(9);
        self.op_i32_const(0);
        self.op_local_get(12);
        self.op_i32_sub();
        self.op_local_set(6);
        self.op_local_get(11);
        self.op_i32_eqz();
        self.op_local_get(11);
        self.op_i32_const(-1);
        self.op_i32_xor();
        self.op_i32_add();
        self.op_local_set(7);
        self.op_i32_const(0);
        self.op_local_set(11);
        self.op_i32_const(64);
        self.op_local_set(5);
        self.op_i32_const(0);
        self.op_local_set(4);
        self.op_i32_const(0);
        self.op_local_set(3);
        self.op_i32_const(0);
        self.op_local_set(2);
        self.op_local_get(4);
        self.op_i32_const(1);
        self.op_i32_shl();
        self.op_local_get(9);
        self.op_i32_const(31);
        self.op_i32_shr_u();
        self.op_i32_or();
        self.op_local_set(13);
        self.op_local_get(11);
        self.op_i32_const(1);
        self.op_i32_shl();
        self.op_local_get(5);
        self.op_i32_const(31);
        self.op_i32_shr_u();
        self.op_i32_or();
        self.op_local_tee(12);
        self.op_local_get(8);
        self.op_i32_gt_u();
        self.op_br_if_nez(14);
        self.op_i32_const(0);
        self.op_local_set(1);
        self.op_local_get(11);
        self.op_local_get(8);
        self.op_i32_ne();
        self.op_br_if_nez(5);
        self.op_local_get(13);
        self.op_local_get(7);
        self.op_i32_ge_u();
        self.op_br_if_nez(4);
        self.op_local_get(13);
        self.op_local_set(4);
        self.op_br(15);
        self.op_local_get(11);
        self.op_local_get(8);
        self.op_i32_sub();
        self.op_local_get(14);
        self.op_local_get(8);
        self.op_i32_lt_u();
        self.op_i32_sub();
        self.op_local_set(11);
        self.op_local_get(13);
        self.op_local_get(7);
        self.op_i32_sub();
        self.op_local_set(4);
        self.op_i32_const(1);
        self.op_local_set(1);
        self.op_local_get(8);
        self.op_i32_const(1);
        self.op_i32_shl();
        self.op_local_get(10);
        self.op_i32_const(31);
        self.op_i32_shr_u();
        self.op_i32_or();
        self.op_local_set(8);
        self.op_local_get(9);
        self.op_i32_const(1);
        self.op_i32_shl();
        self.op_local_set(9);
        self.op_local_get(3);
        self.op_i32_const(1);
        self.op_i32_shl();
        self.op_local_get(3);
        self.op_i32_const(31);
        self.op_i32_shr_u();
        self.op_i32_or();
        self.op_local_set(3);
        self.op_local_get(1);
        self.op_local_get(3);
        self.op_i32_const(1);
        self.op_i32_shl();
        self.op_i32_or();
        self.op_local_set(2);
        self.op_local_get(5);
        self.op_i32_const(-1);
        self.op_i32_add();
        self.op_local_tee(6);
        self.op_br_if_nez(-76);
        self.op_local_get(10);
        self.op_local_get(13);
        self.op_i32_xor();
        self.op_i32_const(-1);
        self.op_i32_le_s();
        self.op_br_if_nez(4);
        self.op_local_get(2);
        self.op_local_set(11);
        self.op_br(12);
        self.op_i32_const(0);
        self.op_local_get(3);
        self.op_i32_sub();
        self.op_local_set(11);
        self.op_local_get(2);
        self.op_i32_eqz();
        self.op_local_get(4);
        self.op_i32_const(-1);
        self.op_i32_xor();
        self.op_i32_add();
        self.op_local_set(3);
        self.op_local_get(3);
        self.op_i32_const(0);
        self.op_i32_const(32);
        self.op_i32_const(0);
        self.op_local_get(2);
        self.op_i32_const(63);
        self.op_i32_and();
        self.op_local_set(2);
        self.op_local_get(2);
        self.op_br_if_eqz(32);
        self.op_local_get(2);
        self.op_i32_const(31);
        self.op_i32_gt_u();
        self.op_br_if_eqz(10);
        self.op_local_get(4);
        self.op_local_get(3);
        self.op_i32_const(32);
        self.op_i32_sub();
        self.op_i32_shl();
        self.op_local_set(3);
        self.op_i32_const(0);
        self.op_local_set(4);
        self.op_br(19);
        self.op_local_get(4);
        self.op_local_get(3);
        self.op_i32_shl();
        self.op_local_set(4);
        self.op_local_get(3);
        self.op_local_get(3);
        self.op_i32_shl();
        self.op_local_set(3);
        self.op_local_get(4);
        self.op_i32_const(32);
        self.op_local_get(4);
        self.op_i32_const(31);
        self.op_i32_and();
        self.op_i32_sub();
        self.op_i32_shr_u();
        self.op_local_get(4);
        self.op_i32_or();
        self.op_local_set(3);
        self.op_drop();
        self.op_drop();
        self.op_local_get(13);
        self.op_i32_const(0);
        self.op_local_get(3);
        self.op_i32_or();
        self.op_local_set(2);
        self.op_local_get(3);
        self.op_i32_or();
        self.op_local_set(2);
        self.op_local_set(13);
        self.op_local_set(13);
        self.op_drop();
        self.op_drop();
        self.op_drop();
        self.op_drop();
        self.op_drop();
        self.op_drop();
        self.op_drop();
        self.op_drop();
        self.op_drop();
        self.op_drop();
        self.op_drop();
    }
}
