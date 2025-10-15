use crate::{CallStack, ValueStack};

#[derive(Default)]
pub struct ReusableStacks {
    pub value_stack: ValueStack,
    pub call_stack: CallStack,
}

impl ReusableStacks {
    pub fn make_recyclable(&mut self) {
        self.value_stack.reset();
        self.call_stack.reset();
    }
}
