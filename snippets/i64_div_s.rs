/// -------------------------------------------------------------------------
/// signed division implementation (two-limb in / two-limb out)
/// -------------------------------------------------------------------------
#[inline(always)]
pub(crate) fn i64_div_s_impl(
    n_lo: u32,
    n_hi: u32,
    d_lo: u32,
    d_hi: u32,
) -> (u32 /* q_lo */, u32 /* q_hi */) {
    // 0. Zero division
    if d_hi == 0 && d_lo == 0 {
        unsafe {
            core::intrinsics::breakpoint();
        }
    }
    // 1. Overflow: i64::MIN / -1 triggers overflow
    let is_n_min = n_hi == 0x8000_0000 && n_lo == 0;
    let is_d_neg_one = d_hi == 0xFFFF_FFFF && d_lo == 0xFFFF_FFFF;
    if is_n_min && is_d_neg_one {
        unsafe {
            core::intrinsics::breakpoint();
        }
    }

    // 2. Extract signs
    let n_neg = (n_hi >> 31) != 0;
    let d_neg = (d_hi >> 31) != 0;

    // 3. Absolute values
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

    // 4. Unsigned divide
    let (mut q_lo, mut q_hi) = div64_impl(n_lo, n_hi, d_lo, d_hi);

    // 5. Apply sign to quotient (truncate toward zero)
    if n_neg ^ d_neg {
        let (lo, hi) = neg64(q_lo, q_hi);
        q_lo = lo;
        q_hi = hi;
    }
    (q_lo, q_hi)
}

/// Two’s-complement negates of a 64-bit value held in little-endian limbs.
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
/// public wrapper: packed i64 result
/// -------------------------------------------------------------------------
#[no_mangle]
pub fn i64_div_s(n_lo: u32, n_hi: u32, d_lo: u32, d_hi: u32) -> i64 {
    let (q_lo, q_hi) = i64_div_s_impl(n_lo, n_hi, d_lo, d_hi);
    let bits = ((q_hi as u64) << 32) | q_lo as u64;
    bits as i64
}
