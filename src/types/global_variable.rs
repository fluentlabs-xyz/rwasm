use crate::{ExternRef, FuncRef, Value, F32, F64};
use wasmparser::{GlobalType, ValType};

#[derive(Debug)]
/// Describes a module's global (type and initial value) as seen at compile time.
/// The value can be materialized into a runtime `Value` when the type permits.
pub struct GlobalVariable {
    /// Wasm global type (content type and mutability).
    pub global_type: GlobalType,
    /// Default value encoded as i64; interpreted according to `global_type`.
    pub default_value: i64,
}

impl GlobalVariable {
    pub fn new(global_type: GlobalType, default_value: i64) -> Self {
        Self {
            global_type,
            default_value,
        }
    }

    pub fn value(&self) -> Option<Value> {
        match self.global_type.content_type {
            ValType::I32 => Some(Value::I32(self.default_value as i32)),
            ValType::I64 => Some(Value::I64(self.default_value)),
            ValType::F32 => Some(Value::F32(F32::from_bits(self.default_value as i32 as u32))),
            ValType::F64 => Some(Value::F64(F64::from_bits(self.default_value as u64))),
            ValType::V128 => None,
            ValType::FuncRef => Some(Value::FuncRef(FuncRef::new(
                self.default_value.try_into().ok()?,
            ))),
            ValType::ExternRef => Some(Value::ExternRef(ExternRef::new(
                self.default_value.try_into().ok()?,
            ))),
        }
    }
}
