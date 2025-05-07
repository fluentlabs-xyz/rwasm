use crate::types::UntypedValue;

/// An instantiated [`ElementSegmentEntity`].
///
/// # Note
///
/// With the `bulk-memory` Wasm proposal it is possible to interact
/// with element segments at runtime. Therefore Wasm instances now have
/// a need to have an instantiated representation of data segments.
#[derive(Debug)]
pub struct ElementSegmentEntity {
    /// The underlying items of the instance element segment.
    ///
    /// # Note
    ///
    /// These items are just readable after instantiation.
    /// Using Wasm `elem.drop` simply replaces the instance
    /// with an empty one.
    pub(crate) items: Option<Vec<UntypedValue>>,
}

impl ElementSegmentEntity {
    /// Create an empty [`ElementSegmentEntity`] representing dropped element segments.
    pub fn empty() -> Self {
        Self { items: None }
    }

    pub fn new(items: Vec<UntypedValue>) -> Self {
        Self { items: Some(items) }
    }

    /// Performs an emptiness check.
    /// This function returns `true` only if the segment contains no items and not just an empty
    /// array.
    /// This check is crucial to determine if a segment has been dropped.
    pub fn is_empty(&self) -> bool {
        self.items.is_none()
    }

    /// Returns the number of items in the [`ElementSegment`].
    pub fn size(&self) -> u32 {
        self.items().len() as u32
    }

    /// Returns the items of the [`ElementSegmentEntity`].
    pub fn items(&self) -> &[UntypedValue] {
        self.items.as_ref().map(|v| v.as_ref()).unwrap_or(&[])
    }

    /// Drops the items of the [`ElementSegmentEntity`].
    pub fn drop_items(&mut self) {
        self.items = None;
    }
}
