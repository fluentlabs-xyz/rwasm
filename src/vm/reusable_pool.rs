use alloc::vec::Vec;
use core::marker::PhantomData;

pub trait ItemConfig<ITEM>: Clone + Sized {
    fn create_item(&self) -> ITEM;
    fn reset_for_reuse(item: &mut ITEM);
}

#[derive(Clone)]
pub struct ReusablePoolConfig<ITEM, CONFIG: ItemConfig<ITEM>> {
    pub keep: usize,
    pub item_config: CONFIG,
    pub _phantom: PhantomData<ITEM>,
}

impl<ITEM, CONFIG: ItemConfig<ITEM>> ReusablePoolConfig<ITEM, CONFIG> {
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
    pub fn new(config: ReusablePoolConfig<ITEM, CONFIG>) -> Self {
        Self {
            items: Vec::new(),
            item_config: config.item_config,
            keep: config.keep,
        }
    }

    #[inline]
    pub fn warmup(&mut self, count: Option<usize>) {
        let count = count
            .map(|v| if v <= self.keep { v } else { self.keep })
            .unwrap_or(self.keep);
        self.items.reserve(count);
        while self.items.len() < count {
            let item = self.new_item();
            self.recycle(item);
        }
    }

    #[inline]
    pub fn new_item(&mut self) -> ITEM {
        self.item_config.create_item()
    }

    #[inline]
    pub fn reuse_or_new_item(&mut self) -> ITEM {
        match self.items.pop() {
            Some(item) => item,
            None => self.new_item(),
        }
    }

    #[inline]
    pub fn recycle(&mut self, mut item: ITEM) {
        if self.items.len() < self.keep {
            CONFIG::reset_for_reuse(&mut item);
            self.items.push(item);
        }
    }
}
