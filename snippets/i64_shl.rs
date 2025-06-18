#[inline(always)]
pub(crate) fn i64_shl_impl(a_lo: u32, a_hi: u32, b_lo: u32, _b_hi: u32) -> (u32, u32) {
    // WASM uses only the low 6 bits of the shift count
    let shamt = b_lo & 0x3F;

    match shamt {
        0 => (a_lo, a_hi),
        n @ 1..=31 => {
            // Cross bits from lo to hi
            let res_lo = a_lo << n;
            let res_hi = (a_hi << n) | (a_lo >> (32 - n));
            (res_lo, res_hi)
        }
        32 => (0, a_lo),
        n @ 33..=63 => {
            // Only low 32 bits (a_lo) matter, shifted up to hi
            let res_hi = a_lo << (n - 32);
            (0, res_hi)
        }
        _ => (0, 0), // For completeness (shouldn't hit, due to masking)
    }
}

#[no_mangle]
pub fn i64_shl(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
    let (res_lo, res_hi) = i64_shl_impl(a_lo, a_hi, b_lo, b_hi);
    (res_hi as u64) << 32 | res_lo as u64
}
