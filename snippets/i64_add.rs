#[inline(always)]
pub(crate) fn i64_add_impl(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> (u32, u32) {
    // low part
    let sum_lo = a_lo.wrapping_add(b_lo);
    // compute carry without branches
    let carry = (sum_lo < a_lo) as u32;
    // high part + carry
    let sum_hi = a_hi.wrapping_add(b_hi).wrapping_add(carry);
    // push result
    (sum_lo, sum_hi)
}

#[no_mangle]
pub fn i64_add(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
    let (res_lo, res_hi) = i64_add_impl(a_lo, a_hi, b_lo, b_hi);
    (res_hi as u64) << 32 | res_lo as u64
}
