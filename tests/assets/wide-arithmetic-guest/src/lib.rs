//! See `README.md`. `main(base)` reads its inputs from and writes its outputs to linear memory at
//! the offsets below, relative to `base`.
#![no_std]

mod mont;

use mont::{mont_mul, P256, P256_INV, P384, P384_INV};

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// Byte offsets relative to `base`, shared with `tests/wide_arithmetic.rs`.
pub const A256: usize = 0;
pub const B256: usize = 32;
pub const A384: usize = 64;
pub const B384: usize = 112;
/// `mont_mul(a256, b256)`
pub const R256: usize = 160;
/// `mont_mul(a384, b384)`
pub const R384: usize = 192;
/// `a256[0] * b256[0]` unsigned as (lo, hi), then signed as (lo, hi)
pub const WIDE: usize = 240;
pub const END: usize = 272;

unsafe fn read<const N: usize>(base: *mut u8, offset: usize) -> [u64; N] {
    core::ptr::read_unaligned(base.add(offset) as *const [u64; N])
}

unsafe fn write<const N: usize>(base: *mut u8, offset: usize, limbs: [u64; N]) {
    core::ptr::write_unaligned(base.add(offset) as *mut [u64; N], limbs)
}

#[no_mangle]
pub extern "C" fn main(base: *mut u8) {
    unsafe {
        let a: [u64; 4] = read(base, A256);
        let b: [u64; 4] = read(base, B256);
        write(base, R256, mont_mul(&a, &b, &P256, P256_INV));
        let a6: [u64; 6] = read(base, A384);
        let b6: [u64; 6] = read(base, B384);
        write(base, R384, mont_mul(&a6, &b6, &P384, P384_INV));
        let unsigned = (a[0] as u128) * (b[0] as u128);
        let signed = (a[0] as i64 as i128) * (b[0] as i64 as i128);
        write(
            base,
            WIDE,
            [
                unsigned as u64,
                (unsigned >> 64) as u64,
                signed as u64,
                (signed >> 64) as u64,
            ],
        );
    }
}
