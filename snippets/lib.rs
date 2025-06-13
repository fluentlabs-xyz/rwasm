#![allow(
    clippy::needless_range_loop,
    internal_features,
    unused_unsafe,
    dead_code
)]
#![feature(core_intrinsics)]

#[cfg(test)]
mod extractor;
#[cfg(test)]
mod fuzz;
mod i64_add;
mod i64_div_s;
mod i64_div_u;
mod i64_mul;
mod i64_ne;
mod i64_rem_s;
mod i64_rem_u;
mod i64_rotl;
mod i64_rotr;
mod i64_shl;
mod i64_shr_s;
mod i64_shr_u;
mod i64_sub;
