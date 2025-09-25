use crate::{UntypedValue, NULL_FUNC_IDX};

#[derive(Clone, Debug)]
/// Opaque reference to a function within the module's function index space.
/// A zero value is reserved for the null reference, mirroring Wasm's funcref null semantics.
pub struct FuncRef(pub u32);

impl FuncRef {
    pub fn new(func_idx: u32) -> Self {
        Self(func_idx)
    }

    pub fn null() -> Self {
        Self(NULL_FUNC_IDX)
    }

    pub fn resolve_index(&self) -> u32 {
        self.0
    }

    pub fn is_null(&self) -> bool {
        self.0 == NULL_FUNC_IDX
    }
}

impl From<UntypedValue> for FuncRef {
    fn from(value: UntypedValue) -> Self {
        let value = value.as_u32();
        if value == 0 {
            Self::null()
        } else {
            Self(value)
        }
    }
}
impl Into<UntypedValue> for FuncRef {
    fn into(self) -> UntypedValue {
        UntypedValue::from(self.0)
    }
}

pub type ExternRef = FuncRef;
