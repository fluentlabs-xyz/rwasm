/// Computes the unsigned 64-bit integer remainder (modulus) using only 32-bit arithmetic,
/// returning the 64-bit remainder as two `u32` limbs (low, high).
///
/// Emulates full unsigned 64-bit remainder operation for platforms or VMs lacking native
/// 64-bit division/modulo, such as WASM VMs with 32-bit stack elements.
///
/// # Arguments
/// - `n_lo`, `n_hi`: Low and high 32 bits of the dividend (`n = (n_hi << 32) | n_lo`, interpreted
///   as u64)
/// - `d_lo`, `d_hi`: Low and high 32 bits of the divisor  (`d = (d_hi << 32) | d_lo`, interpreted
///   as u64)
///
/// # Returns
/// - `(rem_lo, rem_hi)`: Low and high 32 bits of the unsigned remainder
///
/// # Algorithm
/// - Traps on division by zero, matching WASM and Rust semantics.
/// - Fast path: When divisor fits in 32 bits, uses a custom 64-by-32 remainder loop.
/// - General path: For arbitrary 64-bit divisors, performs classic binary long division over 64
///   iterations, building up quotient and remainder bitwise using only 32-bit arithmetic.
/// - Returns the unsigned remainder as two 32-bit limbs.
///
/// # Panics / Traps
/// - Division by zero triggers a trap (`core::intrinsics::abort()`).
///
/// # Example
/// ```
/// let (rem_lo, rem_hi) = i64_rem_u_impl(5, 0, 2, 0); // 5 % 2 = 1
/// assert_eq!(((rem_hi as u64) << 32 | rem_lo as u64), 1);
/// let (rem_lo, rem_hi) = i64_rem_u_impl(0xFFFF_FFFF, 0xFFFF_FFFF, 0x12345678, 0); // u64::MAX % 0x12345678
/// ```
///
/// # Note
/// Input and output values are two-limb representations of unsigned 64-bit integers.
#[inline(always)]
pub(crate) fn i64_rem_u_impl(
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

    // fast path: divisor fits in one limb and below 1 << 31 to prevent reminder overflow
    if d_hi == 0 && d_lo < 1 << 31 {
        // divide (n_hi<<32 | n_lo) by 32-bit d_lo using two 32-bit loops
        #[inline(always)]
        fn rem_64_by_32(hi: u32, lo: u32, d: u32) -> u32 {
            let mut r = 0u32;
            for i in (0..64).rev() {
                r <<= 1;
                r |= if i >= 32 {
                    (hi >> (i - 32)) & 1
                } else {
                    (lo >> i) & 1
                };
                if r >= d {
                    r -= d;
                }
            }
            r
        }
        let rem_hi = rem_64_by_32(0, n_hi, d_lo);
        let rem_lo = rem_64_by_32(rem_hi, n_lo, d_lo);
        return (rem_lo, 0);
    }

    // 2. General case: 64-bit divisor, classic long division
    let mut n_hi = n_hi;
    let mut n_lo = n_lo;
    let mut r_hi = 0u32;
    let mut r_lo = 0u32;

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
    }
    (r_lo, r_hi)
}

#[no_mangle]
pub fn i64_rem_u(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
    let (res_lo, res_hi) = i64_rem_u_impl(a_lo, a_hi, b_lo, b_hi);
    (res_hi as u64) << 32 | res_lo as u64
}

/// Special case for 'fast path' highlighting problem with causing data loss in case of
/// divisible overflow (causing MSb loss when shifting left) in case of divisor GE 1<<31
#[test]
fn test_i64_rem_u_divisible_overflow() {
    let test_case = |a: u64, b: u64| {
        let c = a % b;
        let (r_lo, r_hi) = i64_rem_u_impl(a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32);
        let r = (r_hi as u64) << 32 | r_lo as u64;
        assert_eq!(c, r);
    };
    // divisor much greater max (slow path)
    let a: u64 = 9223372036854775807;
    let b: u64 = 3707827967; // 0b11011101000000001111011011111111
    test_case(a, b);
    // divisor equals max (slow path)
    let a: u64 = 9223372036854775807;
    let b: u64 = 1 << 31;
    test_case(a, b);
    // divisor below max by 1 (fast path)
    let a: u64 = 9223372036854775807;
    let b: u64 = (1 << 31) - 1;
    test_case(a, b);
}
