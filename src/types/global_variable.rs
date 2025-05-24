use crate::{UntypedValue, Value};
use wasmparser::{GlobalType, ValType};

#[derive(Debug)]
pub struct GlobalVariable {
    pub global_type: GlobalType,
    pub default_value: UntypedValue,
}

impl GlobalVariable {
    pub fn new(global_type: GlobalType, default_value: UntypedValue) -> Self {
        Self {
            global_type,
            default_value,
        }
    }

    pub fn value(&self) -> Option<Value> {
        match self.global_type.content_type {
            ValType::I32 => Some(Value::I32(self.default_value.as_i32())),
            ValType::I64 => Some(Value::I64(self.default_value.as_i64())),
            ValType::F32 => Some(Value::F32(self.default_value.as_f32())),
            ValType::F64 => Some(Value::F64(self.default_value.as_f64())),
            ValType::V128 => None,
            ValType::FuncRef => Some(Value::FuncRef(self.default_value.into())),
            ValType::ExternRef => Some(Value::ExternRef(self.default_value.into())),
        }
    }
}
