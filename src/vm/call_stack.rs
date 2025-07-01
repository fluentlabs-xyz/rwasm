use crate::{InstructionPtr, ValueStackPtr};
use smallvec::SmallVec;

#[derive(Default, Clone)]
pub struct CallStack {
    buf: SmallVec<[(InstructionPtr, ValueStackPtr); 128]>,
}

impl CallStack {
    pub fn push(&mut self, ip: InstructionPtr, vs: ValueStackPtr) {
        self.buf.push((ip, vs));
    }

    pub fn pop(&mut self) -> Option<(InstructionPtr, ValueStackPtr)> {
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
