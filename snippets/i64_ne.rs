#[no_mangle]
pub fn i64_ne(a_lo: u32, a_hi: u32, b_lo: u32, b_hi: u32) -> u32 {
    ((a_lo != b_lo) || (a_hi != b_hi)) as u32
}
