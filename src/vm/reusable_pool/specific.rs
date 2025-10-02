use crate::vm::reusable_pool::ItemConfig;
use crate::{CallStack, ValueStack};

#[derive(Clone)]
pub struct ValueStackItemConfig {
    initial_len: usize,
    maximum_len: usize,
}
impl ValueStackItemConfig {
    pub fn new(initial_len: usize, maximum_len: usize) -> Self {
        Self {
            initial_len,
            maximum_len,
        }
    }
}

impl ItemConfig<ValueStack> for ValueStackItemConfig {
    fn create_item(&self) -> ValueStack {
        ValueStack::new(self.initial_len, self.maximum_len)
    }

    fn reset_for_reuse(item: &mut ValueStack) {
        item.reset()
    }
}

#[derive(Clone)]
pub struct CallStackItemConfig {}

impl CallStackItemConfig {
    pub fn new() -> Self {
        Self {}
    }
}

impl ItemConfig<CallStack> for CallStackItemConfig {
    fn create_item(&self) -> CallStack {
        CallStack::default()
    }

    fn reset_for_reuse(item: &mut CallStack) {
        item.reset()
    }
}
