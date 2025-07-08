use crate::InstructionPtr;
use smallvec::SmallVec;

#[derive(Default, Clone)]
pub struct CallStack {
    buf: SmallVec<[InstructionPtr; 16]>,
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
