use alloc::vec::Vec;

#[derive(Clone)]
pub struct ReusablePool<ITEM> {
    items: Vec<ITEM>,
    max_len: usize,
}

impl<ITEM> ReusablePool<ITEM> {
    pub fn new(max_len: usize) -> Self {
        let items = Vec::with_capacity(max_len);
        Self { items, max_len }
    }

    #[inline]
    pub fn try_reuse_item(&mut self) -> Option<ITEM> {
        self.items.pop()
    }

    #[inline]
    pub fn recycle(&mut self, item: ITEM) {
        if self.items.len() < self.max_len {
            self.items.push(item);
        }
    }
}
