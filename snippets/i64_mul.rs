/// Stack layout (little-endian limbs)
/// before: …, a_lo, a_hi, b_lo, b_hi
/// after: …, p_lo, p_hi // 64-bit product a · b (mod 2⁶⁴)
#[inline(always)]
pub(crate) fn i64_mul_impl(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> (u32, u32) {
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
pub fn i64_mul(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u64 {
    let (res_lo, res_hi) = i64_mul_impl(a_lo, a_hi, b_lo, b_hi);
    (res_hi as u64) << 32 | res_lo as u64
}
