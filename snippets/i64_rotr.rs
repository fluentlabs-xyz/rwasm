#[inline(always)]
pub(crate) fn i64_rotr_impl(a_lo: u32, a_hi: u32, b_lo: u32, _b_hi: u32) -> (u32, u32) {
    let k = b_lo & 0x3F;
    match k {
        0 => (a_lo, a_hi),
        32 => (a_hi, a_lo),
        n @ 1..=31 => {
            let lo = (a_lo >> n) | (a_hi << (32 - n));
            let hi = (a_hi >> n) | (a_lo << (32 - n));
            (lo, hi)
        }
        n @ 33..=63 => {
            let m = n - 32;
            let lo = (a_hi >> m) | (a_lo << (32 - m));
            let hi = (a_lo >> m) | (a_hi << (32 - m));
            (lo, hi)
        }
        _ => unsafe {
            core::intrinsics::unreachable();
        },
    }
}

#[no_mangle]
pub fn i64_rotr(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
    let (res_lo, res_hi) = i64_rotr_impl(a_lo, a_hi, b_lo, b_hi);
    (res_hi as u64) << 32 | res_lo as u64
}
