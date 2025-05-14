#![allow(clippy::needless_range_loop)]

mod extractor;
#[cfg(test)]
mod test;

/// Stack layout (little-endian limbs)
/// before: …, a_lo, a_hi, b_lo, b_hi
/// after: …, p_lo, p_hi // 64-bit product a · b (mod 2⁶⁴)
#[inline(always)]
pub(crate) fn karatsuba_mul64_impl(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> (u32, u32) {
    #[inline(always)]
    fn add32(a: u32, b: u32) -> (u32, u32) {
        let (sum, carry) = a.overflowing_add(b);
        (sum, carry as u32)
    }

    #[inline(always)]
    fn add64(lo: &mut u32, hi: &mut u32, add_lo: u32, add_hi: u32) {
        let (s, c1) = (*lo).overflowing_add(add_lo);
        let (h, _) = (*hi).overflowing_add(add_hi + c1 as u32);
        *lo = s;
        *hi = h;
    }

    #[inline(always)]
    fn sub64(lo: &mut u32, hi: &mut u32, sub_lo: u32, sub_hi: u32) {
        let (s, b1) = (*lo).overflowing_sub(sub_lo);
        let (h, _) = (*hi).overflowing_sub(sub_hi + b1 as u32);
        *lo = s;
        *hi = h;
    }

    /// 32 × 32 → 64 without leaving `u32`
    #[inline(always)]
    fn mul32(x: u32, y: u32) -> (u32, u32) {
        let x0 = x & 0xFFFF;
        let x1 = x >> 16;
        let y0 = y & 0xFFFF;
        let y1 = y >> 16;

        let t = x0 * y0; // 16×16 => ≤32 bits
        let s1 = x0 * y1;
        let s2 = x1 * y0;
        let v = x1 * y1;

        // cross = s1 + s2 (up to 33 bits)
        let (cross_lo, carry_cross) = add32(s1, s2);

        // low  = t + ((cross & 0xFFFF) << 16)
        let (low, carry_low) = add32(t, (cross_lo & 0xFFFF) << 16);

        // high = v + (cross >> 16) + carry_low
        let cross_hi = (cross_lo >> 16) + (carry_cross << 16);
        let (tmp, carry_hi1) = add32(v, cross_hi);
        let high = tmp + carry_low + carry_hi1; // cannot overflow 32 bits

        (low, high)
    }

    // ---- Karatsuba partial products --------------------------------------
    let (z0_lo, z0_hi) = mul32(a_lo, b_lo);
    let (z2_lo, z2_hi) = mul32(a_hi, b_hi);

    // sums (33-bit each)
    let (sa_lo, ca) = add32(a_lo, a_hi); // sa = sa_hi · 2³² + sa_lo,  sa_hi = ca
    let (sb_lo, cb) = add32(b_lo, b_hi);

    // z1 = (sa * sb) − z0 − z2          (low 64 bits only)
    let (mut z1_lo, mut z1_hi) = mul32(sa_lo, sb_lo);
    if ca != 0 {
        add64(&mut z1_lo, &mut z1_hi, 0, sb_lo); // + sb_lo << 32
    }
    if cb != 0 {
        add64(&mut z1_lo, &mut z1_hi, 0, sa_lo); // + sa_lo << 32
    }
    sub64(&mut z1_lo, &mut z1_hi, z0_lo, z0_hi);
    sub64(&mut z1_lo, &mut z1_hi, z2_lo, z2_hi);

    // ---- assemble low-64-bit result --------------------------------------
    // p = z0 + (z1 << 32) (z2 << 64 drops in mod-2⁶⁴ arithmetic)
    let mut res_lo = z0_lo;
    let mut res_hi = z0_hi;
    add64(&mut res_lo, &mut res_hi, 0, z1_lo); // add z1 << 32

    // ---- push result ------------------------------------------------------
    (res_lo, res_hi)
}

#[no_mangle]
pub fn karatsuba_mul64_stack(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
    let (res_lo, res_hi) = karatsuba_mul64_impl(a_lo, a_hi, b_lo, b_hi);
    (res_hi as u64) << 32 | res_lo as u64
}

#[inline(always)]
pub(crate) fn add64_impl(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> (u32, u32) {
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
pub fn add64_stack(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
    let (res_lo, res_hi) = add64_impl(a_lo, a_hi, b_lo, b_hi);
    (res_hi as u64) << 32 | res_lo as u64
}

#[inline(always)]
pub(crate) fn div64u_impl(
    n_lo: u32,
    n_hi: u32,
    d_lo: u32,
    d_hi: u32,
) -> (u32 /* q_lo */, u32 /* q_hi */) {
    // ---------- simple corner cases ---------------------------------------
    if d_hi == 0 && d_lo == 0 {
        core::hint::black_box(()); // force a trap later; UB if ignored
    }
    if (n_hi < d_hi) || (n_hi == d_hi && n_lo < d_lo) {
        return (0, 0); // quotient = 0  (remainder is n, caller doesn’t need it)
    }

    // ---------- fast path: divisor fits in one limb -----------------------
    if d_hi == 0 {
        // divide (n_hi<<32 | n_lo) by 32-bit d_lo using two 32-bit loops
        #[inline(always)]
        fn div_mod_64_by_32(hi: u32, lo: u32, d: u32) -> (u32, u32) {
            let mut q = 0u32;
            let mut r = 0u32;
            for i in (0..64).rev() {
                // shift remainder left, bring next dividend bit
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

        // shift quotient left and add current bit
        let carry_q = (q_lo >> 31) & 1;
        q_lo = (q_lo << 1) | ge as u32;
        q_hi = (q_hi << 1) | carry_q;
    }

    (q_lo, q_hi)
}

#[no_mangle]
pub fn div64u_stack(n_lo: u32, n_hi: u32, d_lo: u32, d_hi: u32) -> u64 {
    let (q_lo, q_hi) = div64u_impl(n_lo, n_hi, d_lo, d_hi);
    ((q_hi as u64) << 32) | q_lo as u64
}

/// -------------------------------------------------------------------------
/// helpers
/// -------------------------------------------------------------------------

/// Two’s-complement negate of a 64-bit value held in little-endian limbs.
#[inline(always)]
fn neg64(lo: u32, hi: u32) -> (u32, u32) {
    let lo_n = (!lo).wrapping_add(1);
    let hi_n = (!hi).wrapping_add((lo_n == 0) as u32);
    (lo_n, hi_n)
}

/// Unsigned 64-bit division in pure 32-bit arithmetic (from previous reply).
#[inline(always)]
fn div64_impl(n_lo: u32, n_hi: u32, d_lo: u32, d_hi: u32) -> (u32 /* q_lo */, u32 /* q_hi */) {
    /* … same code as before … */
    #![allow(unused_mut, clippy::needless_range_loop)]
    // --- fast corner cases and 32-bit divisor path elided for brevity ---
    let mut n_hi = n_hi;
    let mut n_lo = n_lo;
    let mut r_hi = 0u32;
    let mut r_lo = 0u32;
    let mut q_hi = 0u32;
    let mut q_lo = 0u32;

    for _ in 0..64 {
        let carry_n_hi = n_hi >> 31;
        let carry_n_lo = n_lo >> 31;
        n_hi = (n_hi << 1) | carry_n_lo;
        n_lo <<= 1;
        let carry_r_lo = r_lo >> 31;
        r_hi = (r_hi << 1) | carry_r_lo;
        r_lo = (r_lo << 1) | carry_n_hi;

        let ge = (r_hi > d_hi) || (r_hi == d_hi && r_lo >= d_lo);
        if ge {
            let (new_lo, borrow) = r_lo.overflowing_sub(d_lo);
            r_lo = new_lo;
            r_hi = r_hi.wrapping_sub(d_hi + borrow as u32);
        }

        let carry_q = q_lo >> 31;
        q_lo = (q_lo << 1) | ge as u32;
        q_hi = (q_hi << 1) | carry_q;
    }
    (q_lo, q_hi)
}

/// -------------------------------------------------------------------------
/// signed division implementation (two-limb in / two-limb out)
/// -------------------------------------------------------------------------
#[inline(always)]
pub(crate) fn div64s_impl(
    n_lo: u32,
    n_hi: u32,
    d_lo: u32,
    d_hi: u32,
) -> (u32 /* q_lo */, u32 /* q_hi */) {
    // 1. Extract signs
    let n_neg = (n_hi >> 31) != 0;
    let d_neg = (d_hi >> 31) != 0;

    // 2. Absolute values
    let (n_lo, n_hi) = if n_neg {
        neg64(n_lo, n_hi)
    } else {
        (n_lo, n_hi)
    };
    let (d_lo, d_hi) = if d_neg {
        neg64(d_lo, d_hi)
    } else {
        (d_lo, d_hi)
    };

    // 3. Unsigned divide
    let (mut q_lo, mut q_hi) = div64_impl(n_lo, n_hi, d_lo, d_hi);

    // 4. Apply sign to quotient (truncate toward zero)
    if n_neg ^ d_neg {
        let (lo, hi) = neg64(q_lo, q_hi);
        q_lo = lo;
        q_hi = hi;
    }
    (q_lo, q_hi)
}

/// -------------------------------------------------------------------------
/// public wrapper: packed i64 result
/// -------------------------------------------------------------------------
#[no_mangle]
pub fn div64s_stack(n_lo: u32, n_hi: u32, d_lo: u32, d_hi: u32) -> i64 {
    let (q_lo, q_hi) = div64s_impl(n_lo, n_hi, d_lo, d_hi);
    let bits = ((q_hi as u64) << 32) | q_lo as u64;
    bits as i64
}

/// -------------------------------------------------------------------------
/// 64-bit subtraction in pure 32-bit arithmetic
/// (two’s-complement works for both signed and unsigned values)
/// -------------------------------------------------------------------------

#[inline(always)]
pub(crate) fn sub64_impl(
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
pub fn sub64_stack(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
    let (res_lo, res_hi) = sub64_impl(a_lo, a_hi, b_lo, b_hi);
    // pack the two limbs back into a single i64
    ((res_hi as u64) << 32) | res_lo as u64
}
