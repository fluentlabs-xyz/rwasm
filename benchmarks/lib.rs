#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub fn main(n: u32) -> u32 {
    let (mut a, mut b) = (0, 1);
    for _ in 0..n {
        let temp = a;
        a = b;
        b = temp + b;
    }
    a
}
