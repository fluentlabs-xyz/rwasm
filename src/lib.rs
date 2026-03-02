#![cfg_attr(not(feature = "std"), no_std)]
#![warn(unused_crate_dependencies)]
#![allow(unused_variables, dead_code)]
#![recursion_limit = "750"]

extern crate alloc;
extern crate core;

mod compiler;
mod isa;
mod module;
mod strategy;
mod types;
mod vm;
#[cfg(feature = "wasmtime")]
pub mod wasmtime;

pub use compiler::*;
#[cfg(test)]
use hex_literal as _;
pub use isa::*;
use libm as _;
pub use module::*;
pub use rwasm_fuel_policy::*;
pub use strategy::*;
pub use types::*;
pub use vm::*;
pub use wasmparser::{FuncType, ValType};
#[cfg(test)]
use wat as _;
#[cfg(test)]
use criterion as _;
#[cfg(test)]
use fib_example as _;
