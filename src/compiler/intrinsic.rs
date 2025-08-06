use crate::{FuncIdx, Opcode};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Intrinsic {
    Replace(Vec<Opcode>),
    Remove,
}

#[derive(Default, Debug)]
pub struct IntrinsicHandler {
    pub intrinsics: HashMap<FuncIdx, Intrinsic>,
}
