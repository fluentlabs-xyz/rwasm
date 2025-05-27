#[inline(always)]
pub(crate) fn i64_div_u_impl(
    n_lo: u32,
    n_hi: u32,
    d_lo: u32,
    d_hi: u32,
) -> (u32 /* q_lo */, u32 /* q_hi */) {
    // ---------- simple corner cases ---------------------------------------
    if d_hi == 0 && d_lo == 0 {
        unsafe {
            core::intrinsics::breakpoint();
        }
    }
    if (n_hi < d_hi) || (n_hi == d_hi && n_lo < d_lo) {
        return (0, 0); // quotient = 0 (the remainder is n, caller doesn’t need it)
    }

    // ---------- fast path: divisor fits in one limb -----------------------
    if d_hi == 0 {
        // divide (n_hi<<32 | n_lo) by 32-bit d_lo using two 32-bit loops
        #[inline(always)]
        fn div_mod_64_by_32(hi: u32, lo: u32, d: u32) -> (u32, u32) {
            let mut q = 0u32;
            let mut r = 0u32;
            for i in (0..64).rev() {
                // shift the remainder left, bring the next dividend bit
                r <<= 1;
                r |= if i >= 32 {
                    (hi >> (i - 32)) & 1
                } else {
                    (lo >> i) & 1
                };
                if r >= d {
                    r -= d;
                    if i >= 32 {
                        q |= 1 << (i - 32);
                    } else {
                        q |= 1 << i;
                    }
                }
            }
            (q, r)
        }

        // high half first, then low half with carry remainder
        let (q_hi, rem) = div_mod_64_by_32(0, n_hi, d_lo);
        let (q_lo, _) = div_mod_64_by_32(rem, n_lo, d_lo);
        return (q_lo, q_hi);
    }

    // ---------- general 64-bit ÷ 64-bit long division ---------------------
    let mut n_hi = n_hi;
    let mut n_lo = n_lo;
    let mut r_hi = 0u32;
    let mut r_lo = 0u32;
    let mut q_hi = 0u32;
    let mut q_lo = 0u32;

    for _ in 0..64 {
        // left-shift (r_hi,r_lo,n_hi,n_lo) by 1
        let carry_n_hi = (n_hi >> 31) & 1;
        let carry_n_lo = (n_lo >> 31) & 1;
        n_hi = (n_hi << 1) | carry_n_lo;
        n_lo <<= 1;
        let carry_r_lo = (r_lo >> 31) & 1;
        r_hi = (r_hi << 1) | carry_r_lo;
        r_lo = (r_lo << 1) | carry_n_hi;

        // compare remainder with divisor
        let ge = (r_hi > d_hi) || (r_hi == d_hi && r_lo >= d_lo);
        if ge {
            // r -= d
            let (new_lo, borrow) = r_lo.overflowing_sub(d_lo);
            r_lo = new_lo;
            r_hi = r_hi.wrapping_sub(d_hi + borrow as u32);
        }

        // shift quotient left and add the current bit
        let carry_q = (q_lo >> 31) & 1;
        q_lo = (q_lo << 1) | ge as u32;
        q_hi = (q_hi << 1) | carry_q;
    }

    (q_lo, q_hi)
}

#[no_mangle]
pub fn i64_div_u(n_lo: u32, n_hi: u32, d_lo: u32, d_hi: u32) -> u64 {
    let (q_lo, q_hi) = i64_div_u_impl(n_lo, n_hi, d_lo, d_hi);
    ((q_hi as u64) << 32) | q_lo as u64
}
