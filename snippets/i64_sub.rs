/// -------------------------------------------------------------------------
/// 64-bit subtraction in pure 32-bit arithmetic
/// (two’s-complement works for both signed and unsigned values)
/// -------------------------------------------------------------------------
#[inline(always)]
pub(crate) fn i64_sub_impl(
    a_lo: u32,
    a_hi: u32,
    b_lo: u32,
    b_hi: u32,
) -> (u32 /* res_lo */, u32 /* res_hi */) {
    // low 32-bit difference
    let diff_lo = a_lo.wrapping_sub(b_lo);

    // detect borrow without branches
    let borrow = (a_lo < b_lo) as u32;

    // high 32-bit difference minus borrow
    let diff_hi = a_hi.wrapping_sub(b_hi).wrapping_sub(borrow);

    (diff_lo, diff_hi)
}

#[no_mangle]
pub fn i64_sub(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
    let (res_lo, res_hi) = i64_sub_impl(a_lo, a_hi, b_lo, b_hi);
    // pack the two limbs back into a single i64
    ((res_hi as u64) << 32) | res_lo as u64
}
