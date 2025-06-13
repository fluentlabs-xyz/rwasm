#[inline(always)]
pub(crate) fn i64_shr_u_impl(a_lo: u32, a_hi: u32, b_lo: u32, _b_hi: u32) -> (u32, u32) {
    // Only the lower 6 bits of the shift amount are used in wasm
    let shamt = b_lo & 0x3F;

    let (res_lo, res_hi) = match shamt {
        0 => (a_lo, a_hi), // No shift
        n @ 1..=31 => {
            // Shift both halves right, with cross-over
            let new_lo = (a_lo >> n) | (a_hi << (32 - n));
            let new_hi = a_hi >> n;
            (new_lo, new_hi)
        }
        32 => (a_hi, 0),
        n @ 33..=63 => {
            let new_lo = a_hi >> (n - 32);
            (new_lo, 0)
        }
        _ => (0, 0), // For completeness; shamt only goes to 63
    };

    (res_lo, res_hi)
}

#[no_mangle]
pub fn i64_shr_u(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
    let (res_lo, res_hi) = i64_shr_u_impl(a_lo, a_hi, b_lo, b_hi);
    (res_hi as u64) << 32 | res_lo as u64
}
