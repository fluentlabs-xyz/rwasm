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
            ValType::FUNCREF => Some(Value::FuncRef(FuncRef::new(
                self.default_value.try_into().ok()?,
            ))),
            ValType::EXTERNREF => Some(Value::ExternRef(ExternRef::new(
                self.default_value.try_into().ok()?,
            ))),
            ValType::Ref(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmparser::RefType;

    fn global(content_type: ValType, default_value: i64) -> GlobalVariable {
        GlobalVariable::new(
            GlobalType {
                content_type,
                mutable: false,
                shared: false,
            },
            default_value,
        )
    }

    /// Reference globals materialize as `funcref`/`externref` values; an index that does not fit
    /// a reference, and a reference type the runtime does not model (a non-nullable
    /// `(ref func)`), have no value.
    #[test]
    fn reference_globals_materialize_by_type() {
        assert_eq!(
            global(ValType::FUNCREF, 3).value(),
            Some(Value::FuncRef(FuncRef::new(3)))
        );
        assert_eq!(
            global(ValType::EXTERNREF, 4).value(),
            Some(Value::ExternRef(ExternRef::new(4)))
        );
        assert_eq!(global(ValType::FUNCREF, -1).value(), None);
        assert_eq!(global(ValType::Ref(RefType::FUNC), 0).value(), None);
    }
}
