//! Hand-written i64 division/remainder snippets built around one shared unsigned core.
//!
//! The four public emitters (`op_i64_div_u`, `op_i64_rem_u`, `op_i64_div_s`, `op_i64_rem_s`)
//! used to carry four independent machine-traced copies of 64-bit long division totaling
//! ~1,400 instructions per module. They are now thin wrappers around [`op_udivmod64`], a single
//! unsigned divide-and-remainder routine.
//!
//! Calling convention (all values are 32-bit limbs, lo pushed before hi):
//!
//! - `op_udivmod64`: consumes `[n_lo, n_hi, d_lo, d_hi]`, produces `[q_lo, q_hi, r_lo, r_hi]`
//!   where `q = n / d` and `r = n % d` (unsigned). Traps with `IntegerDivisionByZero` when
//!   `d == 0`.
//! - the four public snippets keep the wasm `i64` two-limb convention: consume
//!   `[n_lo, n_hi, d_lo, d_hi]`, produce the two result limbs.
//!
//! In snippet mode the wrappers reach the core via `CallInternal`; the callee index is patched
//! in by `emit_snippets` (see `Snippet::dependencies`). The `*_inline` variants splice the core
//! body in place for the snippets-off compilation mode.

use super::asm::SnippetAsm;
use crate::{InstructionSet, TrapCode};

