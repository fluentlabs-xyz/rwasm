#![feature(test)]

#[cfg(test)]
mod bench;
#[cfg(test)]
mod test;

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub fn main(n: i32) -> i32 {
    let (mut a, mut b) = (0, 1);
    for _ in 0..n {
        let temp = a;
        a = b;
        b = temp + b;
    }
    a
}
