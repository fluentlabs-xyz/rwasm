use std::cell::UnsafeCell;

pub(crate) struct UnsafeSyncCell<T>(UnsafeCell<T>);

impl<T> UnsafeSyncCell<T> {
    pub fn new(val: T) -> Self {
        Self(UnsafeCell::new(val))
    }

    pub fn borrow_mut(&self) -> &mut T {
        unsafe { &mut *self.0.get() }
    }

    pub fn borrow(&self) -> &T {
        unsafe { &*self.0.get() }
    }
}

unsafe impl<T: Send> Send for UnsafeSyncCell<T> {}
unsafe impl<T: Send> Sync for UnsafeSyncCell<T> {}
