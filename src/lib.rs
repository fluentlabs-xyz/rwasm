#![cfg_attr(not(feature = "std"), no_std)]
#![allow(dead_code)]
#![warn(unused_crate_dependencies)]

mod binary_format;
// mod compiler;
mod executor;
mod types;

extern crate alloc;
extern crate core;

pub use executor::*;
pub use types::*;
