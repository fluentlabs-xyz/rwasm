use crate::{ImportName, ValueType};
use core::ops::{Deref, DerefMut};
use hashbrown::HashMap;

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
    pub func_idx: u32,
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

impl ImportLinker {
    pub fn insert_function(
        &mut self,
        import_name: ImportName,
        func_idx: u32,
        block_fuel: u32,
        params: &'static [ValueType],
        result: &'static [ValueType],
    ) {
        let last_value = self.func_by_name.insert(
            import_name,
            ImportLinkerEntity {
                func_idx,
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
