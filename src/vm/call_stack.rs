use crate::InstructionPtr;
use alloc::vec::Vec;

#[derive(Default, Clone)]
/// A lightweight call stack used by the interpreter to track return addresses.
/// It stores instruction pointers for active calls and allows fast push/pop without heap churn.
/// The capacity grows on demand but is typically small due to Wasm's structured control flow.
pub struct CallStack {
    /// Return address stack backing storage; holds instruction pointers for nested calls.
    buf: Vec<InstructionPtr>,
}

impl CallStack {
    pub fn push(&mut self, ip: InstructionPtr) {
        self.buf.push(ip);
    }

    pub fn pop(&mut self) -> Option<InstructionPtr> {
        self.buf.pop()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.len() == 0
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn reset(&mut self) {
        unsafe {
            self.buf.set_len(0);
        }
    }
}