/// Emits the shared unsigned 64-bit divide-and-remainder body.
///
/// Transforms the top four stack slots `[n_lo, n_hi, d_lo, d_hi]` into
/// `[q_lo, q_hi, r_lo, r_hi]`. Contains no `Return`; every exit path falls through to the end,
/// so the body can be used both as a called snippet and spliced inline.
///
/// Three paths:
/// - `d_hi == 0 && n_hi == 0`: native 32-bit `I32DivU`/`I32RemU`.
/// - `d_hi == 0`: 64÷32 division — the high dividend limb via native 32-bit div/rem, the low
///   limb via 32 iterations of restoring binary division on a 32-bit remainder.
/// - `d_hi != 0`: the quotient fits 32 bits, so 32 iterations of restoring binary division on a
///   64-bit remainder seeded with `n_hi`.
fn emit_udivmod64(is: &mut InstructionSet) -> u32 {
    let mut asm = SnippetAsm::new(is, &["n_lo", "n_hi", "d_lo", "d_hi"]);

    // Trap on a zero divisor.
    asm.get("d_lo");
    asm.get("d_hi");
    asm.i32_or();
    asm.br_if_nez("nonzero_divisor");
    asm.trap(TrapCode::IntegerDivisionByZero);
    asm.label("nonzero_divisor");

    // Dispatch on operand width.
    asm.get("d_hi");
    asm.br_if_nez("wide_divisor");
    asm.get("n_hi");
    asm.br_if_nez("div_64_by_32");

    // --- 32÷32: both operands fit in 32 bits, use native division. ---
    // `n_hi` and `d_hi` are already zero, which are exactly `q_hi` and `r_hi`.
    asm.get("n_lo");
    asm.get("d_lo");
    asm.i32_div_u();
    asm.get("n_lo");
    asm.get("d_lo");
    asm.i32_rem_u();
    asm.set("d_lo"); // r_lo
    asm.set("n_lo"); // q_lo
    asm.br("end");

    // --- 64÷32: divisor fits in 32 bits, dividend does not. ---
    asm.label("div_64_by_32");
    asm.get("n_hi");
    asm.get("d_lo");
    asm.i32_div_u();
    asm.def("q_hi");
    asm.i32_const(0);
    asm.def("q_lo");
    asm.get("n_hi");
    asm.get("d_lo");
    asm.i32_rem_u();
    asm.def("rem"); // 32-bit running remainder, invariant: rem < d_lo
    asm.i32_const(32);
    asm.def("i");
    asm.label("loop_64_32");
    {
        // Shift the next dividend bit into the remainder. The shifted-out bit 31 of `rem`
        // ("carry") means the true remainder exceeds 32 bits and is therefore >= d_lo.
        asm.get("rem");
        asm.i32_const(31);
        asm.i32_shr_u();
        asm.def("carry");
        asm.get("rem");
        asm.i32_const(1);
        asm.i32_shl();
        asm.get("n_lo");
        asm.i32_const(31);
        asm.i32_shr_u();
        asm.i32_or();
        asm.set("rem");
        asm.get("n_lo");
        asm.i32_const(1);
        asm.i32_shl();
        asm.set("n_lo");
        asm.get("q_lo");
        asm.i32_const(1);
        asm.i32_shl();
        asm.set("q_lo");
        // Subtract the divisor when the (33-bit) remainder reaches it.
        asm.get("rem");
        asm.get("d_lo");
        asm.i32_ge_u();
        asm.get("carry");
        asm.i32_or();
        asm.br_if_eqz("skip_64_32");
        asm.get("rem");
        asm.get("d_lo");
        asm.i32_sub();
        asm.set("rem");
        asm.get("q_lo");
        asm.i32_const(1);
        asm.i32_or();
        asm.set("q_lo");
        asm.label("skip_64_32");
        asm.drop(); // carry
        asm.get("i");
        asm.i32_const(-1);
        asm.i32_add();
        asm.tee("i");
        asm.br_if_nez("loop_64_32");
    }
    asm.drop(); // i
    asm.set("d_lo"); // r_lo = rem
    asm.set("n_lo"); // q_lo
    asm.set("n_hi"); // q_hi
    asm.i32_const(0);
    asm.set("d_hi"); // r_hi = 0
    asm.br("end");

    // --- 64÷64: divisor is at least 2^32, so the quotient fits in 32 bits. ---
    // Restoring binary division over the 32 low dividend bits with a 64-bit remainder seeded
    // with `n_hi` (the 32 high-bit iterations cannot subtract: remainder < 2^32 <= d).
    asm.label("wide_divisor");
    asm.i32_const(0);
    asm.def("q_lo");
    asm.get("n_hi");
    asm.def("r_lo");
    asm.i32_const(0);
    asm.def("r_hi");
    asm.i32_const(32);
    asm.def("i");
    asm.label("loop_64_64");
    {
        // 64-bit left shift of the remainder pulling in the next dividend bit; the shifted-out
        // bit ("carry") means the true remainder exceeds 64 bits and is therefore >= d.
        asm.get("r_hi");
        asm.i32_const(31);
        asm.i32_shr_u();
        asm.def("carry");
        asm.get("r_hi");
        asm.i32_const(1);
        asm.i32_shl();
        asm.get("r_lo");
        asm.i32_const(31);
        asm.i32_shr_u();
        asm.i32_or();
        asm.set("r_hi");
        asm.get("r_lo");
        asm.i32_const(1);
        asm.i32_shl();
        asm.get("n_lo");
        asm.i32_const(31);
        asm.i32_shr_u();
        asm.i32_or();
        asm.set("r_lo");
        asm.get("n_lo");
        asm.i32_const(1);
        asm.i32_shl();
        asm.set("n_lo");
        asm.get("q_lo");
        asm.i32_const(1);
        asm.i32_shl();
        asm.set("q_lo");
        // cond = carry | (r_hi > d_hi) | (r_hi == d_hi && r_lo >= d_lo)
        asm.get("r_lo");
        asm.get("d_lo");
        asm.i32_ge_u();
        asm.get("r_hi");
        asm.get("d_hi");
        asm.i32_eq();
        asm.i32_and();
        asm.get("r_hi");
        asm.get("d_hi");
        asm.i32_gt_u();
        asm.i32_or();
        asm.get("carry");
        asm.i32_or();
        asm.br_if_eqz("skip_64_64");
        // r -= d with borrow.
        asm.get("r_hi");
        asm.get("d_hi");
        asm.i32_sub();
        asm.get("r_lo");
        asm.get("d_lo");
        asm.i32_lt_u();
        asm.i32_sub();
        asm.set("r_hi");
        asm.get("r_lo");
        asm.get("d_lo");
        asm.i32_sub();
        asm.set("r_lo");
        asm.get("q_lo");
        asm.i32_const(1);
        asm.i32_or();
        asm.set("q_lo");
        asm.label("skip_64_64");
        asm.drop(); // carry
        asm.get("i");
        asm.i32_const(-1);
        asm.i32_add();
        asm.tee("i");
        asm.br_if_nez("loop_64_64");
    }
    asm.drop(); // i
    asm.set("d_hi"); // r_hi
    asm.set("d_lo"); // r_lo
    asm.set("n_lo"); // q_lo
    asm.i32_const(0);
    asm.set("n_hi"); // q_hi = 0

    asm.label("end");
    asm.finish()
}

