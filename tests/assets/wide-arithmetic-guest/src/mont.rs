//! Montgomery multiplication (CIOS) over `u128` intermediates.
//!
//! Shared verbatim by the guest and by `tests/wide_arithmetic.rs`, so the native reference and
//! the guest execute the same algorithm: every limb product is a `u64 * u64 -> u128` and every
//! accumulation a 128-bit add, which is exactly what the wide-arithmetic instructions lower.

/// BN254 base field modulus, little-endian limbs, and `-p^-1 mod 2^64`.
pub const P256: [u64; 4] = [
    0x3c208c16d87cfd47,
    0x97816a916871ca8d,
    0xb85045b68181585d,
    0x30644e72e131a029,
];
pub const P256_INV: u64 = 0x87d20782e4866389;

/// BLS12-381 base field modulus, little-endian limbs, and `-p^-1 mod 2^64`.
pub const P384: [u64; 6] = [
    0xb9feffffffffaaab,
    0x1eabfffeb153ffff,
    0x6730d2a0f6b0f624,
    0x64774b84f38512bf,
    0x4b1ba7b6434bacd7,
    0x1a0111ea397fe69a,
];
pub const P384_INV: u64 = 0x89f3fffcfffcfffd;

/// Returns `a * b * 2^(-64 N) mod p` for `a, b < p` (`N <= 6` limbs).
pub fn mont_mul<const N: usize>(a: &[u64; N], b: &[u64; N], p: &[u64; N], inv: u64) -> [u64; N] {
    // N + 2 accumulator limbs
    let mut t = [0u64; 8];
    for &b_i in b.iter() {
        let mut carry = 0u64;
        for j in 0..N {
            let s = t[j] as u128 + (a[j] as u128) * (b_i as u128) + carry as u128;
            t[j] = s as u64;
            carry = (s >> 64) as u64;
        }
        let s = t[N] as u128 + carry as u128;
        t[N] = s as u64;
        t[N + 1] = (s >> 64) as u64;

        let m = t[0].wrapping_mul(inv);
        let s = t[0] as u128 + (m as u128) * (p[0] as u128);
        let mut carry = (s >> 64) as u64;
        for j in 1..N {
            let s = t[j] as u128 + (m as u128) * (p[j] as u128) + carry as u128;
            t[j - 1] = s as u64;
            carry = (s >> 64) as u64;
        }
        let s = t[N] as u128 + carry as u128;
        t[N - 1] = s as u64;
        t[N] = t[N + 1] + (s >> 64) as u64;
    }
    // t < 2p: subtract p once if t >= p
    let mut reduced = [0u64; N];
    let mut borrow = 0u64;
    for j in 0..N {
        let d = (t[j] as u128)
            .wrapping_sub(p[j] as u128)
            .wrapping_sub(borrow as u128);
        reduced[j] = d as u64;
        borrow = ((d >> 64) != 0) as u64;
    }
    let mut result = [0u64; N];
    let keep = if t[N] >= borrow { &reduced } else { &t[..N] };
    result.copy_from_slice(keep);
    result
}
