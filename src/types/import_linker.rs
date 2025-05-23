use crate::{ImportName, ValueType};
use core::ops::{Deref, DerefMut};
use hashbrown::HashMap;
use wasmparser::{FuncType, ValType};

#[derive(Debug, Default, Clone)]
pub struct ImportLinker {
    func_by_name: HashMap<ImportName, ImportLinkerEntity>,
}

impl Deref for ImportLinker {
    type Target = HashMap<ImportName, ImportLinkerEntity>;

    fn deref(&self) -> &Self::Target {
        &self.func_by_name
    }
}
impl DerefMut for ImportLinker {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.func_by_name
    }
}

#[derive(Debug, Clone)]
pub struct ImportLinkerEntity {
    pub sys_func_idx: u32,
    pub block_fuel: u32,
    pub params: &'static [ValueType],
    pub result: &'static [ValueType],
}

impl<const N: usize> From<[(&'static str, &'static str, ImportLinkerEntity); N]> for ImportLinker {
    fn from(arr: [(&'static str, &'static str, ImportLinkerEntity); N]) -> Self {
        Self {
            func_by_name: HashMap::from_iter(arr.into_iter().map(
                |(module_name, fn_name, entity)| (ImportName::new(module_name, fn_name), entity),
            )),
        }
    }
}

impl<const N: usize> From<[(ImportName, ImportLinkerEntity); N]> for ImportLinker {
    fn from(arr: [(ImportName, ImportLinkerEntity); N]) -> Self {
        Self {
            func_by_name: HashMap::from(arr),
        }
    }
}

impl ImportLinkerEntity {
    pub fn matches_func_type(&self, func_type: &FuncType) -> bool {
        if func_type.params().len() != self.params.len()
            || func_type.results().len() != self.result.len()
        {
            return false;
        }
        fn match_type(a: ValType, b: ValueType) -> bool {
            match (a, b) {
                (ValType::I32, ValueType::I32)
                | (ValType::I64, ValueType::I64)
                | (ValType::F32, ValueType::F32)
                | (ValType::F64, ValueType::F64)
                | (ValType::FuncRef, ValueType::FuncRef)
                | (ValType::ExternRef, ValueType::ExternRef) => true,
                _ => false,
            }
        }
        for (a, b) in func_type.params().iter().zip(self.params.iter()) {
            if !match_type(*a, *b) {
                return false;
            }
        }
        for (a, b) in func_type.results().iter().zip(self.result.iter()) {
            if !match_type(*a, *b) {
                return false;
            }
        }
        false
    }
}

impl ImportLinker {
    pub fn insert_function(
        &mut self,
        import_name: ImportName,
        sys_func_idx: u32,
        block_fuel: u32,
        params: &'static [ValueType],
        result: &'static [ValueType],
    ) {
        let last_value = self.func_by_name.insert(
            import_name,
            ImportLinkerEntity {
                sys_func_idx,
                block_fuel,
                params,
                result,
            },
        );
        assert!(last_value.is_none(), "rwasm: import linker name collision");
    }

    pub fn resolve_by_import_name(&self, import_name: &ImportName) -> Option<&ImportLinkerEntity> {
        self.func_by_name.get(import_name)
    }
}
