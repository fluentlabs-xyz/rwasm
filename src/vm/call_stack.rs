use crate::{InstructionPtr, ValueStackPtr};
use smallvec::SmallVec;

#[derive(Default, Clone)]
pub struct CallStack {
    buf: SmallVec<[(InstructionPtr, ValueStackPtr); 128]>,
    offsets: SmallVec<[usize; 128]>,
    offset: usize,
}

impl CallStack {
    pub fn push(&mut self, ip: InstructionPtr, vs: ValueStackPtr) {
        self.buf.push((ip, vs));
    }

    pub fn pop(&mut self) -> Option<(InstructionPtr, ValueStackPtr)> {
        if self.buf.len() > self.offset {
            self.buf.pop()
        } else {
            None
        }
    }

    pub fn is_empty(&self) -> bool {
        self.buf.len() == self.offset
    }

    pub fn len(&self) -> usize {
        // underflow should never happen here
        self.buf.len() - self.offset
    }

    pub fn commit_offset(&mut self) {
        self.offsets.push(self.offset);
        self.offset = self.buf.len();
    }

    pub fn reset(&mut self) {
        unsafe {
            self.buf.set_len(self.offset);
        }
        // TODO(dmitry123): "replace with unwrap() once e2e is refactored"
        self.offset = self.offsets.pop().unwrap_or(0);
    }
}
