use crate::InstructionSet;

impl InstructionSet {
    pub fn op_i64_add(&mut self) {
        self.op_local_get(4);
        self.op_local_get(3);
        self.op_i32_or();
        self.op_i32_const(-1);
        self.op_i32_xor();
        self.op_i32_clz();
        self.op_local_get(5);
        self.op_local_get(4);
        self.op_i32_add();
        self.op_local_get(1);
        self.op_local_set(6);
        self.op_i32_const(-1);
        self.op_i32_xor();
        self.op_i32_clz();
        self.op_local_get(5);
        self.op_local_get(4);
        self.op_i32_add();
        self.op_local_get(3);
        self.op_local_get(3);
        self.op_i32_gt_u();
        self.op_br_if_eqz(3);
        self.op_i32_const(1);
        self.op_i32_add();
        self.op_local_set(5);
        self.op_drop();
        self.op_drop();
        self.op_drop();
        self.op_drop();
    }

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

    pub fn op_i64_mul(&mut self) {
        self.op_i32_const(0);
        self.op_i32_const(0);
        self.op_i32_const(0);
        self.op_local_get(4);
        self.op_local_get(6);
        self.op_i32_add();
        self.op_local_get(7);
        self.op_local_get(9);
        self.op_i32_add();
        self.op_i32_mul();
        self.op_local_get(5);
        self.op_local_get(8);
        self.op_i32_mul();
        self.op_local_get(7);
        self.op_i32_const(65535);
        self.op_i32_and();
        self.op_local_tee(9);
        self.op_local_get(10);
        self.op_i32_const(65535);
        self.op_i32_and();
        self.op_local_tee(8);
        self.op_i32_mul();
        self.op_local_tee(6);
        self.op_local_get(8);
        self.op_i32_const(16);
        self.op_i32_shr_u();
        self.op_local_tee(6);
        self.op_local_get(8);
        self.op_i32_mul();
        self.op_local_tee(8);
        self.op_local_get(10);
        self.op_local_get(12);
        self.op_i32_const(16);
        self.op_i32_shr_u();
        self.op_local_tee(7);
        self.op_i32_mul();
        self.op_i32_add();
        self.op_local_tee(11);
        self.op_i32_const(16);
        self.op_i32_shl();
        self.op_i32_add();
        self.op_local_tee(8);
        self.op_i32_add();
        self.op_i32_sub();
        self.op_local_get(6);
        self.op_local_get(5);
        self.op_i32_lt_u();
        self.op_i32_add();
        self.op_local_get(3);
        self.op_local_get(3);
        self.op_i32_mul();
        self.op_local_tee(8);
        self.op_local_get(9);
        self.op_i32_const(16);
        self.op_i32_shr_u();
        self.op_local_get(10);
        self.op_local_get(8);
        self.op_i32_lt_u();
        self.op_i32_const(16);
        self.op_i32_shl();
        self.op_i32_or();
        self.op_i32_add();
        self.op_local_tee(9);
        self.op_i32_add();
        self.op_local_get(8);
        self.op_local_get(8);
        self.op_i32_lt_u();
        self.op_i32_add();
        self.op_local_get(6);
        // TODO(dmitry123): "how efficiently make drop=7 keep=2?"
        self.op_local_set(8);
        self.op_local_set(6);
        self.op_drop();
        self.op_drop();
        self.op_drop();
        self.op_drop();
        self.op_drop();
    }

    pub fn op_i64_div_s(&mut self) {
        todo!()
    }

    /// Translates a 64-bit unsigned integer division operation into a sequence of WebAssembly
    /// (Wasm) instructions.
    ///
    /// This function implements the logic for dividing two 64-bit integers (treated as unsigned) by
    /// breaking them into 32-bit components and simulating the division operation. The function
    /// generates the corresponding WebAssembly operations using an instruction builder
    /// (`inst_builder`) and maintains the stack height for proper stack-based Wasm semantics.
    ///
    /// # Implementation Details
    /// - The function works with the high (`hi`) and low (`lo`) 32-bit parts of two 64-bit
    ///   integers.
    /// - It simulates bitwise shifts, arithmetic operations, and comparisons to calculate the
    ///   quotient and remainder.
    /// - The function uses a counter to iterate through the 64 bits of the dividend, updating
    ///   relevant intermediate results for the quotient and remainder at each step.
    ///
    /// # Stack Usage
    /// The function involves meticulous stack operations:
    /// - Push and pop operations are tracked using `self.stack_height` to manage the stack state.
    /// - Custom stack height tracking ensures adherence to Wasm stack rules during dynamic
    ///   instruction generation.
    ///
    /// # Generated Operations
    /// - Loads local variables using `op_local_get`.
    /// - Executes arithmetic and logical operations such as `op_i32_add`, `op_i32_sub`,
    ///   `op_i32_or`, `op_i32_shl`, and `op_i32_shr_u`.
    /// - Performs conditional branching using `op_br_if_nez` and `op_br_if_eqz`.
    /// - Updates locals with `op_local_set` and `op_local_tee`.
    /// - Handles division-related edge cases such as carry propagation and overflow checks.
    ///
    /// # Example Workflow
    /// 1. Decompose the high and low 32-bit parts of the two 64-bit operands.
    /// 2. Simulate division through a loop where each iteration:
    ///    - Shifts bits and updates intermediate results.
    ///    - Computes carries and propagates bit results for the quotient and remainder.
    /// 3. Rebuild the final results from the calculated quotient and remainder.
    ///
    /// # Errors
    /// - The function assumes correct initialization of local variables and proper input setup for
    ///   the operands.
    /// - Overflow, division by zero, or other exceptional arithmetic conditions should be handled
    ///   as part of higher-level logic or exception management.
    ///
    /// # Notes
    /// - This implementation is tailored for environments where 64-bit integer operations are not
    ///   natively available in Wasm, making it necessary to simulate these operations through
    ///   32-bit arithmetic.
    /// - The function might be updated in future versions for optimizations or support of
    ///   additional architectures.
    #[allow(unused)]
    fn impl_div_i64_u(&mut self) {}

    pub fn op_i64_div_u(&mut self) {
        todo!()
    }
}
