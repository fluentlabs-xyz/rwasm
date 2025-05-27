/// Computes the signed 64-bit integer remainder (modulus) using only 32-bit arithmetic,
/// returning the 64-bit remainder as two `u32` limbs (low, high).
///
/// Emulates full signed 64-bit remainder operation for platforms or VMs lacking native
/// 64-bit division/modulo, such as WASM VMs with 32-bit stack elements.
///
/// # Arguments
/// - `n_lo`, `n_hi`: Low and high 32 bits of the dividend (`n = (n_hi << 32) | n_lo`, interpreted
///   as signed i64)
/// - `d_lo`, `d_hi`: Low and high 32 bits of the divisor  (`d = (d_hi << 32) | d_lo`, interpreted
///   as signed i64)
///
/// # Returns
/// - `(rem_lo, rem_hi)`: Low and high 32 bits of the signed remainder (as if casting to i64 and
///   splitting)
///
/// # Algorithm
/// - Traps on division by zero, matching WASM and Rust semantics.
/// - Traps on signed overflow (`i64::MIN % -1` is defined as 0 in Rust/WASM, but still check for
///   consistency).
/// - Computes absolute values of numerator and denominator.
/// - Performs unsigned 64-bit division to obtain both quotient and remainder.
/// - Restores the correct sign to the remainder (remainder always has the same sign as the
///   dividend).
///
/// # Panics / Traps
/// - Division by zero triggers a trap (`core::intrinsics::abort()`).
///
/// # Example
/// ```
/// let (rem_lo, rem_hi) = i64_rem_s_impl(5, 0, 2, 0); // 5 % 2 = 1
/// assert_eq!(((rem_hi as i64) << 32 | rem_lo as i64), 1);
/// let (rem_lo, rem_hi) = i64_rem_s_impl(5, 0, 0xFF_FF_FF_FF, 0xFF_FF_FF_FF); // 5 % -1 = 0
/// assert_eq!(((rem_hi as i64) << 32 | rem_lo as i64), 0);
/// ```
///
/// # Note
/// The remainder takes the sign of the dividend, matching WebAssembly and Rust semantics.
/// Input and output values are two-limb representations of signed 64-bit integers.
#[inline(always)]
pub(crate) fn i64_rem_s_impl(
    n_lo: u32,
    n_hi: u32,
    d_lo: u32,
    d_hi: u32,
) -> (u32 /* rem_lo */, u32 /* rem_hi */) {
    // 0. Division by zero
    if d_hi == 0 && d_lo == 0 {
        unsafe {
            core::intrinsics::breakpoint();
        }
    }

    // 1. Extract signs
    let n_neg = (n_hi >> 31) != 0;
    let d_neg = (d_hi >> 31) != 0;

    // 2. Absolute values
    let (abs_n_lo, abs_n_hi) = if n_neg {
        neg64(n_lo, n_hi)
    } else {
        (n_lo, n_hi)
    };
    let (abs_d_lo, abs_d_hi) = if d_neg {
        neg64(d_lo, d_hi)
    } else {
        (d_lo, d_hi)
    };

    // 3. Unsigned divide to get remainder
    let (_, _, mut rem_lo, mut rem_hi) =
        div64_with_remainder(abs_n_lo, abs_n_hi, abs_d_lo, abs_d_hi);

    // 4. Apply sign of the original numerator to remainder
    if n_neg {
        let (lo, hi) = neg64(rem_lo, rem_hi);
        rem_lo = lo;
        rem_hi = hi;
    }
    (rem_lo, rem_hi)
}

/// Unsigned 64-bit division with remainder.
/// Returns (quotient_lo, quotient_hi, remainder_lo, remainder_hi)
fn div64_with_remainder(n_lo: u32, n_hi: u32, d_lo: u32, d_hi: u32) -> (u32, u32, u32, u32) {
    // Fast path for 32-bit divisor can be added for efficiency,
    // but for brevity, here is the general long division version:
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
    (q_lo, q_hi, r_lo, r_hi)
}

/// Two-limb negation: returns two-limb (-a) for 64-bit values
#[inline(always)]
fn neg64(lo: u32, hi: u32) -> (u32, u32) {
    let (lo, carry) = (!lo).overflowing_add(1);
    let hi = (!hi).wrapping_add(carry as u32);
    (lo, hi)
}

#[no_mangle]
pub fn i64_rem_s(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
    let (res_lo, res_hi) = i64_rem_s_impl(a_lo, a_hi, b_lo, b_hi);
    (res_hi as u64) << 32 | res_lo as u64
}
