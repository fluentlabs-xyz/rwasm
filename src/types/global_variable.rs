use crate::{ExternRef, FuncRef, Value, F32, F64};
use wasmparser::{GlobalType, ValType};

#[derive(Debug)]
pub struct GlobalVariable {
    pub global_type: GlobalType,
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
            ValType::Ref(ref_type) if ref_type == wasmparser::RefType::FUNC => Some(Value::FuncRef(FuncRef::new(
                self.default_value.try_into().ok()?,
            ))),
            ValType::Ref(ref_type) if ref_type == wasmparser::RefType::EXTERN => Some(Value::ExternRef(ExternRef::new(
                self.default_value.try_into().ok()?,
            ))),
            ValType::Ref(ref_type)  => panic!("ref type not supported {:?}", ref_type),
        }
    }
}
