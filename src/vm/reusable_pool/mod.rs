use std::marker::PhantomData;

pub mod specific;

pub trait ItemConfig<ITEM>: Clone + Sized {
    fn create_item(&self) -> ITEM;
    fn reset_for_reuse(item: &mut ITEM);
}

#[derive(Clone)]
pub struct Config<ITEM, CONFIG: ItemConfig<ITEM>> {
    pub keep: usize,
    pub item_config: CONFIG,
    pub _phantom: PhantomData<ITEM>,
}

impl<ITEM, CONFIG: ItemConfig<ITEM>> Config<ITEM, CONFIG> {
    pub fn new(keep: usize, item_config: CONFIG) -> Self {
        Self {
            keep,
            item_config,
            _phantom: PhantomData::default(),
        }
    }
}

#[derive(Clone)]
pub struct ReusablePool<ITEM, CONFIG: ItemConfig<ITEM>> {
    items: Vec<ITEM>,
    item_config: CONFIG,
    keep: usize,
}

impl<ITEM, CONFIG: ItemConfig<ITEM>> ReusablePool<ITEM, CONFIG> {
    pub fn new(config: Config<ITEM, CONFIG>) -> Self {
        Self {
            items: Vec::new(),
            item_config: config.item_config,
            keep: config.keep,
        }
    }

    pub fn reuse_or_new(&mut self) -> ITEM {
        match self.items.pop() {
            Some(stack) => {
                // println!("reused");
                stack
            }
            None => {
                // println!("created");
                self.item_config.create_item()
                // ValueStack::new(self.item_config.initial_len, self.item_config.maximum_len)
            }
        }
    }

    pub fn recycle(&mut self, mut item: ITEM) {
        // TODO add check for capacity? stack.entries().capacity() > N_DEFAULT_STACK_SIZE
        if self.items.len() < self.keep {
            CONFIG::reset_for_reuse(&mut item);
            self.items.push(item);
        }
    }
}