/// Emits an in-place two-limb negation of the named slot pair:
/// `(lo, hi) = 0 - (lo, hi)`.
fn emit_negate64(asm: &mut SnippetAsm, lo: &'static str, hi: &'static str) {
    // hi' = ~hi + (lo == 0), computed first because it needs the original `lo`.
    asm.get(hi);
    asm.i32_const(-1);
    asm.i32_xor();
    asm.get(lo);
    asm.i32_eqz();
    asm.i32_add();
    asm.set(hi);
    asm.i32_const(0);
    asm.get(lo);
    asm.i32_sub();
    asm.set(lo);
}

/// Emits an in-place `abs` of the named i64 slot pair, branching over the negation when the
/// value is non-negative. `skip_label` must be unique within the body.
fn emit_abs64(asm: &mut SnippetAsm, lo: &'static str, hi: &'static str, skip_label: &'static str) {
    asm.get(hi);
    asm.i32_const(-1);
    asm.i32_gt_s();
    asm.br_if_nez(skip_label);
    emit_negate64(asm, lo, hi);
    asm.label(skip_label);
}

/// The signed wrappers share everything except the overflow trap, which half of the core's
/// output survives, and which sign the result takes.
enum SignedResult {
    Quotient,
    Remainder,
}

/// How a wrapper reaches the shared core: a `CallInternal` placeholder resolved by
/// `emit_snippets` (snippet mode), or the core body spliced in place (snippets-off mode).
enum CoreMode {
    Call,
    Inline,
}

fn emit_core(asm: &mut SnippetAsm, mode: CoreMode) {
    match mode {
        CoreMode::Call => asm.call_snippet_unresolved(),
        CoreMode::Inline => {
            asm.splice(InstructionSet::MSH_UDIVMOD64, emit_udivmod64);
        }
    }
}

/// Emits the shared signed wrapper: sign bookkeeping, `abs` of both operands, the call into
/// [`InstructionSet::op_udivmod64`], and conditional negation of the selected result.
fn emit_signed_div_rem(is: &mut InstructionSet, result: SignedResult, mode: CoreMode) {
    let mut asm = SnippetAsm::new(is, &["n_lo", "n_hi", "d_lo", "d_hi"]);

    if matches!(result, SignedResult::Quotient) {
        // i64::MIN / -1 overflows; wasm requires a trap. (i64::MIN % -1 is 0 and must not trap,
        // which the unsigned path below produces naturally.)
        asm.get("n_hi");
        asm.i32_const(i32::MIN);
        asm.i32_ne();
        asm.br_if_nez("no_overflow");
        asm.get("n_lo");
        asm.br_if_nez("no_overflow");
        asm.get("d_lo");
        asm.get("d_hi");
        asm.i32_and();
        asm.i32_const(-1);
        asm.i32_ne();
        asm.br_if_nez("no_overflow");
        asm.trap(TrapCode::IntegerOverflow);
        asm.label("no_overflow");
    }

    // The quotient is negative when operand signs differ; the remainder follows the dividend.
    match result {
        SignedResult::Quotient => {
            asm.get("n_hi");
            asm.get("d_hi");
            asm.i32_xor();
        }
        SignedResult::Remainder => asm.get("n_hi"),
    }
    asm.def("sign");

    emit_abs64(&mut asm, "n_lo", "n_hi", "n_abs_done");
    emit_abs64(&mut asm, "d_lo", "d_hi", "d_abs_done");

    // Re-push the unsigned operands above `sign` and divide.
    asm.get("n_lo");
    asm.def("t0");
    asm.get("n_hi");
    asm.def("t1");
    asm.get("d_lo");
    asm.def("t2");
    asm.get("d_hi");
    asm.def("t3");
    emit_core(&mut asm, mode);
    asm.rename("t0", "q_lo");
    asm.rename("t1", "q_hi");
    asm.rename("t2", "r_lo");
    asm.rename("t3", "r_hi");

    // Keep the selected result in the two slots right above `sign`.
    match result {
        SignedResult::Quotient => {
            asm.drop(); // r_hi
            asm.drop(); // r_lo
            asm.rename("q_lo", "res_lo");
            asm.rename("q_hi", "res_hi");
        }
        SignedResult::Remainder => {
            asm.set("q_hi"); // r_hi over q_hi
            asm.set("q_lo"); // r_lo over q_lo
            asm.rename("q_lo", "res_lo");
            asm.rename("q_hi", "res_hi");
        }
    }

    // Apply the recorded sign.
    asm.get("sign");
    asm.i32_const(-1);
    asm.i32_gt_s();
    asm.br_if_nez("result_done");
    emit_negate64(&mut asm, "res_lo", "res_hi");
    asm.label("result_done");

    // Fold the result down into the input slots and clean up.
    asm.set("n_hi");
    asm.set("n_lo");
    asm.drop(); // sign
    asm.drop(); // d_hi
    asm.drop(); // d_lo
    asm.finish();
}

