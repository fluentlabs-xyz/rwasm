use crate::{InstructionPtr, ValueStackPtr};
use core::ops::{Deref, DerefMut};
use smallvec::SmallVec;

#[derive(Default, Clone)]
pub struct CallStack(SmallVec<[(InstructionPtr, ValueStackPtr); 128]>);

impl Deref for CallStack {
    type Target = SmallVec<[(InstructionPtr, ValueStackPtr); 128]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for CallStack {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl CallStack {
    pub fn reset(&mut self) {
        unsafe {
            self.0.set_len(0);
        }
    }
}
