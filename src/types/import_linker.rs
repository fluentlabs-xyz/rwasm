use crate::{ImportName, InstructionSet};
use alloc::vec::Vec;
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
    pub block_fuel: InstructionSet,
    pub params: &'static [ValType],
    pub result: &'static [ValType],
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
        for (a, b) in func_type.params().iter().zip(self.params.iter()) {
            if a != b {
                return false;
            }
        }
        for (a, b) in func_type.results().iter().zip(self.result.iter()) {
            if a != b {
                return false;
            }
        }
        true
    }
}

impl ImportLinker {
    pub fn insert_function(
        &mut self,
        import_name: ImportName,
        sys_func_idx: u32,
        block_fuel: InstructionSet,
        params: &'static [ValType],
        result: &'static [ValType],
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
        debug_assert!(
            self.func_by_name
                .values()
                .find(|v| v.sys_func_idx == sys_func_idx)
                .is_some(),
            "import linker sys func idx collision"
        );
        assert!(last_value.is_none(), "import linker name collision");
    }

    pub fn find_symbols(&self) -> Vec<ImportName> {
        let mut symbols: Vec<ImportName> = self.keys().cloned().collect();
        symbols.sort();
        symbols
    }

    pub fn resolve_by_import_name(&self, import_name: &ImportName) -> Option<&ImportLinkerEntity> {
        self.func_by_name.get(import_name)
    }
}
