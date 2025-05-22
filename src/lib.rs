#![cfg_attr(not(feature = "std"), no_std)]
#![allow(dead_code)]
#![warn(unused_crate_dependencies)]
#![recursion_limit = "750"]

mod compiler;
mod types;
mod vm;

extern crate alloc;
extern crate core;

pub use compiler::*;
use libm as _;
pub use types::*;
pub use vm::*;
pub use wasmparser::{FuncType, ValType};
