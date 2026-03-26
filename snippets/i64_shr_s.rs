#[inline(always)]
pub(crate) fn i64_shr_s_impl(a_lo: u32, a_hi: u32, b_lo: u32, _b_hi: u32) -> (u32, u32) {
    // WASM uses only the low 6 bits of the shift count
    let shamt = b_lo & 0x3F;

    match shamt {
        0 => (a_lo, a_hi),
        n @ 1..=31 => {
            // Arithmetic right shift for hi: sign is preserved
            let hi = a_hi as i32;
            let res_hi = (hi >> n) as u32;
            let res_lo = (a_lo >> n) | (a_hi << (32 - n));
            (res_lo, res_hi)
        }
        32 => {
            let hi = a_hi as i32;
            let res_lo = hi as u32;
            let res_hi = if hi < 0 { u32::MAX } else { 0 };
            (res_lo, res_hi)
        }
        n @ 33..=63 => {
            let hi = a_hi as i32;
            let res_hi = if hi < 0 { u32::MAX } else { 0 };
            let res_lo = (hi >> (n - 32)) as u32;
            (res_lo, res_hi)
        }
        _ => (0, 0), // unreachable due to masking, but keeps exhaustiveness
    }
}

#[no_mangle]
pub fn i64_shr_s(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
    let (res_lo, res_hi) = i64_shr_s_impl(a_lo, a_hi, b_lo, b_hi);
    (res_hi as u64) << 32 | res_lo as u64
}
