use crate::{add64_impl, div64s_impl, div64u_impl, karatsuba_mul64_impl, sub64_impl};
use rand::Rng;

#[test]
fn test_i64_mul_fuzz() {
    for _ in 0..1_000_000 {
        let a = rand::rng().random::<u64>();
        let b = rand::rng().random::<u64>();
        let c = a.wrapping_mul(b);
        let (r_lo, r_hi) =
            karatsuba_mul64_impl(a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32);
        let r = (r_hi as u64) << 32 | r_lo as u64;
        assert_eq!(c, r);
    }
}

#[test]
fn test_i64_add_fuzz() {
    for _ in 0..1_000_000 {
        let a = rand::rng().random::<u64>();
        let b = rand::rng().random::<u64>();
        let c = a.wrapping_add(b);
        let (r_lo, r_hi) = add64_impl(a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32);
        let r = (r_hi as u64) << 32 | r_lo as u64;
        assert_eq!(c, r);
    }
}

#[test]
fn test_i64_div_u_fuzz() {
    for _ in 0..1_000_000 {
        let a = rand::rng().random::<u64>();
        let b = rand::rng().random::<u64>();
        let c = a.wrapping_div(b);
        let (r_lo, r_hi) = div64u_impl(a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32);
        let r = (r_hi as u64) << 32 | r_lo as u64;
        assert_eq!(c, r);
    }
}

#[test]
fn test_i64_div_s_fuzz() {
    for _ in 0..1_000_000 {
        let a = rand::rng().random::<i64>();
        let b = rand::rng().random::<i64>();
        let c = a.wrapping_div(b);
        let (r_lo, r_hi) = div64s_impl(a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32);
        let r = (r_hi as i64) << 32 | r_lo as i64;
        assert_eq!(c, r);
    }
}

#[test]
fn test_i64_sub_fuzz() {
    for _ in 0..1_000_000 {
        let a = rand::rng().random::<u64>();
        let b = rand::rng().random::<u64>();
        let c = a.wrapping_sub(b);
        let (r_lo, r_hi) = sub64_impl(a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32);
        let r = (r_hi as u64) << 32 | r_lo as u64;
        assert_eq!(c, r);
    }
}
