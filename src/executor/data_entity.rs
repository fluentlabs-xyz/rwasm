use alloc::sync::Arc;

/// An instantiated [`DataSegmentEntity`].
///
/// # Note
///
/// With the `bulk-memory` Wasm proposal it is possible to interact
/// with data segments at runtime. Therefore Wasm instances now have
/// a need to have an instantiated representation of data segments.
#[derive(Debug)]
pub struct DataSegmentEntity {
    /// The underlying bytes of the instance data segment.
    ///
    /// # Note
    ///
    /// These bytes are just readable after instantiation.
    /// Using Wasm `data.drop` simply replaces the instance
    /// with an empty one.
    bytes: Option<Arc<[u8]>>,
}

impl DataSegmentEntity {
    /// Create an empty [`DataSegmentEntity`] representing dropped data segments.
    pub fn empty() -> Self {
        Self { bytes: None }
    }

    pub fn new(bytes: Arc<[u8]>) -> Self {
        Self { bytes: Some(bytes) }
    }

    /// Performs an emptiness check.
    /// This function returns `true` only if the segment contains no items and not just an empty
    /// array.
    /// This check is crucial to determine if a segment has been dropped.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_none()
    }

    /// Returns the bytes of the [`DataSegmentEntity`].
    pub fn bytes(&self) -> &[u8] {
        self.bytes
            .as_ref()
            .map(|bytes| &bytes[..])
            .unwrap_or_else(|| &[])
    }

    /// Drops the bytes of the [`DataSegmentEntity`].
    pub fn drop_bytes(&mut self) {
        self.bytes = None;
    }
}