impl InstructionSet {
    /// Maximum stack growth of [`op_udivmod64`] above its four input slots.
    pub const MSH_UDIVMOD64: u32 = 8;
    /// Maximum stack growth of the unsigned wrappers: the call into the core dominates.
    pub const MSH_I64_DIV_U: u32 = 8;
    pub const MSH_I64_REM_U: u32 = 8;
    /// Maximum stack growth of the signed wrappers: one sign slot plus four re-pushed operand
    /// limbs plus the core's own growth.
    pub const MSH_I64_DIV_S: u32 = 13;
    pub const MSH_I64_REM_S: u32 = 13;

    /// Shared unsigned 64-bit divide-and-remainder core; see the module docs for the contract.
    pub fn op_udivmod64(&mut self) {
        emit_udivmod64(self);
    }

    /// `i64.div_u` as a call into [`Self::op_udivmod64`]: drop the remainder limbs.
    pub fn op_i64_div_u(&mut self) {
        self.op_call_internal(crate::SNIPPET_FUNC_IDX_UNRESOLVED);
        self.emit_div_u_epilogue();
    }

    /// `i64.div_u` with the core body spliced in place, for the snippets-off mode.
    pub fn op_i64_div_u_inline(&mut self) {
        emit_udivmod64(self);
        self.emit_div_u_epilogue();
    }

    fn emit_div_u_epilogue(&mut self) {
        self.op_drop();
        self.op_drop();
    }

    /// `i64.rem_u` as a call into [`Self::op_udivmod64`]: fold the remainder limbs over the
    /// quotient limbs.
    pub fn op_i64_rem_u(&mut self) {
        self.op_call_internal(crate::SNIPPET_FUNC_IDX_UNRESOLVED);
        self.emit_rem_u_epilogue();
    }

    /// `i64.rem_u` with the core body spliced in place, for the snippets-off mode.
    pub fn op_i64_rem_u_inline(&mut self) {
        emit_udivmod64(self);
        self.emit_rem_u_epilogue();
    }

    fn emit_rem_u_epilogue(&mut self) {
        self.op_local_set(2);
        self.op_local_set(2);
    }

    /// `i64.div_s`: signed wrapper around the unsigned core.
    pub fn op_i64_div_s(&mut self) {
        emit_signed_div_rem(self, SignedResult::Quotient, CoreMode::Call);
    }

    /// `i64.div_s` with the core body spliced in place, for the snippets-off mode.
    pub fn op_i64_div_s_inline(&mut self) {
        emit_signed_div_rem(self, SignedResult::Quotient, CoreMode::Inline);
    }

    /// `i64.rem_s`: signed wrapper around the unsigned core.
    pub fn op_i64_rem_s(&mut self) {
        emit_signed_div_rem(self, SignedResult::Remainder, CoreMode::Call);
    }

    /// `i64.rem_s` with the core body spliced in place, for the snippets-off mode.
    pub fn op_i64_rem_s_inline(&mut self) {
        emit_signed_div_rem(self, SignedResult::Remainder, CoreMode::Inline);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `MSH_UDIVMOD64` is growth above the four input slots; the assembler tracks the absolute
    /// frame height. Behavioral coverage lives in `tests/snippets.rs`.
    #[test]
    fn udivmod64_max_stack_height_is_pinned() {
        let mut is = InstructionSet::new();
        let max_height = emit_udivmod64(&mut is);
        assert_eq!(max_height, 4 + InstructionSet::MSH_UDIVMOD64);
    }
}
