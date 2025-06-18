#[inline(always)]
pub(crate) fn i64_rotl_impl(a_lo: u32, a_hi: u32, b_lo: u32, _b_hi: u32) -> (u32, u32) {
    let k = b_lo & 0x3F;
    if k == 0 {
        (a_lo, a_hi)
    } else if k < 32 {
        let lo = (a_lo << k) | (a_hi >> (32 - k));
        let hi = (a_hi << k) | (a_lo >> (32 - k));
        (lo, hi)
    } else if k == 32 {
        (a_hi, a_lo)
    } else {
        // k in 33..=63
        let m = k - 32;
        let lo = (a_hi << m) | (a_lo >> (32 - m));
        let hi = (a_lo << m) | (a_hi >> (32 - m));
        (lo, hi)
    }
}

#[no_mangle]
pub fn i64_rotl(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
    let (res_lo, res_hi) = i64_rotl_impl(a_lo, a_hi, b_lo, b_hi);
    (res_hi as u64) << 32 | res_lo as u64
}

#[test]
fn test_rotl() {
    fn rotl_ref(a: u64, k: u32) -> u64 {
        let k = k & 63;
        if k == 0 {
            a
        } else {
            (a << k) | (a >> (64 - k))
        }
    }
    let a: u64 = 0x123456789ABCDEF0;
    for k in 0..=64 {
        let b_lo = k;
        let (a_lo, a_hi) = (a as u32, (a >> 32) as u32);
        let (r_lo, r_hi) = i64_rotl_impl(a_lo, a_hi, b_lo, 0);
        let expect = rotl_ref(a, k);
        assert_eq!((r_lo as u64) | ((r_hi as u64) << 32), expect);
    }
}

#[test]
fn test_i64_rotl_highest_bit() {
    let a: u64 = 0x8000_0000_0000_0000;
    let b: u32 = 1;
    let a_lo = a as u32;
    let a_hi = (a >> 32) as u32;
    let (res_lo, res_hi) = i64_rotl_impl(a_lo, a_hi, b, 0);

    let result = (res_hi as u64) << 32 | (res_lo as u64);
    assert_eq!(result, 0x0000_0000_0000_0001);
}
