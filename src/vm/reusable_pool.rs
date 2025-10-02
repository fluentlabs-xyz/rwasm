use crate::ValueStack;

#[derive(Clone)]
pub struct ItemConfig {
    pub initial_len: usize,
    pub maximum_len: usize,
}

#[derive(Clone)]
pub struct Config {
    pub keep: usize,
    pub item_config: ItemConfig,
}

#[derive(Clone)]
pub struct ReusablePool {
    items: Vec<ValueStack>,
    item_config: ItemConfig,
    keep: usize,
}

impl ReusablePool {
    pub fn new(config: Config) -> Self {
        Self {
            items: Vec::new(),
            item_config: config.item_config,
            keep: config.keep,
        }
    }

    pub fn reuse_or_new(&mut self) -> ValueStack {
        match self.items.pop() {
            Some(stack) => {
                // println!("reused");
                stack
            }
            None => {
                // println!("created");
                ValueStack::new(self.item_config.initial_len, self.item_config.maximum_len)
            }
        }
    }

    pub fn recycle(&mut self, mut item: ValueStack) {
        // TODO add check for capacity? stack.entries().capacity() > N_DEFAULT_STACK_SIZE
        if self.items.len() < self.keep {
            item.reset_for_reuse();
            self.items.push(item);
        }
    }
}
